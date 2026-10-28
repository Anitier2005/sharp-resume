mod agent;
mod assets;
mod db;
mod docx;
mod export;
mod extract;
mod model;
mod net;
mod pool;
mod render;
mod session;
mod snapshot;
mod storage;
mod tools;

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

/// UI 字号的可读下限：初版设计主体只有 9–11.5px，偏小。
/// 用户没动系统设置时也用这个值，保证默认就是可读的。
const MIN_TEXT_SCALE: f64 = 1.2;

/// 读取 Windows「设置 → 辅助功能 → 文本大小」的缩放系数。
///
/// 这个设置独立于显示缩放（DPI）：DPI 由 WebView2 自动处理，
/// 而文本大小只会改注册表里的 TextScaleFactor，网页不会自动跟随，必须自己读。
/// 未设置时返回 1.0（100%）。
#[cfg(target_os = "windows")]
fn system_text_scale() -> f64 {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(key) = hkcu.open_subkey(r"Software\Microsoft\Accessibility") else {
        return 1.0;
    };
    let Ok(raw) = key.get_value::<u32, _>("TextScaleFactor") else {
        return 1.0;
    };
    // 有效区间 100–225；越界视为未设置，避免脏注册表把界面撑爆
    if !(100..=225).contains(&raw) {
        return 1.0;
    }
    raw as f64 / 100.0
}

#[cfg(not(target_os = "windows"))]
fn system_text_scale() -> f64 {
    1.0
}

/// 返回系统文本缩放，前端据此决定 UI 字号倍率。
#[tauri::command]
fn text_scale() -> f64 {
    system_text_scale().max(MIN_TEXT_SCALE)
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
///
/// `pub(crate)`：会话文件也要落在同一个目录 —— 便携版拷走整个 SharpResumeData
/// 时，简历和对话必须一起走，不能一个在 exe 旁边、一个在 %APPDATA%。
pub(crate) fn data_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
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

/// 读取工作区。
///
/// 旧版是一份 workspace.json，现在是 SQLite。顺序：
///   1. 数据库里有内容 → 直接返回（权威）
///   2. 数据库是空的、但旁边躺着老的 workspace.json → 导进来，然后把老文件
///      改名成 `workspace.json.imported` 留档。不改名的话每次启动都会重导一遍，
///      用户在数据库里做的改动会被旧文件「复活」。
///
/// **v1 到 v5 的升级只在这里做一次**：`model::adopt` 一条路走到底，前端拿到的
/// 永远是当前版本的完整结构。前端曾经也留着一份同样的迁移逻辑，两份规则迟早会漂移
/// （实际已经漂移过一次：年限字段在前端保住了，在后端被当成空串丢掉）。
#[tauri::command]
fn read_state(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let dir = data_dir(&app)?;
    let mut conn = db::open(&dir.join("app.db"))?;
    if let Some(workspace) = db::load(&conn)? {
        // 内容已经升到 v5，但 meta 里的版本号可能还停在 4（老库第一次被新版本打开）。
        // 不追平的话，库里会出现「写着 v4、实际是 v5」的状态，排查问题时最容易被它骗。
        if db::version(&conn).unwrap_or(0) < model::VERSION {
            db::save(&mut conn, &workspace)?;
        }
        return serde_json::to_string(&workspace)
            .map(Some)
            .map_err(|error| format!("工作区序列化失败：{error}"));
    }

    let legacy = state_file(&app)?;
    if !legacy.exists() {
        return Ok(None);
    }
    let Some(text) = storage::read_raw(&legacy)? else {
        return Ok(None);
    };
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| format!("工作区文件不是合法 JSON：{error}"))?;

    let workspace = model::adopt(value)?;
    model::validate(&workspace)?;
    db::save(&mut conn, &workspace)?;

    let backup = legacy.with_file_name("workspace.json.imported");
    std::fs::rename(&legacy, &backup)
        .map_err(|error| format!("导入成功，但归档旧文件失败：{error}"))?;

    serde_json::to_string(&workspace)
        .map(Some)
        .map_err(|error| format!("工作区序列化失败：{error}"))
}

