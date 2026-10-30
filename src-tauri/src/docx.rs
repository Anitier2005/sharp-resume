//! 极简 DOCX 生成：手写 OOXML + store（不压缩）zip，不加任何依赖。
//!
//! 为什么在 Rust：导出是数据处理，产物结构不该由页面层决定。
//! 搬到这里之后，前端只负责发起「导出这一页」，不再拼 XML 也不写字。
//!
//! v5 起按 block 排版，代码里没有「姓名 / 公司 / 教育」这类预设键：
//!   fields 块的第一个字段 = 大标题，其余按「标签：值」；
//!   section 块的标题就是用户写的那句话，条目的名称 / 中间字段 / 时间落在同一段
//!   （名称加粗，时间靠右制表位推到版心右缘）—— 与屏幕上的 `.erow` 同构；
//!   media 块在正文里留一行「见图」，图片本体旁挂到附件目录（不硬塞进 docx，
//!   否则一张 8MB 的证书会把简历文档撑得没法发）。

use std::collections::HashMap;

use crate::model::{Asset, Block, BlockStyle, Field, Paper, ResumePage};

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
/// 图**真的内嵌**进 docx（用户的核心诉求就是「照片进简历」，Word 里看不到脸等于没做到），
/// 但必须限尺寸：一张 8MB 的证书会把文档撑得发不出去，简历文档要能直接发给 HR。
/// 这里给的是**显示尺寸**；实际像素由 [`compress_photo`] 压到 160px 宽以内。
/// 取不到图 / 解不开 / 压不下来时回落成同尺寸的文字占位框，版式不跟着塌。
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

/// 1 px = 9525 EMU（OOXML 的 drawing 尺寸单位，914400 EMU = 1 inch = 96px）
pub fn emu_of_px(px: f64) -> u32 {
    (px * 9525.0).round().max(0.0) as u32
}

/// 内嵌照片的像素宽上限。
///
/// 依据：62×82px 的证件照按 96dpi 显示，但打印时 300dpi 才是「不糊」的门槛，
/// 62px ÷ 96 × 300 ≈ 194 设备像素。160px 在 A4 上约 245dpi，肉眼看不出差别，
/// 而文件比原图小一个量级 —— 再往上加只是白占体积，简历不需要印刷级照片。
/// 为什么不是 300：300px 宽在 62px 的框里是 ~465dpi，体积翻三倍，纸上分辨不出。
const PHOTO_MAX_PX: u32 = 160;

/// 压完仍超过这个尺寸就放弃内嵌。
///
/// **这是防御，不是常态**：160px 宽、质量 0.8 的 JPEG 实测只有 5–15KB，
/// 离 2MB 差三个数量级，正常证件照永远不会触发。它防的是解码器对畸形输入
/// 给出异常结果（超大画布、坏尺寸头），以及将来有人把 `PHOTO_MAX_PX` 调大时
/// 没有第二道闸。删掉不会有任何现有行为变化 —— 所以更不能删：留下的是一条
/// 「当初为什么有它」的记录。
const PHOTO_MAX_BYTES: usize = 2 * 1024 * 1024;

/// 压成可内嵌的 JPEG：宽度 ≤160px、质量 0.8。
///
/// 返回 `None` 表示这张图内嵌不了（解码失败 / 压完仍超 2MB），
/// 调用方据此回落成占位框 —— 这里不抛错、不写字，导出流程不因为一张图停下。
fn compress_photo(src: &[u8]) -> Option<Vec<u8>> {
    let image = image::load_from_memory(src).ok()?;
    let image = if image.width() > PHOTO_MAX_PX {
        image.resize(PHOTO_MAX_PX, u32::MAX, image::imageops::FilterType::Lanczos3)
    } else {
        image
    };
    let rgb = image.to_rgb8();
    let mut jpeg = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 80);
    encoder
        .encode_image(&image::DynamicImage::ImageRgb8(rgb))
        .ok()?;
    if jpeg.len() > PHOTO_MAX_BYTES {
        return None;
    }
    Some(jpeg)
}

/// 已经压好、可以写进 `word/media/` 的那一份照片
struct Embedded {
    jpeg: Vec<u8>,
    cx: u32,
    cy: u32,
}

