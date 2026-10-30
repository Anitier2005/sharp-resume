//! `render_doc_page`：按需把原件的**某一页**渲成图片。
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
//! 只对 PDF：`.docx` / `.txt` 没有「页」这个概念（分页是别人渲染出来的），
//! 硬渲只会给出一张和真实分页无关的图，比不给更误导。

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

pub const NON_PDF: &str =
    "这种格式没有页图可渲染（只有 PDF 能渲成图）。要看内容用 read_source，要看结构看材料段的【原件结构】。";

pub const NO_VISION: &str =
    "当前模型看不了图，渲染出来也看不见。把内容读成文字：read_source。";

pub const VIEWS_FULL: &str = "这一轮已经附了 4 张图，先看完再要下一页。";

pub fn params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "stored": {
                "type": "string",
                "description": "原件标识，材料段里「（附件标识：…）」那串就是它，不是文件名"
            },
            "page": {
                "type": "integer",
                "description": "要看第几页，从 1 起，默认 1"
            },
            "width": {
                "type": "integer",
                "description": "渲染像素宽，默认 1600（够看清小字）。范围 800-2400，越界按边界算"
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
        .map_err(|error| format!("没有 stored = {stored} 的原件：{error}。材料段里那行「附件标识」抄全了吗？"))?;
    if !is_pdf(&stored, &bytes) {
        return Err(NON_PDF.into());
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

    /// 非 PDF 要一句话说清「为什么不行 + 该用什么」
    #[test]
    fn a_non_pdf_file_is_refused_with_an_exit() {
        let stored = "abcd1234.docx";
        let root = inbox_with(b"PK\x03\x04 not really a docx", stored);
        let ws = json!({ "people": [] });
        let mut out = Outbox::default();
        out.vision = true;
        let ctx = ctx_at(&root, &ws, &out);

        let error = run(&json!({ "stored": stored, "page": 1 }), &ctx).unwrap_err();
        assert_eq!(error, NON_PDF);
        assert!(error.contains("read_source"), "{error}");
        assert!(error.contains("原件结构"), "{error}");
        assert!(out.views.lock().unwrap().is_empty(), "拒绝时不该推图");

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
