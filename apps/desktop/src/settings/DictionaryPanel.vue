<script setup lang="ts">
import { ref } from 'vue';
import {
  errorText,
  pickDictFolder,
  resetDictFolder,
  setEnglishDefinitions,
  setOxford,
  type SettingsView,
} from './ipc';

const props = defineProps<{ settings: SettingsView }>();
const emit = defineEmits<{ update: [settings: SettingsView] }>();

const error = ref<string | null>(null);
/** A change is on its way to Rust (the folder dialog may be open). */
const busy = ref(false);

async function run(change: () => Promise<SettingsView>): Promise<boolean> {
  if (busy.value) {
    return false;
  }
  busy.value = true;
  error.value = null;
  try {
    emit('update', await change());
    return true;
  } catch (e: unknown) {
    error.value = errorText(e);
    return false;
  } finally {
    busy.value = false;
  }
}

/** A switch: put back when the change fails or another one is under way. */
async function toggle(event: Event, send: (on: boolean) => Promise<SettingsView>): Promise<void> {
  const box = event.target as HTMLInputElement;
  const on = box.checked;
  if (!(await run(() => send(on)))) {
    box.checked = !on;
  }
}

const dictionaries = () => props.settings.dictionaries;
</script>

<template>
  <section class="panel">
    <h2 class="panel__title">词典</h2>
    <div class="field">
      <p class="field__label">扩展词典文件夹</p>
      <div class="dict__row">
        <span id="dict-folder" class="dict__folder">{{ dictionaries().folder || '（未知）' }}</span>
        <span v-if="dictionaries().folderIsDefault" class="dict__default">默认位置</span>
      </div>
      <div class="dict__row">
        <button
          id="dict-pick"
          type="button"
          class="button"
          :disabled="busy || !!settings.readOnly"
          @click="run(pickDictFolder)"
        >
          选择文件夹…
        </button>
        <button
          id="dict-reset"
          type="button"
          class="button"
          :disabled="busy || !!settings.readOnly || dictionaries().folderIsDefault"
          @click="run(resetDictFolder)"
        >
          恢复默认
        </button>
      </div>
      <p
        id="dict-status"
        role="status"
        class="message"
        :class="{
          'message--ok': dictionaries().status === 'ready',
          'message--error': dictionaries().status === 'unusable',
        }"
      >
        {{ dictionaries().statusText }}
      </p>
      <p class="field__help">
        放牛津词典文件 oalecd10.db
        的文件夹。设置文件会同步到其他电脑，那里没有这个文件夹时不使用牛津词典，查词照常。
      </p>
    </div>
    <div class="field">
      <label class="switch">
        <input
          id="dict-oxford"
          type="checkbox"
          class="switch__input"
          :checked="dictionaries().oxford"
          :disabled="!!settings.readOnly"
          @change="toggle($event, setOxford)"
        />
        <span class="switch__label">使用牛津词典</span>
      </label>
      <p class="field__help">关掉后查词只用内置词典，从下一次查词起生效。</p>
    </div>
    <div class="field">
      <label class="switch">
        <input
          id="dict-english"
          type="checkbox"
          class="switch__input"
          :checked="dictionaries().englishDefinitions"
          :disabled="!!settings.readOnly"
          @change="toggle($event, setEnglishDefinitions)"
        />
        <span class="switch__label">显示英文释义</span>
      </label>
      <p class="field__help">牛津词条的每个义项都显示英文释义。关掉后，英文释义和例句一起折叠。</p>
    </div>
    <p v-if="error" id="dict-error" role="alert" class="message message--error">{{ error }}</p>
  </section>
</template>