/// 写入工作区（前端自动保存调用）。
///
/// 空字符串 = 清空（前端「清空工作区」走这条）。
/// 非空时先解析 + 校验，不合法直接拒绝：宁可这次保存失败，
/// 也不把坏数据写进库 —— 那会连累之后每一次读取。
#[tauri::command]
fn write_state(
    app: tauri::AppHandle,
    state: tauri::State<'_, db::Shared>,
    json: String,
) -> Result<(), String> {
    if json.trim().is_empty() {
        return state.with(|conn| db::clear(conn));
    }

    let workspace: model::Workspace = serde_json::from_str(&json)
        .map_err(|error| format!("拒绝写入：结构不合法（{error}）"))?;
    // 版本号不在这里锁死。以前写的是「只接受 version 4」，
    // 结果前端一升到 v5，每一次自动保存都被拒绝，用户改什么都存不进去。
    // 该拦的是「比本程序更新」的数据 —— 那件事 `validate` 已经在做。
    model::validate(&workspace)?;
    state.with(|conn| db::save(conn, &workspace))?;

    // 保存后同步资产引用：某个文件从「有人用」变成「没人用」时挪进回收站。
    // 放在这里而不是删除时：前端只管把引用从 state 里摘掉，文件回收是后端的事。
    let _ = pool::sync(&app, &state, &workspace);
    Ok(())
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
/// WebView2 运行时的固定 GUID（微软官方 EdgeUpdate 客户端 ID）
#[cfg(target_os = "windows")]
const WEBVIEW2_CLIENT: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";

/// 检测本机是否装了 WebView2 运行时。
///
/// Tauri 的窗口本身由 WebView2 渲染：没有运行时时进程会直接崩掉，
/// 用户看到的是"双击没反应"或闪一下就没了——这种静默失败最难排查，
/// 所以在创建任何窗口之前先自己检查一次，给出能看懂的提示。
///
/// 注册表位置有三处，缺一不可：系统级 64 位视图、系统级 32 位视图
/// （WOW6432Node，安装器在 x64 上也常写这里）、以及当前用户级安装
/// （Edge 自带的不进 HKLM，只写 HKCU）。
#[cfg(target_os = "windows")]
fn webview2_version() -> Option<String> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    const BASES: [&str; 3] = [
        r"SOFTWARE\Microsoft\EdgeUpdate\Clients",
        r"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients",
        r"SOFTWARE\Microsoft\EdgeUpdate\Clients",
    ];

    for hive in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        for base in BASES {
            let root = RegKey::predef(hive);
            let Ok(clients) = root.open_subkey(base) else {
                continue;
            };
            let Ok(key) = clients.open_subkey(WEBVIEW2_CLIENT) else {
                continue;
            };
            let Ok(pv) = key.get_value::<String, _>("pv") else {
                continue;
            };
            // 卸载后残留的键会把 pv 写成 0.0.0.0，不予采信
            let trimmed = pv.trim();
            if !trimmed.is_empty() && trimmed != "0.0.0.0" {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// 用系统消息框告知缺少 WebView2，并询问是否打开下载页。
///
/// 这里刻意不用 Tauri 的 dialog 插件：窗口都建不起来时插件还没初始化，
/// 只能直接调 user32 / shell32。
#[cfg(target_os = "windows")]
fn report_missing_webview2() {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    const MB_YESNO: u32 = 0x0000_0004;
    const MB_ICONERROR: u32 = 0x0000_0010;
    const IDYES: i32 = 6;

    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(hwnd: *mut core::ffi::c_void, text: *const u16, caption: *const u16, utype: u32) -> i32;
    }
    #[link(name = "shell32")]
    extern "system" {
        fn ShellExecuteW(
            hwnd: *mut core::ffi::c_void,
            op: *const u16,
            file: *const u16,
            params: *const u16,
            dir: *const u16,
            show: i32,
        ) -> *mut core::ffi::c_void;
    }

    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    let url = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";
    let text = format!(
        "缺少 Microsoft Edge WebView2 运行时，界面无法显示。\n\n\
         这是 Windows 10 常见情况（Windows 11 自带）。\n\
         点「是」将打开微软官方下载页，安装后重新打开本程序即可。\n\n\
         手动下载：{url}"
    );

    let text = wide(&text);
    let caption = wide("Sharp Resume 无法启动");
    let flags = MB_YESNO | MB_ICONERROR;

    let answer = unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            flags,
        )
    };

    if answer == IDYES {
        let op = wide("open");
        let file = wide(url);
        unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                op.as_ptr(),
                file.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1, // SW_SHOWNORMAL
            );
        }
    }
}

/// 启动前的环境自检：缺 WebView2 就提示并退出，不留白窗或静默闪退。
#[cfg(target_os = "windows")]
fn ensure_webview2() {
    if webview2_version().is_some() {
        return;
    }
    report_missing_webview2();
    std::process::exit(1);
}

#[cfg(not(target_os = "windows"))]
fn ensure_webview2() {}