fn embed_photo(photo: &Photo, src: Option<&[u8]>) -> Option<Embedded> {
    Some(Embedded {
        jpeg: compress_photo(src?)?,
        cx: emu_of_px(photo.w_px),
        cy: emu_of_px(photo.h_px),
    })
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
    /// 照片原图的字节（任意 image 能解的格式）。给不出 / 解不开 / 压不下来
    /// 都回落到占位框，**导出里绝不出现「失败」字样**。
    pub photo_src: Option<Vec<u8>>,
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
            photo_src: None,
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

/// 一块在 Word 里的排版上下文。
///
/// 为什么攒成一个值而不是一路传三个参数：块级样式（对齐 / 字号 / 加粗）与
/// 「这一行有多宽」对同一块里的每个段落都是同一份。散着传必然会漏掉某一条路径
/// —— 页头吃了样式、要点没吃，比干脆不支持更糟。
#[derive(Clone, Copy)]
struct Flow<'a> {
    opts: &'a DocxOptions,
    /// 块级样式覆盖；`None` = 完全按版式默认
    style: Option<&'a BlockStyle>,
    /// 可用宽度（twips）：正文里是版心宽，表格单元格里是列宽。
    /// 条目头的时间靠右制表位打在这一点上 —— 单元格里还打版心右缘会把那一行撑破。
    width: i64,
}

impl<'a> Flow<'a> {
    fn body(opts: &'a DocxOptions, style: Option<&'a BlockStyle>) -> Self {
        Self {
            opts,
            style,
            width: opts.content_width_twips(),
        }
    }

    fn cell(opts: &'a DocxOptions, style: Option<&'a BlockStyle>, width: i64) -> Self {
        Self {
            opts,
            style,
            width,
        }
    }

    /// 字号：样式给了 pt 就用它（`w:sz` 是半点，pt×2），否则按版式基线缩放
    fn size(&self, base: u32) -> u32 {
        match self.style.and_then(BlockStyle::size_pt) {
            Some(pt) => pt * 2,
            None => self.opts.sz(base),
        }
    }

    /// 加粗：样式喊加粗就加。
    ///
    /// `bold: false` 不做事 —— 条目名、分区标题的加粗是**结构**的一部分，
    /// 取消它只会让这一块「看起来不一样」却说不清哪不一样。
    /// 所以这里只加不减，模型能做的就是「更醒目」。
    fn bold(&self, base: bool) -> bool {
        base || self
            .style
            .map(|style| style.bold == Some(true))
            .unwrap_or(false)
    }

    /// 对齐：块级样式优先，其次页头那条版式默认（`center`）。
    ///
    /// `left` 不写 `<w:jc>` —— 那是 Word 的默认值，写出来只是一行噪声，
    /// 而且会让「有没有设样式」在 XML 上分不出来。
    fn jc(&self, center: bool) -> String {
        let align = self
            .style
            .and_then(BlockStyle::align)
            .unwrap_or(if center { "center" } else { "left" });
        match align {
            "left" => String::new(),
            other => format!("<w:jc w:val=\"{other}\"/>"),
        }
    }

    fn para(&self, text: &str, bold: bool, size: u32, color: &str, center: bool) -> String {
        let jc = self.jc(center);
        format!(
            "<w:p><w:pPr><w:spacing w:after=\"40\" w:line=\"{line}\" w:lineRule=\"auto\"/>{jc}</w:pPr>{}</w:p>",
            run(text, self.bold(bold), self.size(size), color),
            line = self.opts.line()
        )
    }

    /// 条目头：名称（加粗）+ 中间字段 + 时间，**同一个段落**，与屏幕上的 `.erow` 同构。
    ///
    /// 时间用右制表位推到可用宽度的右缘：位置取自宽度，不随字号漂移，
    /// 也不去估「这段字有多宽」—— 估出来的宽度换个字号就错位，比不齐更难看。
    fn entry_head(&self, head: &crate::render::EntryHead) -> String {
        let mut inner = run(&head.lead, self.bold(true), self.size(21), "141412");
        for role in &head.roles {
            inner.push_str(&run(
                &format!("  {role}"),
                self.bold(false),
                self.size(18),
                "55554F",
            ));
        }
        let tabs = match &head.time {
            Some(time) => {
                let size = self.size(18);
                inner.push_str(&format!(
                    "<w:r><w:rPr><w:sz w:val=\"{size}\"/><w:szCs w:val=\"{size}\"/></w:rPr><w:tab/></w:r>"
                ));
                inner.push_str(&run(time, self.bold(false), size, "55554F"));
                format!(
                    "<w:tabs><w:tab w:val=\"right\" w:pos=\"{width}\"/></w:tabs>",
                    width = self.width
                )
            }
            None => String::new(),
        };
        format!(
            "<w:p><w:pPr>{tabs}<w:spacing w:after=\"40\" w:line=\"{line}\" w:lineRule=\"auto\"/>{jc}</w:pPr>{inner}</w:p>",
            jc = self.jc(false),
            line = self.opts.line()
        )
    }

