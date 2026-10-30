//! 极简 DOCX 生成：手写 OOXML + store（不压缩）zip，不加任何依赖。
//!
//! 为什么在 Rust：导出是数据处理，产物结构不该由页面层决定。
//! 搬到这里之后，前端只负责发起「导出这一页」，不再拼 XML 也不写字。
//!
//! v5 起按 block 排版，代码里没有「姓名 / 公司 / 教育」这类预设键：
//!   fields 块的第一个字段 = 大标题，其余按「标签：值」；
//!   section 块的标题就是用户写的那句话，条目第一个字段加粗；
//!   media 块在正文里留一行「见图」，图片本体旁挂到附件目录（不硬塞进 docx，
//!   否则一张 8MB 的证书会把简历文档撑得没法发）。

use std::collections::HashMap;

use crate::model::{Asset, Block, Field, Paper, ResumePage};

const ACCENT_HEX_WHITE: &str = "242422";

/// 版面度量：字号 px + 行距倍数。**由调用方按 preset 派生后传进来**。
///
/// 不在排版函数里写死数字，是因为同一页在纸上（CSS）和 Word 里必须同源：
/// 这里多一份常量，模型换了 preset 之后两边就会悄悄错开。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    pub font_px: f64,
    /// CSS `line-height`，1.0 = 单倍
    pub line: f64,
}

/// preset → 字号/行距。与 `render/paper.css` 的 `--fs` / `--lh` 一一对应，
/// 改 CSS 时这里要跟着改 —— **Rust 侧只此一份**。
pub fn preset_metrics(preset: &str) -> Metrics {
    match preset.trim() {
        "compact" => Metrics { font_px: 9.4, line: 1.55 },
        "roomy" => Metrics { font_px: 10.6, line: 1.92 },
        "dense" => Metrics { font_px: 9.6, line: 1.42 },
        _ => Metrics { font_px: 10.0, line: 1.7 },
    }
}

/// px → twips（1 inch = 96px = 1440 twips）
pub fn px_to_twips(px: f64) -> u32 {
    (px / 96.0 * 1440.0).round().max(0.0) as u32
}

/// mm → twips（1 inch = 25.4mm = 1440 twips）
fn mm_to_twips(mm: f64) -> i64 {
    (mm / 25.4 * 1440.0).round() as i64
}

/// 渲染层 `.abs` 块的结构化描述。
///
/// 屏幕上是 `position:absolute; left/top/width`，Word 里最接近的对应物是
/// 「文字框架」`w:framePr`（绝对定位段落最便宜的等价物）。
/// 坐标相对**版心左上角** —— CSS 里绝对定位元素落在内容盒上，两者同源；
/// 换 twips 时再加上页边距才是「相对页边」的 `w:x` / `w:y`。
#[derive(Debug, Clone, PartialEq)]
pub struct Absolute {
    pub x_px: f64,
    pub y_px: f64,
    pub w_px: f64,
    pub text: String,
    pub bold: bool,
}

/// 页头照片（放在右上角）。
///
/// docx **不内嵌图片**（一张 8MB 的证书会把简历撑得发不出去，图本体照旧旁挂）。
/// 这里只留一个定尺寸的框架占位，让版式和屏幕上一致 —— 见文件末尾「表达不了的部分」。
#[derive(Debug, Clone, PartialEq)]
pub struct Photo {
    pub w_px: f64,
    pub h_px: f64,
    pub label: String,
}

impl Default for Photo {
    fn default() -> Self {
        Self { w_px: 62.0, h_px: 82.0, label: "［照片］".into() }
    }
}

