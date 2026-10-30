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

pub(crate) struct Parsed {
    pub(crate) text: String,
    pub(crate) scanned: bool,
    pub(crate) note: String,
    /// PDF 的页数（F-005 的唯一来源）。读不出 = None，**绝不写 0**
    pub(crate) pages: Option<u32>,
}

/// `materials()` 的产物：材料段 + 本轮附给视觉模型的图 + 原件信息
pub struct Materials {
    pub section: String,
    pub images: Vec<String>,
    /// 本轮第一个 PDF 附件。`pages: None` = 读不出（损坏 / 不是 PDF）
    pub origin: Option<crate::agent::facts::Origin>,
    /// 原件文档指针：只记 stored / name / pages，**不再预渲染页图**。
    ///
    /// 之前这里是一张页图（`Asset`），只覆盖第 1 页、又被压到看不清，还占着材料池；
    /// 现在模型要看哪一页，自己调 `render_doc_page(stored, page)`。
    pub origin_doc: Option<crate::model::OriginDoc>,
}

/// 解析结果缓存。
///
/// 键必须同时含 `stored` 与 `ext`：`parse` 的走向完全由 ext 决定（pdf 走
/// pdf-extract，空/未知扩展走「不认识」那条分支），只用 stored 做键的话，
/// 任何一次空 ext 的解析都会把「空正文」钉死在这份内容上 —— 之后每一轮都拿不到
/// 正文，而抽图是直接读字节的，于是表现成「图抽得出来、文字永远没有」。
fn cache() -> &'static Mutex<HashMap<String, Parsed>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Parsed>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cache_key(stored: &str, ext: &str) -> String {
    format!("{stored}\0{ext}")
}

/// 扩展名归一化：小写、去掉前导点与空白；拿不到就从 `stored` 的后缀补。
///
/// 解析走向与缓存键都取决于它，所以只留这一处算法 —— 两侧各算一遍迟早对不上
/// （上传侧按文件名算、材料侧按存储名算，正是上一版「空 ext 污染缓存」的入口）。
fn normalize_ext(raw: &str, stored: &str) -> String {
    let ext = raw.trim().trim_start_matches('.').to_ascii_lowercase();
    if !ext.is_empty() {
        return ext;
    }
    stored
        .rsplit_once('.')
        .map(|(_, tail)| tail.trim().to_ascii_lowercase())
        .unwrap_or_default()
}

/// 魔数识别：扩展名不可信时（文件名没后缀 / 后缀写错）按字节认类型。
///
/// 只认 PDF / PNG / JPEG 三种：认错了会走错分支，比不认更糟。
fn sniff_ext(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(b"%PDF-") {
        return Some("pdf");
    }
    if data.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Some("png");
    }
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("jpg");
    }
    None
}

/// 单个附件进上下文的字数上限：200KB 字符足够一份简历，
/// 再长的东西该让模型用工具分段读，而不是一次塞进 prompt
const MAX_CHARS: usize = 200 * 1024;
/// 一次请求最多几张图（含 PDF 内嵌图）：再多 token 和请求体都撑不住
const MAX_IMAGES: usize = 4;
/// 一份 PDF 最多附几张内嵌图。页图不再默认渲染，名额基本都留给内嵌图
const MAX_INNER: usize = 3;
/// 【原件结构】最多列几块。一份简历 30~60 块，列满会把材料段撑成一篇文章；
/// 超出的部分留给 `read_source`（它给全文，结构自然也在里面）
const MAX_OUTLINE_BLOCKS: usize = 40;
/// 正文超过这么多字，Projection 就只给摘要 + 指针。800 是一页简历正文的量级：
/// 不超过它就整段给（省一次工具调用），超了说明是长文档，值得让模型按需读
const SUMMARY_CHARS: usize = 800;

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
    let root = crate::data_dir(&app)?;
    put(&root, &name, &data_base64)
}

/// `attach_put` 的实行：只认数据目录，不碰 `AppHandle`。
///
/// 分出来是为了能测 —— 「上传即落事实层」这条必须在没有 Tauri 运行时的地方也跑得起来，
/// 否则它只能靠真机试。
fn put(root: &Path, name: &str, data_base64: &str) -> Result<AttachInfo, String> {
    use base64::Engine as _;
    let data = base64::engine::general_purpose::STANDARD
        .decode(data_base64.as_bytes())
        .map_err(|error| format!("附件解码失败：{error}"))?;
    if data.is_empty() {
        return Err(format!("「{name}」是空文件"));
    }
    let dir = inbox_dir(root);
    std::fs::create_dir_all(&dir).map_err(|error| format!("建附件目录失败：{error}"))?;

    let raw_ext = name
        .rsplit_once('.')
        .map(|(_, tail)| tail.to_ascii_lowercase())
        .unwrap_or_default();
    let stored = crate::snapshot::pool_name(&crate::pool::sha256_hex(&data), Some(&raw_ext))?;
    // 归一化要拿 stored 兜底：文件名没有扩展名时，池里那份是 `<sha>.bin`，
    // 这也正是「上传侧按文件名算、材料侧按存储名算」会打架的那一格
    let ext = normalize_ext(&raw_ext, &stored);
    let path = dir.join(&stored);
    if !path.exists() {
        crate::storage::atomic_write(&path, &data)?;
    }

    let parsed = parse(&ext, &data);
    let kind = kind_of(&ext);
    let info = AttachInfo {
        stored: stored.clone(),
        name: name.to_string(),
        ext: ext.clone(),
        kind,
        chars: parsed.text.chars().count(),
        scanned: parsed.scanned,
    };
    // 上传当刻就把正文写进事实层：附件是「按轮一次性」的，这一行不写，
    // 一旦 materials 提前失败（或根本没跑），下一轮 read_source 就只能回 inbox 找文件，
    // 名字/路径任何一处对不上，模型看到的就是「原件已经不在系统里了」。
    // 空正文照写（扫描件 / 图片 chars = 0）——「有这份原件、但没有文字层」也是事实。
    persist_at_upload(root, &info.stored, &info.name, &info.ext, parsed.pages, &parsed.text);
    if let Ok(mut map) = cache().lock() {
        map.insert(cache_key(&stored, &ext), parsed);
    }
    Ok(info)
}

