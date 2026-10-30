//! `render_doc_page`：按需把原件的**某一页 / 整张原图**变成这一轮看得见的图片。
//!
//! 为什么有它：材料段现在只给摘要和指针，不再默认塞页图 —— 一份十几页的 PDF
//! 全渲一遍，光 token 就吃掉半个窗口，而绝大多数轮次只关心第 1 页。
//! 于是「想看哪页就自己要哪页」：模型读到「原件结构」里写着第 3 页有照片位，
//! 就传 `page: 3` 把它渲出来看。
//!
//! 与 `view_asset` 的分工：
//!   - `view_asset` 按 assetId 看材料（图片直接附、PDF 渲前几页）；
//!   - 这里按 `stored` 指名页码，清晰度可调，一次一页 —— 材料段里的「附件标识」
//!     就是 `stored`，模型手上没有 assetId 也能用。
//!
//! 三种原件三种处理：
//!   - PDF：渲染第 N 页（分页由 PDF 自己定义，渲出来的就是真分页）；
//!   - 图片：原图直接附上（它本来就是一张图，再渲一遍只会更糊）；
//!   - 其它（docx / pptx / xlsx / txt…）：没有「页」这个概念（分页是别人渲染出来的），
//!     硬渲只会给出一张和真实分页无关的图，比不给更误导 —— 所以指路去 `read_source`。

use serde_json::{json, Value};

use super::ToolCtx;
use crate::pdfimage;

/// 缺省渲染宽度：1600px 宽的 A4 ≈ 190dpi，小六号字和表格线都还看得清。
/// 「完整清晰」是这个工具的定位 —— 模型主动要图的时候，本来就是为了看清楚。
pub const DEFAULT_WIDTH: u32 = 1600;
/// 下限：再窄字段名会糊，看了也判断不了版式
pub const MIN_WIDTH: u32 = 800;
/// 上限：2400px 一页 JPEG 已过 300dpi，再大只是烧 token 和内存
pub const MAX_WIDTH: u32 = 2400;
/// 一轮最多几张：超过这个数模型也看不过来，反而挤掉正文
pub const MAX_VIEWS: usize = 4;
/// 一张原图超过这个体积就别直接附：base64 之后还要再涨三分之一，
/// 一次就可能把整个上下文吃掉（与 `view_asset` 同一道闸）
const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;

/// 渲不成页图的类型：不说「不行」，说「该怎么看」。
///
/// 原来这里是一句「只有 PDF 能渲成图」，模型读完只能放弃 —— 而它真正想要的内容，
/// `read_source` 里就有，只是没人告诉它。
pub const NON_PAGE: &str =
    "这类原件渲不成页图，用 read_source 读正文（它带格式层级）；要看成什么样请用户在 Word / 预览里打开。";

pub const NO_VISION: &str =
    "当前模型看不了图，渲染出来也看不见。把内容读成文字：read_source。";

pub const VIEWS_FULL: &str = "这一轮已经附了 4 张图，先看完再要下一页。";

pub fn params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "stored": {
                "type": "string",
                "description": "原件的 stored（list_sources 里那个），不是文件名。材料段里的「（附件标识：…）」也是它"
            },
            "page": {
                "type": "integer",
                "description": "PDF 要看第几页，从 1 起，默认 1。图片原件用不到这个参数"
            },
            "width": {
                "type": "integer",
                "description": "渲染像素宽，默认 1600（够看清小字）。范围 800-2400，越界按边界算。图片原件按原分辨率给，不受它影响"
            }
        },
        "required": ["stored"],
        "additionalProperties": false
    })
}

/// `width` 缺省 1600，越界钳到 [800, 2400]。
///
/// 钳而不是报错：模型传 400 是真的看不清，传 4000 是真的浪费 —— 两个都被纠正到
/// 一个能用的值，比让它为参数格式再跑一轮好。
fn width_of(args: &Value) -> u32 {
    args.get("width")
        .and_then(Value::as_u64)
        .map(|value| value as u32)
        .unwrap_or(DEFAULT_WIDTH)
        .clamp(MIN_WIDTH, MAX_WIDTH)
}

