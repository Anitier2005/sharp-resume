//! 会话附件：**上传即入库，解析在后端**。
//!
//! 为什么不再放在前端做：
//!   1. 前端只该管「怎么显示」。`file.text()`、PDF 取字、图片 OCR 散在 WebView 里，
//!      刷新一次缓存就没了，浏览器调试模式与桌面模式还各有一套分支；
//!   2. 同一份文件在「附件」和「材料」两条路上被解析两次，行为迟早分叉；
//!   3. 模型上下文是一段字符串，拼它的人越少越不容易错位。
//!
//! 存储位置与材料池分开：`<数据目录>/inbox/`。用户拖进对话的一份 PDF 不是
//! 他的「材料」，不该出现在材料清单里，也不该被材料池的引用回收逻辑扫到。
//! 文件名按内容寻址（sha 前 16 位 + 扩展名）：同一份文件拖两次只落一份。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

/// 前端发来的附件引用。`name` 是用户看见的原文件名，`stored` 是 inbox 里的名字
#[derive(Debug, Clone, Deserialize)]
pub struct AttachRef {
    #[serde(default)]
    pub stored: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub ext: String,
}

/// 附件上传后的回执
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachInfo {
    pub stored: String,
    pub name: String,
    pub ext: String,
    /// text / image / file
    pub kind: String,
    /// 解析出来的字数（图片为 0，取字失败也为 0）
    pub chars: usize,
    /// PDF 提不出文字 = 扫描件，要等 OCR
    pub scanned: bool,
}

/// OCR 接口配置。地址/密钥留空表示沿用对话模型那把
#[derive(Debug, Clone, Default, Deserialize)]
pub struct OcrConf {
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub model: String,
}

struct Parsed {
    text: String,
    scanned: bool,
    note: String,
    /// PDF 的页数（F-005 的唯一来源）。读不出 = None，**绝不写 0**
    pages: Option<u32>,
}

/// `materials()` 的产物：材料段 + 本轮附给视觉模型的图 + 原件信息
pub struct Materials {
    pub section: String,
    pub images: Vec<String>,
    /// 本轮第一个 PDF 附件。`pages: None` = 读不出（损坏 / 不是 PDF）
    pub origin: Option<crate::agent::facts::Origin>,
}

/// 解析结果缓存。键是内容寻址的 stored 名，同一份内容永远不会读出错的结果
fn cache() -> &'static Mutex<HashMap<String, Parsed>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Parsed>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 单个附件进上下文的字数上限：200KB 字符足够一份简历，
/// 再长的东西该让模型用工具分段读，而不是一次塞进 prompt
const MAX_CHARS: usize = 200 * 1024;
/// 一次请求最多几张图（含 PDF 页面）：再多 token 和请求体都撑不住
const MAX_IMAGES: usize = 4;
/// 附件里最多渲染几页
const MAX_PAGES: usize = 3;

/// 纯文本类扩展名。和 `lib/attach.ts` 那张表是同一批，只是判断点挪到了这里
const TEXT_EXTS: &[&str] = &[
    "txt", "text", "md", "markdown", "csv", "tsv", "json", "log", "yml", "yaml", "html", "htm",
    "xml", "css", "ts", "tsx", "js", "jsx", "py", "java", "go", "rs", "sql",
];

const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif"];

pub fn is_image(ext: &str) -> bool {
    IMAGE_EXTS.contains(&ext.to_ascii_lowercase().as_str())
}

fn inbox_dir(root: &Path) -> PathBuf {
    root.join("inbox")
}

