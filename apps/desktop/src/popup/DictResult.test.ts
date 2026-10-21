import { mount } from '@vue/test-utils';
import { describe, expect, test } from 'vitest';
import type { EntryView, LookupView, SenseLine } from './content';
import DictResult from './DictResult.vue';

function line(text: string, pos: string | null = null, domain: string | null = null): SenseLine {
  return { pos, domain, text };
}

function entry(fields: Partial<EntryView> = {}): EntryView {
  return {
    word: 'run',
    relation: null,
    phonetic: null,
    labels: [],
    collins: 0,
    general: [],
    domain: [],
    domain_open: false,
    max_lines: null,
    ...fields,
  };
}

function render(view: LookupView | null, text = 'run') {
  return mount(DictResult, { props: { text, view } });
}

// Spaces between the spans come from CSS; the text has none.
function lines(wrapper: ReturnType<typeof render>): string[] {
  return wrapper.findAll('.dict__line').map((p) => p.text());
}

describe('entries', () => {
  test('title: word, relation, phonetic, labels and Collins stars', () => {
    const wrapper = render({
      kind: 'entries',
      note: null,
      entries: [
        entry({
          word: 'ran',
          relation: 'run 的过去式',
          phonetic: 'ræn',
          labels: ['中考', 'CET4'],
          collins: 3,
        }),
      ],
    });
    expect(wrapper.find('.dict__word').text()).toBe('ran');
    expect(wrapper.find('.dict__relation').text()).toBe('（run 的过去式）');
    expect(wrapper.find('.dict__phonetic').text()).toBe('/ræn/');
    expect(wrapper.findAll('.dict__label').map((l) => l.text())).toEqual(['中考', 'CET4']);
    const stars = wrapper.find('.dict__stars');
    expect(stars.text()).toBe('★★★');
    expect(stars.attributes('title')).toBe('柯林斯 3 星');
  });

  test('missing parts of the title are left out', () => {
    const wrapper = render({ kind: 'entries', note: null, entries: [entry()] });
    expect(wrapper.find('.dict__relation').exists()).toBe(false);
    expect(wrapper.find('.dict__phonetic').exists()).toBe(false);
    expect(wrapper.find('.dict__label').exists()).toBe(false);
    expect(wrapper.find('.dict__stars').exists()).toBe(false);
  });

  test('note above the entries', () => {
    const wrapper = render({ kind: 'entries', note: '按原形查到', entries: [entry()] });
    expect(wrapper.find('.dict__note').text()).toBe('按原形查到');
  });

  test('sense lines with and without part of speech', () => {
    const wrapper = render({
      kind: 'entries',
      note: null,
      entries: [entry({ general: [line('跑', 'v.'), line('奔跑')] })],
    });
    expect(lines(wrapper)).toEqual(['v.跑', '奔跑']);
    expect(wrapper.findAll('.dict__pos').map((p) => p.text())).toEqual(['v.']);
  });

  test('"更多" shows the hidden lines and folds them again', async () => {
    const wrapper = render({
      kind: 'entries',
      note: null,
      entries: [entry({ general: [line('一'), line('二'), line('三'), line('四')], max_lines: 2 })],
    });
    expect(lines(wrapper)).toEqual(['一', '二']);
    const toggle = wrapper.find('.dict__toggle');
    expect(toggle.text()).toBe('▸ 更多（2）');

    await toggle.trigger('click');
    expect(lines(wrapper)).toEqual(['一', '二', '三', '四']);
    expect(wrapper.find('.dict__toggle').text()).toBe('▾ 收起');

    await wrapper.find('.dict__toggle').trigger('click');
    expect(lines(wrapper)).toEqual(['一', '二']);
    expect(wrapper.emitted('toggled')).toHaveLength(2);
  });

  test('no "更多" when every line fits', () => {
    const wrapper = render({
      kind: 'entries',
      note: null,
      entries: [entry({ general: [line('一'), line('二')], max_lines: 2 })],
    });
    expect(wrapper.find('.dict__toggle').exists()).toBe(false);
  });

  test('domain senses start folded and unfold with their domain', async () => {
    const wrapper = render({
      kind: 'entries',
      note: null,
      entries: [
        entry({
          general: [line('跑', 'v.')],
          domain: [line('运行', 'v.', '计算机'), line('游程', 'n.', '统计')],
        }),
      ],
    });
    expect(wrapper.find('.dict__line--domain').exists()).toBe(false);
    const toggle = wrapper.find('.dict__toggle');
    expect(toggle.text()).toBe('▸ 专业释义（2）');

    await toggle.trigger('click');
    expect(wrapper.findAll('.dict__line--domain').map((p) => p.text())).toEqual([
      '[计算机]v.运行',
      '[统计]n.游程',
    ]);
    expect(wrapper.find('.dict__toggle').text()).toBe('▾ 专业释义（2）');
    expect(wrapper.emitted('toggled')).toHaveLength(1);
  });

  test('domain senses already open have no toggle', () => {
    const wrapper = render({
      kind: 'entries',
      note: null,
      entries: [entry({ domain: [line('游程', 'n.', '统计')], domain_open: true })],
    });
    expect(wrapper.find('.dict__toggle').exists()).toBe(false);
    expect(wrapper.find('.dict__line--domain').text()).toBe('[统计]n.游程');
  });

  test('each entry unfolds on its own', async () => {
    const folded = { general: [line('一'), line('二')], max_lines: 1 };
    const wrapper = render({
      kind: 'entries',
      note: null,
      entries: [entry({ word: 'a', ...folded }), entry({ word: 'b', ...folded })],
    });
    const [first, second] = wrapper.findAll('.dict__entry');
    await second?.find('.dict__toggle').trigger('click');
    expect(first?.findAll('.dict__line')).toHaveLength(1);
    expect(second?.findAll('.dict__line')).toHaveLength(2);
  });
});

