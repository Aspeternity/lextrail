<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { UNAVAILABLE_MESSAGES, type Content } from './content';
import DictResult from './DictResult.vue';
import { call, subscribe } from './ipc';

const content = ref<Content>({ kind: 'empty' });
// Changes with every content event, so the result's unfolded sections start closed.
const lookupId = ref(0);

const unavailable = computed(() =>
  content.value.kind === 'unavailable' ? UNAVAILABLE_MESSAGES[content.value.reason] : null,
);

onMounted(() => {
  void subscribe((next) => {
    content.value = next;
    lookupId.value += 1;
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
      <!-- Scrolls when the result is long; the sentence and source stay in view. -->
      <div class="popup__body">
        <DictResult :key="lookupId" :text="content.text" :view="content.lookup" />
      </div>
      <footer class="popup__footer">
        <p v-if="content.sentence" class="popup__sentence">{{ content.sentence }}</p>
        <p class="popup__meta">
          <span>AI 语境解析：未配置（M2 接入）</span>
          <span v-if="content.source_app">来自 {{ content.source_app }}</span>
        </p>
      </footer>
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