/// 一页 docx 的排版参数。
///
/// 页边距与纸张**必须**从 `Paper` 派生：`--pad` / `@page` / `pgMar` 三处同源，
/// 这里是第三处，回退成固定 1134 会让「屏幕上 20mm、Word 里 18mm」复活。
#[derive(Debug, Clone)]
pub struct DocxOptions {
    pub metrics: Metrics,
    /// `[上, 右, 下, 左]`，毫米，与 `Paper::margin` 同序
    pub margin_mm: [f64; 4],
    /// 纸张宽高，毫米
    pub paper_mm: (f64, f64),
    /// 页头（第一个 fields 块）居中：原件版式是「姓名居中 + 联系方式居中」
    pub header_center: bool,
    /// `.abs` 块，按文档顺序输出；框架段落不占正文流，放最前面
    pub absolutes: Vec<Absolute>,
    /// 页头照片；留空时自动从页头字段里的图片项推断
    pub photo: Option<Photo>,
}

impl DocxOptions {
    pub fn new(paper: &Paper, preset: &str) -> Self {
        Self {
            metrics: preset_metrics(preset),
            margin_mm: paper.margin,
            paper_mm: paper.size_mm(),
            header_center: true,
            absolutes: Vec::new(),
            photo: None,
        }
    }

    /// 从页自身取预设：`layout.preset` → 旧数据 `style.density` → classic，
    /// 与 `render::pick_preset` 同一套回落顺序（那边认不出的值也是回默认）。
    pub fn for_page(paper: &Paper, page: &ResumePage) -> Self {
        let preset = page
            .layout
            .as_ref()
            .map(|layout| layout.preset.trim())
            .filter(|preset| !preset.is_empty())
            .map(str::to_string)
            .or_else(|| {
                page.style
                    .as_ref()
                    .and_then(|style| style.get("density"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "classic".into());
        Self::new(paper, &preset)
    }

    /// 各级字号按正文字号等比缩放：classic（10px）时正好是原来的 34 / 21 / 18 / 17 半点
    fn sz(&self, base: u32) -> u32 {
        (base as f64 * self.metrics.font_px / 10.0).round().max(1.0) as u32
    }

    /// 行距：`lineRule="auto"` 下 `w:line` 的单位是 1/240 行
    fn line(&self) -> u32 {
        (self.metrics.line * 240.0).round().max(1.0) as u32
    }

    fn margin_twips(&self) -> [i64; 4] {
        self.margin_mm.map(mm_to_twips)
    }

    /// 版心宽（twips）= 纸宽 − 左右边距
    fn content_width_twips(&self) -> i64 {
        let margin = self.margin_twips();
        (mm_to_twips(self.paper_mm.0) - margin[1] - margin[3]).max(0)
    }
}

fn accent_hex(accent: &str) -> &'static str {
    match accent {
        "indigo" => "3B50B4",
        "pine" => "2A6650",
        "clay" => "B2452C",
        _ => ACCENT_HEX_WHITE,
    }
}

/// OOXML 只允许这些字符，控制符会让 Word 报文件损坏
fn clean(value: &str) -> String {
    value
        .chars()
        .filter(|ch| {
            let code = *ch as u32;
            code == 9 || code == 10 || code == 13 || code >= 32
        })
        .collect()
}

fn esc(value: &str) -> String {
    clean(value)
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn run(text: &str, bold: bool, size: u32, color: &str) -> String {
    let mut props = String::new();
    if bold {
        props.push_str("<w:b/>");
    }
    props.push_str(&format!("<w:color w:val=\"{color}\"/>"));
    props.push_str(&format!("<w:sz w:val=\"{size}\"/><w:szCs w:val=\"{size}\"/>"));
    format!(
        "<w:r><w:rPr>{props}</w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r>",
        esc(text)
    )
}

fn para(text: &str, bold: bool, size: u32, color: &str, opts: &DocxOptions, center: bool) -> String {
    let jc = if center { "<w:jc w:val=\"center\"/>" } else { "" };
    format!(
        "<w:p><w:pPr><w:spacing w:after=\"40\" w:line=\"{line}\" w:lineRule=\"auto\"/>{jc}</w:pPr>{}</w:p>",
        run(text, bold, opts.sz(size), color),
        line = opts.line()
    )
}

/// 绝对定位段落：`w:framePr` 把这一段从正文流里摘出来，按页边坐标摆放。
///
/// `w:hAnchor` / `w:vAnchor` 都是 `page`：屏幕上的 `left/top` 也是相对纸边量的。
/// `h` 只有照片这种知道高度的才给；纯文字用 `hRule="auto"` 让 Word 自己长。
fn frame_para(
    width: u32,
    height: Option<u32>,
    x: i64,
    y: i64,
    h_rule: &str,
    inner: &str,
    opts: &DocxOptions,
) -> String {
    let height = height.map(|value| format!(" w:h=\"{value}\"")).unwrap_or_default();
    format!(
        "<w:p><w:pPr><w:framePr w:w=\"{width}\"{height} w:hAnchor=\"page\" w:vAnchor=\"page\" \
         w:x=\"{x}\" w:y=\"{y}\" w:wrap=\"around\" w:hRule=\"{h_rule}\"/>\
         <w:spacing w:before=\"0\" w:after=\"0\" w:line=\"{line}\" w:lineRule=\"auto\"/></w:pPr>{inner}</w:p>",
        line = opts.line()
    )
}

/// `.abs` 块 → `framePr` 段落。坐标加页边距：CSS 的 left/top 落在内容盒上，
/// Word 的 `w:x` / `w:y` 从纸边量，差的正好是那一圈边距。
fn absolute_para(abs: &Absolute, opts: &DocxOptions) -> String {
    let margin = opts.margin_twips();
    let x = (px_to_twips(abs.x_px) as i64 + margin[3]).max(0);
    let y = (px_to_twips(abs.y_px) as i64 + margin[0]).max(0);
    frame_para(
        px_to_twips(abs.w_px),
        None,
        x,
        y,
        "auto",
        &run(&abs.text, abs.bold, opts.sz(17), "55554F"),
        opts,
    )
}

/// 页头照片 → 右上角的固定框架：`w:x` = 版心宽 − 照片宽，`w:y` = 0（贴内容顶）
fn photo_para(photo: &Photo, opts: &DocxOptions) -> String {
    let margin = opts.margin_twips();
    let width = px_to_twips(photo.w_px);
    let x = (opts.content_width_twips() - width as i64).max(0) + margin[3];
    frame_para(
        width,
        Some(px_to_twips(photo.h_px)),
        x,
        margin[0],
        "atLeast",
        &run(&photo.label, false, opts.sz(17), "77776F"),
        opts,
    )
}

/// 页头里那一项图片字段就是照片；没有就退回人身上第一张（与纸面同一套规则的那部分
/// 已由渲染层决定，这里只看页头自己有没有显式指定）
fn photo_of_page(page: &ResumePage, opts: &DocxOptions) -> Option<Photo> {
    if let Some(photo) = &opts.photo {
        return Some(photo.clone());
    }
    let head = page.blocks.first().filter(|block| block.kind == "fields")?;
    head.fields.iter().find(|field| field.image_id().is_some())?;
    Some(Photo::default())
}

fn field_text(field: &Field) -> String {
    let value = field.value.trim();
    if field.label.trim().is_empty() {
        return value.to_string();
    }
    if value.is_empty() {
        format!("{}：", field.label)
    } else {
        format!("{}：{}", field.label, value)
    }
}

/// 本页字段优先；为空时回落到库里那一条（与纸面、前端同一套规则）
fn entry_fields<'a>(entry: &'a crate::model::SectionEntry, library: &'a [crate::model::LibraryItem]) -> &'a [Field] {
    if !entry.fields.is_empty() {
        return &entry.fields;
    }
    entry
        .ref_id
        .as_deref()
        .and_then(|id| library.iter().find(|item| item.id == id))
        .map(|item| item.fields.as_slice())
        .unwrap_or(&[])
}

fn block_xml(
    block: &Block,
    library: &[crate::model::LibraryItem],
    assets: &HashMap<String, Asset>,
    hex: &str,
    opts: &DocxOptions,
    center: bool,
    out: &mut Vec<String>,
) {
    match block.kind.as_str() {
        "fields" => {
            // 图片字段单独走 framePr（照片位），不能混在「标签：值」里变成半句话
            let mut iter = block.fields.iter().filter(|field| field.image_id().is_none());
            if let Some(lead) = iter.next() {
                let text = if lead.value.trim().is_empty() { &lead.label } else { &lead.value };
                out.push(para(text, true, 34, hex, opts, center));
            }
            for field in iter {
                out.push(para(&field_text(field), false, 17, "77776F", opts, center));
            }
        }
        "section" => {
            if !block.title.trim().is_empty() {
                out.push(para(&block.title, true, 21, hex, opts, false));
            }
            for entry in &block.items {
                for (index, field) in entry_fields(entry, library).iter().enumerate() {
                    if index == 0 {
                        let text = if field.value.trim().is_empty() { &field.label } else { &field.value };
                        out.push(para(text, true, 21, "141412", opts, false));
                    } else {
                        out.push(para(&field_text(field), false, 18, "55554F", opts, false));
                    }
                }
                for bullet in &entry.bullets {
                    if !bullet.trim().is_empty() {
                        out.push(para(&format!("· {bullet}"), false, 18, "55554F", opts, false));
                    }
                }
            }
        }
        "media" => {
            let name = block
                .asset_id
                .as_deref()
                .and_then(|id| assets.get(id))
                .map(|asset| asset.name.as_str())
                .unwrap_or("附件");
            let caption = block.caption.as_deref().unwrap_or("");
            let line = if caption.trim().is_empty() {
                format!("［图：{name}］")
            } else {
                format!("［图：{caption}（{name}）］")
            };
            out.push(para(&line, false, 17, "77776F", opts, false));
        }
        _ => {}
    }
}

fn document_xml(
    page: &ResumePage,
    library: &[crate::model::LibraryItem],
    assets: &HashMap<String, Asset>,
    hex: &str,
    opts: &DocxOptions,
) -> String {
    let mut body: Vec<String> = Vec::new();

    // 框架段落不占正文流：先放，正文从内容顶照常开始
    if let Some(photo) = photo_of_page(page, opts) {
        body.push(photo_para(&photo, opts));
    }
    for abs in &opts.absolutes {
        body.push(absolute_para(abs, opts));
    }

    let header = page.blocks.first().filter(|block| block.kind == "fields").map(|block| block.id.as_str());
    for block in &page.blocks {
        let center = opts.header_center && Some(block.id.as_str()) == header;
        block_xml(block, library, assets, hex, opts, center, &mut body);
    }
    if body.is_empty() {
        body.push(para("（空白页）", false, 18, "999990", opts, false));
    }

    let margin = opts.margin_twips();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{}<w:sectPr><w:pgSz w:w=\"{}\" w:h=\"{}\"/><w:pgMar w:top=\"{}\" w:right=\"{}\" w:bottom=\"{}\" w:left=\"{}\"/></w:sectPr></w:body></w:document>",
        body.join(""),
        mm_to_twips(opts.paper_mm.0),
        mm_to_twips(opts.paper_mm.1),
        margin[0],
        margin[1],
        margin[2],
        margin[3],
    )
}

const STYLES_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"Segoe UI\" w:hAnsi=\"Segoe UI\" w:eastAsia=\"微软雅黑\" w:cs=\"Segoe UI\"/><w:color w:val=\"1F1F1C\"/><w:sz w:val=\"21\"/><w:szCs w:val=\"21\"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after=\"0\" w:line=\"276\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:qFormat/></w:style></w:styles>";

const CONTENT_TYPES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/><Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/><Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/></Types>";

const ROOT_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/></Relationships>";

const DOC_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/></Relationships>";

fn core_xml(title: &str, stamp: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><dc:title>{}</dc:title><dc:creator>Sharp Resume</dc:creator><cp:lastModifiedBy>Sharp Resume</cp:lastModifiedBy><dcterms:created xsi:type=\"dcterms:W3CDTF\">{stamp}</dcterms:created><dcterms:modified xsi:type=\"dcterms:W3CDTF\">{stamp}</dcterms:modified></cp:coreProperties>",
        esc(title)
    )
}

/* ---------- zip（store，不压缩） ---------- */

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= *byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    crc ^ 0xffff_ffff
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// DOS 时间戳。固定成 1980-01-01：zip 是内容容器，时间戳只会让同样的内容
/// 每次导出都不一样的字节，反而妨碍比对。
const DOS_TIME: u16 = 0;
const DOS_DATE: u16 = 0x0021;

fn zip(files: &[(&str, &str)]) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();

