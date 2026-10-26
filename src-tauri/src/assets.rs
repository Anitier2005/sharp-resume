//! 材料文件：图片、证书扫描件、作品附件都落到 `<数据目录>/assets/`。
//!
//! 为什么不塞进 workspace.json：
//!   1. base64 会让 JSON 膨胀 33%，一张 2MB 的证书能把状态文件撑到 3MB，
//!      而这份文件每次改动都要整体重写
//!   2. 便携版拷走 SharpResumeData 时，附件必须跟着走 —— 所以放在同一个根目录下
//!   3. 前端只在要看的时候取一次，取完即可释放，内存里不留全量
//!
//! 文件名用内容无关的 id：重名不覆盖、也不把用户的文件名拼进路径。

use std::path::PathBuf;

fn assets_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = crate::data_dir(app)?.join("assets");
    std::fs::create_dir_all(&dir).map_err(|error| format!("建 assets 目录失败：{error}"))?;
    Ok(dir)
}

/// 只留后缀，其余全部丢掉 —— 防目录穿越，也让文件名不可猜
fn safe_ext(name: &str) -> String {
    let ext = name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .take(8)
        .collect::<String>();
    if ext.is_empty() {
        "bin".to_string()
    } else {
        ext
    }
}

/// 前端生成的 id（时间戳 + 随机串），这里再兜一道：只允许字母数字与短横线
fn safe_id(id: &str) -> String {
    let clean = id
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
        .take(64)
        .collect::<String>();
    if clean.is_empty() {
        "asset".to_string()
    } else {
        clean
    }
}

/// 写入一个材料文件，返回存储名（如 `a-1f2e-9ab.png`），前端存这个名即可
#[tauri::command]
pub fn asset_put(app: tauri::AppHandle, id: String, name: String, data: Vec<u8>) -> Result<String, String> {
    let dir = assets_dir(&app)?;
    let stored = format!("{}.{}", safe_id(&id), safe_ext(&name));
    let path = dir.join(&stored);
    std::fs::write(&path, data).map_err(|error| format!("写入材料失败：{error}"))?;
    Ok(stored)
}

/// 读回一个材料文件（前端转 blob URL 显示）
#[tauri::command]
pub fn asset_get(app: tauri::AppHandle, stored: String) -> Result<Vec<u8>, String> {
    let dir = assets_dir(&app)?;
    let name = PathBuf::from(&stored)
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .ok_or_else(|| "材料名不合法".to_string())?;
    std::fs::read(dir.join(name)).map_err(|error| format!("读取材料失败：{error}"))
}

/// 删除一个材料文件（条目被删时调用；文件不在也不报错）
#[tauri::command]
pub fn asset_drop(app: tauri::AppHandle, stored: String) -> Result<(), String> {
    let dir = assets_dir(&app)?;
    let name = PathBuf::from(&stored)
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .ok_or_else(|| "材料名不合法".to_string())?;
    let path = dir.join(name);
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("删除材料失败：{error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ext_is_sanitized() {
        assert_eq!(safe_ext("证书 2024.PNG"), "png");
        assert_eq!(safe_ext("noext"), "noext");
        assert_eq!(safe_ext("a.b/c"), "bc");
    }

    #[test]
    fn id_strips_path_chars() {
        assert_eq!(safe_id("../../etc/passwd"), "etcpasswd");
        assert_eq!(safe_id(""), "asset");
    }
}
