import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { flushPromises, mount } from '@vue/test-utils';
import { afterEach, describe, expect, test } from 'vitest';
import HotkeyPanel from './HotkeyPanel.vue';
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
    readOnly: null,
    ...fields,
  };
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
