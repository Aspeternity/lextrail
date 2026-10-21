<script setup lang="ts">
import { onMounted, ref } from 'vue';
import HotkeyPanel from './HotkeyPanel.vue';
import PrivacyPanel from './PrivacyPanel.vue';
import { errorText, getSettings, type SettingsView } from './ipc';

// Later categories (popup, AI, sync, ...) are added to this list (D047).
const categories = [
  { id: 'hotkeys', label: '快捷键' },
  { id: 'privacy', label: '隐私' },
] as const;
type CategoryId = (typeof categories)[number]['id'];

const current = ref<CategoryId>('hotkeys');
const settings = ref<SettingsView | null>(null);
const loadError = ref<string | null>(null);

onMounted(async () => {
  try {
    settings.value = await getSettings();
  } catch (error: unknown) {
    loadError.value = errorText(error);
  }
});

function update(view: SettingsView): void {
  settings.value = view;
}
</script>

<template>
  <div class="settings">
    <nav class="settings__nav" aria-label="设置分类">
      <button
        v-for="category in categories"
        :id="`nav-${category.id}`"
        :key="category.id"
        type="button"
        class="settings__category"
        :class="{ 'settings__category--current': category.id === current }"
        :aria-current="category.id === current ? 'page' : undefined"
        @click="current = category.id"
      >
        {{ category.label }}
      </button>
    </nav>
    <main class="settings__content">
      <p v-if="loadError" class="message message--error">无法读取设置：{{ loadError }}</p>
      <template v-else-if="settings">
        <p v-if="settings.readOnly" id="read-only" role="alert" class="message message--warning">
          {{ settings.readOnly }}
        </p>
        <HotkeyPanel v-if="current === 'hotkeys'" :settings="settings" @update="update" />
        <PrivacyPanel v-else :settings="settings" @update="update" />
      </template>
    </main>
  </div>
</template>