/// 存一份附件并顺手解析。前端选完文件就调它，发送时只传 stored 名字。
///
/// `data_base64` 是 base64：附件动辄几 MB，走 JSON 数字数组要膨胀三四倍，
/// IPC 会明显卡顿。
#[tauri::command]
pub fn attach_put(
    app: tauri::AppHandle,
    name: String,
    data_base64: String,
) -> Result<AttachInfo, String> {
    use base64::Engine as _;
    let data = base64::engine::general_purpose::STANDARD
        .decode(data_base64.as_bytes())
        .map_err(|error| format!("附件解码失败：{error}"))?;
    if data.is_empty() {
        return Err(format!("「{name}」是空文件"));
    }
    let root = crate::data_dir(&app)?;
    let dir = inbox_dir(&root);
    std::fs::create_dir_all(&dir).map_err(|error| format!("建附件目录失败：{error}"))?;

    let ext = name
        .rsplit_once('.')
        .map(|(_, tail)| tail.to_ascii_lowercase())
        .unwrap_or_default();
    let stored = crate::snapshot::pool_name(&crate::pool::sha256_hex(&data), Some(&ext))?;
    let path = dir.join(&stored);
    if !path.exists() {
        crate::storage::atomic_write(&path, &data)?;
    }

    let parsed = parse(&ext, &data);
    let kind = kind_of(&ext);
    let info = AttachInfo {
        stored: stored.clone(),
        name,
        ext,
        kind,
        chars: parsed.text.chars().count(),
        scanned: parsed.scanned,
    };
    if let Ok(mut map) = cache().lock() {
        map.insert(stored, parsed);
    }
    Ok(info)
}

/// 取回附件原字节。历史消息里的缩略图靠它 —— preview 的 object URL 刷新就没了。
///
/// 返回值走 `ipc::Response`（原始字节，JS 拿到 ArrayBuffer），
/// 不用 `Vec<u8>`：那会被序列化成一串数字，几 MB 的图光传输就要好几秒。
#[tauri::command]
pub fn attach_get(app: tauri::AppHandle, stored: String) -> Result<tauri::ipc::Response, String> {
    let root = crate::data_dir(&app)?;
    Ok(tauri::ipc::Response::new(read(&root, &stored)?))
}

fn kind_of(ext: &str) -> String {
    let ext = ext.to_ascii_lowercase();
    if is_image(&ext) {
        "image".into()
    } else if ext == "pdf" || TEXT_EXTS.contains(&ext.as_str()) {
        "text".into()
    } else {
        "file".into()
    }
}

pub(crate) fn read(root: &Path, stored: &str) -> Result<Vec<u8>, String> {
    // stored 由后端生成（sha + 扩展名），不接受前端拼路径
    let name = crate::pool::safe_stored(stored)?;
    std::fs::read(inbox_dir(root).join(name)).map_err(|error| format!("附件已不在：{error}"))
}

/// 解析：文本直读、PDF 取字、图片留给 OCR。纯函数，缓存由调用方管
fn parse(ext: &str, data: &[u8]) -> Parsed {
    let ext = ext.to_ascii_lowercase();
    if is_image(&ext) {
        return Parsed {
            text: String::new(),
            scanned: false,
            note: String::new(),
            pages: None,
        };
    }
    if ext == "pdf" {
        // 页数只有这一处算：读不出就是 None，不要写 0（写成 0 会被当成「原件是空文档」）
        let pages = crate::pdfembed::page_count(data);
        return match pdf_extract::extract_text_from_mem(data) {
            Ok(text) if !text.trim().is_empty() => Parsed {
                text: clip(text.trim()),
                scanned: false,
                note: String::new(),
                pages,
            },
            // 提不出字 ≠ 坏文件：扫描件天生没有文字层，这条要留给 OCR
            _ => Parsed {
                text: String::new(),
                scanned: true,
                note: String::new(),
                pages,
            },
        };
    }
    if TEXT_EXTS.contains(&ext.as_str()) {
        let text = String::from_utf8_lossy(data).to_string();
        return Parsed {
            text: clip(text.trim()),
            scanned: false,
            note: String::new(),
            pages: None,
        };
    }
    Parsed {
        text: String::new(),
        scanned: false,
        note: format!("（.{} 暂不支持解析 —— 目前能读 PDF / 文本 / 图片）", ext),
        pages: None,
    }
}

fn clip(text: &str) -> String {
    text.chars().take(MAX_CHARS).collect()
}

