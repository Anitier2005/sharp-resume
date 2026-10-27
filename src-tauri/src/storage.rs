//! workspace.json 的读写：原子写 + 结构校验 + 损坏兜底。
//!
//! 整份覆盖写最大的风险不是慢，是「写到一半崩了」：
//! `fs::write` 直接截断目标文件，断电时留下的半截 JSON 会让下次启动
//! 解析失败 → 前端 catch 住 → 用户看到一个空工作区。文件还在，内容没了。
//!
//! 所以这里一律走 tmp → fsync → rename：
//!   - 写 tmp 时目标文件完全没动
//!   - rename 在同一分区是原子的，任何时刻要么旧内容要么新内容
//!   - 崩在中间只会留下一个 .tmp 垃圾文件，清理即可
//!
//! 读取侧的另一条原则：**解析失败绝不静默返回空**。把坏文件改名留档再报错，
//! 让用户知道「出事了、原件在哪」，而不是假装这是第一次启动。

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::model::{self, Workspace};

/// 同时只允许一个写入者。多窗口 / 自动保存与手动保存撞在一起时，
/// 两个 tmp 会互相覆盖，rename 的顺序也不可预期。
fn write_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

fn tmp_path(path: &Path) -> PathBuf {
    let name = path.file_name().and_then(|value| value.to_str()).unwrap_or("state.json");
    path.with_file_name(format!(".{name}.tmp-{}", std::process::id()))
}

/// 原子替换。失败时清掉 tmp，不留下半成品。
///
/// 目前 workspace 已改由 SQLite 承载，这个函数留给「落盘不可事务化的东西」：
/// 导出快照、资产池里的原件、manifest。它们没有数据库兜底，只能靠 rename 保证原子性。
#[allow(dead_code)]
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let _guard = write_lock().lock().map_err(|_| "写入锁不可用".to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建目录失败：{error}"))?;
    }

    let tmp = tmp_path(path);
    let write = (|| -> std::io::Result<()> {
        let mut file = File::create(&tmp)?;
        file.write_all(bytes)?;
        // 必须先落盘再 rename：否则断电时可能 rename 成功但内容还在页缓存里，
        // 得到一个「存在但为空」的文件 —— 比没写更糟
        file.sync_all()?;
        Ok(())
    })();

    if let Err(error) = write {
        let _ = fs::remove_file(&tmp);
        return Err(format!("写入临时文件失败：{error}"));
    }

    // Windows 上 std 的 rename 走 MoveFileEx + REPLACE_EXISTING，可以覆盖已存在文件
    if let Err(error) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(format!("替换文件失败：{error}"));
    }
    Ok(())
}

/// 把坏文件挪到一边留档，名字带时间戳，不覆盖历史
fn quarantine(path: &Path) -> Option<PathBuf> {
    let stamp = crate::snapshot::now_ms();
    let name = path.file_name().and_then(|value| value.to_str()).unwrap_or("state.json");
    let target = path.with_file_name(format!("{name}.corrupt-{stamp}"));
    fs::rename(path, &target).ok().map(|_| target)
}

/// 读取原始文本。文件不存在返回 None。
///
/// v4 会做一次强类型解析 + 校验（结构坏了就报错，别让它带着病往下走）；
/// 更低版本原样返回，交给前端迁移 —— 迁移逻辑目前仍在前端，等整体搬到 Rust 再收口。
pub fn read_raw(path: &Path) -> Result<Option<String>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(path).map_err(|error| format!("读取工作区失败：{error}"))?;
    if text.trim().is_empty() {
        return Ok(None);
    }

    let value: serde_json::Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(error) => {
            let backup = quarantine(path);
            let hint = match backup {
                Some(target) => format!("已备份为 {}", target.display()),
                None => "备份失败，请手动检查该文件".to_string(),
            };
            return Err(format!("工作区文件损坏（{error}）。{hint}"));
        }
    };

    let version = value.get("version").and_then(serde_json::Value::as_u64).unwrap_or(0);
    if version != 4 {
        // 老数据：不在这里迁移，原样交出去
        return Ok(Some(text));
    }

    let workspace: Workspace = serde_json::from_value(value)
        .map_err(|error| format!("工作区结构不符合 v4：{error}"))?;
    model::validate(&workspace)?;
    // 重新序列化：把解析时补上的默认值（accent/mode 等）定下来，
    // 未知字段通过 extra 原样带回
    serde_json::to_string(&workspace)
        .map(Some)
        .map_err(|error| format!("工作区序列化失败：{error}"))
}