/// `page` 1 起，缺省 1。
fn page_of(args: &Value) -> usize {
    args.get("page")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(1)
        .max(1)
}

/// 只对 PDF。扩展名和字节各看一半：存成 `<sha>.bin` 的 PDF 也认，
/// 而把一张 `foo.pdf` 的截图重命名的冒牌货会被字节那一关挡掉 —— 但两关是「或」，
/// 只要有一边像 PDF 就往下走，真打不开时渲染器自己会报「这份 PDF 打不开」。
fn is_pdf(stored: &str, bytes: &[u8]) -> bool {
    stored.to_ascii_lowercase().ends_with(".pdf") || bytes.starts_with(b"%PDF")
}

/// 扩展名：stored 的后缀（后端生成的名字，值可信）。
fn ext_of(stored: &str) -> String {
    stored
        .rsplit_once('.')
        .map(|(_, tail)| tail.to_ascii_lowercase())
        .unwrap_or_default()
}

/// 图片的宽高。只读文件头，不解全图 —— 一张 4000×3000 的照片全解一遍要几十 MB 内存。
///
/// `None` = 这不是一张能认出来的图片（扩展名说它是，字节说它不是）。
fn image_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    reader.into_dimensions().ok()
}

fn out_of_range(pages: usize, page: usize) -> String {
    format!("这份 PDF 只有 {pages} 页，第 {page} 页不存在。")
}

pub fn run(args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let stored = args
        .get("stored")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    if stored.is_empty() {
        return Err("要给 stored：材料段里那行「（附件标识：…）」就是它".into());
    }
    let page = page_of(args);
    let width = width_of(args);

    // 看不了图就先别渲：命令一跑一秒钟，渲染出来也只是一串模型读不到的 base64。
    // 这一步放在读文件之前 —— 文档不存在之类的报错对非视觉模型毫无意义。
    if !ctx.out.vision {
        return Err(NO_VISION.into());
    }
    // 上限先拦：这一轮已经贴了 4 张，再渲只会把前面几张挤出上下文
    if ctx.out.views.lock().map_err(|_| "图片队列不可用".to_string())?.len() >= MAX_VIEWS {
        return Err(VIEWS_FULL.into());
    }

    let root = ctx
        .root
        .ok_or_else(|| "当前环境拿不到数据目录（浏览器调试模式没有原件）".to_string())?;
    let bytes = crate::attach::read(root, &stored)
        .map_err(|error| format!("没有 stored = {stored} 的原件：{error}。清单用 list_sources 列一次，stored 从那里抄。"))?;
    if !is_pdf(&stored, &bytes) {
        return render_other(&stored, &bytes, ctx);
    }

    // 页数用 lopdf 数（纯 Rust、几毫秒），不用为了问一句「有几页」先渲一遍。
    // 读不出（加密文档）就不拦，交给下面的渲染结果兜底。
    if let Some(total) = crate::pdfembed::page_count(&bytes) {
        if page > total as usize {
            return Err(out_of_range(total as usize, page));
        }
    }

    // `pdf_pages` 渲的是「前 N 页」：要到第 page 页就渲 page 页、取最后一张。
    // 更省的做法需要一个「从第几页起」的入口（WinRT 侧是 `GetPage(index)`，
    // 命令行退路是 `-f/-dFirstPage`），那要改 `pdfimage` 的公共签名；
    // 现在一次最多多渲 3 页，不值得为它动另一条链路的接口。
    let images = pdfimage::pdf_pages(&bytes, page, width)?;
    let Some(image) = images.last() else {
        return Err("这一页没渲染出来（渲染器返回了空结果）。".into());
    };
    // page_count 读不出时的兜底：渲染器只给了 images.len() 张，说明原件就这么多页
    if images.len() < page {
        return Err(out_of_range(images.len(), page));
    }

    super::push_view(ctx, &pdfimage::data_url(&image.jpeg))?;
    Ok(format!(
        "第 {page} 页已作为图片附在下一轮（{width}px 宽）。看完再排版／回答。"
    ))
}