/// 组装本轮的材料段与图片。
///
/// 这是「解析下沉」的落点：`chat_send` 只拿到一串 stored 名字，
/// 取字、OCR、渲染全在这里做，前端不再碰任何字节。
pub async fn materials(
    root: &Path,
    refs: &[AttachRef],
    vision: bool,
    ocr: Option<&OcrConf>,
) -> Materials {
    let mut blocks: Vec<String> = Vec::new();
    let mut images: Vec<String> = Vec::new();
    // 只取本轮第一个 PDF：原件页数是「这一轮上传了什么」这么一件事，不是清单
    let mut first_pdf: Option<(String, Option<u32>)> = None;

    for file in refs {
        let Some(stored) = non_empty(&file.stored) else {
            continue;
        };
        let name = if file.name.is_empty() { stored } else { &file.name };
        let ext = if file.ext.is_empty() {
            stored.rsplit_once('.').map(|(_, tail)| tail).unwrap_or_default().to_string()
        } else {
            file.ext.to_ascii_lowercase()
        };
        let bytes = match read(root, stored) {
            Ok(bytes) => bytes,
            Err(error) => {
                if ext == "pdf" && first_pdf.is_none() {
                    first_pdf = Some((name.to_string(), None));
                }
                blocks.push(format!("（附件 {name}：{error}）"));
                continue;
            }
        };
        let parsed = cached(stored, &ext, &bytes);
        if ext == "pdf" && first_pdf.is_none() {
            first_pdf = Some((name.to_string(), parsed.pages));
        }

        if is_image(&ext) {
            if vision {
                images.push(crate::pdfimage::data_url_of(&mime_of(&ext), &bytes));
                blocks.push(format!(
                    "──── {name} ────\n（图片已作为视觉输入附上，说明文字只描述版式，不要复述画面内容）\n──── 文件结束 ────"
                ));
            } else {
                let text = ocr_text(&bytes, &ext, ocr).await;
                blocks.push(format!("──── {name} ────\n{text}\n──── 文件结束 ────"));
            }
            continue;
        }

        // 能看图的模型：PDF 再补上前几页，版式只能看图
        let pages = if ext == "pdf" && vision {
            render_pages(&bytes, MAX_PAGES)
        } else {
            Vec::new()
        };
        let note = if pages.is_empty() {
            String::new()
        } else {
            images.extend(pages.iter().cloned());
            format!("\n（另附前 {} 页的图片，版式看图片）", pages.len())
        };

        // 扫描件 + 看不了图：只剩 OCR 这一条路，否则这份文件在模型眼里是空的
        let text = if parsed.text.is_empty() && parsed.scanned && !vision {
            let scanned = render_pages(&bytes, 2);
            let mut parts: Vec<String> = Vec::new();
            for (index, page) in scanned.iter().enumerate() {
                let jpeg = data_url_bytes(page);
                let text = ocr_text(&jpeg, "jpg", ocr).await;
                if text.starts_with("（OCR 失败") {
                    parts.clear();
                    parts.push(text);
                    break;
                }
                parts.push(format!("【第 {} 页】\n{text}", index + 1));
            }
            format!(
                "（这份 PDF 没有文字层，下面是页面 OCR 出来的）\n{}",
                parts.join("\n\n")
            )
        } else {
            parsed.text.clone()
        };

        // PDF 附件额外交代两件事：模型要拿 `stored` 才能调 extract_images（它看不到文件名），
        // 页数是「原件几页」的唯一来源。扫描件（读不到内容）同样要交代 —— 有照片的正好是它
        let marks = if ext == "pdf" {
            let pages = match parsed.pages {
                Some(count) => format!("（原件共 {count} 页）"),
                None => "（页数读不出）".to_string(),
            };
            format!("（附件标识：{stored}）\n{pages}\n")
        } else {
            String::new()
        };

        let body = if text.trim().is_empty() {
            if marks.is_empty() {
                format!("（附件 {name}：读不到内容）")
            } else {
                format!("──── {name} ────\n{marks}（附件 {name}：读不到内容）\n──── 文件结束 ────")
            }
        } else {
            format!(
                "──── {name} ────\n{marks}{}{note}\n──── 文件结束 ────",
                text.trim()
            )
        };
        blocks.push(body);
        if !parsed.note.is_empty() {
            blocks.push(parsed.note.clone());
        }
    }

    images.truncate(MAX_IMAGES);
    let section = if blocks.is_empty() {
        String::new()
    } else {
        format!("【本次附件】\n{}", blocks.join("\n\n"))
    };
    let origin = first_pdf.map(|(name, pages)| crate::agent::facts::Origin { name, pages });
    Materials {
        section,
        images,
        origin,
    }
}