    for (name, text) in files {
        let name_bytes = name.as_bytes();
        let data = text.as_bytes();
        let crc = crc32(data);
        let offset = body.len() as u32;

        push_u32(&mut body, 0x0403_4b50);
        push_u16(&mut body, 20);
        push_u16(&mut body, 0x0800);
        push_u16(&mut body, 0);
        push_u16(&mut body, DOS_TIME);
        push_u16(&mut body, DOS_DATE);
        push_u32(&mut body, crc);
        push_u32(&mut body, data.len() as u32);
        push_u32(&mut body, data.len() as u32);
        push_u16(&mut body, name_bytes.len() as u16);
        push_u16(&mut body, 0);
        body.extend_from_slice(name_bytes);
        body.extend_from_slice(data);

        push_u32(&mut central, 0x0201_4b50);
        push_u16(&mut central, 20);
        push_u16(&mut central, 20);
        push_u16(&mut central, 0x0800);
        push_u16(&mut central, 0);
        push_u16(&mut central, DOS_TIME);
        push_u16(&mut central, DOS_DATE);
        push_u32(&mut central, crc);
        push_u32(&mut central, data.len() as u32);
        push_u32(&mut central, data.len() as u32);
        push_u16(&mut central, name_bytes.len() as u16);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u32(&mut central, 0);
        push_u32(&mut central, offset);
        central.extend_from_slice(name_bytes);
    }

