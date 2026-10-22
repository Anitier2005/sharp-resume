use serde::Serialize;
use tauri::Manager;

#[derive(Serialize)]
struct AppInfo {
    name: &'static str,
    version: &'static str,
    data_dir: String,
}

fn state_file(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("无法定位数据目录：{error}"))?;
    Ok(dir.join("workspace.json"))
}

/// 返回应用信息与本地数据目录，前端用于状态栏与「打开目录」。
#[tauri::command]
fn app_info(app: tauri::AppHandle) -> AppInfo {
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

/// 读取工作区 JSON；文件不存在时返回 None。
#[tauri::command]
fn read_state(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let file = state_file(&app)?;
    if !file.exists() {
        return Ok(None);
    }
    std::fs::read_to_string(file)
        .map(Some)
        .map_err(|error| format!("读取工作区失败：{error}"))
}

/// 写入工作区 JSON（前端自动保存调用）。
#[tauri::command]
fn write_state(app: tauri::AppHandle, json: String) -> Result<(), String> {
    let file = state_file(&app)?;
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("创建目录失败：{error}"))?;
    }
    std::fs::write(file, json).map_err(|error| format!("写入工作区失败：{error}"))
}

/// 把文本写到用户选择的任意路径（导出 Markdown 用）。
#[tauri::command]
fn save_text_file(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, content).map_err(|error| format!("写入文件失败：{error}"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
            app_info,
            read_state,
            write_state,
            save_text_file
        ])
        .run(tauri::generate_context!())
        .expect("error while running Sharp Resume");
}