fn cached(stored: &str, ext: &str, bytes: &[u8]) -> Parsed {
    if let Ok(map) = cache().lock() {
        if let Some(hit) = map.get(stored) {
            return Parsed {
                text: hit.text.clone(),
                scanned: hit.scanned,
                note: hit.note.clone(),
                pages: hit.pages,
            };
        }
    }
    let parsed = parse(ext, bytes);
    let cloned = Parsed {
        text: parsed.text.clone(),
        scanned: parsed.scanned,
        note: parsed.note.clone(),
        pages: parsed.pages,
    };
    if let Ok(mut map) = cache().lock() {
        map.insert(stored.to_string(), cloned);
    }
    parsed
}

/// OCR：先内置引擎，再设置里配的接口。两条都没有时把「为什么」说清楚，
/// 让模型别把「读不到」理解成「用户什么都没传」。
async fn ocr_text(bytes: &[u8], ext: &str, ocr: Option<&OcrConf>) -> String {
    match crate::extract::ocr_image(bytes.to_vec(), ext.to_string()) {
        Ok(text) => return clip(text.trim()),
        Err(error) => {
            let Some(conf) = ocr.filter(|item| !item.model.trim().is_empty()) else {
                return format!(
                    "（OCR 失败：{error}；也可在 设置 → 模型 → OCR 配置一个能看图的模型）"
                );
            };
            match crate::extract::ocr_api(
                bytes.to_vec(),
                ext.to_string(),
                conf.base_url.clone(),
                conf.api_key.clone(),
                conf.model.clone(),
            )
            .await
            {
                Ok(text) => clip(text.trim()),
                Err(fail) => format!("（OCR 失败：{error}；OCR 接口：{fail}）"),
            }
        }
    }
}

/// PDF → 页面 data URL。渲染器按平台挑，都不可用时返回空数组（按只有文字继续）
fn render_pages(bytes: &[u8], max_pages: usize) -> Vec<String> {
    crate::pdfimage::pdf_pages(bytes, max_pages, crate::pdfimage::DEFAULT_WIDTH)
        .map(|pages| pages.iter().map(|page| crate::pdfimage::data_url(&page.jpeg)).collect())
        .unwrap_or_default()
}

fn data_url_bytes(url: &str) -> Vec<u8> {
    use base64::Engine as _;
    let encoded = url.split_once(',').map(|(_, tail)| tail).unwrap_or(url);
    base64::engine::general_purpose::STANDARD
        .decode(encoded.as_bytes())
        .unwrap_or_default()
}

fn mime_of(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "avif" => "image/avif",
        _ => "image/jpeg",
    }
}