/// 非 PDF 的两条路：图片直接附原图，其余指路 `read_source`。
///
/// 为什么图片不走渲染：它本来就是像素，渲染器只会再压一遍、更糊。
/// 宽高要从文件头读（`image_size`），读不出（gif / bmp / avif 这几个没编解码器）
/// 也照样附 —— 「读不出宽高」不是「看不了」。
fn render_other(stored: &str, bytes: &[u8], ctx: &ToolCtx) -> Result<String, String> {
    let ext = ext_of(stored);
    let declared_image = crate::attach::is_image(&ext);
    let size = image_size(bytes);
    if size.is_none() && !declared_image {
        return Err(NON_PAGE.into());
    }
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(format!(
            "这张图太大（{}MB），直接附上会把上下文挤爆。请用户压一版再传（或另存成 jpg）。",
            bytes.len() / 1024 / 1024
        ));
    }
    super::push_view(ctx, &crate::pdfimage::data_url_of(mime_of(&ext), bytes))?;
    Ok(match size {
        Some((width, height)) => format!("已附上原图（{width}×{height}）。"),
        None => "已附上原图（宽高读不出，按原样附的）。".to_string(),
    })
}

fn mime_of(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "avif" => "image/avif",
        _ => "image/jpeg",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{Outbox, ToolCtx};

    /// 临时数据目录 + inbox 里放一份文件（stored 就是那个文件名）
    fn inbox_with(bytes: &[u8], stored: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("sr-render-{}", crate::session::uuid_like()));
        std::fs::create_dir_all(root.join("inbox")).unwrap();
        std::fs::write(root.join("inbox").join(stored), bytes).unwrap();
        root
    }

    fn ctx_at<'a>(root: &'a std::path::Path, ws: &'a Value, out: &'a Outbox) -> ToolCtx<'a> {
        ToolCtx {
            workspace: ws,
            person_id: "p",
            resume_id: "r",
            page_id: "g",
            out,
            root: Some(root),
            facts: None,
        }
    }

    /// 清晰度可控：缺省 1600，越界钳到 800 / 2400，而不是让模型为参数再跑一轮
    #[test]
    fn width_is_clamped_into_a_readable_range() {
        assert_eq!(width_of(&json!({})), DEFAULT_WIDTH);
        assert_eq!(width_of(&json!({ "width": 1200 })), 1200);
        assert_eq!(width_of(&json!({ "width": 100 })), MIN_WIDTH);
        assert_eq!(width_of(&json!({ "width": 99_999 })), MAX_WIDTH);
        // 页面从 1 起：传 0 当第 1 页
        assert_eq!(page_of(&json!({})), 1);
        assert_eq!(page_of(&json!({ "page": 0 })), 1);
        assert_eq!(page_of(&json!({ "page": 3 })), 3);
    }

    /// docx 渲不成页图，但回答不许止于「不行」：要说清正文在哪、版式找谁看
    #[test]
    fn render_refuses_a_type_it_cannot_draw_but_says_what_to_do() {
        let stored = "abcd1234.docx";
        let root = inbox_with(b"PK\x03\x04 not really a docx", stored);
        let ws = json!({ "people": [] });
        let mut out = Outbox::default();
        out.vision = true;
        let ctx = ctx_at(&root, &ws, &out);

        let error = run(&json!({ "stored": stored, "page": 1 }), &ctx).unwrap_err();
        assert_eq!(error, NON_PAGE);
        assert!(error.contains("read_source"), "{error}");
        assert!(error.contains("格式层级"), "{error}");
        assert!(error.contains("Word"), "要告诉用户去哪儿看：{error}");
        assert!(!error.contains("只有 PDF 能"), "旧文案不该再出现：{error}");
        assert!(out.views.lock().unwrap().is_empty(), "渲不出来就别推图");

        let _ = std::fs::remove_dir_all(&root);
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

    /// 图片原件：原图直接附（不再渲染），并报出真实宽高
    #[test]
    fn an_image_attachment_is_attached_as_the_original() {
        let stored = "1234abcd5678ef90.png";
        let root = inbox_with(&png_bytes(6, 4), stored);
        let ws = json!({ "people": [] });
        let mut out = Outbox::default();
        out.vision = true;
        let ctx = ctx_at(&root, &ws, &out);

        let text = run(&json!({ "stored": stored }), &ctx).unwrap();
        assert!(text.contains("已附上原图"), "{text}");
        assert!(text.contains("6×4"), "要报真实宽高：{text}");
        let views = out.views.lock().unwrap();
        assert_eq!(views.len(), 1);
        assert!(views[0].starts_with("data:image/png;base64,"), "{}", views[0]);

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 页数越界：报出真实页数，别让模型以为「再往后还有」
    #[test]
    fn a_page_beyond_the_end_names_the_real_count() {
        let stored = "feedfacefeedface.pdf";
        // 两页的最小 PDF；这里不会走到渲染（越界在渲染前就拦掉了）
        let root = inbox_with(&crate::pdfembed::fixtures::pdf_with_pages(2), stored);
        let ws = json!({ "people": [] });
        let mut out = Outbox::default();
        out.vision = true;
        let ctx = ctx_at(&root, &ws, &out);

        let error = run(&json!({ "stored": stored, "page": 5 }), &ctx).unwrap_err();
        assert_eq!(error, out_of_range(2, 5));
        assert!(error.contains("只有 2 页"), "{error}");
        assert!(out.views.lock().unwrap().is_empty(), "越界时不该推图");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 非视觉模型：不渲染，直接告诉它「把内容读成文字」
    #[test]
    fn a_non_vision_model_is_sent_to_read_source() {
        let root = std::env::temp_dir().join(format!("sr-render-{}", crate::session::uuid_like()));
        let ws = json!({ "people": [] });
        let out = Outbox::default(); // vision 默认 false
        let ctx = ctx_at(&root, &ws, &out);

        let error = run(&json!({ "stored": "deadbeefdeadbeef.pdf" }), &ctx).unwrap_err();
        assert_eq!(error, NO_VISION);
        assert!(error.contains("read_source"), "{error}");
        assert!(out.views.lock().unwrap().is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 一轮最多 4 张：第 5 张要被拦下，且拦在读文件之前（不留半张图）
    #[test]
    fn a_turn_caps_at_four_views() {
        let root = std::env::temp_dir().join(format!("sr-render-{}", crate::session::uuid_like()));
        let ws = json!({ "people": [] });
        let mut out = Outbox::default();
        out.vision = true;
        for index in 0..MAX_VIEWS {
            out.views.lock().unwrap().push(format!("data:image/jpeg;base64,{index}"));
        }
        let ctx = ctx_at(&root, &ws, &out);

        let error = run(&json!({ "stored": "missing.pdf", "page": 2 }), &ctx).unwrap_err();
        assert_eq!(error, VIEWS_FULL);
        assert_eq!(out.views.lock().unwrap().len(), MAX_VIEWS);

        let _ = std::fs::remove_dir_all(&root);
    }

    /// render_doc_page 必须真的能被调到工具自身的判断：版面在纸上，
    /// 看不到原件版式就只能瞎猜。
    #[test]
    fn render_doc_page_reaches_its_own_vision_guard() {
        let workspace = json!({ "people": [] });
        let out = Outbox::default(); // 非视觉：走到工具自己的判断
        let ctx = ToolCtx::of(&workspace, "p", "r", "g", &out);

        let error = crate::tools::call("render_doc_page", &json!({ "stored": "x.pdf" }), &ctx)
            .unwrap_err();
        assert!(!error.contains("没有名为"), "{error}");
        assert_eq!(error, NO_VISION);
    }
}