    let central_size = central.len() as u32;
    let central_offset = body.len() as u32;
    let mut end: Vec<u8> = Vec::new();
    push_u32(&mut end, 0x0605_4b50);
    push_u16(&mut end, 0);
    push_u16(&mut end, 0);
    push_u16(&mut end, files.len() as u16);
    push_u16(&mut end, files.len() as u16);
    push_u32(&mut end, central_size);
    push_u32(&mut end, central_offset);
    push_u16(&mut end, 0);

    let mut out = body;
    out.extend_from_slice(&central);
    out.extend_from_slice(&end);
    out
}

/* ---------- 出口 ---------- */

/// 生成一页 docx 的字节。
pub fn page_docx(
    person_name: &str,
    page: &ResumePage,
    library: &[crate::model::LibraryItem],
    assets: &HashMap<String, Asset>,
    accent: &str,
    opts: &DocxOptions,
) -> Vec<u8> {
    let hex = accent_hex(accent);
    // 固定时间戳：内容相同则字节相同，方便校验与去重
    let stamp = "2026-01-01T00:00:00Z";
    let title = if page.label.trim().is_empty() {
        person_name.to_string()
    } else {
        format!("{person_name} · {}", page.label)
    };
    let document = document_xml(page, library, assets, hex, opts);
    let core = core_xml(&title, stamp);
    zip(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("docProps/core.xml", &core),
        ("word/_rels/document.xml.rels", DOC_RELS),
        ("word/document.xml", &document),
        ("word/styles.xml", STYLES_XML),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn page_with(blocks: serde_json::Value) -> ResumePage {
        serde_json::from_value(json!({
            "id": "pg1",
            "kind": "native",
            "label": "中文简历",
            "blocks": blocks,
        }))
        .unwrap()
    }

    fn opts_for(preset: &str) -> DocxOptions {
        DocxOptions::new(&Paper::default(), preset)
    }

    fn document_of(page: &ResumePage, opts: &DocxOptions) -> String {
        document_xml(page, &[], &HashMap::new(), "242422", opts)
    }

    #[test]
    fn docx_starts_with_zip_signature() {
        let page = page_with(json!([
            { "id": "b1", "kind": "fields", "fields": [{ "id": "f1", "label": "姓名", "value": "张三" }] }
        ]));
        let bytes = page_docx("张三", &page, &[], &HashMap::new(), "white", &opts_for("classic"));
        assert_eq!(&bytes[..4], b"PK\x03\x04");
        // 中央目录结尾在尾部
        assert_eq!(&bytes[bytes.len() - 22..bytes.len() - 18], b"PK\x05\x06");
    }

    #[test]
    fn crc32_matches_known_value() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }

    #[test]
    fn control_chars_are_dropped_and_xml_escaped() {
        assert_eq!(clean("a\u{0}b\u{7}c"), "abc");
        assert_eq!(esc("a & b < c"), "a &amp; b &lt; c");
    }

    #[test]
    fn media_block_does_not_panic_without_asset() {
        let page = page_with(json!([
            { "id": "b1", "kind": "media", "assetId": "missing", "caption": "报关员证书" }
        ]));
        let bytes = page_docx("张三", &page, &[], &HashMap::new(), "pine", &opts_for("dense"));
        assert!(!bytes.is_empty());
    }

    #[test]
    fn px_to_twips_lands_on_a4_width() {
        assert_eq!(px_to_twips(96.0), 1440);
        assert_eq!(px_to_twips(793.7), 11906, "A4 宽 793.7px → 11906 twips");
        assert_eq!(px_to_twips(0.0), 0);
    }

    #[test]
    fn absolute_block_becomes_a_page_anchored_frame() {
        let page = page_with(json!([
            { "id": "b1", "kind": "fields", "fields": [{ "id": "f1", "label": "姓名", "value": "张三" }] }
        ]));
        let mut opts = opts_for("classic");
        opts.absolutes.push(Absolute {
            x_px: 480.0,
            y_px: 64.0,
            w_px: 250.0,
            text: "独立文本框".into(),
            bold: true,
        });
        let xml = document_of(&page, &opts);
        assert!(xml.contains("w:framePr"), "{xml}");
        assert!(xml.contains("w:hAnchor=\"page\""), "{xml}");
        assert!(xml.contains("w:vAnchor=\"page\""), "{xml}");
        // 480px = 7200twips + 20mm(1134) 左边距
        assert!(xml.contains("w:x=\"8334\""), "{xml}");
        assert!(xml.contains("独立文本框"), "{xml}");
    }

    #[test]
    fn header_paragraph_is_centered_and_contacts_follow() {
        let page = page_with(json!([
            { "id": "b1", "kind": "fields", "fields": [
                { "id": "f1", "label": "姓名", "value": "张三" },
                { "id": "f2", "label": "电话", "value": "13800000000" }
            ]}
        ]));
        let xml = document_of(&page, &opts_for("classic"));
        assert!(xml.contains("<w:jc w:val=\"center\"/>"), "{xml}");
        assert_eq!(xml.matches("<w:jc w:val=\"center\"/>").count(), 2, "姓名与联系方式都居中");
    }

    #[test]
    fn centering_can_be_turned_off() {
        let page = page_with(json!([
            { "id": "b1", "kind": "fields", "fields": [{ "id": "f1", "label": "姓名", "value": "张三" }] }
        ]));
        let mut opts = opts_for("classic");
        opts.header_center = false;
        assert!(!document_of(&page, &opts).contains("w:jc"));
    }

    #[test]
    fn photo_field_becomes_a_top_right_frame_and_leaves_the_flow() {
        let page = page_with(json!([
            { "id": "b1", "kind": "fields", "fields": [
                { "id": "f1", "label": "姓名", "value": "张三" },
                { "id": "f2", "label": "照片", "value": "", "assetId": "a1" }
            ]}
        ]));
        let mut opts = opts_for("classic");
        opts.photo = Some(Photo { w_px: 62.0, h_px: 82.0, label: "［照片］".into() });
        let xml = document_of(&page, &opts);
        // 版心宽 170mm=9638，照片 62px=930，左边界 20mm=1134 → x = 9638-930+1134
        assert!(xml.contains("w:x=\"9842\""), "{xml}");
        assert!(xml.contains("w:y=\"1134\""), "{xml}");
        assert_eq!(xml.matches("w:hRule=\"atLeast\"").count(), 1, "{xml}");
        assert!(!xml.contains("照片："), "图片字段不该再输出成半句话：{xml}");
    }

    #[test]
    fn four_presets_scale_the_font_size() {
        let cases = [("classic", 34u32), ("compact", 32), ("roomy", 36), ("dense", 33)];
        for (preset, expected) in cases {
            let opts = opts_for(preset);
            assert_eq!(opts.sz(34), expected, "{preset} 姓名（基准 34 半点）");
        }
        // 正文（基准 17 半点）也跟着走，不是只放大标题
        assert_eq!(opts_for("dense").sz(17), 16);
        assert_eq!(opts_for("roomy").sz(17), 18);
    }

    #[test]
    fn line_spacing_follows_preset() {
        assert_eq!(opts_for("classic").line(), 408, "1.7 × 240");
        assert_eq!(opts_for("compact").line(), 372, "1.55 × 240");
        assert_eq!(opts_for("roomy").line(), 461, "1.92 × 240");
        assert_eq!(opts_for("dense").line(), 341, "1.42 × 240");
    }

    #[test]
    fn page_margins_derived_from_paper_mm() {
        let page = page_with(json!([]));
        let xml = document_of(&page, &opts_for("classic"));
        assert!(xml.contains("w:top=\"1134\" w:right=\"1134\" w:bottom=\"1134\" w:left=\"1134\""), "{xml}");

        let mut paper = Paper::default();
        paper.margin = [15.0, 30.0, 15.0, 30.0];
        let xml = document_of(&page, &DocxOptions::new(&paper, "classic"));
        assert!(xml.contains("w:top=\"850\""), "15mm → 850 twips：{xml}");
        assert!(xml.contains("w:right=\"1701\""), "30mm → 1701 twips：{xml}");
        assert!(xml.contains("w:left=\"1701\""), "{xml}");
    }

    #[test]
    fn preset_comes_from_the_page_itself() {
        let page: ResumePage = serde_json::from_value(json!({
            "id": "pg1",
            "kind": "native",
            "layout": { "preset": "dense" },
        }))
        .unwrap();
        assert_eq!(DocxOptions::for_page(&Paper::default(), &page).metrics, preset_metrics("dense"));

        let legacy: ResumePage = serde_json::from_value(json!({
            "id": "pg2",
            "kind": "native",
            "style": { "density": "roomy" },
        }))
        .unwrap();
        assert_eq!(DocxOptions::for_page(&Paper::default(), &legacy).metrics, preset_metrics("roomy"));
    }

    /// 生成一份 dense 样例，确认文件真能落盘且非空（打开效果不在这里验）
    #[test]
    fn dense_sample_docx_is_written() {
        let page = page_with(json!([
            { "id": "b1", "kind": "fields", "fields": [
                { "id": "f1", "label": "姓名", "value": "张三" },
                { "id": "f2", "label": "电话", "value": "13800000000" }
            ]},
            { "id": "b2", "kind": "section", "title": "工作经历", "items": [
                { "id": "e1", "fields": [
                    { "id": "f3", "label": "公司", "value": "某某科技" },
                    { "id": "f4", "label": "职位", "value": "后端工程师" }
                ], "bullets": ["把导出耗时从 3s 压到 400ms"] }
            ]}
        ]));
        let mut opts = opts_for("dense");
        opts.absolutes.push(Absolute {
            x_px: 560.0,
            y_px: 24.0,
            w_px: 200.0,
            text: "求职意向：后端".into(),
            bold: true,
        });
        let bytes = page_docx("张三", &page, &[], &HashMap::new(), "white", &opts);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dense-sample.docx");
        std::fs::write(&path, &bytes).unwrap();
        let size = std::fs::metadata(&path).unwrap().len();
        assert!(size > 1024, "样例 docx 太小（{size} 字节）：{}", path.display());
        assert_eq!(bytes.len() as u64, size);
    }
}
