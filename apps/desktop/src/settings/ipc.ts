import { invoke } from '@tauri-apps/api/core';

/** A hotkey as Rust formats it: canonical form and display text. */
export interface HotkeyView {
  accelerator: string;
  display: string;
}

/** Everything the settings page shows (`SettingsView` in settings_window.rs). */
export interface SettingsView {
  lookup: HotkeyView | null;
  lookupActive: boolean;
  lookupError: string | null;
  defaultLookup: HotkeyView;
  doubleCtrlC: boolean;
  recordHistory: boolean;
  readOnly: string | null;
}

/** A recorded key press checked by Rust (`CheckView`). */
export interface CheckView {
  hotkey: HotkeyView;
  blocked: string | null;
  warnings: string[];
}

export type ApplyOutcome =
  'changed' | 'unchanged' | 'blocked' | 'needs_confirmation' | 'register_failed' | 'save_failed';

export interface ApplyView {
  outcome: ApplyOutcome;
  message: string | null;
  warnings: string[];
  settings: SettingsView;
}

export interface Modifiers {
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  meta: boolean;
}

export function modifiersOf(event: KeyboardEvent): Modifiers {
  return {
    ctrl: event.ctrlKey,
    alt: event.altKey,
    shift: event.shiftKey,
    meta: event.metaKey,
  };
}

export const getSettings = (): Promise<SettingsView> => invoke('settings_get');

/** Starts recording (Rust turns the hotkey off) or cancels it (Rust turns it back on). */
export const setRecording = (active: boolean): Promise<SettingsView> =>
  invoke('hotkey_recording', { active });

/** `null`: a modifier or an unsupported key; keep recording. */
export const checkKey = (code: string, keys: Modifiers): Promise<CheckView | null> =>
  invoke('hotkey_check', { code, keys });

export const applyHotkey = (accelerator: string, confirmed: boolean): Promise<ApplyView> =>
  invoke('hotkey_apply', { accelerator, confirmed });

export const setDoubleCtrlC = (on: boolean): Promise<SettingsView> =>
  invoke('settings_set_double_ctrl_c', { on });

export const setRecordHistory = (record: boolean): Promise<SettingsView> =>
  invoke('settings_set_record_history', { record });

/** The message of a rejected command (Rust returns a string). */
export function errorText(error: unknown): string {
  return typeof error === 'string' ? error : String(error);
}
