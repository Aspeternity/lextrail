<script setup lang="ts">
/**
 * Records the lookup hotkey (spec section 4). The recorder is a button, not an <input>:
 * an editable element would let the Chinese IME take the keys. Everything about the
 * keys (names, rules, warnings) comes from Rust; this only collects key presses.
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

type State =
  | { kind: 'idle' }
  | { kind: 'recording' }
  /** Waiting for "仍要使用" / "重新录制" (spec 3.2). */
  | { kind: 'confirm'; hotkey: HotkeyView; warnings: string[] };

const state = ref<State>({ kind: 'idle' });
/** A line under the recorder: the result of the last action, or a recording hint. */
const notice = ref<{ text: string; error: boolean } | null>(null);
const held = ref<Modifiers | null>(null);
const busy = ref(false);
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
  if (current.kind === 'confirm') return current.hotkey.display;
  return props.settings.lookup?.display ?? '未设置';
});

const status = computed(() => {
  if (state.value.kind !== 'idle') return '录制期间快捷键暂停使用';
  if (props.settings.lookupActive) return '已生效';
  return `未生效：${props.settings.lookupError ?? '原因未知'}`;
});

const isDefault = computed(
  () => props.settings.lookup?.accelerator === props.settings.defaultLookup.accelerator,
);

async function startRecording(): Promise<void> {
  if (busy.value || state.value.kind !== 'idle') return;
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

async function apply(hotkey: HotkeyView, confirmed: boolean): Promise<void> {
  listen(false);
  busy.value = true;
  try {
    const result = await applyHotkey(hotkey.accelerator, confirmed);
    emit('update', result.settings);
    switch (result.outcome) {
      case 'needs_confirmation':
        state.value = { kind: 'confirm', hotkey, warnings: result.warnings };
        return;
      case 'changed':
        notice.value = { text: `已改为 ${hotkey.display}`, error: false };
        break;
      case 'unchanged':
        notice.value = null;
        break;
      case 'blocked':
        notice.value = { text: result.message ?? '', error: true };
        break;
      case 'register_failed':
        notice.value = {
          text: `${hotkey.display} 注册失败：${result.message ?? ''}。已保留原来的快捷键`,
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
    busy.value = false;
  }
  state.value = { kind: 'idle' };
}

function rerecord(): void {
  notice.value = null;
  enterRecording();
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
  void apply(checked.hotkey, false);
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

/** Switching to another program while recording or confirming cancels (spec 4.5). */
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

async function restoreDefault(): Promise<void> {
  notice.value = null;
  await apply(props.settings.defaultLookup, false);
}
</script>

<template>
  <div class="hotkey">
    <div class="hotkey__row">
      <button
        id="hotkey-recorder"
        ref="recorder"
        type="button"
        class="hotkey__recorder"
        :class="{ 'hotkey__recorder--recording': state.kind === 'recording' }"
        :disabled="!!settings.readOnly || busy"
        @click="startRecording"
      >
        {{ recorderText }}
      </button>
      <template v-if="state.kind === 'idle'">
        <button
          id="hotkey-change"
          type="button"
          class="button"
          :disabled="!!settings.readOnly || busy"
          @click="startRecording"
        >
          更改
        </button>
        <button
          id="hotkey-default"
          type="button"
          class="button"
          :disabled="!!settings.readOnly || busy || isDefault"
          @click="restoreDefault"
        >
          恢复默认（{{ settings.defaultLookup.display }}）
        </button>
      </template>
      <button
        v-else-if="state.kind === 'recording'"
        id="hotkey-cancel-recording"
        type="button"
        class="button"
        @click="cancel"
      >
        取消
      </button>
    </div>
    <p
      id="hotkey-status"
      role="status"
      class="hotkey__status"
      :class="{ 'hotkey__status--failed': state.kind === 'idle' && !settings.lookupActive }"
    >
      {{ status }}
    </p>
    <div
      v-if="state.kind === 'confirm'"
      id="hotkey-confirm"
      class="confirm"
      role="alertdialog"
      aria-label="确认快捷键"
    >
      <p class="confirm__title">{{ state.hotkey.display }} 会影响：</p>
      <ul class="confirm__list">
        <li v-for="warning in state.warnings" :key="warning">{{ warning }}</li>
      </ul>
      <div class="confirm__actions">
        <button
          id="hotkey-confirm-use"
          type="button"
          class="button button--primary"
          @click="apply(state.hotkey, true)"
        >
          仍要使用
        </button>
        <button id="hotkey-rerecord" type="button" class="button" @click="rerecord">
          重新录制
        </button>
        <button id="hotkey-confirm-cancel" type="button" class="button" @click="cancel">
          取消
        </button>
      </div>
    </div>
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
