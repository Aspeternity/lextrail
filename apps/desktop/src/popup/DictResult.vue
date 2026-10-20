<script setup lang="ts">
import { reactive } from 'vue';
import type { EntryView, LookupView, SenseLine } from './content';

defineProps<{ text: string; view: LookupView | null }>();
// A section was unfolded or folded: the window follows the new height.
const emit = defineEmits<{ toggled: [] }>();

// Unfolded sections, by entry index: "domain" (专业释义) and "more" (更多). The parent
// re-creates this component for each lookup, so nothing stays open across lookups.
const open = reactive(new Set<string>());

function toggle(key: string): void {
  if (open.has(key)) {
    open.delete(key);
  } else {
    open.add(key);
  }
  emit('toggled');
}

function generalLines(entry: EntryView, index: number): SenseLine[] {
  if (entry.max_lines === null || open.has(`${index}:more`)) {
    return entry.general;
  }
  return entry.general.slice(0, entry.max_lines);
}

function hiddenLines(entry: EntryView): number {
  return entry.max_lines === null ? 0 : Math.max(0, entry.general.length - entry.max_lines);
}

function showDomain(entry: EntryView, index: number): boolean {
  return entry.domain_open || open.has(`${index}:domain`);
}

function stars(count: number): string {
  return '★'.repeat(count);
}
</script>

<template>
  <template v-if="view === null">
    <header class="popup__header">{{ text }}</header>
    <p class="popup__hint">词典文件缺失，暂时无法查词。</p>
  </template>

  <template v-else-if="view.kind === 'entries'">
    <p v-if="view.note" class="dict__note">{{ view.note }}</p>
    <section v-for="(entry, i) in view.entries" :key="i" class="dict__entry">
      <header class="dict__title">
        <span class="dict__word">{{ entry.word }}</span>
        <span v-if="entry.relation" class="dict__relation">（{{ entry.relation }}）</span>
        <span v-if="entry.phonetic" class="dict__phonetic">/{{ entry.phonetic }}/</span>
        <span v-for="label in entry.labels" :key="label" class="dict__label">{{ label }}</span>
        <span v-if="entry.collins > 0" class="dict__stars" :title="`柯林斯 ${entry.collins} 星`">
          {{ stars(entry.collins) }}
        </span>
      </header>
      <p v-for="(line, j) in generalLines(entry, i)" :key="j" class="dict__line">
        <span v-if="line.pos" class="dict__pos">{{ line.pos }}</span
        >{{ line.text }}
      </p>
      <button
        v-if="hiddenLines(entry) > 0"
        type="button"
        class="dict__toggle"
        @click="toggle(`${i}:more`)"
      >
        {{ open.has(`${i}:more`) ? '▾ 收起' : `▸ 更多（${hiddenLines(entry)}）` }}
      </button>
      <template v-if="entry.domain.length > 0">
        <button
          v-if="!entry.domain_open"
          type="button"
          class="dict__toggle"
          @click="toggle(`${i}:domain`)"
        >
          {{ open.has(`${i}:domain`) ? '▾' : '▸' }} 专业释义（{{ entry.domain.length }}）
        </button>
        <template v-if="showDomain(entry, i)">
          <p v-for="(line, j) in entry.domain" :key="j" class="dict__line dict__line--domain">
            <span class="dict__domain">[{{ line.domain }}]</span>
            <span v-if="line.pos" class="dict__pos">{{ line.pos }}</span
            >{{ line.text }}
          </p>
        </template>
      </template>
    </section>
  </template>

  <template v-else-if="view.kind === 'parts'">
    <header class="popup__header">{{ text }}</header>
    <p class="dict__note">词典没有整词，以下是各部分的释义</p>
    <p v-for="(part, i) in view.parts" :key="i" class="dict__part">
      <span class="dict__word">{{ part.word }}</span>
      <span v-if="part.relation" class="dict__relation">（{{ part.relation }}）</span>
      <span v-if="part.phonetic" class="dict__phonetic">/{{ part.phonetic }}/</span>
      <template v-if="part.line">
        <span v-if="part.line.domain" class="dict__domain">[{{ part.line.domain }}]</span>
        <span v-if="part.line.pos" class="dict__pos">{{ part.line.pos }}</span
        >{{ part.line.text }}
      </template>
    </p>
    <p v-if="view.not_found.length > 0" class="dict__note">
      未收录：{{ view.not_found.join('、') }}
    </p>
  </template>

  <template v-else-if="view.kind === 'not_found'">
    <header class="popup__header">{{ text }}</header>
    <p class="popup__hint">词典中没有收录。</p>
  </template>

  <template v-else-if="view.kind === 'sentence'">
    <p class="dict__sentence">{{ text }}</p>
    <p class="popup__hint">句子翻译需要 AI（M2 接入）。</p>
  </template>

  <template v-else-if="view.kind === 'not_english'">
    <header class="popup__title">目前只支持英文</header>
    <p class="dict__sentence">{{ text }}</p>
  </template>
</template>
