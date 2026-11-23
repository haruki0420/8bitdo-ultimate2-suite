use serde::Serialize;

/// Display name of the application (kept in sync with `productName` in
/// `tauri.conf.json`).
const APP_NAME: &str = "Padbench";

/// Information surfaced by the placeholder "about" badge in the UI.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// User-facing application name.
    name: String,
    /// Application version, taken from the crate manifest.
    version: String,
    /// Tauri runtime version linked into the binary.
    tauri_version: String,
}

/// Returns static application metadata.  This is the only command for now; BLE
/// device I/O is intentionally not implemented in this scaffold.
#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo {
        name: APP_NAME.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        tauri_version: tauri::VERSION.to_string(),
    }
}

/// Entry point shared by the desktop binary and (later) mobile targets.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![app_info])
        .run(tauri::generate_context!())
        .expect("error while running the Tauri application");
}