pub fn run() {
    // 必须最先执行：窗口一旦创建就依赖 WebView2，届时再检查已经来不及
    ensure_webview2();

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
                // 导出、资产池这些命令通过 State 拿连接。不注册的话，
                // 命令一被调用就是「state not managed」panic —— 编译期看不出来。
                app.manage(db::Shared::new(dir.join("app.db")));
                let _ = DATA_DIR.set(dir);
            }

            if let Some(window) = app.get_webview_window("main") {
                clamp_to_monitor(&window);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            text_scale,
            read_state,
            write_state,
            reveal_data_dir,
            save_text_file,
            save_binary_file,
            net::chat_once,
            net::chat_stream,
            session::chat_history,
            session::chat_push,
            session::chat_patch,
            session::chat_clear,
            session::chat_cancel,
            session::chat_send,
            agent::layout::layout_submit,
            render::compile_page,
            assets::asset_put,
            assets::asset_get,
            assets::asset_drop,
            snapshot::plan_export,
            export::export_resume,
            pool::pool_put,
            pool::pool_get,
            pool::pool_list,
            extract::extract_pdf,
            extract::ocr_image
        ])
        .manage(session::Sessions::default())
        // 排版信箱：每页一份，跨命令存活。放在 manage 里而不是每次新建，
        // 否则「同一个页面同时只跑一个」这个约束就不存在了
        .manage(agent::layout::Layouts::default())
        .run(tauri::generate_context!())
        .expect("error while running Sharp Resume");
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(tag: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir()
            .join("sharp-resume-lib-test")
            .join(format!("{tag}-{stamp}"));
        std::fs::create_dir_all(&dir).expect("建临时目录");
        dir
    }

    fn sample_v5() -> serde_json::Value {
        json!({
            "version": 5,
            "people": [{
                "id": "p1",
                "fields": [{ "id": "f1", "label": "姓名", "value": "张三" }],
                "items": [],
                "messages": [],
                "resumes": [{
                    "id": "r1",
                    "personId": "p1",
                    "title": "简历",
                    "updatedAt": 0,
                    "pages": [{
                        "id": "pg1",
                        "kind": "native",
                        "label": "中文",
                        "blocks": [{
                            "id": "b1",
                            "kind": "fields",
                            "fields": [
                                { "id": "f2", "label": "姓名", "value": "张三" },
                                { "id": "f3", "label": "职位", "value": "工程师" }
                            ]
                        }]
                    }]
                }]
            }],
            "ui": { "accent": "white" }
        })
    }

    /// 模拟前端的一次自动保存：v5 的工作区发过来，必须能存进去、再原样读回来。
    ///
    /// 这个用例针对一个真实事故：`write_state` 曾经写死「只接受 version 4」，
    /// 前端升到 v5 之后每一次保存都被拒绝，而前端只 console.error ——
    /// 界面看着一切正常，重启就回到旧数据。
    #[test]
    fn v5_state_survives_save_and_reload() {
        let dir = temp_dir("save");
        let mut conn = db::open(&dir.join("app.db")).expect("建库");

        let workspace = model::adopt(sample_v5()).expect("v5 要能采纳");
        let payload = serde_json::to_string(&workspace).unwrap();
        assert!(payload.contains("\"version\":5"), "前端发出去的就是 v5");

        // write_state 走的路径：解析 → 校验 → 落库
        let incoming: model::Workspace = serde_json::from_str(&payload).expect("解析");
        model::validate(&incoming).expect("v5 必须能通过校验，不能因为版本号被拒");
        db::save(&mut conn, &incoming).expect("v5 必须能存进库");

        let loaded = db::load(&conn).unwrap().expect("库里应当有数据");
        assert_eq!(loaded.version, 5);
        assert_eq!(loaded.people.len(), 1);
        let fields = &loaded.people[0].resumes[0].pages[0].blocks[0].fields;
        assert_eq!(fields[0].value, "张三");
        assert_eq!(fields[1].value, "工程师");

        // 改一个字段再存一次 —— 这才是用户真正会做的事
        let mut edited = loaded;
        edited.people[0].resumes[0].pages[0].blocks[0].fields[0].value = "李四".into();
        let payload = serde_json::to_string(&edited).unwrap();
        let incoming: model::Workspace = serde_json::from_str(&payload).expect("解析");
        model::validate(&incoming).expect("改完还要能过校验");
        db::save(&mut conn, &incoming).expect("改完还要能存");

        let again = db::load(&conn).unwrap().unwrap();
        assert_eq!(
            again.people[0].resumes[0].pages[0].blocks[0].fields[0].value,
            "李四",
            "改完的字必须在库里"
        );
        // 没动过的字段不能被顺手清掉
        assert_eq!(again.people[0].resumes[0].pages[0].blocks[0].fields[1].value, "工程师");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 比本程序更新的数据要明确拒绝，而不是悄悄存进去
    #[test]
    fn newer_version_is_rejected_with_a_readable_message() {
        let mut raw = sample_v5();
        raw["version"] = json!(99);
        let workspace: model::Workspace = serde_json::from_value(raw).expect("解析");
        let error = model::validate(&workspace).expect_err("更高版本应当被拒");
        assert!(error.contains("99"), "错误里要带上版本号：{error}");
        assert!(error.contains("只到 v5"), "要说明本程序支持到哪一版：{error}");
    }
}