test('parts: each part with its line, and the words not found', () => {
  const wrapper = render(
    {
      kind: 'parts',
      parts: [
        { word: 'state', relation: null, phonetic: 'steit', line: line('国家', 'n.') },
        { word: 'art', relation: null, phonetic: null, line: line('技术', 'n.', '计算机') },
        { word: 'of', relation: null, phonetic: null, line: null },
      ],
      not_found: ['xyzzy', 'qux'],
    },
    'state-of-the-art-xyzzy-qux',
  );
  expect(wrapper.find('.popup__header').text()).toBe('state-of-the-art-xyzzy-qux');
  expect(wrapper.findAll('.dict__part').map((p) => p.text())).toEqual([
    'state/steit/n.国家',
    'art[计算机]n.技术',
    'of',
  ]);
  const notes = wrapper.findAll('.dict__note').map((p) => p.text());
  expect(notes).toEqual(['词典没有整词，以下是各部分的释义', '未收录：xyzzy、qux']);
});

test('parts: no "未收录" line when every part is found', () => {
  const wrapper = render({ kind: 'parts', parts: [], not_found: [] });
  expect(wrapper.findAll('.dict__note')).toHaveLength(1);
});

test.each([
  [null, 'run', '词典文件缺失，暂时无法查词。'],
  [{ kind: 'not_found' } as const, 'run', '词典中没有收录。'],
  [{ kind: 'sentence' } as const, 'I run.', '句子翻译需要 AI（M2 接入）。'],
])('%o: the text and a hint', (view, text, hint) => {
  const wrapper = render(view, text);
  expect(wrapper.text()).toContain(text);
  expect(wrapper.find('.popup__hint').text()).toBe(hint);
});

test('not English', () => {
  const wrapper = render({ kind: 'not_english' }, '你好');
  expect(wrapper.find('.popup__title').text()).toBe('目前只支持英文');
  expect(wrapper.find('.dict__sentence').text()).toBe('你好');
});