fn non_empty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_files_are_decoded_eagerly() {
        let parsed = parse("md", "# 标题\n正文".as_bytes());
        assert_eq!(parsed.text, "# 标题\n正文");
        assert!(!parsed.scanned);
    }

    /// 提不出字的 PDF 一律当扫描件：真正的坏文件也会在 OCR 那一步报错，
    /// 而扫描件如果被当成「读不到内容」，模型就会说「请把内容贴给我」
    #[test]
    fn unreadable_pdf_is_marked_scanned() {
        let parsed = parse("pdf", b"not a pdf at all");
        assert!(parsed.scanned);
        assert!(parsed.text.is_empty());
    }

    #[test]
    fn unknown_extension_says_why() {
        let parsed = parse("docx", b"PK\x03\x04");
        assert!(parsed.note.contains("暂不支持解析"), "{}", parsed.note);
    }

    #[test]
    fn image_kind_has_no_text() {
        let parsed = parse("png", &[0x89, 0x50, 0x4e, 0x47]);
        assert!(parsed.text.is_empty());
        assert!(!parsed.scanned);
        assert_eq!(kind_of("PNG"), "image");
    }

    /// 端到端：写进 inbox、按 stored 取回、拼成材料段。这条跑通就说明
    /// 「前端只给名字，后端自己找正文」这件事成立。
    #[tokio::test]
    async fn materials_read_the_body_from_inbox() {
        let root = std::env::temp_dir().join(format!("sr-attach-{}", crate::session::uuid_like()));
        std::fs::create_dir_all(root.join("inbox")).unwrap();
        let body = "# 教育背景\n华北电力大学 本科";
        let stored = crate::snapshot::pool_name(&crate::pool::sha256_hex(body.as_bytes()), Some("md")).unwrap();
        std::fs::write(root.join("inbox").join(&stored), body.as_bytes()).unwrap();

        let refs = vec![AttachRef {
            stored: stored.clone(),
            name: "简历.md".into(),
            ext: "md".into(),
        }];
        let materials = materials(&root, &refs, false, None).await;
        assert!(materials.section.contains("【本次附件】"), "{}", materials.section);
        assert!(materials.section.contains("华北电力大学"), "{}", materials.section);
        assert!(materials.images.is_empty());
        // 纯 .md 附件没有原件页数可说
        assert!(materials.origin.is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 页数只有这一处算：读不出写 None，**绝不写 0**；`stored` 要交代给模型
    #[tokio::test]
    async fn pdf_materials_carry_pages_and_the_stored_mark() {
        let root = std::env::temp_dir().join(format!("sr-attach-{}", crate::session::uuid_like()));
        std::fs::create_dir_all(root.join("inbox")).unwrap();
        let body = crate::pdfembed::fixtures::pdf_with_pages(2);
        let stored = crate::snapshot::pool_name(&crate::pool::sha256_hex(&body), Some("pdf")).unwrap();
        std::fs::write(root.join("inbox").join(&stored), &body).unwrap();

        let refs = vec![
            AttachRef {
                stored: stored.clone(),
                name: "原件.pdf".into(),
                ext: "pdf".into(),
            },
            AttachRef {
                stored: "second.pdf".into(),
                name: "第二份.pdf".into(),
                ext: "pdf".into(),
            },
        ];
        let materials = materials(&root, &refs, false, None).await;
        let origin = materials.origin.expect("有 PDF 附件就该有 origin");
        assert_eq!(origin.name, "原件.pdf");
        assert_eq!(origin.pages, Some(2));
        assert!(
            materials.section.contains(&format!("（附件标识：{stored}）")),
            "{}",
            materials.section
        );
        assert!(materials.section.contains("（原件共 2 页）"), "{}", materials.section);

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 坏 PDF：读不出页数 → None（不是 0），且「读不到内容」这句话不能丢
    #[tokio::test]
    async fn broken_pdf_reports_unknown_pages() {
        let root = std::env::temp_dir().join(format!("sr-attach-{}", crate::session::uuid_like()));
        std::fs::create_dir_all(root.join("inbox")).unwrap();
        let body = b"not a pdf".to_vec();
        let stored = crate::snapshot::pool_name(&crate::pool::sha256_hex(&body), Some("pdf")).unwrap();
        std::fs::write(root.join("inbox").join(&stored), &body).unwrap();

        let refs = vec![AttachRef {
            stored: stored.clone(),
            name: "坏.pdf".into(),
            ext: "pdf".into(),
        }];
        let materials = materials(&root, &refs, false, None).await;
        assert_eq!(materials.origin.expect("文件名还是要报").pages, None);
        // 坏文件不能变成「原件 0 页」
        assert!(materials.section.contains("（页数读不出）"), "{}", materials.section);
        assert!(!materials.section.contains("（原件共 0 页）"), "{}", materials.section);
        // 附件标识还是要给：扫描件正是最需要抽图的那种
        assert!(
            materials.section.contains(&format!("（附件标识：{stored}）")),
            "{}",
            materials.section
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 文件被删了要给模型一句实话，而不是静默少一段材料
    #[tokio::test]
    async fn missing_attachment_says_so() {
        let root = std::env::temp_dir().join(format!("sr-attach-{}", crate::session::uuid_like()));
        std::fs::create_dir_all(&root).unwrap();
        let refs = vec![AttachRef {
            stored: "deadbeefdeadbeef.md".into(),
            name: "丢了.md".into(),
            ext: "md".into(),
        }];
        let materials = materials(&root, &refs, false, None).await;
        assert!(materials.section.contains("附件已不在"), "{}", materials.section);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn data_url_round_trips_to_bytes() {
        let raw = vec![1u8, 2, 3, 4];
        let url = crate::pdfimage::data_url_of("image/jpeg", &raw);
        assert_eq!(data_url_bytes(&url), raw);
    }
}