    /// 一条条目 → 若干段落：条目头 + 要点。屏幕上 `.erow` + `.bullets` 的同一件事。
    ///
    /// 正文分区与表格单元格都走这一条路径：条目怎么排只写一次。
    /// 复制一份到表格里，屏幕上两栏与 Word 里两栏迟早会长得不一样。
    fn entry_paras(
        &self,
        entry: &crate::model::SectionEntry,
        library: &[crate::model::LibraryItem],
        out: &mut Vec<String>,
    ) {
        let has_bullets = entry.bullets.iter().any(|line| !line.trim().is_empty());
        if let Some(head) = crate::render::entry_head(entry_fields(entry, library), has_bullets) {
            out.push(self.entry_head(&head));
        }
        for bullet in &entry.bullets {
            if !bullet.trim().is_empty() {
                out.push(self.para(&format!("· {bullet}"), false, 18, "55554F", false));
            }
        }
    }
}

/// 本块的样式覆盖；空对象（`{}`，前端「清掉」的产物）等同于没有
fn block_style_of(block: &Block) -> Option<&BlockStyle> {
    block.style.as_ref().filter(|style| !style.is_empty())
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

/// 页头照片 → 右上角的固定框架：`w:x` = 版心宽 − 照片宽，`w:y` = 0（贴内容顶）。
///
/// 有内嵌图时框架里放 `wp:inline` 图片；没有就放同尺寸的文字占位 ——
/// 两条路的版面完全一样，降级不会让页面重排。
fn photo_para(photo: &Photo, embedded: Option<&Embedded>, opts: &DocxOptions) -> String {
    let margin = opts.margin_twips();
    let width = px_to_twips(photo.w_px);
    let x = (opts.content_width_twips() - width as i64).max(0) + margin[3];
    let inner = match embedded {
        Some(image) => drawing_run(image, PHOTO_REL_ID, PHOTO_MEDIA_NAME),
        None => run(&photo.label, false, opts.sz(17), "77776F"),
    };
    frame_para(
        width,
        Some(px_to_twips(photo.h_px)),
        x,
        margin[0],
        "atLeast",
        &inner,
        opts,
    )
}

/// 内嵌照片在包里的固定位置。一页只有一张照片，固定名字比算序号好读，
/// 也让「同一份内容导出字节相同」这条性质保住。
const PHOTO_MEDIA_NAME: &str = "photo1.jpg";
const PHOTO_REL_ID: &str = "rId2";

/// `wp:inline`（嵌入型图片）。
///
/// 用 `inline` 而不是 `anchor`：外层段落已经是 `w:framePr` 了，位置由框架给，
/// 图只需要「填满这个框」。锚定图会把两套定位叠在一起，Word 里容易跑到第二页去。
/// 命名空间就地声明，这样没有照片的文档根节点一个字都不用改。
fn drawing_run(image: &Embedded, rid: &str, name: &str) -> String {
    let (cx, cy) = (image.cx, image.cy);
    format!(
        "<w:r><w:drawing><wp:inline xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" \
         xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
         xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\" \
         xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
         distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\">\
         <wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/>\
         <wp:docPr id=\"1\" name=\"照片\"/>\
         <a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
         <pic:pic><pic:nvPicPr><pic:cNvPr id=\"0\" name=\"{name}\"/><pic:cNvPicPr/></pic:nvPicPr>\
         <pic:blipFill><a:blip r:embed=\"{rid}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
         <pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm>\
         <a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic>\
         </a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"
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

/// 表格的列宽：均分可用宽度，最后一列吃掉除不尽的零头。
///
/// 各列之和必须**正好**等于可用宽度：差几个 twips，`fixed` 布局就会在右边
/// 留一条说不出来源的空白，或者把最后一列挤出去一格。
fn column_widths(total: i64, cols: usize) -> Vec<i64> {
    let cols = cols.max(1);
    let base = total / cols as i64;
    (0..cols)
        .map(|index| {
            if index + 1 == cols {
                total - base * (cols as i64 - 1)
            } else {
                base
            }
        })
        .collect()
}

/// 表格横线的颜色。
///
/// 屏幕上的 `.doc td` 只有 `border-bottom: 1px solid var(--line)`（`paper.css:600`），
/// `--line` 的取值是 `#e2e2dc`（`paper.css:23`）。这里必须跟屏幕同源：
/// 换成正文那种近黑，Word 里的表格会比屏幕上重一档；换成一整圈框，
/// 屏幕上根本不是那个样子。改 CSS 时这里要跟着改（同 `preset_metrics` 的约定）。
const TABLE_RULE_HEX: &str = "E2E2DC";

/// 表格边框。
///
/// 无边框网格（`grid-2` / `grid-3`）：六个方向全 `none` —— 屏幕上
/// `.doc .sec.grid-2` 本来就没有框，分栏靠留白，Word 里加框等于凭空多一张表。
///
/// `table` 角色只画横线：屏幕的表格只有每格下边框，没有竖线也没有外框。
fn tbl_borders(ruled: bool) -> String {
    fn none(side: &str) -> String {
        format!("<w:{side} w:val=\"none\" w:sz=\"0\" w:space=\"0\" w:color=\"auto\"/>")
    }
    let inside_h = if ruled {
        format!("<w:insideH w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"{TABLE_RULE_HEX}\"/>")
    } else {
        none("insideH")
    };
    format!(
        "<w:tblBorders>{}{}{}{}{}{}</w:tblBorders>",
        none("top"),
        none("left"),
        none("bottom"),
        none("right"),
        inside_h,
        none("insideV"),
    )
}

/// 一张表：`rows` 里每个元素已经是「一个单元格的段落 XML」。
///
/// 单元格里**不再套空段落**：那一层会让每格多出半行高度，两栏就比屏幕上松。
/// 内容为空的格子补 `<w:p/>` —— `<w:tc>` 里没有 `<w:p>` 时 Word 会报文档损坏，
/// 这是唯一必须补段落的地方。
fn table_xml(rows: &[Vec<String>], cols: usize, opts: &DocxOptions, ruled: bool) -> String {
    let cols = cols.max(1);
    let width = opts.content_width_twips();
    let widths = column_widths(width, cols);
    let grid: String = widths
        .iter()
        .map(|value| format!("<w:gridCol w:w=\"{value}\"/>"))
        .collect();
    let mut body = String::new();
    for row in rows {
        body.push_str("<w:tr>");
        for (index, cell_width) in widths.iter().enumerate() {
            let cell = row.get(index).map(String::as_str).unwrap_or("");
            let cell = if cell.is_empty() { "<w:p/>" } else { cell };
            body.push_str(&format!(
                "<w:tc><w:tcPr><w:tcW w:w=\"{cell_width}\" w:type=\"dxa\"/></w:tcPr>{cell}</w:tc>"
            ));
        }
        body.push_str("</w:tr>");
    }
    format!(
        "<w:tbl><w:tblPr><w:tblW w:w=\"{width}\" w:type=\"dxa\"/><w:tblLayout w:type=\"fixed\"/>\
         {borders}<w:tblCellMar><w:left w:w=\"0\" w:type=\"dxa\"/><w:right w:w=\"0\" w:type=\"dxa\"/>\
         </w:tblCellMar></w:tblPr><w:tblGrid>{grid}</w:tblGrid>{body}</w:tbl>",
        borders = tbl_borders(ruled)
    )
}

/// `grid-2` / `grid-3`：条目按行优先填格，一格一条。
///
/// 屏幕上 `.doc .sec.grid-2` 是 `display:grid; grid-template-columns: 1fr 1fr`，
/// 条目按 DOM 顺序落格 —— 行优先，与这里一致。最后一行不满就补空格子，
/// 否则 Word 在 fixed 布局下会把那一行拉成整宽，上一行的列线就对不上了。
fn grid_table(
    block: &Block,
    library: &[crate::model::LibraryItem],
    opts: &DocxOptions,
    cols: usize,
) -> String {
    let cols = cols.max(1);
    let widths = column_widths(opts.content_width_twips(), cols);
    let style = block_style_of(block);
    let mut rows: Vec<Vec<String>> = Vec::new();
    for chunk in block.items.chunks(cols) {
        let mut row = Vec::with_capacity(cols);
        for (index, entry) in chunk.iter().enumerate() {
            let flow = Flow::cell(opts, style, widths[index]);
            let mut paras: Vec<String> = Vec::new();
            flow.entry_paras(entry, library, &mut paras);
            row.push(paras.join(""));
        }
        rows.push(row);
    }
    if rows.is_empty() {
        return String::new();
    }
    table_xml(&rows, cols, opts, false)
}

/// `table` 角色：一条一行、一个字段一格，**只排字段值**（标签去掉、第一格加粗）。
///
/// 与屏幕上的 `render::render_table` 是同一条规则 —— 那边也只取值、
/// 要点同样不进表（丢内容由 `warn_if_lossy` 替它吭声）。两边都一样的缺口
/// 不算偏差，一边进一边不进才是。
fn field_table(
    block: &Block,
    library: &[crate::model::LibraryItem],
    opts: &DocxOptions,
) -> String {
    let style = block_style_of(block);
    let mut values: Vec<Vec<String>> = Vec::new();
    for entry in &block.items {
        let row: Vec<String> = entry_fields(entry, library)
            .iter()
            .map(|field| field.value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect();
        if !row.is_empty() {
            values.push(row);
        }
    }
    if values.is_empty() {
        return String::new();
    }
    let cols = values.iter().map(Vec::len).max().unwrap_or(1);
    let widths = column_widths(opts.content_width_twips(), cols);
    let rows: Vec<Vec<String>> = values
        .iter()
        .map(|row| {
            row.iter()
                .enumerate()
                .map(|(index, value)| {
                    // 第一格是这一条的「名字」，与屏幕上 `<td><b>` 同一个位置
                    let color = if index == 0 { "141412" } else { "55554F" };
                    Flow::cell(opts, style, widths[index]).para(value, index == 0, 18, color, false)
                })
                .collect()
        })
        .collect();
    table_xml(&rows, cols, opts, true)
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
    let flow = Flow::body(opts, block_style_of(block));
    match block.kind.as_str() {
        "fields" => {
            // 图片字段单独走 framePr（照片位），不能混在「标签：值」里变成半句话
            let mut iter = block.fields.iter().filter(|field| field.image_id().is_none());
            if let Some(lead) = iter.next() {
                let text = if lead.value.trim().is_empty() { &lead.label } else { &lead.value };
                out.push(flow.para(text, true, 34, hex, center));
            }
            for field in iter {
                out.push(flow.para(&field_text(field), false, 17, "77776F", center));
            }
        }
        "section" => {
            // 标题先出，占满整行：屏幕上 `.sec.grid-2 > .t` 是 `grid-column: 1/-1`
            // 的第一行，视觉就是「标题在上、表格在下」。
            // 不做成表里的合并单元格 —— 那样标题会被绑进表格这一整块，
            // Word 分页时会把标题跟着表一起推到下一页。
            if !block.title.trim().is_empty() {
                out.push(flow.para(&block.title, true, 21, hex, false));
            }
            let table = match block.role.as_deref().map(str::trim) {
                Some("grid-2") => Some(grid_table(block, library, opts, 2)),
                Some("grid-3") => Some(grid_table(block, library, opts, 3)),
                Some("table") => Some(field_table(block, library, opts)),
                _ => None,
            };
            match table {
                // 没有条目时 `table_xml` 会返回空串：别往正文里塞一个空元素
                Some(xml) if !xml.is_empty() => out.push(xml),
                Some(_) => {}
                None => {
                    for entry in &block.items {
                        // 与屏幕同构：名称 + 中间字段 + 时间落在同一个段落里，
                        // 拆成一段一个字段会多占两三倍的行数（屏幕上早就不这么排了）
                        flow.entry_paras(entry, library, out);
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
            out.push(flow.para(&line, false, 17, "77776F", false));
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
    photo: Option<&Photo>,
    embedded: Option<&Embedded>,
) -> String {
    let mut body: Vec<String> = Vec::new();

    // 框架段落不占正文流：先放，正文从内容顶照常开始
    if let Some(photo) = photo {
        body.push(photo_para(photo, embedded, opts));
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
        body.push(Flow::body(opts, None).para("（空白页）", false, 18, "999990", false));
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

/// 没有照片时连 `image/jpeg` 的 Default 也不写：包里的每个声明都要有落脚点，
/// 否则 Word 会为一条「声明了但没人用」的类型报一条可修复性警告。
fn content_types(with_jpeg: bool) -> String {
    let jpeg = if with_jpeg {
        "<Default Extension=\"jpg\" ContentType=\"image/jpeg\"/>"
    } else {
        ""
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/>{jpeg}<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/><Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/><Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/></Types>"
    )
}

const ROOT_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/></Relationships>";

/// 正文关系：样式必有一份，照片有才加一条图片关系（`rId2` 与 `a:blip` 里的
/// `r:embed` 是同一个号，改一处必须改另一处）。
fn doc_rels(with_photo: bool) -> String {
    let photo = if with_photo {
        format!(
            "<Relationship Id=\"{PHOTO_REL_ID}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/{PHOTO_MEDIA_NAME}\"/>"
        )
    } else {
        String::new()
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>{photo}</Relationships>"
    )
}

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

fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();

    for (name, data) in files {
        let name_bytes = name.as_bytes();
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
    let photo = photo_of_page(page, opts);
    let embedded = photo.as_ref().and_then(|photo| embed_photo(photo, opts.photo_src.as_deref()));
    let document = document_xml(page, library, assets, hex, opts, photo.as_ref(), embedded.as_ref());
    let core = core_xml(&title, stamp);
    let types = content_types(embedded.is_some());
    let rels = doc_rels(embedded.is_some());
    let mut files: Vec<(&str, &[u8])> = vec![        ("[Content_Types].xml", types.as_bytes()),
        ("_rels/.rels", ROOT_RELS.as_bytes()),
        ("docProps/core.xml", core.as_bytes()),
        ("word/_rels/document.xml.rels", rels.as_bytes()),
        ("word/document.xml", document.as_bytes()),
        ("word/styles.xml", STYLES_XML.as_bytes()),
    ];
    let media = format!("word/media/{PHOTO_MEDIA_NAME}");
    if let Some(image) = embedded.as_ref() {
        // 图片排在正文之后：目录顺序不影响 Word，按「先文档后素材」读着顺
        files.push((media.as_str(), image.jpeg.as_slice()));
    }
    zip(&files)
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
        let photo = photo_of_page(page, opts);
        let embedded = photo.as_ref().and_then(|photo| embed_photo(photo, opts.photo_src.as_deref()));
        document_xml(
            page,
            &[],
            &HashMap::new(),
            "242422",
            opts,
            photo.as_ref(),
            embedded.as_ref(),
        )
    }

    /// 测试用的 PNG：宽 400，足够触发「压到 160」那条分支
    fn test_photo(width: u32, height: u32) -> Vec<u8> {
        let image = image::RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        });
        let mut out = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    fn zip_has(bytes: &[u8], needle: &str) -> bool {
        let needle = needle.as_bytes();
        bytes.windows(needle.len()).any(|window| window == needle)
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

    /// 条目头在 Word 里必须是**一个段落**：名称加粗，中间字段与时间同在段末
    #[test]
    fn entry_head_becomes_a_single_paragraph() {
        let page = page_with(json!([
            { "id": "b2", "kind": "section", "title": "教育经历", "items": [
                { "id": "e1", "fields": [
                    { "id": "f1", "label": "学校", "value": "中山大学" },
                    { "id": "f2", "label": "院校属性", "value": "教育部直属 211" },
                    { "id": "f3", "label": "专业", "value": "行政管理" },
                    { "id": "f4", "label": "时间", "value": "2016年9月-2020年6月" }
                ], "bullets": [] }
            ]}
        ]));
        let xml = document_of(&page, &opts_for("classic"));

        // 分区标题 + 条目头 = 两个段落：四个字段不再各占一段
        assert_eq!(xml.matches("<w:p>").count(), 2, "{xml}");
        let head = xml
            .split("<w:p>")
            .find(|chunk| chunk.contains("中山大学"))
            .expect("条目头那一段");
        assert!(head.contains("<w:b/>"), "名称要加粗：{head}");
        assert!(head.contains("教育部直属 211"), "{head}");
        assert!(head.contains("行政管理"), "{head}");
        assert!(head.contains("2016年9月-2020年6月"), "时间要在同一段：{head}");
        // 时间靠右制表位推到版心右缘，不靠数全角空格猜宽度
        assert!(head.contains(r#"<w:tab w:val="right""#), "{head}");
        // 也不再逐字段印「标签：值」
        assert!(!xml.contains("时间："), "{xml}");
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
    fn page_margins_derived_from_paper_mm() {        let page = page_with(json!([]));
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

    #[test]
    fn emu_keeps_the_display_box_in_pixels() {
        assert_eq!(emu_of_px(62.0), 590_550, "62px → 590550 EMU");
        assert_eq!(emu_of_px(82.0), 781_050);
        let page = page_with(json!([
            { "id": "b1", "kind": "fields", "fields": [
                { "id": "f1", "label": "姓名", "value": "张三" },
                { "id": "f2", "label": "照片", "value": "", "assetId": "a1" }
            ]}
        ]));
        let mut opts = opts_for("classic");
        opts.photo_src = Some(test_photo(400, 520));
        let xml = document_of(&page, &opts);
        assert!(xml.contains("wp:extent cx=\"590550\" cy=\"781050\""), "{xml}");
        assert!(xml.contains("r:embed=\"rId2\""), "{xml}");
    }

    #[test]
    fn photo_is_really_embedded_into_the_package() {
        let page = page_with(json!([
            { "id": "b1", "kind": "fields", "fields": [
                { "id": "f1", "label": "姓名", "value": "张三" },
                { "id": "f2", "label": "照片", "value": "", "assetId": "a1" }
            ]}
        ]));
        let mut opts = opts_for("classic");
        opts.photo_src = Some(test_photo(400, 520));
        let bytes = page_docx("张三", &page, &[], &HashMap::new(), "white", &opts);
        assert!(zip_has(&bytes, "word/media/photo1.jpg"), "媒体部件没进包");
        assert!(zip_has(&bytes, "Target=\"media/photo1.jpg\""), "rels 没指向图片");
        assert!(zip_has(&bytes, "ContentType=\"image/jpeg\""), "没声明 jpeg 类型");
        assert!(zip_has(&bytes, "r:embed=\"rId2\""));
    }

    #[test]
    fn oversized_photo_is_downscaled_to_160px() {
        let jpeg = compress_photo(&test_photo(400, 520)).expect("400px 的图应该压得下来");
        let decoded = image::load_from_memory(&jpeg).unwrap();
        assert_eq!(decoded.width(), 160, "宽度必须压到 160");
        assert_eq!(decoded.height(), 208, "高度等比");
    }

    #[test]
    fn small_photo_keeps_its_own_width() {
        let jpeg = compress_photo(&test_photo(64, 82)).unwrap();
        assert_eq!(image::load_from_memory(&jpeg).unwrap().width(), 64);
    }

    #[test]
    fn undecodable_photo_falls_back_to_the_placeholder_frame() {
        let page = page_with(json!([
            { "id": "b1", "kind": "fields", "fields": [
                { "id": "f1", "label": "姓名", "value": "张三" },
                { "id": "f2", "label": "照片", "value": "", "assetId": "a1" }
            ]}
        ]));
        let mut opts = opts_for("classic");
        opts.photo_src = Some(b"not an image".to_vec());
        let bytes = page_docx("张三", &page, &[], &HashMap::new(), "white", &opts);
        assert!(!zip_has(&bytes, "word/media/"), "解不开的图不该进包");
        assert!(!zip_has(&bytes, "<w:drawing>"), "解不开时不许出现 drawing");
        assert!(zip_has(&bytes, "［照片］"), "应当回落到占位框");
        // 降级不许往导出结果里写任何提示文字
        assert!(!zip_has(&bytes, "失败"), "导出里不许出现报错文字");
        assert!(!zip_has(&bytes, "无法"), "导出里不许出现报错文字");
    }

    #[test]
    fn missing_photo_bytes_leave_no_drawing() {
        let page = page_with(json!([
            { "id": "b1", "kind": "fields", "fields": [
                { "id": "f1", "label": "姓名", "value": "张三" },
                { "id": "f2", "label": "照片", "value": "", "assetId": "a1" }
            ]}
        ]));
        let opts = opts_for("classic");
        let bytes = page_docx("张三", &page, &[], &HashMap::new(), "white", &opts);
        assert!(!zip_has(&bytes, "<w:drawing>"));
        assert!(zip_has(&bytes, "［照片］"));
    }

    /// 生成一份带真照片的样例：压图、内嵌、附件三件事一起走到
    #[test]
    fn photo_sample_docx_is_written() {
        let page = page_with(json!([
            { "id": "b1", "kind": "fields", "fields": [
                { "id": "f1", "label": "姓名", "value": "张三" },
                { "id": "f2", "label": "电话", "value": "13800000000" },
                { "id": "f3", "label": "照片", "value": "", "assetId": "a1" }
            ]},
            { "id": "b2", "kind": "section", "title": "工作经历", "items": [
                { "id": "e1", "fields": [
                    { "id": "f4", "label": "公司", "value": "某某科技" },
                    { "id": "f5", "label": "职位", "value": "后端工程师" }
                ], "bullets": ["把导出耗时从 3s 压到 400ms"] }
            ]}
        ]));
        let mut opts = opts_for("classic");
        opts.photo_src = Some(test_photo(360, 480));
        let bytes = page_docx("张三", &page, &[], &HashMap::new(), "white", &opts);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("photo-sample.docx");
        std::fs::write(&path, &bytes).unwrap();
        let size = std::fs::metadata(&path).unwrap().len();
        assert!(size > 2048, "样例 docx 太小（{size} 字节）：{}", path.display());
        assert!(zip_has(&bytes, "word/media/photo1.jpg"));
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

    /// 网格角色必须落成真正的表格。
    ///
    /// 没有 `w:tbl` 的话，屏幕上并排的两栏在 Word 里会变成一栏竖排 ——
    /// 用户看到的「网格排版」在导出里整个消失，这是这次要堵的窟窿。
    #[test]
    fn grid_role_becomes_a_borderless_table() {
        let page = page_with(json!([
            { "id": "b2", "kind": "section", "title": "技能", "role": "grid-2", "items": [
                { "id": "e1", "fields": [{ "id": "f1", "label": "语言", "value": "英语 CET-6" }] },
                { "id": "e2", "fields": [{ "id": "f2", "label": "工具", "value": "Excel 透视" }] }
            ]}
        ]));
        let opts = opts_for("classic");
        let xml = document_of(&page, &opts);

        assert!(xml.contains("<w:tbl>"), "没有表格：{xml}");
        assert!(xml.contains(r#"<w:tblLayout w:type="fixed"/>"#), "{xml}");
        // 无边框：六个方向全是 none，一个 single 都不许出现
        assert!(xml.contains("<w:tblBorders>"), "{xml}");
        assert_eq!(xml.matches(r#"w:val="single""#).count(), 0, "网格不该有框：{xml}");
        assert_eq!(xml.matches(r#"w:val="none""#).count(), 6, "{xml}");
        // 两列均分版心
        let half = opts.content_width_twips() / 2;
        let col = format!(r#"<w:gridCol w:w="{half}"/>"#);
        assert_eq!(xml.matches(&col).count(), 2, "列宽不是版心的一半：{xml}");
        // 一格一条：两条条目落在同一行
        assert_eq!(xml.matches("<w:tr>").count(), 1, "{xml}");
        assert!(xml.contains("英语 CET-6") && xml.contains("Excel 透视"), "{xml}");
    }

    /// `table` 角色跟屏幕一样**只画横线**：`paper.css` 里 `.doc td` 只有
    /// `border-bottom`，没有竖线也没有外框。加一圈框，Word 里就不是那张表了。
    #[test]
    fn table_role_keeps_the_lines() {
        let page = page_with(json!([
            { "id": "b2", "kind": "section", "title": "证书", "role": "table", "items": [
                { "id": "e1", "fields": [
                    { "id": "f1", "label": "证书", "value": "报关员资格证" },
                    { "id": "f2", "label": "时间", "value": "2019" }
                ]},
                { "id": "e2", "fields": [
                    { "id": "f3", "label": "证书", "value": "CET-6" },
                    { "id": "f4", "label": "时间", "value": "2018" }
                ]}
            ]}
        ]));
        let xml = document_of(&page, &opts_for("classic"));

        assert!(xml.contains("<w:tbl>"), "{xml}");
        let line = format!(
            r#"<w:insideH w:val="single" w:sz="4" w:space="0" w:color="{TABLE_RULE_HEX}"/>"#
        );
        assert!(xml.contains(&line), "缺少横线：{xml}");
        assert!(!xml.contains(r#"<w:insideH w:val="none""#), "{xml}");
        assert!(!xml.contains(r#"<w:insideV w:val="single""#), "屏幕的表格没有竖线：{xml}");
        // 两条条目 = 两行
        assert_eq!(xml.matches("<w:tr>").count(), 2, "{xml}");
        // 只排字段值，不印「标签：」—— 与屏幕上 `render_table` 同一条规则
        assert!(!xml.contains("时间："), "{xml}");
    }

    /// 同一份 `BlockStyle` 必须在两个渲染器里都生效。
    ///
    /// 只在一处生效比干脆不支持更糟：用户以为自己设好了，
    /// 屏幕上看着对、导出 Word 又是另一副样子。
    #[test]
    fn block_style_reaches_both_renderers() {
        let page = page_with(json!([
            { "id": "b2", "kind": "section", "title": "技能", "items": [
                { "id": "e1", "fields": [{ "id": "f1", "label": "工具", "value": "Excel" }], "bullets": [] }
            ], "style": { "align": "center", "size": 12, "bold": true } }
        ]));
        let person: crate::model::Person = serde_json::from_value(json!({
            "id": "p1",
            "fields": [],
            "assets": [],
            "items": [],
            "messages": [],
            "resumes": []
        }))
        .unwrap();

        let html = crate::render::compile(&page, &person).html;
        assert!(html.contains("text-align:center"), "屏幕没吃对齐：{html}");
        assert!(html.contains("font-size:12pt"), "屏幕没吃字号：{html}");
        assert!(html.contains("font-weight:700"), "屏幕没吃加粗：{html}");

        let xml = document_of(&page, &opts_for("classic"));
        assert!(xml.contains(r#"<w:jc w:val="center"/>"#), "Word 没吃对齐：{xml}");
        assert!(xml.contains(r#"<w:sz w:val="24"/>"#), "12pt 要写成 24 半点：{xml}");
        assert!(xml.contains("<w:b/>"), "Word 没吃加粗：{xml}");
    }
}
