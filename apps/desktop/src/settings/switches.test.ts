import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { flushPromises, mount } from '@vue/test-utils';
import { afterEach, describe, expect, test } from 'vitest';
import HotkeyPanel from './HotkeyPanel.vue';
import DictionaryPanel from './DictionaryPanel.vue';
import type { SettingsView } from './ipc';
import PrivacyPanel from './PrivacyPanel.vue';

function settingsView(fields: Partial<SettingsView> = {}): SettingsView {
  return {
    lookup: { accelerator: 'Alt+Space', display: 'Alt + Space' },
    lookupOff: false,
    lookupActive: true,
    lookupError: null,
    defaultLookup: { accelerator: 'Alt+Space', display: 'Alt + Space' },
    doubleCtrlC: false,
    recordHistory: true,
    dictionaries: {
      folder: 'C:\\Users\\me\\AppData\\Local\\io.github.aspeternity.lextrail\\dicts',
      folderIsDefault: true,
      oxford: false,
      englishDefinitions: false,
      status: 'missing',
      statusText: '没有找到牛津词典',
    },
    readOnly: null,
    ...fields,
  };
}

/** `settingsView()` with some dictionary fields changed. */
function withDictionaries(fields: Partial<SettingsView['dictionaries']>): SettingsView {
  const view = settingsView();
  return { ...view, dictionaries: { ...view.dictionaries, ...fields } };
}

afterEach(() => {
  clearMocks();
});

// Each switch: its id, the component, the Rust command and argument it sends when
// switched on, and the settings field it shows (inverted for "不记录查询历史").
const SWITCHES = [
  {
    name: '连按两次 Ctrl+C',
    id: '#double-ctrl-c',
    error: '#double-ctrl-c-error',
    component: HotkeyPanel,
    command: 'settings_set_double_ctrl_c',
    on: { on: true },
    after: settingsView({ doubleCtrlC: true }),
  },
  {
    name: '不记录查询历史',
    id: '#no-history',
    error: '#history-error',
    component: PrivacyPanel,
    command: 'settings_set_record_history',
    on: { record: false },
    after: settingsView({ recordHistory: false }),
  },
  {
    name: '使用牛津词典',
    id: '#dict-oxford',
    error: '#dict-error',
    component: DictionaryPanel,
    command: 'settings_set_oxford',
    on: { on: true },
    after: withDictionaries({ oxford: true }),
  },
  {
    name: '显示英文释义',
    id: '#dict-english',
    error: '#dict-error',
    component: DictionaryPanel,
    command: 'settings_set_english_definitions',
    on: { on: true },
    after: withDictionaries({ englishDefinitions: true }),
  },
] as const;

describe.each(SWITCHES)('$name', (s) => {
  test('switching on sends the setting to Rust', async () => {
    const calls: { cmd: string; args: unknown }[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      return s.after;
    });
    const wrapper = mount(s.component, { props: { settings: settingsView() } });
    const box = wrapper.find<HTMLInputElement>(s.id);
    expect(box.element.checked).toBe(false);
    await box.setValue(true);
    await flushPromises();
    expect(calls).toEqual([{ cmd: s.command, args: s.on }]);
    expect(wrapper.emitted('update')?.[0]).toEqual([s.after]);
    expect(wrapper.find(s.error).exists()).toBe(false);
  });

  test('a failure puts the switch back and shows the error', async () => {
    mockIPC(() => {
      throw '无法写入设置文件';
    });
    const wrapper = mount(s.component, { props: { settings: settingsView() } });
    const box = wrapper.find<HTMLInputElement>(s.id);
    await box.setValue(true);
    await flushPromises();
    expect(box.element.checked).toBe(false);
    expect(wrapper.find(s.error).text()).toBe('无法写入设置文件');
    expect(wrapper.emitted('update')).toBeUndefined();
  });

  test('disabled when the settings are read-only', () => {
    const wrapper = mount(s.component, {
      props: { settings: settingsView({ readOnly: '设置文件有错误' }) },
    });
    expect(wrapper.find(s.id).attributes('disabled')).toBeDefined();
  });
});

test('"不记录查询历史" shows the opposite of recordHistory', () => {
  const wrapper = mount(PrivacyPanel, {
    props: { settings: settingsView({ recordHistory: false }) },
  });
  expect(wrapper.find<HTMLInputElement>('#no-history').element.checked).toBe(true);
});

describe('扩展词典文件夹', () => {
  test('shows the folder and the status from Rust', () => {
    const wrapper = mount(DictionaryPanel, { props: { settings: settingsView() } });
    expect(wrapper.find('#dict-folder').text()).toContain('io.github.aspeternity.lextrail');
    expect(wrapper.text()).toContain('默认位置');
    expect(wrapper.find('#dict-status').text()).toBe('没有找到牛津词典');
    // Already the default folder.
    expect(wrapper.find('#dict-reset').attributes('disabled')).toBeDefined();
  });

  test('a usable dictionary is shown as such, an unusable one as an error', () => {
    const ready = mount(DictionaryPanel, {
      props: {
        settings: withDictionaries({ status: 'ready', statusText: '牛津 · 3 个词条' }),
      },
    });
    expect(ready.find('#dict-status').classes()).toContain('message--ok');
    const unusable = mount(DictionaryPanel, {
      props: { settings: withDictionaries({ status: 'unusable', statusText: '无法使用' }) },
    });
    expect(unusable.find('#dict-status').classes()).toContain('message--error');
  });

  test('"选择文件夹…" and "恢复默认" ask Rust', async () => {
    const chosen = withDictionaries({ folder: 'D:\\LexTrailData\\dicts', folderIsDefault: false });
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      return cmd === 'settings_pick_dict_folder' ? chosen : settingsView();
    });
    const wrapper = mount(DictionaryPanel, { props: { settings: settingsView() } });
    await wrapper.find('#dict-pick').trigger('click');
    await flushPromises();
    expect(wrapper.emitted('update')?.[0]).toEqual([chosen]);
    await wrapper.setProps({ settings: chosen });
    expect(wrapper.find('#dict-reset').attributes('disabled')).toBeUndefined();
    await wrapper.find('#dict-reset').trigger('click');
    await flushPromises();
    expect(calls).toEqual(['settings_pick_dict_folder', 'settings_reset_dict_folder']);
  });

  test('a failure is shown', async () => {
    mockIPC(() => {
      throw '无法写入设置文件';
    });
    const wrapper = mount(DictionaryPanel, { props: { settings: settingsView() } });
    await wrapper.find('#dict-pick').trigger('click');
    await flushPromises();
    expect(wrapper.find('#dict-error').text()).toBe('无法写入设置文件');
    expect(wrapper.emitted('update')).toBeUndefined();
  });
});
