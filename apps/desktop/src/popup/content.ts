// Mirrors `Content` in src-tauri/src/popup/content.rs (serde tag "kind").
export type Content =
  | { kind: 'empty' }
  | { kind: 'loading' }
  | { kind: 'release_keys' }
  | {
      kind: 'selection';
      text: string;
      sentence: string | null;
      /** Debug builds only: the program captured from. Release builds never send it. */
      source_app?: string;
      /** `null` when the dictionary file is missing. */
      lookup: LookupView | null;
    }
  | { kind: 'unavailable'; reason: UnavailableReason }
  | { kind: 'error'; message: string };

// Mirrors `LookupView` in crates/lextrail-dict/src/view.rs.
export type LookupView =
  | { kind: 'entries'; note: string | null; entries: EntryView[] }
  | { kind: 'parts'; parts: PartView[]; not_found: string[] }
  | { kind: 'not_found' }
  | { kind: 'sentence' }
  | { kind: 'not_english' };

export interface EntryView {
  word: string;
  relation: string | null;
  phonetic: string | null;
  labels: string[];
  collins: number;
  general: SenseLine[];
  domain: SenseLine[];
  /** Domain senses start unfolded (the entry has no other senses). */
  domain_open: boolean;
  /** General lines shown before "更多"; `null` for all. */
  max_lines: number | null;
}

export interface SenseLine {
  pos: string | null;
  domain: string | null;
  text: string;
}

export interface PartView {
  word: string;
  relation: string | null;
  phonetic: string | null;
  line: SenseLine | null;
}

export type UnavailableReason =
  | 'no_selection'
  | 'elevated'
  | 'elevated_double_copy'
  | 'no_foreground_window'
  | 'unsupported'
  | 'timed_out'
  | 'keys_held';

export const CONTENT_EVENT = 'popup:content';

/** The `popup:content` event (`ContentEvent` in content.rs): the page reports `seq` back
 * with the content's height. */
export interface ContentEvent {
  seq: number;
  content: Content;
}

export const UNAVAILABLE_MESSAGES: Record<UnavailableReason, { title: string; hint?: string }> = {
  no_selection: { title: '没有选中文字', hint: '先选中要查的英文，再按快捷键。' },
  elevated: {
    title: '无法读取以管理员身份运行的程序',
    // Double Ctrl+C still works there: the program writes the clipboard itself (D012, R1-9).
    hint: 'Windows 不允许普通程序读取以管理员身份运行的窗口中的文字。可以改用连按两次 Ctrl+C 查词（需先在设置中打开）。',
  },
  // The same, with double Ctrl+C already switched on (D012 supplement).
  elevated_double_copy: {
    title: '无法读取以管理员身份运行的程序',
    hint: 'Windows 不允许普通程序读取以管理员身份运行的窗口中的文字。可以改用连按两次 Ctrl+C 查词。',
  },
  no_foreground_window: { title: '没有找到当前窗口', hint: '切换到要查词的窗口后再试。' },
  unsupported: { title: '这个窗口暂时无法取词', hint: '这个程序没有提供选中的文字。' },
  timed_out: { title: '取词超时', hint: '程序响应较慢，请稍后再试。' },
  keys_held: { title: '请松开快捷键后再试', hint: '按住快捷键时无法复制选中的文字。' },
};
