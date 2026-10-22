import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils';
import { afterEach, beforeEach, describe, expect, test } from 'vitest';
import HotkeyRecorder from './HotkeyRecorder.vue';
import type { ApplyView, CheckView, Modifiers, SettingsView } from './ipc';

const ALT_SPACE = { accelerator: 'Alt+Space', display: 'Alt + Space' };
const CTRL_K = { accelerator: 'Ctrl+KeyK', display: 'Ctrl + K' };

function settingsView(fields: Partial<SettingsView> = {}): SettingsView {
  return {
    lookup: ALT_SPACE,
    lookupOff: false,
    lookupActive: true,
    lookupError: null,
    defaultLookup: ALT_SPACE,
    doubleCtrlC: false,
    recordHistory: true,
    dictionaries: {
      folder: '',
      folderIsDefault: true,
      oxford: true,
      englishDefinitions: true,
      status: 'missing',
      statusText: '没有找到牛津词典',
    },
    readOnly: null,
    ...fields,
  };
}

/**
 * A stand-in for the Rust commands. `check` answers `hotkey_check`, `apply` answers
 * `hotkey_apply`; a function that throws makes the command fail with that message.
 */
let backend: {
  check: (code: string, keys: Modifiers) => CheckView | null;
  apply: (accelerator: string) => ApplyView;
};
let calls: { cmd: string; args: Record<string, unknown> }[];

beforeEach(() => {
  calls = [];
  backend = {
    check: (code, keys) =>
      code === 'KeyK' && keys.ctrl ? { hotkey: CTRL_K, blocked: null } : null,
    apply: (accelerator) => ({
      outcome: 'changed',
      message: null,
      settings:
        accelerator === ''
          ? settingsView({ lookup: null, lookupOff: true })
          : settingsView({ lookup: accelerator === CTRL_K.accelerator ? CTRL_K : ALT_SPACE }),
    }),
  };
  mockIPC((cmd, payload) => {
    const args = (payload ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args });
    switch (cmd) {
      case 'hotkey_recording':
        return settingsView({ lookupActive: !args.active });
      case 'hotkey_check':
        return backend.check(args.code as string, args.keys as Modifiers);
      case 'hotkey_apply':
        return backend.apply(args.accelerator as string);
    }
    throw new Error(`unexpected command ${cmd}`);
  });
});

let wrapper: VueWrapper | null = null;

afterEach(() => {
  wrapper?.unmount();
  wrapper = null;
  clearMocks();
  document.body.innerHTML = '';
});

/** Mounts the recorder; like the settings page, it shows the settings it last emitted. */
function mountRecorder(settings: SettingsView = settingsView()) {
  const mounted = mount(HotkeyRecorder, {
    props: { settings },
    attachTo: document.body,
  });
  wrapper = mounted;
  return mounted;
}

async function follow(mounted: VueWrapper): Promise<void> {
  await flushPromises();
  const emitted = mounted.emitted<[SettingsView]>('update');
  const last = emitted?.[emitted.length - 1];
  if (last) {
    await mounted.setProps({ settings: last[0] });
  }
}

const recorder = (w: VueWrapper) => w.find('#hotkey-recorder');
const message = (w: VueWrapper) => w.find('#hotkey-message');

function key(
  type: 'keydown' | 'keyup',
  code: string,
  modifiers: Partial<Record<'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey', boolean>> = {},
): void {
  window.dispatchEvent(new KeyboardEvent(type, { code, bubbles: true, ...modifiers }));
}

async function startRecording(w: VueWrapper): Promise<void> {
  await recorder(w).trigger('click');
  await follow(w);
}

function commands(name: string) {
  return calls.filter((c) => c.cmd === name).map((c) => c.args);
}

