/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** "true" in a debug build run by `tauri dev` or `tauri build --debug`; unset outside Tauri, as in the UI tests. */
  readonly TAURI_ENV_DEBUG?: string;
}
