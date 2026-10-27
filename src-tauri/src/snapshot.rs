//! 导出快照的计划层：只算「应该有哪些文件」，不碰磁盘。
//!
//! 为什么在 Rust 而不是前端：
//!   命名规则、manifest 结构、池键推导都属于数据处理层。放在前端等于让页面层
//!   拥有一份「什么算合法产物」的知识，以后 Rust 写盘时还得再实现一遍 —— 两份
//!   规则迟早对不上。前端只负责把计划画出来。
//!
//! 与磁盘职责的切分：
//!   - 这里：算文件名、编号、扩展名、manifest 内容
//!   - `export.rs`（后续）：照着计划写 .tmp 目录 → rename 替换 → 删旧
//!
//! 这一层不认识 v4 / v5 的差别：入参是「页的最小描述」，v4 全是 native 页，
//! v5 多出 file 页，计划器只关心扩展名和顺序。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Windows 文件名禁用字符
const ILLEGAL: &[char] = &['\\', '/', ':', '*', '?', '"', '<', '>', '|'];
/// 单个文件名（不含扩展名）的字符上限。中文按 1 个字符算，总路径还留得住
const MAX_BASE: usize = 60;
/// 目录名上限
const MAX_DIR: usize = 60;
/// 池键取哈希前多少位。16 位十六进制 = 64 bit，本地资产量级下碰撞可忽略
// 等导出器接上 pool_name 就会用到
#[allow(dead_code)]
const HASH_CHARS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PageKind {
    /// 我们渲染成 docx / pdf
    Native,
    /// 用户原件，逐字节拷贝
    File,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotPage {
    pub id: String,
    #[serde(default)]
    pub label: String,
    pub kind: PageKind,
    /// file 页的原件扩展名（不含点）；native 页忽略
    #[serde(default)]
    pub ext: Option<String>,
    /// file 页原件的 sha256，写进 manifest 供校验
    #[serde(default)]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanInput {
    pub person_id: String,
    pub person_name: String,
    pub resume_id: String,
    pub resume_title: String,
    pub pages: Vec<SnapshotPage>,
    /// 毫秒时间戳；不传就取当前时间。显式传是为了测试可复现
    #[serde(default)]
    pub exported_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotFile {
    /// 文件夹内的文件名，含编号前缀与扩展名
    pub name: String,
    /// 该页在简历里的原始序号，1 起
    pub page_index: usize,
    pub page_id: String,
    pub label: String,
    pub kind: PageKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestPage {
    pub index: usize,
    pub id: String,
    pub label: String,
    pub file: String,
    pub kind: PageKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    /// 结构版本，与 workspace 的 version 无关，只描述 manifest 自身
    pub version: u8,
    pub app: &'static str,
    pub exported_at: u64,
    pub person_id: String,
    pub person_name: String,
    pub resume_id: String,
    pub resume_title: String,
    pub pages: Vec<ManifestPage>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotPlan {
    /// 导出目录名，如「张三 - 外贸单证」
    pub dir_name: String,
    pub files: Vec<SnapshotFile>,
    pub manifest: Manifest,
}

/* ---------- 命名 ---------- */

/// 去掉不可见控制字符；制表/换行留着交给空白压缩
fn strip_control(text: &str) -> String {
    text.chars()
        .filter(|ch| {
            let code = *ch as u32;
            code >= 0x20 || matches!(ch, '\t' | '\n' | '\r')
        })
        .collect()
}

/// 连续空白压成一个半角空格（含全角空格，`is_whitespace` 覆盖 Unicode）
fn squeeze(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !in_space {
                out.push(' ');
                in_space = true;
            }
        } else {
            out.push(ch);
            in_space = false;
        }
    }
    out
}

/// 清理成可用的文件/目录名。
/// Windows 还有两条隐性规则：结尾不能是点或空格，否则资源管理器会截掉。
pub fn clean_name(text: &str) -> String {
    let replaced: String = strip_control(text)
        .chars()
        .map(|ch| if ILLEGAL.contains(&ch) { '-' } else { ch })
        .collect();
    let squeezed = squeeze(&replaced);
    squeezed
        .trim()
        .trim_end_matches(['.', ' '])
        .to_string()
}

fn cut(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut out: String = text.chars().take(limit.saturating_sub(1)).collect();
    out.truncate(out.trim_end().len());
    out.push('…');
    out
}

/// 只留字母数字，防止把奇怪的扩展名带进路径
pub fn clean_ext(ext: Option<&str>, fallback: &str) -> String {
    let value: String = ext
        .unwrap_or("")
        .trim_start_matches('.')
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect();
    if value.is_empty() {
        fallback.to_string()
    } else {
        value
    }
}

/// 文件名：`01 中文简历.docx`。
///
/// 编号是刻意的，而且它顺带解决了重名：文件夹里按名排序就是页序，
/// 同时「两页都叫『证书』」也不会撞名 —— 编号来自下标，天然唯一，
/// 所以这里不需要 `(2)` 之类的避让。
fn file_name(index: usize, label: &str, ext: &str) -> String {
    let cleaned = clean_name(label);
    let base = if cleaned.is_empty() {
        cut(&format!("第 {index} 页"), MAX_BASE)
    } else {
        cut(&cleaned, MAX_BASE)
    };
    // 位置参数 {:02} 不会被自动捕获，index 必须显式传
    format!("{:02} {}.{}", index, base, ext)
}

/* ---------- 资产池键 ---------- */

/// 池内存储名：`a1b2c3d4e5f60718.png`。
///
/// 为什么用哈希而不是「时间戳 + 随机」：
///   1. 同一份文件拖两次只落盘一次，去重是自然结果
///   2. 导出、校验时能直接比对，证明原件没被动过
///   3. 名字与内容绑定，改名不可能让两份内容混在一起
#[allow(dead_code)] // 资产池落盘（export.rs）接上才调用
pub fn pool_name(sha256: &str, ext: Option<&str>) -> Result<String, String> {
    let hash: String = sha256
        .to_lowercase()
        .chars()
        .filter(|ch| ch.is_ascii_hexdigit())
        .take(HASH_CHARS)
        .collect();
    if hash.len() < HASH_CHARS {
        return Err(format!("sha256 不完整：{sha256}"));
    }
    Ok(format!("{}.{}", hash, clean_ext(ext, "bin")))
}

/* ---------- 计划 ---------- */

pub fn plan(input: &PlanInput) -> SnapshotPlan {
    let files: Vec<SnapshotFile> = input
        .pages
        .iter()
        .enumerate()
        .map(|(offset, page)| {
            let index = offset + 1;
            let ext = match page.kind {
                PageKind::File => clean_ext(page.ext.as_deref(), "bin"),
                PageKind::Native => "docx".to_string(),
            };
            SnapshotFile {
                name: file_name(index, &page.label, &ext),
                page_index: index,
                page_id: page.id.clone(),
                label: page.label.clone(),
                kind: page.kind,
                sha256: match page.kind {
                    PageKind::File => page.sha256.clone(),
                    PageKind::Native => None,
                },
            }
        })
        .collect();

    let person_name = {
        let cleaned = clean_name(&input.person_name);
        if cleaned.is_empty() { "未命名".to_string() } else { cleaned }
    };
    let resume_title = {
        let cleaned = clean_name(&input.resume_title);
        if cleaned.is_empty() { "未命名简历".to_string() } else { cleaned }
    };
    let dir_name = cut(&format!("{person_name} - {resume_title}"), MAX_DIR);

    let exported_at = input.exported_at.unwrap_or_else(now_ms);

    let manifest = Manifest {
        version: 1,
        app: "sharp-resume",
        exported_at,
        person_id: input.person_id.clone(),
        person_name: input.person_name.clone(),
        resume_id: input.resume_id.clone(),
        resume_title: input.resume_title.clone(),
        pages: files
            .iter()
            .map(|file| ManifestPage {
                index: file.page_index,
                id: file.page_id.clone(),
                label: file.label.clone(),
                file: file.name.clone(),
                kind: file.kind,
                sha256: file.sha256.clone(),
            })
            .collect(),
    };

    SnapshotPlan { dir_name, files, manifest }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or(0)
}

/// 判断「这个目录是不是本应用生成的快照」。
///
/// 导出要整目录替换，所以必须先有把握才敢删。只认 manifest 里的 `app` 与 `version`，
/// 不认「看起来像」—— 用户把自己的材料放进导出目录是常事，猜错就是删他的文件。
#[allow(dead_code)] // 导出前判断目录归属时调用
pub fn is_our_manifest(value: &Value) -> bool {
    if value.get("app").and_then(Value::as_str) != Some("sharp-resume") {
        return false;
    }
    if value.get("version").and_then(Value::as_u64) != Some(1) {
        return false;
    }
    let Some(pages) = value.get("pages").and_then(Value::as_array) else {
        return false;
    };
    pages.iter().all(|page| {
        page.get("file").map(Value::is_string).unwrap_or(false)
            && page.get("index").map(Value::is_u64).unwrap_or(false)
    })
}

/// 写进 manifest.json 的文本（2 空格缩进 + 结尾换行，diff 友好）
#[allow(dead_code)] // 写 manifest.json 时调用
pub fn manifest_text(manifest: &Manifest) -> Result<String, String> {
    let body = serde_json::to_string_pretty(manifest)
        .map_err(|error| format!("manifest 序列化失败：{error}"))?;
    Ok(format!("{body}\n"))
}

/// 给前端的命令：算一份计划，不落盘
#[tauri::command]
pub fn plan_export(input: PlanInput) -> Result<Value, String> {
    serde_json::to_value(plan(&input)).map_err(|error| format!("计划序列化失败：{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(id: &str, label: &str, kind: PageKind) -> SnapshotPage {
        SnapshotPage { id: id.into(), label: label.into(), kind, ext: None, sha256: None }
    }

    fn file_page(id: &str, label: &str, ext: &str, sha256: &str) -> SnapshotPage {
        SnapshotPage {
            id: id.into(),
            label: label.into(),
            kind: PageKind::File,
            ext: Some(ext.into()),
            sha256: Some(sha256.into()),
        }
    }

    fn run(pages: Vec<SnapshotPage>) -> SnapshotPlan {
        plan(&PlanInput {
            person_id: "p1".into(),
            person_name: "张三".into(),
            resume_id: "r1".into(),
            resume_title: "外贸单证".into(),
            pages,
            exported_at: Some(1_764_000_000_000),
        })
    }

    #[test]
    fn names_are_numbered_and_keep_extension() {
        let result = run(vec![
            page("a", "中文简历", PageKind::Native),
            page("b", "English", PageKind::Native),
            file_page("c", "报关员证书", "PNG", "abc"),
        ]);
        let names: Vec<&str> = result.files.iter().map(|file| file.name.as_str()).collect();
        assert_eq!(names, vec!["01 中文简历.docx", "02 English.docx", "03 报关员证书.png"]);
        assert_eq!(result.files[2].sha256.as_deref(), Some("abc"));
        assert_eq!(result.files[0].sha256, None);
        assert_eq!(result.dir_name, "张三 - 外贸单证");
    }

    #[test]
    fn illegal_chars_and_trailing_dots_are_cleaned() {
        assert_eq!(clean_name(r#"a/b\c:d*e?f"g<h>i|j"#), "a-b-c-d-e-f-g-h-i-j");
        assert_eq!(clean_name("季度报表. "), "季度报表");
        assert_eq!(clean_name("  多  空格  "), "多 空格");
        // 控制字符直接消失，不是变成短横线
        assert_eq!(clean_name("a\u{0}b"), "ab");
    }

    #[test]
    fn empty_labels_fall_back_to_page_number() {
        let result = run(vec![page("a", "", PageKind::Native), page("b", "   ", PageKind::Native)]);
        let names: Vec<&str> = result.files.iter().map(|file| file.name.as_str()).collect();
        assert_eq!(names, vec!["01 第 1 页.docx", "02 第 2 页.docx"]);
    }

    #[test]
    fn same_labels_do_not_collide() {
        let result = run(vec![
            page("a", "证书", PageKind::Native),
            page("b", "证书", PageKind::Native),
            page("c", "证书", PageKind::Native),
        ]);
        let names: Vec<String> = result.files.iter().map(|file| file.name.to_lowercase()).collect();
        assert_eq!(names, vec!["01 证书.docx", "02 证书.docx", "03 证书.docx"]);
        // 真正要保证的是不重名
        let unique: std::collections::HashSet<&String> = names.iter().collect();
        assert_eq!(unique.len(), names.len());
    }

    #[test]
    fn long_labels_are_cut_by_chars_not_bytes() {
        let long = "很长的页名".repeat(30);
        let result = run(vec![page("a", &long, PageKind::Native)]);
        let name = &result.files[0].name;
        assert!(name.ends_with("….docx"), "{name}");
        // 「01 」+ 最多 60 字符 + 扩展名
        assert!(name.chars().count() <= 3 + MAX_BASE + 5, "{name}");
    }

    #[test]
    fn file_pages_without_ext_fall_back_to_bin() {
        let result = run(vec![
            SnapshotPage { id: "a".into(), label: "无后缀".into(), kind: PageKind::File, ext: None, sha256: None },
            SnapshotPage {
                id: "b".into(),
                label: "怪后缀".into(),
                kind: PageKind::File,
                ext: Some("../e?xe".into()),
                sha256: None,
            },
        ]);
        assert_eq!(result.files[0].name, "01 无后缀.bin");
        assert_eq!(result.files[1].name, "02 怪后缀.exe");
    }

    #[test]
    fn dir_name_is_cleaned_and_has_fallback() {
        let result = plan(&PlanInput {
            person_id: "p1".into(),
            person_name: "张/三".into(),
            resume_id: "r1".into(),
            resume_title: String::new(),
            pages: Vec::new(),
            exported_at: Some(1),
        });
        assert_eq!(result.dir_name, "张-三 - 未命名简历");
    }

    #[test]
    fn manifest_matches_files_in_order() {
        let result = run(vec![
            page("a", "一", PageKind::Native),
            file_page("b", "二", "mp4", "def"),
        ]);
        assert_eq!(result.manifest.exported_at, 1_764_000_000_000);
        let from_files: Vec<&String> = result.files.iter().map(|file| &file.name).collect();
        let from_manifest: Vec<&String> = result.manifest.pages.iter().map(|page| &page.file).collect();
        assert_eq!(from_files, from_manifest);
        let indexes: Vec<usize> = result.manifest.pages.iter().map(|page| page.index).collect();
        assert_eq!(indexes, vec![1, 2]);
        assert_eq!(result.manifest.pages[1].sha256.as_deref(), Some("def"));
    }

    #[test]
    fn pool_name_is_hash_prefixed_and_ext_safe() {
        let hash = "A1B2C3D4E5F60718293A4B5C6D7E8F90A1B2C3D4E5F60718293A4B5C6D7E8F90";
        assert_eq!(pool_name(hash, Some("PNG")).unwrap(), "a1b2c3d4e5f60718.png");
        assert_eq!(pool_name(hash, None).unwrap(), "a1b2c3d4e5f60718.bin");
        // 路径片段不会带进名字
        assert_eq!(pool_name(hash, Some(r"..\..\e xe")).unwrap(), "a1b2c3d4e5f60718.exe");
        // 哈希不够长要报错，不能悄悄生成短名（短名会撞车）
        assert!(pool_name("abc", Some("png")).is_err());
    }

    #[test]
    fn ours_manifest_roundtrip() {
        let plan = run(vec![page("a", "一", PageKind::Native)]);
        let text = manifest_text(&plan.manifest).unwrap();
        assert!(text.ends_with("}\n"));
        let value: Value = serde_json::from_str(&text).unwrap();
        assert!(is_our_manifest(&value));

        assert!(!is_our_manifest(&Value::Null));
        assert!(!is_our_manifest(&serde_json::json!({})));
        assert!(!is_our_manifest(&serde_json::json!({ "app": "other", "version": 1, "pages": [] })));
        assert!(!is_our_manifest(&serde_json::json!({ "app": "sharp-resume", "version": 2, "pages": [] })));
        assert!(!is_our_manifest(
            &serde_json::json!({ "app": "sharp-resume", "version": 1, "pages": [{ "file": 1, "index": 1 }] })
        ));
        // 空页列表也算自家产物（刚建、还没导页）
        assert!(is_our_manifest(
            &serde_json::json!({ "app": "sharp-resume", "version": 1, "pages": [] })
        ));
    }
}
