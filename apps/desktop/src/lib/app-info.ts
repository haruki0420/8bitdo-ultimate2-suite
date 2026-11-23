import { invoke } from "@tauri-apps/api/core";

/** Payload returned by the `app_info` command in `src-tauri/src/lib.rs`. */
export interface AppInfo {
  name: string;
  version: string;
  tauriVersion: string;
}

/** Reads the application name/version from the Rust side over the Tauri IPC. */
export async function getAppInfo(): Promise<AppInfo> {
  return invoke<AppInfo>("app_info");
}
