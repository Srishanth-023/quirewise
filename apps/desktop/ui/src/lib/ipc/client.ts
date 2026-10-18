import { invoke } from '@tauri-apps/api/core';

/** Returns the application version reported by the Rust core. */
export function appVersion(): Promise<string> {
  return invoke<string>('app_version');
}
