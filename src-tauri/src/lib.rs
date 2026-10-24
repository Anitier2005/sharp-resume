use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tauri::{LogicalSize, Manager};

/// 便携数据目录名：与 exe 同级，便于整目录拷贝 / 备份 / 放 U 盘。
const DATA_DIR_NAME: &str = "SharpResumeData";

/// 数据目录只解析一次，避免每次保存都重新探测可写性。
static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

#[derive(Serialize)]
struct AppInfo {
    name: &'static str,
    version: &'static str,
    data_dir: String,
    portable: bool,
}

/// 把窗口收进当前显示器的可用范围。
///
/// tauri.conf.json 里写的是逻辑像素，在 125% / 150% 缩放的笔记本上换算成
/// 物理像素后会超出屏幕，窗口底部被裁掉且无法拖动。这里按实际显示器尺寸收缩。
fn clamp_to_monitor(window: &tauri::WebviewWindow) {
    let Ok(Some(monitor)) = window.current_monitor() else {
        return;
    };
    let scale = monitor.scale_factor();
    let screen = monitor.size().to_logical::<f64>(scale);

    // 减去任务栏与窗口边缘的安全余量
    let max_w = (screen.width - 24.0).max(1120.0);
    let max_h = (screen.height - 96.0).max(720.0);

    let Ok(current) = window.inner_size() else {
        return;
    };
    let current = current.to_logical::<f64>(scale);

    let width = current.width.min(max_w);
    let height = current.height.min(max_h);

    if (width - current.width).abs() > 1.0 || (height - current.height).abs() > 1.0 {
        let _ = window.set_size(LogicalSize::new(width.round(), height.round()));
        let _ = window.center();
    }
}

/// 可执行文件旁边的相邻数据目录。
fn portable_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join(DATA_DIR_NAME))
}

/// 目录可写性探测：能建目录、能写入探针文件才算可写。
///
/// 只判断「目录存在」不够 —— Program Files、只读介质、写保护 U 盘都可能
/// 让目录存在却写不进去，那样保存会静默失败、用户丢数据。
fn is_writable(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(format!(".writable-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"probe").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// 解析数据目录：优先 exe 旁（便携），不可写时回退系统应用数据目录。
///
/// 这里刻意**不做**「从旧位置自动导入 workspace.json」。
/// 便携版的语义是：数据只跟着 exe 所在目录走，换到新目录就是一个全新实例。
/// 曾加过自动迁移，结果在发布包里凭空出现了一份旧示例数据 —— 与「干净安装」直接冲突。
/// 需要搬旧数据时，用户手动把 workspace.json 放进 SharpResumeData 即可。
fn resolve_data_dir(app: &tauri::AppHandle) -> Result<(PathBuf, bool), String> {
    if let Some(dir) = portable_dir() {
        if is_writable(&dir) {
            return Ok((dir, true));
        }
    }
    let fallback = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("无法定位数据目录：{error}"))?;
    Ok((fallback, false))
}

/// 取数据目录（带缓存）。
fn data_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    if let Some(dir) = DATA_DIR.get() {
        return Ok(dir.clone());
    }
    let (dir, _) = resolve_data_dir(app)?;
    Ok(DATA_DIR.get_or_init(|| dir).clone())
}

fn state_file(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("workspace.json"))
}

/// 返回应用信息与数据目录，前端用于状态栏与「打开目录」。
#[tauri::command]
fn app_info(app: tauri::AppHandle) -> AppInfo {
    let (dir, portable) = match DATA_DIR.get() {
        Some(cached) => {
            let is_portable = portable_dir().as_deref() == Some(cached.as_path());
            (cached.clone(), is_portable)
        }
        None => resolve_data_dir(&app).unwrap_or_else(|_| (PathBuf::new(), false)),
    };

    AppInfo {
        name: "Sharp Resume",
        version: env!("CARGO_PKG_VERSION"),
        data_dir: dir.to_string_lossy().to_string(),
        portable,
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

/// 在文件管理器里打开数据目录，方便用户备份 / 拷走。
#[tauri::command]
fn reveal_data_dir(app: tauri::AppHandle) -> Result<(), String> {
    let dir = data_dir(&app)?;
    std::fs::create_dir_all(&dir).map_err(|error| format!("创建目录失败：{error}"))?;

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&dir)
            .spawn()
            .map_err(|error| format!("打开目录失败：{error}"))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        std::process::Command::new(opener)
            .arg(&dir)
            .spawn()
            .map_err(|error| format!("打开目录失败：{error}"))?;
    }
    Ok(())
}

/// 把文本写到用户选择的任意路径（导出 Markdown 用）。
#[tauri::command]
fn save_text_file(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, content).map_err(|error| format!("写入文件失败：{error}"))
}

/// 把二进制写到用户选择的任意路径（导出 DOCX 用；前端传字节数组）。
#[tauri::command]
fn save_binary_file(path: String, data: Vec<u8>) -> Result<(), String> {
    std::fs::write(&path, data).map_err(|error| format!("写入文件失败：{error}"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            // 先确定数据目录并建好，用户双击后就能在 exe 旁看到它。
            let handle = app.handle().clone();
            if let Ok((dir, portable)) = resolve_data_dir(&handle) {
                let _ = std::fs::create_dir_all(&dir);
                if !portable {
                    // 回退到系统目录说明 exe 所在位置不可写，值得留痕。
                    eprintln!(
                        "[storage] exe 所在目录不可写，数据回退到 {}",
                        dir.display()
                    );
                }
                let _ = DATA_DIR.set(dir);
            }

            if let Some(window) = app.get_webview_window("main") {
                clamp_to_monitor(&window);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            read_state,
            write_state,
            reveal_data_dir,
            save_text_file,
            save_binary_file
        ])
        .run(tauri::generate_context!())
        .expect("error while running Sharp Resume");
}