describe('status line', () => {
  test('the current hotkey, in effect', () => {
    const w = mountRecorder();
    expect(recorder(w).text()).toBe('Alt + Space');
    expect(message(w).text()).toBe('已生效');
    expect(message(w).classes()).toContain('message--ok');
  });

  test('registration failed', () => {
    const w = mountRecorder(settingsView({ lookupActive: false, lookupError: '已被占用' }));
    expect(message(w).text()).toBe('未生效：已被占用');
    expect(message(w).classes()).toContain('message--error');
  });

  test.each([
    [false, '不使用快捷键，可在下面打开"连按两次 Ctrl+C"查词'],
    [true, '不使用快捷键，可连按两次 Ctrl+C 查词'],
  ])('no hotkey, double Ctrl+C %s', (doubleCtrlC, text) => {
    const w = mountRecorder(
      settingsView({ lookup: null, lookupOff: true, lookupActive: false, doubleCtrlC }),
    );
    expect(recorder(w).text()).toBe('不使用');
    expect(message(w).text()).toBe(text);
    expect(message(w).classes()).toContain('message--ok');
    expect(w.find('#hotkey-off').attributes('disabled')).toBeDefined();
  });

  test('read-only settings disable every button', () => {
    const w = mountRecorder(settingsView({ readOnly: '设置文件有错误' }));
    for (const id of ['#hotkey-recorder', '#hotkey-default', '#hotkey-off']) {
      expect(w.find(id).attributes('disabled')).toBeDefined();
    }
  });

  test('"恢复默认" only when the hotkey is not the default', () => {
    expect(mountRecorder().find('#hotkey-default').attributes('disabled')).toBeDefined();
    wrapper?.unmount();
    const w = mountRecorder(settingsView({ lookup: CTRL_K }));
    expect(w.find('#hotkey-default').attributes('disabled')).toBeUndefined();
  });
});

describe('recording', () => {
  test('a click starts recording and pauses the hotkey', async () => {
    const w = mountRecorder();
    await startRecording(w);
    expect(commands('hotkey_recording')).toEqual([{ active: true }]);
    expect(recorder(w).text()).toBe('请按下新的快捷键（Esc 取消）');
    expect(recorder(w).classes()).toContain('hotkey__recorder--recording');
    expect(message(w).text()).toBe('录制中，快捷键暂停使用');
    expect(document.activeElement).toBe(recorder(w).element);
  });

  test('held modifiers are shown', async () => {
    const w = mountRecorder();
    await startRecording(w);
    key('keydown', 'ControlLeft', { ctrlKey: true });
    key('keydown', 'ShiftLeft', { ctrlKey: true, shiftKey: true });
    await flushPromises();
    expect(recorder(w).text()).toBe('Ctrl + Shift + …');
  });

  test('a combination is checked by Rust and applied', async () => {
    const w = mountRecorder();
    await startRecording(w);
    key('keydown', 'ControlLeft', { ctrlKey: true });
    key('keydown', 'KeyK', { ctrlKey: true });
    await follow(w);
    expect(commands('hotkey_check').at(-1)).toEqual({
      code: 'KeyK',
      keys: { ctrl: true, alt: false, shift: false, meta: false },
    });
    expect(commands('hotkey_apply')).toEqual([{ accelerator: 'Ctrl+KeyK' }]);
    expect(recorder(w).text()).toBe('Ctrl + K');
    expect(message(w).text()).toBe('已改为 Ctrl + K，已生效');
    expect(recorder(w).classes()).not.toContain('hotkey__recorder--recording');
  });

  test('keys reaching the page are swallowed', async () => {
    await startRecording(mountRecorder());
    const event = new KeyboardEvent('keydown', { code: 'F5', cancelable: true, bubbles: true });
    window.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
  });

  test('Esc cancels and turns the hotkey back on', async () => {
    const w = mountRecorder();
    await startRecording(w);
    key('keydown', 'Escape');
    await follow(w);
    expect(commands('hotkey_recording')).toEqual([{ active: true }, { active: false }]);
    expect(commands('hotkey_apply')).toEqual([]);
    expect(recorder(w).text()).toBe('Alt + Space');
    expect(message(w).text()).toBe('已生效');
  });

  test('Esc with a modifier is a combination, not a cancel', async () => {
    backend.check = () => ({ hotkey: CTRL_K, blocked: 'Esc 不能使用' });
    const w = mountRecorder();
    await startRecording(w);
    key('keydown', 'Escape', { ctrlKey: true });
    await follow(w);
    expect(commands('hotkey_recording')).toEqual([{ active: true }]);
    expect(commands('hotkey_check')).toHaveLength(1);
  });

  test('a blocked combination keeps recording and says why', async () => {
    backend.check = () => ({ hotkey: CTRL_K, blocked: 'Ctrl+C 不能使用' });
    const w = mountRecorder();
    await startRecording(w);
    key('keydown', 'KeyC', { ctrlKey: true });
    await follow(w);
    expect(commands('hotkey_apply')).toEqual([]);
    expect(message(w).text()).toBe('Ctrl+C 不能使用');
    expect(message(w).classes()).toContain('message--error');
    expect(recorder(w).classes()).toContain('hotkey__recorder--recording');
  });

  test('modifiers pressed and released with no key arriving', async () => {
    const w = mountRecorder();
    await startRecording(w);
    key('keydown', 'AltLeft', { altKey: true });
    await flushPromises();
    key('keyup', 'AltLeft');
    await flushPromises();
    expect(message(w).text()).toBe('按下没有反应，可能被别的程序占用');
  });

  test('repeated key presses are ignored', async () => {
    await startRecording(mountRecorder());
    window.dispatchEvent(
      new KeyboardEvent('keydown', { code: 'KeyK', ctrlKey: true, repeat: true }),
    );
    await flushPromises();
    expect(commands('hotkey_check')).toEqual([]);
  });

  test('the recorder losing focus cancels', async () => {
    const w = mountRecorder();
    await startRecording(w);
    await recorder(w).trigger('blur');
    await follow(w);
    expect(commands('hotkey_recording')).toEqual([{ active: true }, { active: false }]);
    expect(recorder(w).text()).toBe('Alt + Space');
  });

  test('starting fails: stays idle with the error', async () => {
    mockIPC((cmd) => {
      calls.push({ cmd, args: {} });
      throw '设置文件被占用';
    });
    const w = mountRecorder();
    await startRecording(w);
    expect(recorder(w).text()).toBe('Alt + Space');
    expect(message(w).text()).toBe('设置文件被占用');
    key('keydown', 'KeyK', { ctrlKey: true });
    await flushPromises();
    expect(commands('hotkey_check')).toEqual([]);
  });
});