/// 上传时落事实层：连接在这里自己开（`attach_put` 是同步命令，拿不到 `Shared`，
/// 也不该为它加一个参数），用完即还。
///
/// 失败只打一行日志，绝不把错误抛给调用方 —— 为了「以后能读回正文」而让用户
/// 现在丢掉附件，是更糟的那一头。
fn persist_at_upload(
    root: &Path,
    stored: &str,
    name: &str,
    ext: &str,
    pages: Option<u32>,
    text: &str,
) {
    match crate::db::open(&root.join("app.db")) {
        Ok(conn) => {
            if let Err(error) = crate::source::save(&conn, stored, name, ext, pages, text) {
                eprintln!("[attach] 原件正文没能落库（{stored}）：{error}");
            }
        }
        Err(error) => eprintln!("[attach] 打不开库，原件正文没能落库（{stored}）：{error}"),
    }
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

/// 解析：文本直读、PDF 取字、图片留给 OCR。纯函数，缓存由调用方管。
///
/// `pub(crate)`：`read_source` 在 Fact 层查不到正文时要现场重解析一次（见 tools/source.rs）
pub(crate) fn parse(ext: &str, data: &[u8]) -> Parsed {
    // 扩展名只当提示：真 PDF 被存成 `<sha>.bin`（文件名没后缀）时，正文会永远空、
    // 图却照旧抽得出来 —— 这种「半通不通」最难查，所以让字节说话
    let declared = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    let ext = match sniff_ext(data) {
        Some(sniffed) if sniffed != declared => sniffed.to_string(),
        _ => declared,
    };
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

/// 正文写进 Fact 层。**所有类型都写**：图片与扫描件写的是空正文（chars = 0），
/// 那一行回答的是「系统里有这份原件」，不是「这份原件有几个字」——
/// 跳过它，`read_source` 就只能说「找不到该文件」，而文件明明还在。
fn persist(
    db: &crate::db::Shared,
    stored: &str,
    name: &str,
    ext: &str,
    pages: Option<u32>,
    text: &str,
) {
    // 守卫不跨 await：这里整段是同步的，`with` 返回时连接已经还回去了
    if let Err(error) =
        db.with(|conn| crate::source::save(conn, stored, name, ext, pages, text))
    {
        eprintln!("[attach] 原件正文没能落库（{stored}）：{error}");
    }
}

/// 「这份原件是谁的」。只在真的有事实行之后才记 —— `list_sources` 是按
/// `attach_ref JOIN source` 查的，先记归属后落正文会留下一条查不出东西的影子。
fn bind_origin(db: &crate::db::Shared, person_id: &str, stored: &str) {
    if let Err(error) = db.with(|conn| crate::source::bind(conn, person_id, stored)) {
        eprintln!("[attach] 原件归属没记上（{stored}）：{error}");
    }
}

/// 长正文的投影：摘要 + 一行指针。指针里的 `stored` 是模型唯一的抓手，
/// 少了它就只剩「我记得有份文件」这种无能为力的话。
fn summary_text(chars: usize, stored: &str, text: &str) -> String {
    format!(
        "{}\n（原件正文共 {chars} 字，这里只给开头 120 字；要全文调 read_source，参数 stored = {stored}）",
        crate::source::digest_text(text, chars)
    )
}

/// 组装本轮的材料段与图片。
///
/// 这是「解析下沉」的落点：`chat_send` 只拿到一串 stored 名字，
/// 取字、OCR、渲染全在这里做，前端不再碰任何字节。
///
/// `db` 是 Fact 层的入库口：正文一解析出来就落库（见 `source.rs`），
/// 因为附件是**按轮一次性**的 —— 错过这一轮，正文就只剩库里那一份。
///
/// `person_id` 是「这份原件归谁」：本轮谁在用附件，谁就是它的主人。
pub async fn materials(
    root: &Path,
    person_id: &str,
    refs: &[AttachRef],
    vision: bool,
    ocr: Option<&OcrConf>,
    model: &str,
    db: &crate::db::Shared,
) -> Materials {
    let mut blocks: Vec<String> = Vec::new();
    let mut images: Vec<String> = Vec::new();
    // 只取本轮第一个 PDF：原件页数是「这一轮上传了什么」这么一件事，不是清单
    let mut first_pdf: Option<(String, Option<u32>)> = None;
    let mut origin_doc: Option<crate::model::OriginDoc> = None;

    for file in refs {
        let Some(stored) = non_empty(&file.stored) else {
            continue;
        };
        let name = if file.name.is_empty() { stored } else { &file.name };
        let declared = normalize_ext(&file.ext, stored);
        let bytes = match read(root, stored) {
            Ok(bytes) => bytes,
            Err(error) => {
                if declared == "pdf" && first_pdf.is_none() {
                    first_pdf = Some((name.to_string(), None));
                }
                blocks.push(format!("（附件 {name}：{error}）"));
                continue;
            }
        };
        // 与 parse 同一套判断：扩展名不可信时以字节为准，否则 附件标识 / 页数 /
        // 原件指针这些「只有 pdf 才有」的东西会跟着一起缺席
        let ext = sniff_ext(&bytes)
            .map(str::to_string)
            .unwrap_or(declared);
        let parsed = cached(stored, &ext, &bytes);
        let mut is_origin = false;
        if ext == "pdf" && first_pdf.is_none() {
            first_pdf = Some((name.to_string(), parsed.pages));
            // 原件指针只认这一份（第一个 PDF）：它是「原件是哪一份」的唯一依据
            is_origin = true;
        }

        if is_image(&ext) {
            // 图片没有正文，但「系统里有这份原件」本身是事实：不写这一行，
            // read_source / list_sources 就只剩「找不到」这一种回答
            persist(db, stored, name, &ext, parsed.pages, &parsed.text);
            bind_origin(db, person_id, stored);
            if vision {
                images.push(crate::pdfimage::data_url_of(&mime_of(&ext), &bytes));
                blocks.push(format!(
                    "──── {name} ────\n（图片已作为视觉输入附上，说明文字只描述版式，不要复述画面内容）\n──── 文件结束 ────"
                ));
            } else {
                let text = image_unreadable_text(&bytes, &ext, ocr, model).await;
                blocks.push(format!("──── {name} ────\n{text}\n──── 文件结束 ────"));
            }
            continue;
        }

        // 事实先落库，再谈投影怎么给：这一轮之后附件不会再出现，
        // 正文从此只活在库里。落库失败只打日志 —— 这一轮的材料还得照给
        persist(db, stored, name, &ext, parsed.pages, &parsed.text);
        bind_origin(db, person_id, stored);

        // 默认不渲染页图：版式事实改由【原件结构】给（见下方 docparse），
        // 要真看某一页长什么样，模型自己调 render_doc_page —— 一次一页，按需付费。
        // 原件也随之从「预渲染样张」改成指针：这里不落任何图，只记怎么找回这份文件。
        if is_origin && origin_doc.is_none() {
            origin_doc = Some(crate::model::OriginDoc {
                stored: stored.to_string(),
                name: name.to_string(),
                pages: parsed.pages,
            });
        }
        let mut notes: Vec<String> = Vec::new();
        // 视觉模型为什么要单独给内嵌图：页图已经不默认渲染，但证件照是**图**，
        // 纸面上取不出来。内嵌图是原件分辨率，模型才认得出那张脸。
        if ext == "pdf" && vision {
            let (attached, available) = attach_inner_images(&bytes, &mut images);
            if attached > 0 {
                notes.push(format!("（另附这份 PDF 里的 {attached} 张内嵌图，按出现顺序）"));
            } else if available == 0 {
                notes.push("（这份 PDF 里没有可附的内嵌图）".to_string());
            } else {
                // 有图但名额被别的图占满：不许说「没有」，那是假话
                notes.push(format!(
                    "（这份 PDF 里还有 {available} 张内嵌图，但这一轮的附图名额已满）"
                ));
            }
        }
        let note = if notes.is_empty() {
            String::new()
        } else {
            format!("\n{}", notes.join("\n"))
        };

        // 带格式解析：能读出行内结构（层级/字号/对齐/表格骨架）就给结构大纲。
        // 读不出（`None`）不算错误 —— 回落到纯文本，别伪造一份空结构。
        let outline = crate::docparse::parse(&ext, &bytes);

        // Projection：正文 ≤ 800 字照旧全文；超过就只给指针。
        // 为什么不再整段拼：附件只在那一轮出现，模型没读完就结束时，几千字当场
        // 永久消失（真机事故：2404 字的简历只剩「姓名 + 照片」）。全文已在 Fact 层，
        // 模型按 stored 调 read_source 就能读回来。
        let chars = parsed.text.trim().chars().count();
        let long = chars > SUMMARY_CHARS;
        let text = if ext == "pdf" {
            if long {
                summary_text(chars, stored, &parsed.text)
            } else {
                pdf_text(&bytes, &parsed, stored, vision, ocr).await
            }
        } else {
            parsed.text.clone()
        };
        let structure = outline
            .as_ref()
            .map(|outline| structure_section(outline, stored));
        // 长正文 + 有结构：只给结构。大纲已经说清「原件长什么样」，
        // 再叠一段摘要只是同一件事说两遍，全文交给 read_source
        let projection = if text.trim().is_empty() {
            String::new()
        } else if long && structure.is_some() {
            structure.clone().unwrap_or_default()
        } else {
            match &structure {
                Some(section) => format!("{}\n\n{section}", text.trim()),
                None => text.trim().to_string(),
            }
        };

        // PDF 附件额外交代两件事：模型要拿 `stored` 才能调 extract_images / render_doc_page
        // （它看不到文件名），页数是「原件几页」的唯一来源。扫描件（读不到内容）同样要交代
        let marks = if ext == "pdf" {
            let pages = match parsed.pages {
                Some(count) => format!("（原件共 {count} 页）"),
                None => "（页数读不出）".to_string(),
            };
            format!("（附件标识：{stored}）\n{pages}\n")
        } else {
            String::new()
        };

        let body = if projection.is_empty() {
            if marks.is_empty() {
                format!("（附件 {name}：读不到内容）")
            } else {
                // 读不到正文时附图说明照样要给：扫描件正是最需要看页面的那种
                format!(
                    "──── {name} ────\n{marks}（附件 {name}：读不到内容）{note}\n──── 文件结束 ────"
                )
            }
        } else {
            format!(
                "──── {name} ────\n{marks}{projection}{note}\n──── 文件结束 ────",
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
        origin_doc,
    }
}

/// 结构大纲：把 `outline_json` 摊成「一行一块」。
///
/// 为什么不直接把 JSON 塞进材料段：模型读这个紧凑格式比读 JSON 快得多，
/// token 也少一大半（键名、引号、缩进都省了）。逐块字段是 `k / pt / bold / align`，
/// 与 `outline_json` 同一套口径 —— 两边说法不一致比缺字段更难查。
fn structure_section(outline: &crate::docparse::DocOutline, stored: &str) -> String {
    let value = outline.outline_json();
    let pages = match value["pages"].as_u64() {
        Some(count) => format!("共 {count} 页"),
        None => "页数读不出".to_string(),
    };
    let body = match value["bodyPt"].as_f64() {
        Some(pt) => format!("正文基准 {pt}pt"),
        None => "正文基准读不出".to_string(),
    };
    let margin = match value["marginPt"].as_array() {
        Some(sides) if !sides.is_empty() => {
            let sides: Vec<String> = sides
                .iter()
                .map(|side| match side.as_f64() {
                    Some(pt) => pt.to_string(),
                    None => "?".to_string(),
                })
                .collect();
            format!("版心边距 [{}]pt", sides.join(", "))
        }
        _ => "版心边距读不出".to_string(),
    };

    let all = value["blocks"].as_array().cloned().unwrap_or_default();
    let shown = all.len().min(MAX_OUTLINE_BLOCKS);
    let mut lines: Vec<String> = vec![format!("【原件结构】{pages} · {body} · {margin}")];
    for block in all.iter().take(MAX_OUTLINE_BLOCKS) {
        let kind = block["k"].as_str().unwrap_or("?");
        let mut head = String::from(kind);
        if let Some(pt) = block["pt"].as_f64() {
            head.push_str(&format!(" {pt}pt"));
        }
        if block["bold"].as_bool().unwrap_or(false) {
            head.push_str(" bold");
        }
        if let Some(align) = block["align"].as_str() {
            head.push_str(&format!(" {align}"));
        }
        let text = block["text"].as_str().unwrap_or("").trim();
        lines.push(format!("{head} | {text}"));
    }
    // 指针永远给：结构大纲里没有逐字全文，模型要写字就得去 read_source
    if shown < all.len() {
        lines.push(format!(
            "（全部 {} 块，这里只列前 {shown} 块；全文与完整结构调 read_source，参数 stored = {stored}）",
            all.len()
        ));
    } else {
        lines.push(format!(
            "（全部 {} 块；全文与完整结构调 read_source，参数 stored = {stored}）",
            all.len()
        ));
    }
    lines.join("\n")
}

fn cached(stored: &str, ext: &str, bytes: &[u8]) -> Parsed {
    let ext = normalize_ext(ext, stored);
    let key = cache_key(stored, &ext);
    if let Ok(map) = cache().lock() {
        if let Some(hit) = map.get(&key) {
            return Parsed {
                text: hit.text.clone(),
                scanned: hit.scanned,
                note: hit.note.clone(),
                pages: hit.pages,
            };
        }
    }
    let parsed = parse(&ext, bytes);
    let cloned = Parsed {
        text: parsed.text.clone(),
        scanned: parsed.scanned,
        note: parsed.note.clone(),
        pages: parsed.pages,
    };
    if let Ok(mut map) = cache().lock() {
        map.insert(key, cloned);
    }
    parsed
}

/// PDF 的正文：有文字就照抄；没有就说清**为什么没有**。
///
/// 为什么值得分档：原来只有一句「（附件 X：读不到内容）」，它分不清是扫描件、
/// 加密、还是解析器不支持 —— 用户只能猜，模型也只能瞎试。三种成因给三条可 grep 的路。
async fn pdf_text(
    bytes: &[u8],
    parsed: &Parsed,
    stored: &str,
    vision: bool,
    ocr: Option<&OcrConf>,
) -> String {
    if !parsed.text.trim().is_empty() {
        return parsed.text.clone();
    }
    // 页数读不出 = 文件本身坏了或加密：那不是「扫描件」，别让人去配 OCR
    if parsed.pages.is_none() {
        return "（这份 PDF 解析不出文字（可能加密或结构异常）。把文字直接贴过来最快。）".into();
    }
    if vision {
        // 页图不再默认渲染（也就没有「附上的页面图片」可看）。这里给的是**怎么自己去看**：
        // 扫描件的内容只存在于像素里，不说清这一句，模型只能回一句「我读不到」
        return format!(
            "（这份 PDF 没有文字层，内容只能看页面：调 render_doc_page，参数 stored = {stored}。）"
        );
    }
    if !ocr_ready(ocr) {
        return "（这份 PDF 没有文字层（扫描件），当前也没配 OCR，所以读不到任何文字。要读它，请在 设置 → 模型 → OCR 配一个能看图的模型，或直接把文字贴过来。）".into();
    }
    let rendered = render_pages(bytes, 2);
    if rendered.is_empty() {
        return "（这份 PDF 没有文字层，OCR 也没读出来：这一版渲染不出页面，没法送 OCR。）".into();
    }
    let mut parts: Vec<String> = Vec::new();
    for (index, page) in rendered.iter().enumerate() {
        let text = ocr_text(&data_url_bytes(page), "jpg", ocr).await;
        if text.starts_with("（OCR 失败") {
            return format!(
                "（这份 PDF 没有文字层，OCR 也没读出来：{}。）",
                ocr_reason(&text)
            );
        }
        parts.push(format!("【第 {} 页】\n{text}", index + 1));
    }
    format!(
        "（这份 PDF 没有文字层，下面是页面 OCR 出来的）\n{}",
        parts.join("\n\n")
    )
}

/// 图片附件 + 非视觉模型时的交代：说清「读不了」还是「没读出来」，并给出下一步。
///
/// 为什么不直接复用 `ocr_text` 的返回：它还要服务「扫描件 PDF 逐页 OCR」，
/// 那里是按页说「第 N 页」的措辞；这里只管「单张图片附件」这一个场景。
async fn image_unreadable_text(
    bytes: &[u8],
    ext: &str,
    ocr: Option<&OcrConf>,
    model: &str,
) -> String {
    let raw = ocr_text(bytes, ext, ocr).await;
    if !raw.starts_with("（OCR 失败") {
        return raw;
    }
    if ocr_ready(ocr) {
        // 有 OCR 通道但失败了：给原因，别让模型以为「再试一次就好了」
        format!(
            "（这张图我没读出来：{}。可以换一张，或直接把图里的文字贴过来。）",
            ocr_reason(&raw)
        )
    } else {
        // 模型看不见 + 没有 OCR：这是唯一必须给「两条路」的地方
        format!(
            "（这张图我读不了：当前模型{}不支持看图，也没有配置 OCR。要看图，要么在 设置 → 模型 里换成支持视觉的模型，要么在 设置 → 模型 → OCR 里配一个能看图的模型。）",
            crate::tools::model_label(model)
        )
    }
}

/// 这一轮有没有任何 OCR 通道：内置引擎（带 `ocr` 特性的构建）+ 设置里配的接口。
/// 判断口径与 `ocr_text` 内部一致，否则会出现「说没配 OCR，结果又读出了字」
fn ocr_ready(ocr: Option<&OcrConf>) -> bool {
    crate::extract::ocr_available()
        || ocr
            .map(|conf| !conf.model.trim().is_empty())
            .unwrap_or(false)
}

/// 从 `ocr_text` 的「（OCR 失败：原因；…）」里只取原因，好接进新文案
fn ocr_reason(raw: &str) -> &str {
    raw.trim_start_matches('（')
        .trim_start_matches("OCR 失败：")
        .split('；')
        .next()
        .unwrap_or(raw)
        .trim_end_matches('）')
}

/// 视觉模型的第二份输入：PDF 里的内嵌图。
///
/// 排序与 `extract_images` 同一口径（复用它的 `order_by_share`）：按「占版心」升序
/// 留小的 —— 证件照总比整页扫描件小，名额先花在小的上面才不会全被整页图吃掉。
/// 返回 `(已附张数, 可解出的总张数)`：调用方要用后者区分「没有图」与「名额满了」。
fn attach_inner_images(bytes: &[u8], images: &mut Vec<String>) -> (usize, usize) {
    let Ok(report) = crate::pdfembed::extract(bytes) else {
        return (0, 0);
    };
    if report.images.is_empty() {
        return (0, 0);
    }
    let room = MAX_IMAGES.saturating_sub(images.len()).min(MAX_INNER);
    let mut attached = 0;
    for index in crate::tools::extract::order_by_share(&report) {
        if attached >= room {
            break;
        }
        let image = &report.images[index];
        let Ok((data, ext)) = crate::tools::extract::encode_candidate(image) else {
            continue;
        };
        let mime = if ext == "jpg" { "image/jpeg" } else { "image/png" };
        images.push(crate::pdfimage::data_url_of(mime, &data));
        attached += 1;
    }
    (attached, report.images.len())
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
    use std::path::PathBuf;

    /// 本机真实样本：环境变量优先，其次两个已知路径；都没有就 skip
    fn real_sample() -> Option<Vec<u8>> {
        if let Some(path) = option_env!("SHARP_RESUME_SAMPLE_PDF") {
            if let Ok(bytes) = std::fs::read(path) {
                return Some(bytes);
            }
        }
        for path in [
            "E:/文档/实习/华北电力大学 谭轶天.pdf",
            "E:/文档/实习/谭轶天简历.pdf",
        ] {
            if let Ok(bytes) = std::fs::read(path) {
                return Some(bytes);
            }
        }
        None
    }

    /// 把一份字节放进临时 inbox，返回 (数据目录, stored)
    fn inbox_with(bytes: &[u8], ext: &str) -> (PathBuf, String) {
        let root = std::env::temp_dir().join(format!("sr-attach-{}", crate::session::uuid_like()));
        std::fs::create_dir_all(root.join("inbox")).unwrap();
        let stored = crate::snapshot::pool_name(&crate::pool::sha256_hex(bytes), Some(ext)).unwrap();
        std::fs::write(root.join("inbox").join(&stored), bytes).unwrap();
        (root, stored)
    }

    fn png_bytes(width: u32, height: u32) -> Vec<u8> {
        use std::io::Cursor;
        let image = image::DynamicImage::new_rgb8(width, height);
        let mut out = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

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

    /// 扩展名不可信时以字节为准：文件名没后缀的真 PDF 也要出正文，
    /// 而且 附件标识 / 页数这些「只有 pdf 才有」的交代不能跟着缺席
    #[tokio::test]
    async fn a_pdf_without_a_file_extension_still_gives_its_text() {
        let Some(bytes) = real_sample() else {
            eprintln!("skipped: 本机没有可用的 PDF 样本（设 SHARP_RESUME_SAMPLE_PDF 指一份）");
            return;
        };
        // 存成 .bin（= 文件名没有后缀时 pool_name 的兜底），AttachRef 也不给 ext
        let (root, stored) = inbox_with(&bytes, "bin");
        let refs = vec![AttachRef {
            stored: stored.clone(),
            name: "简历".into(),
            ext: String::new(),
        }];
        let db = crate::db::Shared::new(root.join("app.db"));
        let materials = materials(&root, "p1", &refs, false, None, "test-model", &db).await;
        assert!(materials.section.contains(&format!("（附件标识：{stored}）")), "{}", materials.section);
        assert!(materials.section.contains("（原件共 1 页）"), "{}", materials.section);
        // 正文走纯文本投影时，PDF 里的字距会被提成空格（「谭 轶 天」）；
        // 走结构大纲时又是「谭轶天」。这里只关心「读出来了」，所以先拍平空白再比
        let flat: String = materials.section.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(flat.contains("谭轶天"), "正文要读得出来：{}", materials.section);
        assert_eq!(materials.origin.expect("原件信息").pages, Some(1));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 空正文的第一档：扫描件 + 没配 OCR —— 说清「没有文字层」与去哪儿配
    #[tokio::test]
    async fn a_scanned_pdf_without_ocr_says_it_has_no_text_layer() {
        let (root, stored) = inbox_with(&crate::pdfembed::fixtures::pdf_with_pages(1), "pdf");
        let refs = vec![AttachRef {
            stored,
            name: "扫描件.pdf".into(),
            ext: "pdf".into(),
        }];
        let db = crate::db::Shared::new(root.join("app.db"));
        let materials = materials(&root, "p1", &refs, false, None, "deepseek-chat", &db).await;
        assert!(materials.section.contains("没有文字层"), "{}", materials.section);
        assert!(materials.section.contains("没配 OCR"), "{}", materials.section);
        assert!(materials.section.contains("设置 → 模型"), "{}", materials.section);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 第二档：扫描件 + 配了 OCR 但没读出来 —— 带上原因
    #[tokio::test]
    async fn a_scanned_pdf_with_a_broken_ocr_channel_says_ocr_failed() {
        let (root, stored) = inbox_with(&crate::pdfembed::fixtures::pdf_with_pages(1), "pdf");
        let refs = vec![AttachRef {
            stored,
            name: "扫描件.pdf".into(),
            ext: "pdf".into(),
        }];
        let conf = OcrConf {
            base_url: "http://127.0.0.1:9".into(),
            api_key: "sk-test".into(),
            model: "some-vl".into(),
        };
        let db = crate::db::Shared::new(root.join("app.db"));
        let materials = materials(&root, "p1", &refs, false, Some(&conf), "deepseek-chat", &db).await;
        assert!(materials.section.contains("没有文字层"), "{}", materials.section);
        assert!(materials.section.contains("OCR 也没读出来"), "{}", materials.section);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 第三档：页数都读不出（加密 / 结构异常）—— 那是文件的问题，不是「扫描件」
    #[tokio::test]
    async fn a_broken_pdf_says_it_cannot_be_parsed_instead_of_calling_it_scanned() {
        let (root, stored) = inbox_with(b"not a pdf at all", "pdf");
        let refs = vec![AttachRef {
            stored,
            name: "坏.pdf".into(),
            ext: "pdf".into(),
        }];
        let db = crate::db::Shared::new(root.join("app.db"));
        let materials = materials(&root, "p1", &refs, false, None, "deepseek-chat", &db).await;
        assert!(materials.section.contains("解析不出文字"), "{}", materials.section);
        assert!(
            !materials.section.contains("没有文字层"),
            "坏文件不该被当成扫描件：{}",
            materials.section
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 缓存 key 必须带 ext：`parse` 的分支由 ext 决定（PDF / 图片这种能靠魔数认出来的
    /// 不受影响，但纯文本这一档完全看扩展名）。只用 stored 当键时，一次空 ext 的解析
    /// 会把「空正文」钉死在这份内容上 —— 之后每轮都拿不到正文。
    #[test]
    fn the_cache_key_must_include_the_extension() {
        let bytes = b"# \xe6\xa0\x87\xe9\xa2\x98\n\xe6\xad\xa3\xe6\x96\x87".to_vec();
        let stored = crate::snapshot::pool_name(&crate::pool::sha256_hex(&bytes), Some("bin")).unwrap();

        // 先以「没有扩展名」解析一次：走的是「不认识这个扩展名」那条分支
        let unknown = cached(&stored, "", &bytes);
        assert!(unknown.text.is_empty(), "空 ext 认不出这是文本");
        assert!(unknown.note.contains("暂不支持解析"), "{}", unknown.note);

        // 再来一次真扩展名：必须重新解析，拿到正文
        let as_md = cached(&stored, "md", &bytes);
        assert!(!as_md.text.is_empty(), "换了 ext 必须重解析，不能吃上次的空结果");
        assert!(as_md.note.is_empty());

        // 反向：先按 md 解析过的内容，再用空 ext 问，也不许拿到正文
        let other = crate::snapshot::pool_name(&crate::pool::sha256_hex(b"another-content"), Some("bin")).unwrap();
        assert!(!cached(&other, "md", &bytes).text.is_empty());
        assert!(
            cached(&other, "", &bytes).text.is_empty(),
            "空 ext 不该复用 md 的解析结果"
        );
    }

    /// ext 归一化：大小写、前导点、空白、以及「文件名没扩展名 → 用存储名的后缀」
    #[test]
    fn extension_is_normalized_from_the_name_or_the_stored_name() {
        assert_eq!(normalize_ext("PDF", "a.pdf"), "pdf");
        assert_eq!(normalize_ext(" .Pdf ", "a.pdf"), "pdf");
        assert_eq!(normalize_ext("", "a3f19c04b2e6.pdf"), "pdf");
        assert_eq!(normalize_ext("", "a3f19c04b2e6.bin"), "bin");
        assert_eq!(normalize_ext("", "no-dot"), "");
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
        let db = crate::db::Shared::new(root.join("app.db"));
        let materials = materials(&root, "p1", &refs, false, None, "test-model", &db).await;
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
        let db = crate::db::Shared::new(root.join("app.db"));
        let materials = materials(&root, "p1", &refs, false, None, "test-model", &db).await;
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
        let db = crate::db::Shared::new(root.join("app.db"));
        let materials = materials(&root, "p1", &refs, false, None, "test-model", &db).await;
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
        let db = crate::db::Shared::new(root.join("app.db"));
        let materials = materials(&root, "p1", &refs, false, None, "test-model", &db).await;
        assert!(materials.section.contains("附件已不在"), "{}", materials.section);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 视觉模型：页图不再默认渲染，但内嵌图还要给一份 —— 证件照是图，
    /// 纸面上取不出来，而整页渲染里它只有几十像素，模型认不出那张脸
    #[tokio::test]
    async fn vision_models_also_get_the_pdf_inner_images() {
        let Some(bytes) = real_sample() else {
            eprintln!("skipped: 本机没有可用的 PDF 样本（设 SHARP_RESUME_SAMPLE_PDF 指一份）");
            return;
        };
        let (root, stored) = inbox_with(&bytes, "pdf");
        let refs = vec![AttachRef {
            stored: stored.clone(),
            name: "原件.pdf".into(),
            ext: "pdf".into(),
        }];
        let db = crate::db::Shared::new(root.join("app.db"));
        let materials = materials(&root, "p1", &refs, true, None, "vision-model", &db).await;
        assert!(
            !materials.images.is_empty(),
            "这份样本里至少有一张内嵌图（证件照）：{}",
            materials.section
        );
        assert!(materials.section.contains("内嵌图"), "{}", materials.section);
        // 页图不再默认渲染：附图只剩内嵌图，条数不该被「前 3 页」占满
        assert!(
            materials.images.len() <= MAX_INNER,
            "附图最多 {} 张，实际 {}",
            MAX_INNER,
            materials.images.len()
        );
        assert!(
            materials.images.iter().all(|url| url.starts_with("data:image/")),
            "只许塞 data URL"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 内嵌图的名额受「别的图先占」约束：`(已附, 可解出总数)`
    #[test]
    fn inner_images_take_the_smallest_ones_and_respect_the_room() {
        let names = ["Im0", "Im1", "Im2", "Im3"];
        let mut images = Vec::new();
        let mut content = String::new();
        for (index, name) in names.iter().enumerate() {
            let mut pixels = vec![7u8; 30 * 30 * 3];
            pixels[0] = index as u8;
            images.push((*name, crate::pdfembed::fixtures::raw_image(30, 30, pixels)));
            // 缩放递减 = 占版心递减：升序排完应该从最后一张开始取
            let scale = 200.0 - 20.0 * index as f64;
            content.push_str(&format!("q {scale} 0 0 {scale} 0 0 cm /{name} Do Q "));
        }
        let bytes = crate::pdfembed::fixtures::pdf(images, &content);

        let mut urls = Vec::new();
        let (attached, available) = attach_inner_images(&bytes, &mut urls);
        assert_eq!((attached, available), (MAX_INNER, 4), "空名额时最多 MAX_INNER 张");
        assert_eq!(urls.len(), 3);

        // 名额已经被别的图占掉 3 个，只剩 1 个给内嵌图
        let mut urls = vec!["data:image/jpeg;base64,AA".to_string(); MAX_IMAGES - 1];
        let (attached, available) = attach_inner_images(&bytes, &mut urls);
        assert_eq!((attached, available), (1, 4));
        assert_eq!(urls.len(), MAX_IMAGES);
    }

    /// 抽不出内嵌图时不许沉默
    #[tokio::test]
    async fn a_pdf_without_inner_images_says_so() {
        let (root, stored) = inbox_with(&crate::pdfembed::fixtures::pdf_with_pages(1), "pdf");
        let refs = vec![AttachRef {
            stored,
            name: "纯文字.pdf".into(),
            ext: "pdf".into(),
        }];
        let db = crate::db::Shared::new(root.join("app.db"));
        let materials = materials(&root, "p1", &refs, true, None, "vision-model", &db).await;
        assert!(materials.section.contains("没有可附的内嵌图"), "{}", materials.section);
        let _ = std::fs::remove_dir_all(&root);
    }

    /* ---------- 原件指针 ---------- */

    /// 有 PDF = 有原件指针（stored / name / pages），**不再渲染任何页图**，
    /// 也不往池子里放「样张」。
    ///
    /// 这是行为变更：原来是「第 1 页截图登记成资产」，现在只记怎么找回这份文件。
    #[tokio::test]
    async fn a_pdf_yields_a_pointer_to_the_origin_instead_of_a_rendered_sheet() {
        let bytes = crate::pdfembed::fixtures::pdf_with_text(&["ACME Corp", "Backend Engineer"]);
        let (root, stored) = inbox_with(&bytes, "pdf");
        let refs = vec![AttachRef {
            stored: stored.clone(),
            name: "原件.pdf".into(),
            ext: "pdf".into(),
        }];
        let db = crate::db::Shared::new(root.join("app.db"));
        let materials = materials(&root, "p1", &refs, true, None, "vision-model", &db).await;

        let doc = materials.origin_doc.clone().expect("有 PDF 就该有原件指针");
        assert_eq!(doc.stored, stored, "指针必须带 stored，否则模型调不回这份文件");
        assert_eq!(doc.name, "原件.pdf");
        assert_eq!(doc.pages, Some(1), "页数照旧从 pdfembed 数出来");

        // 默认不再渲染页图：这份 fixture 里没有内嵌图，附图应当为空
        assert!(materials.images.is_empty(), "不许再默认附页图：{}", materials.section);
        assert!(!materials.section.contains("样张"), "{}", materials.section);
        // 池子里不该多出一个「样张.jpg」：那正是这次要拆掉的东西
        if let Ok(entries) = std::fs::read_dir(root.join("pool")) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                assert!(!name.contains("样张"), "不该再登记样张：{name}");
            }
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 原件指针与「模型能不能看图」无关：它是 read_source / render_doc_page 的抓手，
    /// 看不见图的模型同样要拿到 —— 否则下一轮连「原件是哪一份」都不知道
    #[tokio::test]
    async fn a_pdf_without_vision_still_gets_the_pointer() {
        let bytes = crate::pdfembed::fixtures::pdf_with_pages(1);
        let (root, stored) = inbox_with(&bytes, "pdf");
        let refs = vec![AttachRef {
            stored,
            name: "扫描件.pdf".into(),
            ext: "pdf".into(),
        }];
        let db = crate::db::Shared::new(root.join("app.db"));
        let materials = materials(&root, "p1", &refs, false, None, "deepseek-chat", &db).await;
        let doc = materials.origin_doc.expect("没有视觉也要给指针");
        assert_eq!(doc.name, "扫描件.pdf");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 扫描件 + 能看图：不再偷偷渲染页图，改为把「怎么自己去看」写清楚。
    /// 偷偷渲染会让材料段多出一张模型没要过的图，还会和「没有文字层」这句话打架。
    #[tokio::test]
    async fn a_scanned_pdf_with_vision_points_at_render_doc_page() {
        let bytes = crate::pdfembed::fixtures::pdf_with_pages(1);
        let (root, stored) = inbox_with(&bytes, "pdf");
        let refs = vec![AttachRef {
            stored: stored.clone(),
            name: "扫描件.pdf".into(),
            ext: "pdf".into(),
        }];
        let db = crate::db::Shared::new(root.join("app.db"));
        let materials = materials(&root, "p1", &refs, true, None, "vision-model", &db).await;

        assert!(materials.section.contains("没有文字层"), "{}", materials.section);
        assert!(materials.section.contains("render_doc_page"), "{}", materials.section);
        // 只钉「参数名 + 值」这一截：句末的句号是文案的一部分（规格里就有），
        // 断到「）前必须是 stored」会把一句正常的标点判成错
        assert!(
            materials.section.contains(&format!("参数 stored = {stored}")),
            "指针里必须带 stored：{}",
            materials.section
        );
        assert!(
            !materials.section.contains("另附前"),
            "页图不许再默认附上：{}",
            materials.section
        );
        assert!(materials.images.is_empty(), "扫描件也不许偷偷渲染");
        let _ = std::fs::remove_dir_all(&root);
    }

    /* ---------- 【原件结构】 ---------- */

    fn outline_for_test() -> crate::docparse::DocOutline {
        use crate::docparse::{Align, BlockKind, BlockStyle, DocBlock, DocOutline};
        let block = |kind, text: &str, size: f32, bold: bool, align| DocBlock {
            kind,
            text: text.into(),
            style: BlockStyle {
                size_pt: Some(size),
                bold,
                align,
                ..Default::default()
            },
            table: None,
        };
        DocOutline {
            pages: Some(1),
            margin_pt: Some([56.7, 56.7, 56.7, 56.7]),
            body_pt: Some(9.6),
            blocks: vec![
                block(BlockKind::Title, "谭轶天", 19.2, true, Align::Center),
                block(BlockKind::Paragraph, "性别：男 年龄：20", 9.6, false, Align::Left),
                block(BlockKind::Heading, "教育经历", 12.0, true, Align::Left),
            ],
        }
    }

    /// 结构大纲是给模型读的**紧凑文本**，不是 JSON：一行一块，头部交代页数/基准字号/边距
    #[test]
    fn the_structure_section_reads_as_one_line_per_block() {
        let text = structure_section(&outline_for_test(), "abc123.pdf");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[0], "【原件结构】共 1 页 · 正文基准 9.6pt · 版心边距 [56.7, 56.7, 56.7, 56.7]pt",
            "{text}"
        );
        assert_eq!(lines[1], "title 19.2pt bold center | 谭轶天", "{text}");
        assert_eq!(lines[2], "p 9.6pt left | 性别：男 年龄：20", "{text}");
        assert_eq!(lines[3], "heading 12pt bold left | 教育经历", "{text}");
        assert!(lines.last().unwrap().contains("参数 stored = abc123.pdf"), "{text}");
        // JSON 的键名、引号、花括号都不该出现在材料段里
        assert!(!text.contains('"'), "不要塞 JSON：{text}");
        assert!(!text.contains("{"), "不要塞 JSON：{text}");
    }

    /// 块太多时只列前 MAX_OUTLINE_BLOCKS 块，并把「还有多少、去哪儿读」说清楚
    #[test]
    fn a_long_structure_lists_a_prefix_and_says_where_the_rest_is() {
        use crate::docparse::{BlockKind, BlockStyle, DocBlock, DocOutline};
        let blocks = (0..MAX_OUTLINE_BLOCKS + 5)
            .map(|index| DocBlock {
                kind: BlockKind::Paragraph,
                text: format!("第 {index} 行"),
                style: BlockStyle::default(),
                table: None,
            })
            .collect();
        let outline = DocOutline {
            pages: None,
            margin_pt: None,
            body_pt: None,
            blocks,
        };
        let text = structure_section(&outline, "abc.pdf");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 1 + MAX_OUTLINE_BLOCKS + 1, "头部 + 前 N 块 + 尾注");
        assert!(lines[0].contains("页数读不出"), "{}", lines[0]);
        assert!(
            lines.last().unwrap().contains(&format!("全部 {} 块", MAX_OUTLINE_BLOCKS + 5)),
            "{}",
            lines.last().unwrap()
        );
        assert!(lines.last().unwrap().contains("read_source"), "{}", lines.last().unwrap());
    }

    /// 非视觉模型 + 没配 OCR：说清「读不了」在哪两处修，不许提「用提取出来的文字」
    #[cfg(not(feature = "ocr"))]
    #[tokio::test]
    async fn an_image_without_vision_and_without_ocr_says_where_to_fix_it() {
        let (root, stored) = inbox_with(&png_bytes(4, 4), "png");
        let refs = vec![AttachRef {
            stored,
            name: "照片.png".into(),
            ext: "png".into(),
        }];
        let db = crate::db::Shared::new(root.join("app.db"));
        let materials = materials(&root, "p1", &refs, false, None, "deepseek-chat", &db).await;
        assert!(materials.section.contains("不支持看图"), "{}", materials.section);
        assert!(materials.section.contains("设置 → 模型"), "{}", materials.section);
        assert!(materials.section.contains("deepseek-chat"), "要指名道姓：{}", materials.section);
        assert!(
            !materials.section.contains("提取出来的文字"),
            "这句会让人以为有文字：{}",
            materials.section
        );
        assert!(materials.images.is_empty(), "看不了图就别塞图");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 配了 OCR 但读不出来：给原因，并给「换一张 / 贴文字」两条路
    #[tokio::test]
    async fn an_image_with_a_broken_ocr_channel_says_it_failed() {
        let (root, stored) = inbox_with(&png_bytes(4, 4), "png");
        let refs = vec![AttachRef {
            stored,
            name: "照片.png".into(),
            ext: "png".into(),
        }];
        let conf = OcrConf {
            base_url: "http://127.0.0.1:9".into(),
            api_key: "sk-test".into(),
            model: "some-vl".into(),
        };
        let db = crate::db::Shared::new(root.join("app.db"));
        let materials = materials(&root, "p1", &refs, false, Some(&conf), "deepseek-chat", &db).await;
        assert!(materials.section.contains("没读出来"), "{}", materials.section);
        assert!(
            materials.section.contains("贴过来"),
            "要给下一步：{}",
            materials.section
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn data_url_round_trips_to_bytes() {
        let raw = vec![1u8, 2, 3, 4];
        let url = crate::pdfimage::data_url_of("image/jpeg", &raw);
        assert_eq!(data_url_bytes(&url), raw);
    }

    /// ≤800 字：照旧整段拼进材料。短正文再让模型多调一次工具是白花时间
    #[tokio::test]
    async fn a_short_pdf_projects_its_whole_body() {
        let lines = ["ACME Corp - Backend Engineer", "2020 - 2022 Shanghai"];
        let bytes = crate::pdfembed::fixtures::pdf_with_text(&lines);
        let (root, stored) = inbox_with(&bytes, "pdf");
        let db = crate::db::Shared::new(root.join("app.db"));
        let refs = vec![AttachRef {
            stored: stored.clone(),
            name: "简历.pdf".into(),
            ext: "pdf".into(),
        }];
        let materials = materials(&root, "p1", &refs, false, None, "test-model", &db).await;

        assert!(materials.section.contains("ACME Corp"), "{}", materials.section);
        assert!(materials.section.contains("Shanghai"), "短正文要整段给：{}", materials.section);
        // 短正文不许走「摘要投影」：它是唯一一条会把正文换成指针的路。
        // （结构大纲的尾注里也会提 read_source，所以这里不能直接禁掉这个词）
        assert!(
            !materials.section.contains("原件正文共"),
            "没超 800 字不该把正文压成摘要：{}",
            materials.section
        );
        // 短正文照样入 Fact 层：下一轮没有附件了，正文只能从这里读
        let doc = db
            .with(|conn| crate::source::get(conn, &stored))
            .unwrap()
            .expect("正文应已落库");
        assert!(doc.text.contains("ACME Corp"), "{}", doc.text);

        let _ = std::fs::remove_dir_all(&root);
    }

    /// >800 字：Projection 只给 120 字摘要 + 一行指针，全文留在 Fact 层。
    ///
    /// 这条正是真机事故的回归 —— 2404 字的简历被整段拼进那一轮 system，
    /// 模型没写完就结束，正文就永久消失了。
    #[tokio::test]
    async fn a_long_pdf_projects_a_digest_and_a_pointer_to_the_fact_layer() {
        let lines: Vec<String> = (0..60)
            .map(|index| format!("LINE {index:02} backend engineer with ten years of experience"))
            .collect();
        let refs_text: Vec<&str> = lines.iter().map(String::as_str).collect();
        let bytes = crate::pdfembed::fixtures::pdf_with_text(&refs_text);
        let (root, stored) = inbox_with(&bytes, "pdf");
        let db = crate::db::Shared::new(root.join("app.db"));
        let refs = vec![AttachRef {
            stored: stored.clone(),
            name: "简历.pdf".into(),
            ext: "pdf".into(),
        }];
        let materials = materials(&root, "p1", &refs, false, None, "test-model", &db).await;

        let doc = db
            .with(|conn| crate::source::get(conn, &stored))
            .unwrap()
            .expect("正文应已落库");
        let chars = doc.meta.chars;
        assert!(chars > SUMMARY_CHARS, "样本要超过 {SUMMARY_CHARS} 字，实际 {chars}");
        assert!(doc.text.contains("LINE 59"), "Fact 层必须是全文");

        // 两条合法的长正文投影：没有结构大纲时给「摘要 + 字数」，有结构大纲时只给大纲。
        // 共同要求是**正文整段不许出现**，且指针里必须带 stored。
        if materials.section.contains("【原件结构】") {
            assert!(
                materials
                    .section
                    .contains(&format!("全文与完整结构调 read_source，参数 stored = {stored}）")),
                "结构大纲的尾注必须带 stored：{}",
                materials.section
            );
        } else {
            assert!(
                materials.section.contains(&format!("（原件正文共 {chars} 字")),
                "要交代总字数：{}",
                materials.section
            );
            assert!(
                materials
                    .section
                    .contains(&format!("要全文调 read_source，参数 stored = {stored}）")),
                "指针里必须带 stored：{}",
                materials.section
            );
        }
        assert!(
            !materials.section.contains("LINE 59"),
            "正文不许整段进 Projection：{}",
            materials.section
        );
        assert!(
            materials.section.contains("（附件标识：") && materials.section.contains("（原件共 1 页）"),
            "原有的两行交代不能丢：{}",
            materials.section
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 每一类附件都进 Fact 层：文本带正文，图片带一行 chars = 0 的「有这份原件」。
    ///
    /// 图片曾经被跳过（「图没有正文可存」），代价是 read_source 对它只能说
    /// 「找不到该文件」——而文件明明就在 inbox 里。
    #[tokio::test]
    async fn every_attachment_lands_in_the_fact_layer() {
        let body = "# 教育背景\n华北电力大学";
        let (root, stored) = inbox_with(body.as_bytes(), "md");
        let db = crate::db::Shared::new(root.join("app.db"));
        let refs = vec![AttachRef {
            stored: stored.clone(),
            name: "简历.md".into(),
            ext: "md".into(),
        }];
        materials(&root, "p1", &refs, false, None, "test-model", &db).await;
        let doc = db
            .with(|conn| crate::source::get(conn, &stored))
            .unwrap()
            .expect("文本附件也要有正文");
        assert_eq!(doc.text, body);

        let (png_root, png_stored) = inbox_with(&png_bytes(4, 4), "png");
        let png_db = crate::db::Shared::new(png_root.join("app.db"));
        let png_refs = vec![AttachRef {
            stored: png_stored.clone(),
            name: "照片.png".into(),
            ext: "png".into(),
        }];
        materials(&png_root, "p1", &png_refs, false, None, "test-model", &png_db).await;
        let image = png_db
            .with(|conn| crate::source::get(conn, &png_stored))
            .unwrap()
            .expect("图片也要有一行：它没有正文，但它是事实");
        assert_eq!(image.meta.chars, 0);
        assert!(image.text.is_empty());

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&png_root);
    }

    fn base64_of(bytes: &[u8]) -> String {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    /// 上传即落事实层：`attach_put` 返回时库里已经有正文，不用等 materials。
    ///
    /// 这条是真机事故的回归 —— 发送前 materials 提前失败（或根本没跑）时，
    /// `source` 那一行当时不存在，read_source 只能回 inbox 翻文件。
    #[test]
    fn upload_writes_the_fact_layer_immediately() {
        let root = std::env::temp_dir().join(format!("sr-attach-{}", crate::session::uuid_like()));
        let body = "# 教育背景\n华北电力大学 本科";
        let info = put(&root, "简历.md", &base64_of(body.as_bytes())).unwrap();

        // 只查库：这条断言证明正文不是「现场解析出来的」
        let conn = crate::db::open(&root.join("app.db")).unwrap();
        let doc = crate::source::get(&conn, &info.stored)
            .unwrap()
            .expect("上传后立刻要有一行");
        assert_eq!(doc.text, body);
        assert_eq!(doc.meta.chars, body.chars().count());
        assert_eq!(doc.meta.name, "简历.md");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// inbox 里的文件被清掉，read_source 仍然读得到全文：正文在库里，不在磁盘上。
    #[test]
    fn attach_write_survives_a_missing_inbox_file() {
        let root = std::env::temp_dir().join(format!("sr-attach-{}", crate::session::uuid_like()));
        let body = "# 项目\n靠得住的那一版";
        let info = put(&root, "简历.md", &base64_of(body.as_bytes())).unwrap();
        std::fs::remove_file(root.join("inbox").join(&info.stored)).unwrap();

        let ws = serde_json::json!({ "people": [] });
        let out = crate::tools::Outbox::default();
        let ctx = crate::tools::ToolCtx {
            workspace: &ws,
            person_id: "p1",
            resume_id: "r",
            page_id: "g",
            out: &out,
            root: Some(&root),
            facts: None,
        };
        let text =
            crate::tools::call("read_source", &serde_json::json!({ "stored": info.stored }), &ctx)
                .unwrap();
        assert!(text.contains("靠得住的那一版"), "{text}");
        assert!(
            !text.contains("不在"),
            "文件没了也不许说「找不到该文件」：{text}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 临时：把真机那份 PDF 的材料段原文打出来（`--ignored --nocapture`）
    #[tokio::test]
    #[ignore]
    async fn dump_materials_for_the_real_pdf() {
        let path = "target/debug/SharpResumeData/inbox/bcf9d0ec3fd6b381.pdf";
        let bytes = std::fs::read(path).expect("真机样本");
        let (root, stored) = inbox_with(&bytes, "pdf");
        let refs = vec![AttachRef {
            stored: stored.clone(),
            name: "谭轶天简历.pdf".into(),
            ext: "pdf".into(),
        }];
        let db = crate::db::Shared::new(root.join("app.db"));
        for vision in [false, true] {
            let materials = materials(&root, "p1", &refs, vision, None, "deepseek-chat", &db).await;
            println!("===== vision={vision} images={} =====", materials.images.len());
            println!("{}", materials.section);
            println!(
                "origin_doc = {:?}",
                materials.origin_doc.as_ref().map(|doc| (&doc.stored, &doc.name, doc.pages))
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
