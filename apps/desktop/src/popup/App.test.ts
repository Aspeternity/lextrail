import { emit } from '@tauri-apps/api/event';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { flushPromises, mount } from '@vue/test-utils';
import { afterEach, beforeEach, expect, test } from 'vitest';
import App from './App.vue';
import {
  CONTENT_EVENT,
  UNAVAILABLE_MESSAGES,
  type Content,
  type ContentEvent,
  type UnavailableReason,
} from './content';

/** Commands the page sent to Rust, in order. */
let calls: { cmd: string; args: unknown }[];

beforeEach(() => {
  calls = [];
  mockIPC(
    (cmd, args) => {
      calls.push({ cmd, args });
    },
    { shouldMockEvents: true },
  );
});

afterEach(() => {
  clearMocks();
});

function commands(name: string): unknown[] {
  return calls.filter((c) => c.cmd === name).map((c) => c.args);
}

async function mountPopup() {
  const wrapper = mount(App);
  await flushPromises();
  return wrapper;
}

async function show(content: Content, seq = 1): Promise<void> {
  await emit(CONTENT_EVENT, { seq, content } satisfies ContentEvent);
  await flushPromises();
}

test('tells Rust it is ready once subscribed', async () => {
  await mountPopup();
  expect(commands('popup_ready')).toEqual([{ error: null }]);
});

test('starts empty: only the close button', async () => {
  const wrapper = await mountPopup();
  expect(wrapper.text()).toBe('×');
});

test.each([
  [{ kind: 'loading' } as const, '正在取词…'],
  [{ kind: 'release_keys' } as const, '松开快捷键后取词…'],
])('%o', async (content, hint) => {
  const wrapper = await mountPopup();
  await show(content);
  expect(wrapper.find('.popup__hint').text()).toBe(hint);
});

test('reports the height of each content with its seq', async () => {
  const wrapper = await mountPopup();
  await show({ kind: 'loading' }, 7);
  await show({ kind: 'release_keys' }, 8);
  const reports = commands('popup_measured') as { seq: number; height: number }[];
  expect(reports.map((r) => r.seq)).toEqual([7, 8]);
  for (const report of reports) {
    expect(Number.isInteger(report.height)).toBe(true);
  }
  // Measuring leaves no trace on the page.
  expect(wrapper.find('.popup--measuring').exists()).toBe(false);
});

test('selection: the result, the sentence and the source', async () => {
  const wrapper = await mountPopup();
  await show({
    kind: 'selection',
    text: 'run',
    sentence: 'I run every day.',
    source_app: 'msedge.exe',
    lookup: { kind: 'not_found' },
  });
  expect(wrapper.find('.popup__header').text()).toBe('run');
  expect(wrapper.find('.popup__sentence').text()).toBe('I run every day.');
  expect(wrapper.find('.popup__meta').text()).toContain('来自 msedge.exe');
});

test('selection without a sentence or a source (release builds)', async () => {
  const wrapper = await mountPopup();
  await show({ kind: 'selection', text: 'run', sentence: null, lookup: { kind: 'not_found' } });
  expect(wrapper.find('.popup__sentence').exists()).toBe(false);
  expect(wrapper.find('.popup__meta').text()).not.toContain('来自');
});

test.each(Object.keys(UNAVAILABLE_MESSAGES) as UnavailableReason[])(
  'unavailable: %s',
  async (reason) => {
    const wrapper = await mountPopup();
    await show({ kind: 'unavailable', reason });
    const message = UNAVAILABLE_MESSAGES[reason];
    expect(wrapper.find('.popup__title').text()).toBe(message.title);
    expect(wrapper.find('.popup__hint').text()).toBe(message.hint ?? '');
  },
);

test('error', async () => {
  const wrapper = await mountPopup();
  await show({ kind: 'error', message: 'UIA 初始化失败' });
  expect(wrapper.find('.popup__title').text()).toBe('取词出错');
  expect(wrapper.find('.popup__hint').text()).toBe('UIA 初始化失败');
});

test('the close button hides the popup', async () => {
  const wrapper = await mountPopup();
  await show({ kind: 'loading' });
  await wrapper.find('.popup__close').trigger('click');
  expect(commands('hide_popup')).toHaveLength(1);
});

test('unfolding a section resizes the window', async () => {
  const wrapper = await mountPopup();
  await show({
    kind: 'selection',
    text: 'run',
    sentence: null,
    lookup: {
      kind: 'entries',
      note: null,
      entries: [
        {
          word: 'run',
          relation: null,
          phonetic: null,
          labels: [],
          collins: 0,
          general: [{ pos: 'v.', domain: null, text: '跑' }],
          domain: [{ pos: 'v.', domain: '计算机', text: '运行' }],
          domain_open: false,
          max_lines: null,
        },
      ],
    },
  });
  await wrapper.find('.dict__toggle').trigger('click');
  await flushPromises();
  expect(commands('popup_resize')).toHaveLength(1);
  // No scrollbar until the window has its new height (R8); jsdom never resizes, so the
  // page gives up waiting after 100 ms.
  expect(wrapper.find('.popup--resizing').exists()).toBe(true);
  await new Promise((resolve) => setTimeout(resolve, 150));
  expect(wrapper.find('.popup--resizing').exists()).toBe(false);
});

test('a new lookup starts with every section folded', async () => {
  const lookup: Content = {
    kind: 'selection',
    text: 'run',
    sentence: null,
    lookup: {
      kind: 'entries',
      note: null,
      entries: [
        {
          word: 'run',
          relation: null,
          phonetic: null,
          labels: [],
          collins: 0,
          general: [],
          domain: [{ pos: 'v.', domain: '计算机', text: '运行' }],
          domain_open: false,
          max_lines: null,
        },
      ],
    },
  };
  const wrapper = await mountPopup();
  await show(lookup, 1);
  await wrapper.find('.dict__toggle').trigger('click');
  expect(wrapper.find('.dict__line--domain').exists()).toBe(true);
  await show(lookup, 2);
  expect(wrapper.find('.dict__line--domain').exists()).toBe(false);
});