describe('apply outcomes', () => {
  async function record(apply: ApplyView['outcome'], text: string | null) {
    backend.apply = () => ({ outcome: apply, message: text, settings: settingsView() });
    const w = mountRecorder();
    await startRecording(w);
    key('keydown', 'KeyK', { ctrlKey: true });
    await follow(w);
    return w;
  }

  test('register failed', async () => {
    const w = await record('register_failed', '已被占用');
    expect(message(w).text()).toBe('Ctrl + K 注册失败（已被占用），仍用原来的快捷键');
    expect(message(w).classes()).toContain('message--error');
  });

  test('save failed', async () => {
    const w = await record('save_failed', '无法写入设置文件');
    expect(message(w).text()).toBe('无法写入设置文件，仍用原来的快捷键');
  });

  test('blocked by Rust', async () => {
    const w = await record('blocked', 'Ctrl+Insert 不能使用');
    expect(message(w).text()).toBe('Ctrl+Insert 不能使用');
  });

  test('unchanged: back to the status', async () => {
    const w = await record('unchanged', null);
    expect(message(w).text()).toBe('已生效');
  });
});

test('"不使用" turns the hotkey off', async () => {
  const w = mountRecorder();
  await w.find('#hotkey-off').trigger('click');
  await follow(w);
  expect(commands('hotkey_apply')).toEqual([{ accelerator: '' }]);
  expect(recorder(w).text()).toBe('不使用');
  expect(message(w).text()).toBe('不使用快捷键，可在下面打开"连按两次 Ctrl+C"查词');
});

test('"恢复默认" applies the default hotkey', async () => {
  const w = mountRecorder(settingsView({ lookup: CTRL_K }));
  await w.find('#hotkey-default').trigger('click');
  await follow(w);
  expect(commands('hotkey_apply')).toEqual([{ accelerator: 'Alt+Space' }]);
  expect(message(w).text()).toBe('已改为 Alt + Space，已生效');
});

test('unmounting while recording turns the hotkey back on', async () => {
  const w = mountRecorder();
  await startRecording(w);
  w.unmount();
  wrapper = null;
  await flushPromises();
  expect(commands('hotkey_recording')).toEqual([{ active: true }, { active: false }]);
});
