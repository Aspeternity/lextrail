<script setup lang="ts">
/**
 * Records the lookup hotkey (spec section 4). The recorder is a button, not an <input>:
 * an editable element would let the Chinese IME take the keys. Everything about the
 * keys (names, rules) comes from Rust; this only collects key presses.
 */
import { computed, nextTick, onBeforeUnmount, ref } from 'vue';
import {
  applyHotkey,
  checkKey,
  errorText,
  modifiersOf,
  setRecording,
  type CheckView,
  type HotkeyView,
  type Modifiers,
  type SettingsView,
} from './ipc';

const props = defineProps<{ settings: SettingsView }>();
const emit = defineEmits<{ update: [settings: SettingsView] }>();

type State = { kind: 'idle' } | { kind: 'recording' };

const state = ref<State>({ kind: 'idle' });
/** A line under the recorder: the result of the last action, or a recording hint. */
const notice = ref<{ text: string; error: boolean } | null>(null);
const held = ref<Modifiers | null>(null);
/**
 * A change is on its way to Rust. Only guards against a second click: the buttons keep
 * their look meanwhile, or the whole row would flash for the few milliseconds it takes.
 */
let busy = false;
const recorder = ref<HTMLButtonElement | null>(null);
/** Modifiers went down since all were last up, and no main key arrived (spec 4.4). */
let sawModifier = false;
let sawMainKey = false;

const MODIFIER_NAMES: [keyof Modifiers, string][] = [
  ['ctrl', 'Ctrl'],
  ['alt', 'Alt'],
  ['shift', 'Shift'],
  ['meta', 'Win'],
];

const recorderText = computed(() => {
  const current = state.value;
  if (current.kind === 'recording') {
    const names = MODIFIER_NAMES.filter(([key]) => held.value?.[key]).map(([, name]) => name);
    return names.length > 0 ? `${names.join(' + ')} + …` : '请按下新的快捷键（Esc 取消）';
  }
  if (props.settings.lookupOff) return '不使用';
  return props.settings.lookup?.display ?? '未设置';
});

const status = computed(() => {
  if (state.value.kind !== 'idle') return '录制期间快捷键暂停使用';
  if (props.settings.lookupOff) {
    return props.settings.doubleCtrlC
      ? '不使用快捷键，可以连按两次 Ctrl+C 查词'
      : '不使用快捷键。要查词，请设置快捷键或打开下面的"连按两次 Ctrl+C"';
  }
  if (props.settings.lookupActive) return '已生效';
  return `未生效：${props.settings.lookupError ?? '原因未知'}`;
});

const failed = computed(
  () => state.value.kind === 'idle' && !props.settings.lookupActive && !props.settings.lookupOff,
);

const isDefault = computed(
  () => props.settings.lookup?.accelerator === props.settings.defaultLookup.accelerator,
);

async function startRecording(): Promise<void> {
  if (busy || state.value.kind !== 'idle') return;
  notice.value = null;
  try {
    emit('update', await setRecording(true));
  } catch (error: unknown) {
    notice.value = { text: errorText(error), error: true };
    return;
  }
  enterRecording();
}

function enterRecording(): void {
  state.value = { kind: 'recording' };
  held.value = null;
  sawModifier = false;
  sawMainKey = false;
  listen(true);
  void nextTick(() => recorder.value?.focus());
}

async function cancel(): Promise<void> {
  listen(false);
  state.value = { kind: 'idle' };
  held.value = null;
  try {
    emit('update', await setRecording(false));
  } catch (error: unknown) {
    notice.value = { text: errorText(error), error: true };
  }
}

/** `null` turns the lookup hotkey off. */
async function apply(hotkey: HotkeyView | null): Promise<void> {
  if (busy) return;
  listen(false);
  busy = true;
  try {
    const result = await applyHotkey(hotkey?.accelerator ?? '');
    emit('update', result.settings);
    switch (result.outcome) {
      case 'changed':
        notice.value = {
          text: hotkey ? `已改为 ${hotkey.display}` : '已关闭快捷键',
          error: false,
        };
        break;
      case 'unchanged':
        notice.value = null;
        break;
      case 'blocked':
        notice.value = { text: result.message ?? '', error: true };
        break;
      case 'register_failed':
        notice.value = {
          text: `${hotkey?.display ?? ''} 注册失败：${result.message ?? ''}。已保留原来的快捷键`,
          error: true,
        };
        break;
      case 'save_failed':
        notice.value = { text: `${result.message ?? ''}。已保留原来的快捷键`, error: true };
        break;
    }
  } catch (error: unknown) {
    notice.value = { text: errorText(error), error: true };
  } finally {
    busy = false;
  }
  state.value = { kind: 'idle' };
}