/// 写入：先解析校验，再原子落盘。
///
/// 生产路径已经改走 SQLite（`db::save`），这里只剩测试在用：
/// 造一份「合法的老文件」来验证导入与损坏兜底。
#[cfg(test)]
pub fn write_json(path: &Path, json: &str) -> Result<(), String> {
    if json.trim().is_empty() {
        // 清空 = 删文件（前端「清空工作区」走这条）
        if path.exists() {
            fs::remove_file(path).map_err(|error| format!("删除工作区失败：{error}"))?;
        }
        return Ok(());
    }

    let workspace: Workspace =
        serde_json::from_str(json).map_err(|error| format!("拒绝写入：结构不合法（{error}）"))?;
    if workspace.version != 4 {
        return Err(format!("拒绝写入：只接受 version 4，收到 {}", workspace.version));
    }
    model::validate(&workspace)?;

    let body = serde_json::to_string(&workspace)
        .map_err(|error| format!("序列化失败：{error}"))?;
    atomic_write(path, body.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sharp-resume-{tag}-{}-{}",
            std::process::id(),
            crate::snapshot::now_ms()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample() -> String {
        serde_json::json!({
            "version": 4,
            "people": [{
                "id": "p1",
                "fields": [{ "id": "f1", "label": "姓名", "value": "张三" }],
                "items": [],
                "messages": [],
                "resumes": [{
                    "id": "r1",
                    "personId": "p1",
                    "title": "简历",
                    "pages": [{ "id": "pg1", "label": "一", "header": [], "sections": [] }]
                }]
            }],
            "ui": { "accent": "white", "mode": "light", "openPages": ["pg1"] }
        })
        .to_string()
    }

    #[test]
    fn atomic_write_leaves_no_tmp() {
        let dir = temp_dir("atomic");
        let file = dir.join("workspace.json");
        atomic_write(&file, b"{\"a\":1}").unwrap();
        atomic_write(&file, b"{\"a\":2}").unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "{\"a\":2}");
        let leftovers: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupted_file_is_quarantined_not_silently_empty() {
        let dir = temp_dir("corrupt");
        let file = dir.join("workspace.json");
        fs::write(&file, "{\"version\": 4, \"people\": [ 半截").unwrap();

        let error = read_raw(&file).unwrap_err();
        assert!(error.contains("损坏"), "{error}");
        // 原文件被挪走留档，而不是消失
        assert!(!file.exists());
        let backups: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.contains(".corrupt-"))
            .collect();
        assert_eq!(backups.len(), 1, "{backups:?}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rejects_broken_structure_instead_of_writing_it() {
        let dir = temp_dir("reject");
        let file = dir.join("workspace.json");
        // 页面 id 重复 → 结构不合法
        let broken = serde_json::json!({
            "version": 4,
            "people": [{
                "id": "p1",
                "resumes": [{
                    "id": "r1",
                    "personId": "p1",
                    "pages": [{ "id": "dup" }, { "id": "dup" }]
                }]
            }],
            "ui": {}
        })
        .to_string();
        let error = write_json(&file, &broken).unwrap_err();
        assert!(error.contains("页面 id 重复"), "{error}");
        assert!(!file.exists(), "校验失败不该生成文件");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn roundtrip_keeps_unknown_fields_and_defaults() {
        let dir = temp_dir("roundtrip");
        let file = dir.join("workspace.json");
        let mut value: serde_json::Value = serde_json::from_str(&sample()).unwrap();
        // 前端将来的字段
        value["ui"]["futureDock"] = serde_json::json!("right");
        write_json(&file, &value.to_string()).unwrap();

        let text = read_raw(&file).unwrap().expect("应有内容");
        let back: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(back["ui"]["futureDock"], serde_json::json!("right"));
        assert_eq!(back["ui"]["openPages"][0], serde_json::json!("pg1"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn old_version_is_returned_as_is_for_frontend_migration() {
        let dir = temp_dir("old");
        let file = dir.join("workspace.json");
        fs::write(&file, r#"{"version":2,"people":[]}"#).unwrap();
        let text = read_raw(&file).unwrap().unwrap();
        assert!(text.contains("\"version\":2"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_write_removes_file() {
        let dir = temp_dir("clear");
        let file = dir.join("workspace.json");
        atomic_write(&file, b"x").unwrap();
        write_json(&file, "").unwrap();
        assert!(!file.exists());
        fs::remove_dir_all(&dir).ok();
    }

    /// 拿本机真实数据跑一遍，确认强类型不会误杀用户已经存在的文件。
    /// 手动跑：cargo test --lib real_workspace -- --ignored --nocapture
    #[test]
    #[ignore = "依赖本机 target/debug/SharpResumeData/workspace.json"]
    fn real_workspace_from_disk_passes_validation() {
        let path = Path::new("target/debug/SharpResumeData/workspace.json.imported");
        if !path.exists() {
            eprintln!("跳过：没有 {path:?}");
            return;
        }
        let text = read_raw(path).expect("真实数据应该能读").expect("应该有内容");
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        let people = value["people"].as_array().map(Vec::len).unwrap_or(0);
        eprintln!(
            "people={people} version={} bytes={}",
            value["version"],
            text.len()
        );
        // 往返再读一次必须稳定（幂等）
        let dir = temp_dir("real");
        let copy = dir.join("workspace.json");
        write_json(&copy, &text).expect("真实数据应该能写");
        let again = read_raw(&copy).unwrap().unwrap();
        let a: serde_json::Value = serde_json::from_str(&text).unwrap();
        let b: serde_json::Value = serde_json::from_str(&again).unwrap();
        assert_eq!(a, b, "往返后结构应该完全一致");
        fs::remove_dir_all(&dir).ok();
    }
}
