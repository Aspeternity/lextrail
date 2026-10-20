<script setup lang="ts">
import { ref } from 'vue';
import HotkeyRecorder from './HotkeyRecorder.vue';
import { errorText, setDoubleCtrlC, type SettingsView } from './ipc';

const props = defineProps<{ settings: SettingsView }>();
const emit = defineEmits<{ update: [settings: SettingsView] }>();

const error = ref<string | null>(null);
const busy = ref(false);

async function toggleDoubleCopy(event: Event): Promise<void> {
  const box = event.target as HTMLInputElement;
  busy.value = true;
  error.value = null;
  try {
    emit('update', await setDoubleCtrlC(box.checked));
  } catch (e: unknown) {
    // Back to what is actually in effect.
    box.checked = props.settings.doubleCtrlC;
    error.value = errorText(e);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <section class="panel">
    <h2 class="panel__title">快捷键</h2>

    <div class="field">
      <h3 class="field__label">划词翻译</h3>
      <p class="field__help">
        在任何软件中选中英文后按这个快捷键查词。Ctrl+C、Ctrl+Insert 和 Esc 不能使用（LexTrail
        自己要用到）。
      </p>
      <HotkeyRecorder :settings="settings" @update="(view) => emit('update', view)" />
    </div>

    <div class="field">
      <label class="switch">
        <input
          type="checkbox"
          class="switch__input"
          :checked="settings.doubleCtrlC"
          :disabled="!!settings.readOnly || busy"
          @change="toggleDoubleCopy"
        />
        <span class="switch__label">连按两次 Ctrl+C 查词</span>
      </label>
      <p class="field__help">
        选中文字后快速按两次 Ctrl+C 也能查词，适合快捷键取不到词的程序（如以管理员身份运行的程序）。
        只查词典里有的单词，以及短语和句子；无痕 / InPrivate
        窗口和密码管理器复制的内容不会触发；终端里第二次 Ctrl+C 会中断正在运行的程序，请用快捷键。
      </p>
      <p v-if="error" class="message message--error">{{ error }}</p>
    </div>
  </section>
</template>