async function onKeyDown(event: KeyboardEvent): Promise<void> {
  // Nothing reaches the page or WebView2: no Alt menu, no Tab focus move, no F5 reload.
  event.preventDefault();
  event.stopPropagation();
  if (event.repeat || state.value.kind !== 'recording') return;
  const keys = modifiersOf(event);
  const anyModifier = keys.ctrl || keys.alt || keys.shift || keys.meta;
  if (event.code === 'Escape' && !anyModifier) {
    void cancel();
    return;
  }
  held.value = keys;
  let checked: CheckView | null;
  try {
    checked = await checkKey(event.code, keys);
  } catch (error: unknown) {
    notice.value = { text: errorText(error), error: true };
    return;
  }
  if (state.value.kind !== 'recording') return;
  if (checked === null) {
    if (anyModifier) sawModifier = true;
    return;
  }
  sawMainKey = true;
  if (checked.blocked) {
    notice.value = { text: checked.blocked, error: true };
    return;
  }
  notice.value = null;
  void apply(checked.hotkey);
}

function onKeyUp(event: KeyboardEvent): void {
  event.preventDefault();
  event.stopPropagation();
  if (state.value.kind !== 'recording') return;
  const keys = modifiersOf(event);
  held.value = keys;
  if (keys.ctrl || keys.alt || keys.shift || keys.meta) return;
  // All modifiers are up again.
  if (sawModifier && !sawMainKey) {
    notice.value = { text: '按下没有反应，可能被别的程序占用', error: true };
  }
  sawModifier = false;
  sawMainKey = false;
}

/** Switching to another program while recording cancels (spec 4.5). */
function onBlur(): void {
  if (state.value.kind !== 'idle') void cancel();
}

function listen(on: boolean): void {
  if (on) {
    window.addEventListener('keydown', onKeyDown, true);
    window.addEventListener('keyup', onKeyUp, true);
  } else {
    window.removeEventListener('keydown', onKeyDown, true);
    window.removeEventListener('keyup', onKeyUp, true);
  }
}

window.addEventListener('blur', onBlur);
onBeforeUnmount(() => {
  window.removeEventListener('blur', onBlur);
  if (state.value.kind !== 'idle') void cancel();
});

/** The notice is replaced by the result, not cleared first: no line appearing and going. */
async function restoreDefault(): Promise<void> {
  await apply(props.settings.defaultLookup);
}

async function turnOff(): Promise<void> {
  await apply(null);
}
</script>

<template>
  <div class="hotkey">
    <div class="hotkey__row">
      <!-- Click to record; Esc cancels (spec 4). -->
      <button
        id="hotkey-recorder"
        ref="recorder"
        type="button"
        class="hotkey__recorder"
        :class="{ 'hotkey__recorder--recording': state.kind === 'recording' }"
        :disabled="!!settings.readOnly"
        title="点击后按下新的快捷键，Esc 取消"
        @click="startRecording"
      >
        {{ recorderText }}
      </button>
      <button
        id="hotkey-default"
        type="button"
        class="button"
        :disabled="!!settings.readOnly || isDefault"
        :title="`默认：${settings.defaultLookup.display}`"
        @click="restoreDefault"
      >
        恢复默认
      </button>
      <button
        id="hotkey-off"
        type="button"
        class="button"
        :disabled="!!settings.readOnly || settings.lookupOff"
        @click="turnOff"
      >
        不使用
      </button>
    </div>
    <p
      id="hotkey-status"
      role="status"
      class="hotkey__status"
      :class="{ 'hotkey__status--failed': failed }"
    >
      {{ status }}
    </p>
    <p
      v-if="notice"
      id="hotkey-notice"
      role="status"
      class="message"
      :class="notice.error ? 'message--error' : 'message--ok'"
    >
      {{ notice.text }}
    </p>
  </div>
</template>
