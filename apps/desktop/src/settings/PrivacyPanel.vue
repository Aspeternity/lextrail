<script setup lang="ts">
import { ref } from 'vue';
import { errorText, setRecordHistory, type SettingsView } from './ipc';

const props = defineProps<{ settings: SettingsView }>();
const emit = defineEmits<{ update: [settings: SettingsView] }>();

const error = ref<string | null>(null);
const busy = ref(false);

async function toggle(event: Event): Promise<void> {
  const box = event.target as HTMLInputElement;
  busy.value = true;
  error.value = null;
  try {
    // The switch says "不记录", the setting says "record".
    emit('update', await setRecordHistory(!box.checked));
  } catch (e: unknown) {
    box.checked = !props.settings.recordHistory;
    error.value = errorText(e);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <section class="panel">
    <h2 class="panel__title">隐私</h2>
    <div class="field">
      <label class="switch">
        <input
          type="checkbox"
          class="switch__input"
          :checked="!settings.recordHistory"
          :disabled="!!settings.readOnly || busy"
          @change="toggle"
        />
        <span class="switch__label">不记录查询历史</span>
      </label>
      <p class="field__help">
        打开后，从下一次查词起不再把查过的词和原句写进本机的查询历史。已经记录的历史不会删除。
      </p>
      <p v-if="error" class="message message--error">{{ error }}</p>
    </div>
  </section>
</template>
