use serde::Serialize;

#[derive(Serialize)]
struct AppInfo {
    name: &'static str,
    version: &'static str,
    data_dir: String,
}

/// 返回应用信息与本地数据目录，前端用于状态栏与「打开目录」。
#[tauri::command]
fn app_info(app: tauri::AppHandle) -> AppInfo {
    use tauri::Manager;

    let data_dir = app
        .path()
        .app_data_dir()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_default();

    AppInfo {
        name: "Sharp Resume",
        version: env!("CARGO_PKG_VERSION"),
        data_dir,
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![app_info])
        .run(tauri::generate_context!())
        .expect("error while running Sharp Resume");
}
