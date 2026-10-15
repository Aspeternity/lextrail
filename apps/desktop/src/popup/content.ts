// Mirrors `Content` in src-tauri/src/popup/content.rs (serde tag "kind").
export type Content =
  | { kind: 'empty' }
  | { kind: 'loading' }
  | { kind: 'selection'; text: string; sentence: string | null; source_app: string | null }
  | { kind: 'unavailable'; reason: UnavailableReason }
  | { kind: 'error'; message: string };

export type UnavailableReason =
  'no_selection' | 'elevated' | 'no_foreground_window' | 'unsupported' | 'timed_out';

export const CONTENT_EVENT = 'popup:content';

export const UNAVAILABLE_MESSAGES: Record<UnavailableReason, { title: string; hint?: string }> = {
  no_selection: { title: '没有选中文字', hint: '先选中要查的英文，再按快捷键。' },
  elevated: {
    title: '无法读取以管理员身份运行的程序',
    hint: 'Windows 不允许普通程序读取管理员窗口中的文字。可以改用"连按两次 Ctrl+C"查词（在设置中开启）。',
  },
  no_foreground_window: { title: '没有找到当前窗口', hint: '切换到要查词的窗口后再试。' },
  unsupported: {
    title: '这个窗口暂时无法取词',
    hint: '可以先复制文字，再用"连按两次 Ctrl+C"查词。',
  },
  timed_out: { title: '取词超时', hint: '程序响应较慢，请稍后再试。' },
};
