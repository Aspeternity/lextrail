import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { CONTENT_EVENT, type Content, type ContentEvent } from './content';

/** Calls a Rust command; failures are logged instead of disappearing. */
export function call(command: string, args?: Record<string, unknown>): void {
  invoke(command, args).catch((error: unknown) => {
    console.error(`[popup] invoke ${command} failed:`, error);
  });
}

/**
 * Subscribes to content events, then tells Rust the page is ready so the window may be
 * shown. A failed subscription is reported to Rust (and logged), and the window is still
 * allowed to appear, so the failure is visible rather than a silent dead hotkey.
 */
export async function subscribe(onContent: (content: Content, seq: number) => void): Promise<void> {
  let error: string | null = null;
  try {
    await listen<ContentEvent>(CONTENT_EVENT, (event) => {
      onContent(event.payload.content, event.payload.seq);
    });
  } catch (e: unknown) {
    error = String(e);
    console.error('[popup] listen failed:', e);
  }
  call('popup_ready', { error });
}
