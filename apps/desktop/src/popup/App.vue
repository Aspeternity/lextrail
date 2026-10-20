<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { UNAVAILABLE_MESSAGES, type Content } from './content';
import { call, subscribe } from './ipc';

const content = ref<Content>({ kind: 'empty' });

const unavailable = computed(() =>
  content.value.kind === 'unavailable' ? UNAVAILABLE_MESSAGES[content.value.reason] : null,
);

onMounted(() => {
  void subscribe((next) => {
    content.value = next;
  });
});
</script>

<template>
  <main class="popup">
    <!-- Always available: when Windows refuses the popup focus, Esc cannot reach it. -->
    <button
      class="popup__close"
      type="button"
      aria-label="关闭"
      title="关闭（Esc）"
      @click="call('hide_popup')"
    >
      ×
    </button>
    <template v-if="content.kind === 'loading'">
      <p class="popup__hint">正在取词…</p>
    </template>
    <template v-else-if="content.kind === 'release_keys'">
      <p class="popup__hint">松开快捷键后取词…</p>
    </template>
    <template v-else-if="content.kind === 'selection'">
      <header class="popup__header">{{ content.text }}</header>
      <p v-if="content.sentence" class="popup__sentence">{{ content.sentence }}</p>
      <p class="popup__hint">词典结果稍后接入。</p>
      <p v-if="content.source_app" class="popup__source">来自 {{ content.source_app }}</p>
    </template>
    <template v-else-if="unavailable">
      <header class="popup__title">{{ unavailable.title }}</header>
      <p v-if="unavailable.hint" class="popup__hint">{{ unavailable.hint }}</p>
    </template>
    <template v-else-if="content.kind === 'error'">
      <header class="popup__title">取词出错</header>
      <p class="popup__hint">{{ content.message }}</p>
    </template>
  </main>
</template>
