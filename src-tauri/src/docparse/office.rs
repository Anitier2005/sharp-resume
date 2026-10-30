//! docx / pptx / xlsx 的带格式解析。
//!
//! 三种格式都是 zip + XML（OOXML），所以共用同一套「解压 + quick-xml」：
//!   - docx：`word/document.xml` → `w:p` / `w:pPr/w:jc` / `w:rPr/w:sz`(半磅) / `w:b` / `w:tbl`
//!   - pptx：`ppt/slides/slideN.xml` → `a:p` / `a:rPr@sz`(1/100 磅) / `a:t` / `ph type="title"`
//!   - xlsx：`xl/sharedStrings.xml` + `xl/worksheets/sheetN.xml` → 单元格网格
//!
//! 为什么不引 `zip` crate：仓库里 `flate2` 与 `quick-xml` 本来就在依赖树里（png/reqwest、
//! tauri 带进来的），显式声明不新增编译单元；而 `zip` crate 会再拉 `crc32fast` / `zstd` /
//! `bzip2` / `time` 一串，还要处理写、加密、zip64 等我们用不到的分支。
//! 我们只需要「按名字取一项的字节」这一件事，自己走中央目录不到百行、更可控。
//!
//! 契约：`ext` 是 `docx|pptx|xlsx`，输出 `DocOutline`；读不出返回 `None`。

use super::{Align, BlockKind, BlockStyle, DocBlock, DocOutline};
use flate2::read::DeflateDecoder;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::collections::HashMap;
use std::io::Read;

/// 单个容器（一份 docx / 一张 slide / 一个 sheet）最多保留的块数。
/// 正常简历远达不到这个量；超了说明拿错文件了（比如把一整本手册当简历），
/// 与其让下游拿几万块去排版，不如截断并留一句实话。
const MAX_BLOCKS: usize = 5000;

/// 表格护栏：真实简历表不会超过这个量级。40 列 × 2000 行已经足够大
/// （8 万个格子），再大就不是简历而是数据表了。
const MAX_TABLE_ROWS: usize = 2000;
const MAX_TABLE_COLS: usize = 40;

pub fn parse(ext: &str, bytes: &[u8]) -> Option<DocOutline> {
    let zip = Zip::new(bytes)?;
    let mut outline = match ext {
        "docx" => docx(&zip)?,
        "pptx" => pptx(&zip)?,
        "xlsx" => xlsx(&zip)?,
        _ => return None,
    };
    // 空大纲会被下游读成「这份文件是空的」——那是假话。宁可说读不出。
    if outline.blocks.is_empty() {
        return None;
    }
    outline.body_pt = None; // 交给公共层 rank() 用众数兜底
    Some(outline)
}

/* ==========================================================================
   只读 zip：EOCD → 中央目录 → 本地头
   ========================================================================== */

struct Entry {
    name: String,
    /// 0 = stored，8 = deflate。其余方法（bzip2/lzma…）放弃这一项
    method: u16,
    comp_size: usize,
    size: usize,
    /// 数据区在原始字节里的起点（已跳过本地头与文件名/扩展字段）
    data_at: usize,
}

struct Zip<'a> {
    bytes: &'a [u8],
    entries: Vec<Entry>,
}

impl<'a> Zip<'a> {
    fn new(bytes: &'a [u8]) -> Option<Self> {
        // EOCD 固定 22 字节 + 最长 65535 字节注释，从尾部往前找签名
        if bytes.len() < 22 {
            return None;
        }
        let floor = bytes.len().saturating_sub(22 + 0xffff);
        let mut eocd = None;
        let mut at = bytes.len() - 22;
        loop {
            if u32le(bytes, at) == Some(0x0605_4b50) {
                eocd = Some(at);
                break;
            }
            if at == floor {
                break;
            }
            at -= 1;
        }
        let eocd = eocd?;
        let count = u16le(bytes, eocd + 10)? as usize;
        let mut at = u32le(bytes, eocd + 16)? as usize;

        let mut entries = Vec::with_capacity(count.min(512));
        for _ in 0..count {
            if u32le(bytes, at)? != 0x0201_4b50 {
                break;
            }
            let method = u16le(bytes, at + 10)?;
            let comp_size = u32le(bytes, at + 20)? as usize;
            let size = u32le(bytes, at + 24)? as usize;
            let name_len = u16le(bytes, at + 28)? as usize;
            let extra_len = u16le(bytes, at + 30)? as usize;
            let comment_len = u16le(bytes, at + 32)? as usize;
            let local_at = u32le(bytes, at + 42)? as usize;
            let name = String::from_utf8_lossy(bytes.get(at + 46..at + 46 + name_len)?).into_owned();
            let stride = 46 + name_len + extra_len + comment_len;

            // 0xFFFF_FFFF 是 zip64 的哨兵值：附件不可能 >4GB，遇到就直接放弃
            let sentinel = u32::MAX as usize;
            if comp_size == sentinel || size == sentinel || local_at == sentinel {
                at += stride;
                continue;
            }
            // 本地头的文件名/扩展字段长度可能与中央目录里的不同，必须按本地头算数据起点
            if u32le(bytes, local_at)? != 0x0403_4b50 {
                at += stride;
                continue;
            }
            let l_name = u16le(bytes, local_at + 26)? as usize;
            let l_extra = u16le(bytes, local_at + 28)? as usize;
            let data_at = local_at.checked_add(30 + l_name + l_extra)?;

            entries.push(Entry { name, method, comp_size, size, data_at });
            at += stride;
        }
        if entries.is_empty() {
            return None;
        }
        Some(Zip { bytes, entries })
    }

    fn find(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.name == name)
    }

    fn raw(&self, entry: &Entry) -> Option<Vec<u8>> {
        let slice = self.bytes.get(entry.data_at..entry.data_at.checked_add(entry.comp_size)?)?;
        match entry.method {
            0 => Some(slice.to_vec()),
            8 => {
                let mut out = Vec::with_capacity(entry.size);
                DeflateDecoder::new(slice).read_to_end(&mut out).ok()?;
                Some(out)
            }
            _ => None,
        }
    }

    fn text(&self, name: &str) -> Option<String> {
        let entry = self.find(name)?;
        let raw = self.raw(entry)?;
        // OOXML 的 XML 一律 UTF-8
        Some(String::from_utf8_lossy(&raw).into_owned())
    }
}

fn u16le(bytes: &[u8], at: usize) -> Option<u16> {
    let slice = bytes.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([slice[0], slice[1]]))
}

fn u32le(bytes: &[u8], at: usize) -> Option<u32> {
    let slice = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

/* ==========================================================================
   共用小工具
   ========================================================================== */

/// 去掉命名空间前缀：`w:p` → `p`，`p:sp` → `sp`，无名空间时原样返回。
/// 不按前缀匹配 —— 同样的 XML 可能换个前缀写（`ns0:p`），local name 才是语义。
fn local(name: &str) -> &str {
    match name.rfind(':') {
        Some(at) => &name[at + 1..],
        None => name,
    }
}

fn attr(e: &BytesStart<'_>, key: &str) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        if local(a.key.0) == key {
            a.unescape_value().ok().map(|value| value.into_owned())
        } else {
            None
        }
    })
}

fn reader_of(xml: &str) -> Reader<&[u8]> {
    let mut reader = Reader::from_str(xml);
    // 真实文件里偶有未闭合标签、裸 `&`；能读多少读多少，比整体失败有用
    reader.config_mut().check_end_names = false;
    reader.config_mut().allow_dangling_amp = true;
    // 不 trim：`xml:space="preserve"` 的段间空格是排版信息
    reader.config_mut().trim_text(false);
    reader
}

/// 文本事件的内容。实体在 0.42 里是独立的 `GeneralRef` 事件，一并处理。
fn push_text(buf: &mut String, event: &Event<'_>) {
    match event {
        Event::Text(e) => buf.push_str(e),
        Event::GeneralRef(r) => {
            let name: &str = r;
            match name {
                "amp" => buf.push('&'),
                "lt" => buf.push('<'),
                "gt" => buf.push('>'),
                "quot" => buf.push('"'),
                "apos" => buf.push('\''),
                other => {
                    if let Some(ch) = char_ref(other) {
                        buf.push(ch);
                    }
                }
            }
        }
        _ => {}
    }
}

fn char_ref(name: &str) -> Option<char> {
    let digits = name.strip_prefix('#')?;
    match digits.strip_prefix('x').or_else(|| digits.strip_prefix('X')) {
        Some(hex) => u32::from_str_radix(hex, 16).ok().and_then(char::from_u32),
        None => digits.parse::<u32>().ok().and_then(char::from_u32),
    }
}

fn note(text: impl Into<String>) -> DocBlock {
    DocBlock {
        kind: BlockKind::Note,
        text: text.into(),
        style: BlockStyle::default(),
        table: None,
    }
}

fn table_block(rows: Vec<Vec<String>>) -> DocBlock {
    // 扁平版：给搜索/摘要用。单元格用「 | 」连，行之间换行。
    let flat = rows
        .iter()
        .map(|row| row.join(" | "))
        .collect::<Vec<_>>()
        .join("\n");
    DocBlock {
        kind: BlockKind::Table,
        text: flat,
        style: BlockStyle::default(),
        table: Some(rows),
    }
}

/// 超块截断。截断处补一条 Note，让下游知道「不是文档只有这些」。
fn cap(blocks: &mut Vec<DocBlock>) {
    if blocks.len() > MAX_BLOCKS {
        blocks.truncate(MAX_BLOCKS);
        blocks.push(note(format!("（只解析了前 {MAX_BLOCKS} 块）")));
    }
}

/// 表格收口：去掉尾部全空行 / 右侧全空列，再套行列上限。
/// 不做这一步的话，Excel 里常见的整列空格式会让网格宽度炸到 16384。
fn tidy_table(mut rows: Vec<Vec<String>>) -> (Vec<Vec<String>>, bool) {
    while rows.last().is_some_and(|row| row.iter().all(|cell| cell.trim().is_empty())) {
        rows.pop();
    }
    let mut width = rows.iter().map(Vec::len).max().unwrap_or(0);
    while width > 0 {
        let empty = rows
            .iter()
            .all(|row| row.get(width - 1).map_or(true, |cell| cell.trim().is_empty()));
        if !empty {
            break;
        }
        width -= 1;
    }
    let mut truncated = false;
    if rows.len() > MAX_TABLE_ROWS {
        rows.truncate(MAX_TABLE_ROWS);
        truncated = true;
    }
    if width > MAX_TABLE_COLS {
        width = MAX_TABLE_COLS;
        truncated = true;
    }
    for row in &mut rows {
        row.truncate(width);
        while row.len() < width {
            row.push(String::new());
        }
    }
    (rows, truncated)
}

/// OOXML 的开关属性：没有 `val` 就是开，`0`/`false`/`off` 是关
fn toggle_on(value: Option<&str>) -> bool {
    match value {
        None => true,
        Some(raw) => !matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "off" | "none"
        ),
    }
}

/* ==========================================================================
   docx
   ========================================================================== */

#[derive(Default)]
struct Para {
    text: String,
    size_pt: Option<f32>,
    bold: bool,
    italic: bool,
    align: Align,
    /// `w:pStyle@w:val`。**是 styleId，不是名字** —— 中文版 Word 里常常是 "1" / "a3"，
    /// 真名要去 `word/styles.xml` 里查
    style: Option<String>,
    numbered: bool,
    indent: u8,
    /// `w:outlineLvl@w:val`（0 起）。这是「这是几级标题」最可靠的信号：
    /// Word 写内置标题样式时一定会带它，而 styleId 是任意的
    outline: Option<u8>,
}

fn docx(zip: &Zip) -> Option<DocOutline> {
    let xml = zip.text("word/document.xml")?;
    let styles = style_names(zip);

    let mut blocks: Vec<DocBlock> = Vec::new();
    let mut para: Option<Para> = None;
    let mut table: Option<Vec<Vec<String>>> = None;
    let mut table_depth = 0usize;
    let mut in_table_props = 0usize;
    let mut cell: Option<String> = None;
    let mut in_text = 0usize;
    let mut margin: Option<[f32; 4]> = None;

    let mut reader = reader_of(&xml);
    loop {
        let event = match reader.read_event() {
            Ok(Event::Eof) | Err(_) => break,
            Ok(event) => event,
        };
        match &event {
            Event::Start(e) => match local(e.name().0) {
                "p" => para = Some(Para::default()),
                "tbl" => {
                    table_depth += 1;
                    if table_depth == 1 {
                        table = Some(Vec::new());
                    }
                }
                "tblPr" => in_table_props += 1,
                "tr" => {
                    if let Some(rows) = table.as_mut() {
                        rows.push(Vec::new());
                    }
                }
                "tc" => cell = Some(String::new()),
                "t" => in_text += 1,
                "tab" => {
                    if let Some(p) = para.as_mut() {
                        p.text.push('\t');
                    }
                }
                other => docx_props(other, e, &mut para, &mut margin, in_table_props),
            },
            Event::Empty(e) => match local(e.name().0) {
                "br" | "cr" => {
                    if let Some(p) = para.as_mut() {
                        p.text.push('\n');
                    }
                }
                "tab" => {
                    if let Some(p) = para.as_mut() {
                        p.text.push('\t');
                    }
                }
                other => docx_props(other, e, &mut para, &mut margin, in_table_props),
            },
            Event::End(e) => match local(e.name().0) {
                "p" => {
                    let Some(p) = para.take() else { continue };
                    let text = p.text.trim().to_string();
                    if text.is_empty() {
                        continue; // 空段落不产块；表格里的空段落也不占格
                    }
                    if table.is_some() {
                        if let Some(buf) = cell.as_mut() {
                            if !buf.is_empty() {
                                buf.push('\n');
                            }
                            buf.push_str(&text);
                        }
                        continue;
                    }
                    let (kind, level) = classify(&p, &styles);
                    blocks.push(block_of(kind, level, text, &p));
                }
                "tbl" => {
                    table_depth = table_depth.saturating_sub(1);
                    if table_depth == 0 {
                        if let Some(rows) = table.take() {
                            if !rows.is_empty() {
                                blocks.push(table_block(rows));
                            }
                        }
                    }
                }
                "tc" => {
                    let filled = cell.take().unwrap_or_default();
                    if let Some(row) = table.as_mut().and_then(|rows| rows.last_mut()) {
                        row.push(filled);
                    }
                }
                "tblPr" => in_table_props = in_table_props.saturating_sub(1),
                "t" => in_text = in_text.saturating_sub(1),
                _ => {}
            },
            Event::Text(_) | Event::GeneralRef(_) => {
                if in_text > 0 {
                    if let Some(p) = para.as_mut() {
                        push_text(&mut p.text, &event);
                    }
                }
            }
            _ => {}
        }
    }

    if blocks.is_empty() {
        return None;
    }
    cap(&mut blocks);
    Some(DocOutline {
        pages: None, // docx 没有可信页数，不猜
        margin_pt: margin,
        body_pt: None,
        blocks,
    })
}

/// 段落属性都挂在 `w:pPr` / `w:rPr` 下面，标签本身是无状态的
fn docx_props(
    name: &str,
    e: &BytesStart<'_>,
    para: &mut Option<Para>,
    margin: &mut Option<[f32; 4]>,
    in_table_props: usize,
) {
    if name == "pgMar" {
        let twips = |key: &str| attr(e, key).and_then(|v| v.trim().parse::<f32>().ok());
        if let (Some(top), Some(right), Some(bottom), Some(left)) =
            (twips("top"), twips("right"), twips("bottom"), twips("left"))
        {
            // twips → pt：1pt = 20 twips。契约顺序是「左、上、右、下」
            *margin = Some([left / 20.0, top / 20.0, right / 20.0, bottom / 20.0]);
        }
        return;
    }
    // 表格属性里也有 `w:jc`（表格对齐），那不是段落对齐，别串味
    if in_table_props > 0 {
        return;
    }
    let Some(p) = para.as_mut() else { return };
    match name {
        "pStyle" => p.style = attr(e, "val"),
        "jc" => {
            if let Some(value) = attr(e, "val") {
                p.align = Align::of(&value);
            }
        }
        // 半磅：`w:sz w:val="24"` 是 12pt
        "sz" | "szCs" => {
            if let Some(half) = attr(e, "val").and_then(|v| v.trim().parse::<f32>().ok()) {
                if half > 0.0 {
                    p.size_pt = Some(half / 2.0);
                }
            }
        }
        "b" => p.bold = toggle_on(attr(e, "val").as_deref()),
        "i" => p.italic = toggle_on(attr(e, "val").as_deref()),
        "numPr" => p.numbered = true,
        "ilvl" => {
            if let Some(level) = attr(e, "val").and_then(|v| v.trim().parse::<u8>().ok()) {
                p.indent = level.min(8);
            }
        }
        "outlineLvl" => {
            if let Some(level) = attr(e, "val").and_then(|v| v.trim().parse::<u8>().ok()) {
                p.outline = Some(level.saturating_add(1).min(9));
            }
        }
        _ => {}
    }
}

/// 段落 → 块的分类。
/// 优先看样式名（先按 styleId 查 styles.xml），再看 `w:outlineLvl`：
/// 中文版 Word 的 `w:pStyle` 常常只是个数字 id，光靠字符串匹配会全漏。
fn classify(p: &Para, styles: &HashMap<String, String>) -> (BlockKind, Option<u8>) {
    let raw = p.style.as_deref().unwrap_or("");
    let name = styles.get(raw).map(String::as_str).unwrap_or(raw);
    let lower = name.trim().to_ascii_lowercase();

    if lower == "title" || lower == "标题" {
        return (BlockKind::Title, None);
    }
    if let Some(level) = heading_level(&lower) {
        return (BlockKind::Heading, Some(level));
    }
    if p.numbered {
        return (BlockKind::ListItem, None);
    }
    if let Some(level) = p.outline {
        if level <= 6 {
            return (BlockKind::Heading, Some(level));
        }
    }
    (BlockKind::Paragraph, None)
}

/// 样式名 → 标题层级。"Heading1" / "heading 2" / "标题 1"；没数字就当一级。
fn heading_level(name: &str) -> Option<u8> {
    if !(name.contains("heading") || name.contains("标题")) {
        return None;
    }
    let trailing: String = name
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let level = if trailing.is_empty() {
        1
    } else {
        trailing.parse::<u8>().unwrap_or(1)
    };
    Some(level.clamp(1, 6))
}

fn block_of(kind: BlockKind, level: Option<u8>, text: String, p: &Para) -> DocBlock {
    DocBlock {
        kind,
        text,
        style: BlockStyle {
            size_pt: p.size_pt,
            bold: p.bold,
            italic: p.italic,
            align: p.align,
            indent: p.indent,
            level: if kind == BlockKind::Heading { level } else { None },
        },
        table: None,
    }
}

/// `word/styles.xml`：styleId → 样式名（小写）。
/// 为什么要查这张表：`w:pStyle@w:val` 是 styleId，中文版 Word 里写的是 "1"、"a3" 这种
/// 内部 id，光找 "Heading1" 会把它们全漏掉；真名在 styles.xml 里（"heading 1" / "标题 1"）。
fn style_names(zip: &Zip) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Some(xml) = zip.text("word/styles.xml") else {
        return map;
    };
    let mut current: Option<String> = None;
    let mut reader = reader_of(&xml);
    loop {
        let event = match reader.read_event() {
            Ok(Event::Eof) | Err(_) => break,
            Ok(event) => event,
        };
        if let Event::Start(e) | Event::Empty(e) = &event {
            match local(e.name().0) {
                "style" => current = attr(e, "styleId"),
                "name" => {
                    if let (Some(id), Some(name)) = (current.clone(), attr(e, "val")) {
                        map.insert(id, name.trim().to_ascii_lowercase());
                    }
                }
                _ => {}
            }
        }
    }
    map
}

/* ==========================================================================
   pptx
   ========================================================================== */

fn pptx(zip: &Zip) -> Option<DocOutline> {
    let mut slides: Vec<(u32, &str)> = zip
        .entries
        .iter()
        .filter_map(|entry| slide_number(&entry.name).map(|n| (n, entry.name.as_str())))
        .collect();
    if slides.is_empty() {
        return None;
    }
    slides.sort_by_key(|(n, _)| *n);

    let mut blocks = Vec::new();
    for (_, name) in &slides {
        let Some(xml) = zip.text(name) else { continue };
        slide_blocks(&xml, &mut blocks);
    }
    cap(&mut blocks);
    if blocks.is_empty() {
        return None;
    }
    Some(DocOutline {
        // pptx 的页数就是张数 —— 这是真页数，不是估算
        pages: Some(slides.len() as u32),
        margin_pt: None,
        body_pt: None,
        blocks,
    })
}

fn slide_number(name: &str) -> Option<u32> {
    name.strip_prefix("ppt/slides/slide")?.strip_suffix(".xml")?.parse().ok()
}

fn slide_blocks(xml: &str, blocks: &mut Vec<DocBlock>) {
    let mut para: Option<Para> = None;
    let mut table: Option<Vec<Vec<String>>> = None;
    let mut table_depth = 0usize;
    let mut cell: Option<String> = None;
    let mut in_text = 0usize;
    // 当前形状的占位符类型（`title` / `ctrTitle` / `body` / …）
    let mut placeholder: Option<String> = None;

    let mut reader = reader_of(xml);
    loop {
        let event = match reader.read_event() {
            Ok(Event::Eof) | Err(_) => break,
            Ok(event) => event,
        };
        match &event {
            Event::Start(e) => match local(e.name().0) {
                "sp" => placeholder = None,
                "p" => para = Some(Para::default()),
                "tbl" => {
                    table_depth += 1;
                    if table_depth == 1 {
                        table = Some(Vec::new());
                    }
                }
                "tr" => {
                    if let Some(rows) = table.as_mut() {
                        rows.push(Vec::new());
                    }
                }
                "tc" => cell = Some(String::new()),
                "t" => in_text += 1,
                "br" => {
                    if let Some(p) = para.as_mut() {
                        p.text.push('\n');
                    }
                }
                other => pptx_props(other, e, &mut para, &mut placeholder),
            },
            Event::Empty(e) => match local(e.name().0) {
                "br" => {
                    if let Some(p) = para.as_mut() {
                        p.text.push('\n');
                    }
                }
                other => pptx_props(other, e, &mut para, &mut placeholder),
            },
            Event::End(e) => match local(e.name().0) {
                "p" => {
                    let Some(p) = para.take() else { continue };
                    let text = p.text.trim().to_string();
                    if text.is_empty() {
                        continue;
                    }
                    if table.is_some() {
                        if let Some(buf) = cell.as_mut() {
                            if !buf.is_empty() {
                                buf.push('\n');
                            }
                            buf.push_str(&text);
                        }
                        continue;
                    }
                    // 占位符类型在 pptx_props 里已经统一小写
                    let is_title = matches!(placeholder.as_deref(), Some("title") | Some("ctrtitle"));
                    let kind = if is_title { BlockKind::Title } else { BlockKind::Paragraph };
                    blocks.push(block_of(kind, None, text, &p));
                }
                "tbl" => {
                    table_depth = table_depth.saturating_sub(1);
                    if table_depth == 0 {
                        if let Some(rows) = table.take() {
                            if !rows.is_empty() {
                                blocks.push(table_block(rows));
                            }
                        }
                    }
                }
                "tc" => {
                    let filled = cell.take().unwrap_or_default();
                    if let Some(row) = table.as_mut().and_then(|rows| rows.last_mut()) {
                        row.push(filled);
                    }
                }
                "t" => in_text = in_text.saturating_sub(1),
                _ => {}
            },
            Event::Text(_) | Event::GeneralRef(_) => {
                if in_text > 0 {
                    if let Some(p) = para.as_mut() {
                        push_text(&mut p.text, &event);
                    }
                }
            }
            _ => {}
        }
    }
}

fn pptx_props(
    name: &str,
    e: &BytesStart<'_>,
    para: &mut Option<Para>,
    placeholder: &mut Option<String>,
) {
    if name == "ph" {
        // 没有 type 属性的占位符是正文（PowerPoint 的默认值）
        *placeholder = Some(
            attr(e, "type")
                .map(|value| value.trim().to_ascii_lowercase())
                .unwrap_or_else(|| "body".to_string()),
        );
        return;
    }
    let Some(p) = para.as_mut() else { return };
    match name {
        // 1/100 磅：`sz="1200"` 是 12pt
        "rPr" | "defRPr" | "endParaRPr" => {
            if let Some(size) = attr(e, "sz").and_then(|v| v.trim().parse::<f32>().ok()) {
                if size > 0.0 && p.size_pt.is_none() {
                    p.size_pt = Some(size / 100.0);
                }
            }
            if let Some(bold) = attr(e, "b") {
                p.bold = toggle_on(Some(&bold));
            }
            if let Some(italic) = attr(e, "i") {
                p.italic = toggle_on(Some(&italic));
            }
        }
        "pPr" => {
            if let Some(value) = attr(e, "algn") {
                p.align = Align::of(&value);
            }
        }
        _ => {}
    }
}

/* ==========================================================================
   xlsx
   ========================================================================== */

fn xlsx(zip: &Zip) -> Option<DocOutline> {
    let shared = shared_strings(zip);
    let sheets = sheet_targets(zip);
    if sheets.is_empty() {
        return None;
    }

    let mut blocks: Vec<DocBlock> = Vec::new();
    for (name, path) in sheets {
        let Some(xml) = zip.text(&path) else { continue };
        let (rows, truncated) = tidy_table(sheet_rows(&xml, &shared));
        if rows.is_empty() {
            continue; // 空 sheet 不产块，也不该冒充「这里有过内容」
        }
        blocks.push(DocBlock {
            kind: BlockKind::Heading,
            text: name,
            style: BlockStyle { level: Some(1), ..Default::default() },
            table: None,
        });
        blocks.push(table_block(rows));
        if truncated {
            blocks.push(note(format!(
                "（表格过大，只保留前 {MAX_TABLE_ROWS} 行 {MAX_TABLE_COLS} 列）"
            )));
        }
        cap(&mut blocks);
    }
    if blocks.is_empty() {
        return None;
    }
    // pages 留 None：sheet 数不是页数（一张 sheet 能打印成 30 页），别混
    Some(DocOutline { pages: None, margin_pt: None, body_pt: None, blocks })
}

/// `<si><t>…</t></si>`；富文本是 `<si><r><t>a</t></r><r><t>b</t></r></si>`，多段要拼。
/// `<rPh>`（日语注音）里的 `<t>` 不是正文，跳过。
fn shared_strings(zip: &Zip) -> Vec<String> {
    let mut out = Vec::new();
    let Some(xml) = zip.text("xl/sharedStrings.xml") else {
        return out;
    };
    let mut buf = String::new();
    let mut in_si = false;
    let mut in_t = 0usize;
    let mut in_phonetic = 0usize;

    let mut reader = reader_of(&xml);
    loop {
        let event = match reader.read_event() {
            Ok(Event::Eof) | Err(_) => break,
            Ok(event) => event,
        };
        match &event {
            Event::Start(e) => match local(e.name().0) {
                "si" => {
                    in_si = true;
                    buf.clear();
                }
                "rPh" => in_phonetic += 1,
                "t" => in_t += 1,
                _ => {}
            },
            Event::End(e) => match local(e.name().0) {
                "si" => {
                    if in_si {
                        out.push(std::mem::take(&mut buf));
                    }
                    in_si = false;
                }
                "rPh" => in_phonetic = in_phonetic.saturating_sub(1),
                "t" => in_t = in_t.saturating_sub(1),
                _ => {}
            },
            Event::Text(_) | Event::GeneralRef(_) => {
                if in_si && in_t > 0 && in_phonetic == 0 {
                    push_text(&mut buf, &event);
                }
            }
            _ => {}
        }
    }
    out
}

/// workbook.xml 给 sheet 顺序与名字，rels 给 rId → 部件路径。
/// 不用「sheetN.xml 就是第 N 个 sheet」这个巧合：插入/删除过工作表后，
/// 部件名与顺序会错位，只有 rels 是权威的。
fn sheet_targets(zip: &Zip) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some(workbook) = zip.text("xl/workbook.xml") else {
        return out;
    };
    let rels = workbook_rels(zip);
    let mut index = 0usize;

    let mut reader = reader_of(&workbook);
    loop {
        let event = match reader.read_event() {
            Ok(Event::Eof) | Err(_) => break,
            Ok(event) => event,
        };
        if let Event::Start(e) | Event::Empty(e) = &event {
            if local(e.name().0) != "sheet" {
                continue;
            }
            let name = attr(e, "name").unwrap_or_default();
            let path = attr(e, "id")
                .and_then(|rid| rels.get(&rid).cloned())
                .unwrap_or_else(|| format!("xl/worksheets/sheet{}.xml", index + 1));
            out.push((name, path));
            index += 1;
        }
    }
    out
}

fn workbook_rels(zip: &Zip) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Some(xml) = zip.text("xl/_rels/workbook.xml.rels") else {
        return map;
    };
    let mut reader = reader_of(&xml);
    loop {
        let event = match reader.read_event() {
            Ok(Event::Eof) | Err(_) => break,
            Ok(event) => event,
        };
        if let Event::Start(e) | Event::Empty(e) = &event {
            if local(e.name().0) != "Relationship" {
                continue;
            }
            let (Some(id), Some(target)) = (attr(e, "Id"), attr(e, "Target")) else {
                continue;
            };
            // Target 可能是 "worksheets/sheet1.xml"（相对 xl/）或 "/xl/worksheets/..."（绝对）
            let path = if let Some(rest) = target.strip_prefix('/') {
                rest.to_string()
            } else {
                format!("xl/{}", target.trim_start_matches("./"))
            };
            map.insert(id, path);
        }
    }
    map
}

/// 单元格网格。**按 `c@r` 的地址放格子**：B3 就是第 3 行第 2 列，
/// 中间跳过的格子补空串 —— 挤掉空位会让「金额」跑到「项目」下面去。
fn sheet_rows(xml: &str, shared: &[String]) -> Vec<Vec<String>> {
    let mut cells: HashMap<(usize, usize), String> = HashMap::new();
    let mut row_index = 0usize;
    let mut next_row = 0usize;
    let mut next_col = 0usize;
    let mut current: Option<(usize, usize, String)> = None; // 列、行、类型
    let mut buf = String::new();
    let mut in_value = 0usize;
    let mut in_text = 0usize;

    let mut reader = reader_of(xml);
    loop {
        let event = match reader.read_event() {
            Ok(Event::Eof) | Err(_) => break,
            Ok(event) => event,
        };
        match &event {
            Event::Start(e) => match local(e.name().0) {
                "row" => {
                    row_index = attr(e, "r")
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .map(|n| n.saturating_sub(1))
                        .unwrap_or(next_row);
                    next_row = row_index + 1;
                    next_col = 0;
                }
                "c" => {
                    let (col, row) = match attr(e, "r").as_deref().and_then(cell_ref) {
                        Some((col, row)) => (col, row),
                        None => (next_col, row_index),
                    };
                    next_col = col + 1;
                    buf.clear();
                    current = Some((col, row, attr(e, "t").unwrap_or_default()));
                }
                "v" => in_value += 1,
                "t" => in_text += 1,
                _ => {}
            },
            Event::End(e) => match local(e.name().0) {
                "v" => in_value = in_value.saturating_sub(1),
                "t" => in_text = in_text.saturating_sub(1),
                "c" => {
                    if let Some((col, row, kind)) = current.take() {
                        let value = cell_value(&kind, buf.trim(), shared);
                        if !value.is_empty() {
                            cells.insert((row, col), value);
                        }
                    }
                    buf.clear();
                }
                _ => {}
            },
            Event::Text(_) | Event::GeneralRef(_) => {
                if current.is_some() && (in_value > 0 || in_text > 0) {
                    push_text(&mut buf, &event);
                }
            }
            _ => {}
        }
    }

    let Some(max_row) = cells.keys().map(|(row, _)| *row).max() else {
        return Vec::new();
    };
    let max_col = cells.keys().map(|(_, col)| *col).max().unwrap_or(0);
    let mut rows = vec![vec![String::new(); max_col + 1]; max_row + 1];
    for ((row, col), value) in cells {
        rows[row][col] = value;
    }
    rows
}

/// `B3` → (列 1, 行 2)，都从 0 起。
fn cell_ref(value: &str) -> Option<(usize, usize)> {
    let letters: String = value.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    let digits = &value[letters.len()..];
    if letters.is_empty() {
        return None;
    }
    let mut col = 0usize;
    for ch in letters.chars() {
        col = col * 26 + (ch.to_ascii_uppercase() as usize - 'A' as usize + 1);
    }
    let row: usize = digits.trim().parse().ok()?;
    if row == 0 {
        return None;
    }
    Some((col - 1, row - 1))
}

fn cell_value(kind: &str, raw: &str, shared: &[String]) -> String {
    match kind {
        "s" => raw
            .parse::<usize>()
            .ok()
            .and_then(|index| shared.get(index).cloned())
            .unwrap_or_default(),
        "inlineStr" | "str" | "e" => raw.to_string(),
        "b" => match raw {
            "1" => "TRUE".to_string(),
            "0" => "FALSE".to_string(),
            other => other.to_string(),
        },
        _ => raw.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::DeflateEncoder;
    use flate2::Compression;
    use std::io::Write;

    /* ---------- 自己拼一个最小 zip：本地头 + 中央目录 + EOCD ---------- */

    fn zip_with(files: &[(&str, &str)], method: u16) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        let mut meta: Vec<(&str, usize, usize, usize)> = Vec::new(); // name, csize, usize, offset
        for (name, body) in files {
            let raw = body.as_bytes();
            let data = match method {
                8 => {
                    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
                    encoder.write_all(raw).unwrap();
                    encoder.finish().unwrap()
                }
                _ => raw.to_vec(),
            };
            let offset = out.len();
            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&20u16.to_le_bytes()); // version needed
            out.extend_from_slice(&0u16.to_le_bytes()); // flags
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // mod time
            out.extend_from_slice(&0u16.to_le_bytes()); // mod date
            out.extend_from_slice(&0u32.to_le_bytes()); // crc32（我们不校验）
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(raw.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // extra len
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&data);
            meta.push((name, data.len(), raw.len(), offset));
        }
        let central_at = out.len();
        for (name, comp, size, offset) in &meta {
            out.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            out.extend_from_slice(&20u16.to_le_bytes()); // version made by
            out.extend_from_slice(&20u16.to_le_bytes()); // version needed
            out.extend_from_slice(&0u16.to_le_bytes()); // flags
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes()); // crc
            out.extend_from_slice(&(*comp as u32).to_le_bytes());
            out.extend_from_slice(&(*size as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // extra
            out.extend_from_slice(&0u16.to_le_bytes()); // comment
            out.extend_from_slice(&0u16.to_le_bytes()); // disk
            out.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
            out.extend_from_slice(&0u32.to_le_bytes()); // external attrs
            out.extend_from_slice(&(*offset as u32).to_le_bytes());
            out.extend_from_slice(name.as_bytes());
        }
        let central_size = out.len() - central_at;
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(meta.len() as u16).to_le_bytes());
        out.extend_from_slice(&(meta.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central_size as u32).to_le_bytes());
        out.extend_from_slice(&(central_at as u32).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // comment len
        out
    }

    fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
        zip_with(files, 0)
    }

    const DOCX_HEAD: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>"#;
    const DOCX_TAIL: &str = "</w:body></w:document>";

    fn docx_bytes(body: &str) -> Vec<u8> {
        let xml = format!("{DOCX_HEAD}{body}{DOCX_TAIL}");
        zip_of(&[("word/document.xml", &xml)])
    }

    /* ---------- docx ---------- */

    #[test]
    fn docx_reads_heading_bold_center_table_and_margins() {
        let body = r#"
<w:p><w:pPr><w:pStyle w:val="Heading1"/><w:jc w:val="center"/></w:pPr>
  <w:r><w:rPr><w:b/><w:sz w:val="32"/></w:rPr><w:t>项目经历</w:t></w:r></w:p>
<w:p><w:r><w:rPr><w:b w:val="0"/><w:sz w:val="24"/></w:rPr><w:t>谭轶天</w:t></w:r></w:p>
<w:tbl><w:tblPr><w:jc w:val="right"/></w:tblPr>
  <w:tr><w:tc><w:p><w:r><w:t>技能</w:t></w:r></w:p></w:tc>
        <w:tc><w:p><w:r><w:t>Rust</w:t></w:r></w:p></w:tc></w:tr>
  <w:tr><w:tc><w:p><w:r><w:t>语言</w:t></w:r></w:p></w:tc>
        <w:tc><w:p><w:r><w:t>中文</w:t></w:r></w:p></w:tc></w:tr>
</w:tbl>
<w:p><w:pPr><w:numPr><w:ilvl w:val="1"/></w:numPr></w:pPr><w:r><w:t>带缩进的列表项</w:t></w:r></w:p>
<w:sectPr><w:pgMar w:top="1134" w:right="1134" w:bottom="1134" w:left="1701"/></w:sectPr>"#;
        let outline = parse("docx", &docx_bytes(body)).expect("应当解析成功");

        assert_eq!(outline.pages, None, "docx 没有可信页数，不许编");
        let margin = outline.margin_pt.expect("pgMar 读到了");
        // 契约顺序 [左, 上, 右, 下]，twips ÷ 20
        assert!((margin[0] - 85.05).abs() < 0.01, "{margin:?}");
        assert!((margin[1] - 56.7).abs() < 0.01, "{margin:?}");
        assert!((margin[2] - 56.7).abs() < 0.01, "{margin:?}");
        assert!((margin[3] - 56.7).abs() < 0.01, "{margin:?}");

        assert_eq!(outline.blocks.len(), 4, "{:?}", outline.blocks);

        let heading = &outline.blocks[0];
        assert_eq!(heading.kind, BlockKind::Heading);
        assert_eq!(heading.style.level, Some(1));
        assert_eq!(heading.style.align, Align::Center);
        assert!(heading.style.bold);
        assert_eq!(heading.style.size_pt, Some(16.0), "半磅 32 = 16pt");

        let body_block = &outline.blocks[1];
        assert_eq!(body_block.kind, BlockKind::Paragraph);
        assert!(!body_block.style.bold, "w:b w:val=\"0\" 是关");
        assert_eq!(body_block.style.size_pt, Some(12.0));
        assert_eq!(body_block.style.align, Align::Left, "表格对齐不许串到段落上");

        let table = &outline.blocks[2];
        assert_eq!(table.kind, BlockKind::Table);
        let grid = table.table.as_ref().expect("表格要有二维数组");
        assert_eq!(grid.len(), 2);
        assert_eq!(grid[0], ["技能", "Rust"]);
        assert_eq!(grid[1], ["语言", "中文"]);
        assert_eq!(table.text, "技能 | Rust\n语言 | 中文");

        let item = &outline.blocks[3];
        assert_eq!(item.kind, BlockKind::ListItem);
        assert_eq!(item.style.indent, 1);
    }

    #[test]
    fn docx_heading_survives_an_opaque_style_id() {
        // 中文版 Word：w:pStyle 写的是内部 id "1"，真名在 styles.xml 里
        let document = format!(
            "{DOCX_HEAD}<w:p><w:pPr><w:pStyle w:val=\"1\"/></w:pPr><w:r><w:t>教育经历</w:t></w:r></w:p>{DOCX_TAIL}"
        );
        let styles = r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:style w:type="paragraph" w:styleId="1"><w:name w:val="heading 2"/></w:style></w:styles>"#;
        let bytes = zip_of(&[("word/document.xml", &document), ("word/styles.xml", styles)]);
        let outline = parse("docx", &bytes).unwrap();
        assert_eq!(outline.blocks[0].kind, BlockKind::Heading);
        assert_eq!(outline.blocks[0].style.level, Some(2));
    }

    #[test]
    fn docx_outline_level_is_a_fallback_for_headings() {
        let body = r#"<w:p><w:pPr><w:outlineLvl w:val="2"/></w:pPr><w:r><w:t>技能清单</w:t></w:r></w:p>"#;
        let outline = parse("docx", &docx_bytes(body)).unwrap();
        assert_eq!(outline.blocks[0].kind, BlockKind::Heading);
        assert_eq!(outline.blocks[0].style.level, Some(3), "outlineLvl 从 0 起");
    }

    #[test]
    fn docx_heading_name_without_styles_xml_still_works() {
        let body = r#"<w:p><w:pPr><w:pStyle w:val="Heading3"/></w:pPr><w:r><w:t>实习</w:t></w:r></w:p>"#;
        let outline = parse("docx", &docx_bytes(body)).unwrap();
        assert_eq!(outline.blocks[0].style.level, Some(3));
    }

    #[test]
    fn docx_empty_paragraphs_produce_no_blocks_and_an_empty_doc_is_none() {
        let body = r#"<w:p/><w:p><w:pPr><w:jc w:val="center"/></w:pPr></w:p>"#;
        assert!(parse("docx", &docx_bytes(body)).is_none(), "空文档绝不能给空大纲");
    }

    #[test]
    fn deflated_entries_are_read() {
        let body = r#"<w:p><w:r><w:t>压缩过的条目</w:t></w:r></w:p>"#;
        let xml = format!("{DOCX_HEAD}{body}{DOCX_TAIL}");
        let bytes = zip_with(&[("word/document.xml", &xml)], 8);
        let outline = parse("docx", &bytes).expect("method 8 要能解");
        assert_eq!(outline.blocks[0].text, "压缩过的条目");
    }

    #[test]
    fn docx_truncates_at_the_block_limit() {
        let mut body = String::new();
        for i in 0..(MAX_BLOCKS + 5) {
            body.push_str(&format!("<w:p><w:r><w:t>第{i}段</w:t></w:r></w:p>"));
        }
        let outline = parse("docx", &docx_bytes(&body)).unwrap();
        assert_eq!(outline.blocks.len(), MAX_BLOCKS + 1, "5000 块 + 1 条 Note");
        let last = outline.blocks.last().unwrap();
        assert_eq!(last.kind, BlockKind::Note);
        assert!(last.text.contains(&MAX_BLOCKS.to_string()), "{}", last.text);
    }

    /* ---------- xlsx ---------- */

    #[test]
    fn xlsx_keeps_cell_positions_and_joins_rich_text() {
        let workbook = r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<sheets><sheet name="简历" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
        let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;
        let shared = r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<si><t>姓名</t></si>
<si><r><rPr><b/></rPr><t>谭</t></r><r><t>轶天</t></r></si></sst>"#;
        let sheet = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>
<row r="1"><c r="A1" t="s"><v>0</v></c><c r="C1" t="inlineStr"><is><t>备注</t></is></c></row>
<row r="3"><c r="B3" t="s"><v>1</v></c></row>
</sheetData></worksheet>"#;
        let bytes = zip_of(&[
            ("xl/workbook.xml", workbook),
            ("xl/_rels/workbook.xml.rels", rels),
            ("xl/sharedStrings.xml", shared),
            ("xl/worksheets/sheet1.xml", sheet),
        ]);
        let outline = parse("xlsx", &bytes).expect("应当解析成功");
        assert_eq!(outline.pages, None, "sheet 数不是页数");
        assert_eq!(outline.blocks.len(), 2);

        assert_eq!(outline.blocks[0].kind, BlockKind::Heading);
        assert_eq!(outline.blocks[0].text, "简历");

        let table = outline.blocks[1].table.as_ref().expect("要有二维表");
        assert_eq!(table.len(), 3, "第 2 行整行空着也要占位");
        assert_eq!(table[0], ["姓名", "", "备注"]);
        assert_eq!(table[1], ["", "", ""], "跳过的格子补空串，不许挤压");
        assert_eq!(table[2], ["", "谭轶天", ""], "B3 就该在第 3 行第 2 列");
    }

    #[test]
    fn xlsx_multiple_sheets_in_workbook_order() {
        let workbook = r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<sheets><sheet name="第一表" sheetId="1" r:id="rId2"/><sheet name="第二表" sheetId="2" r:id="rId1"/></sheets></workbook>"#;
        // rels 故意把部件顺序反过来：只有 rels 是权威的
        let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Target="worksheets/sheet2.xml"/>
<Relationship Id="rId2" Target="/xl/worksheets/sheet1.xml"/></Relationships>"#;
        let one = r#"<worksheet><sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData></worksheet>"#;
        let two = r#"<worksheet><sheetData><row r="1"><c r="A1"><v>2</v></c></row></sheetData></worksheet>"#;
        let bytes = zip_of(&[
            ("xl/workbook.xml", workbook),
            ("xl/_rels/workbook.xml.rels", rels),
            ("xl/worksheets/sheet1.xml", one),
            ("xl/worksheets/sheet2.xml", two),
        ]);
        let outline = parse("xlsx", &bytes).unwrap();
        let names: Vec<&str> = outline
            .blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Heading)
            .map(|b| b.text.as_str())
            .collect();
        assert_eq!(names, ["第一表", "第二表"]);
        let first = outline.blocks[1].table.as_ref().unwrap();
        assert_eq!(first[0][0], "1", "第一表必须指向 rId2 的 sheet1");
    }

    #[test]
    fn xlsx_skips_phonetic_runs_in_shared_strings() {
        let workbook = r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<sheets><sheet name="表" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
        let shared = r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<si><t>東京</t><rPh sb="0" eb="2"><t>トウキョウ</t></rPh></si></sst>"#;
        let sheet = r#"<worksheet><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c></row></sheetData></worksheet>"#;
        let bytes = zip_of(&[
            ("xl/workbook.xml", workbook),
            ("xl/sharedStrings.xml", shared),
            ("xl/worksheets/sheet1.xml", sheet),
        ]);
        let outline = parse("xlsx", &bytes).unwrap();
        assert_eq!(outline.blocks[1].table.as_ref().unwrap()[0][0], "東京");
    }

    #[test]
    fn xlsx_without_any_cell_is_none() {
        let workbook = r#"<workbook xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<sheets><sheet name="空表" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
        let sheet = r#"<worksheet><sheetData></sheetData></worksheet>"#;
        let bytes = zip_of(&[("xl/workbook.xml", workbook), ("xl/worksheets/sheet1.xml", sheet)]);
        assert!(parse("xlsx", &bytes).is_none());
    }

    /* ---------- pptx ---------- */

    const SLIDE_HEAD: &str = r#"<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
 xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree>"#;
    const SLIDE_TAIL: &str = "</p:spTree></p:cSld></p:sld>";

    #[test]
    fn pptx_title_placeholder_becomes_title() {
        let slide = format!(
            r#"{SLIDE_HEAD}
<p:sp><p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:txBody>
  <a:p><a:r><a:rPr sz="2400" b="1"/><a:t>谭轶天</a:t></a:r></a:p></p:txBody></p:sp>
<p:sp><p:nvSpPr><p:nvPr><p:ph type="body"/></p:nvPr></p:nvSpPr><p:txBody>
  <a:p><a:pPr algn="ctr"/><a:r><a:rPr sz="1200"/><a:t>求职意向：后端工程师</a:t></a:r></a:p>
</p:txBody></p:sp>
{SLIDE_TAIL}"#
        );
        let bytes = zip_of(&[("ppt/slides/slide1.xml", &slide)]);
        let outline = parse("pptx", &bytes).expect("应当解析成功");
        assert_eq!(outline.pages, Some(1));
        assert_eq!(outline.blocks.len(), 2);

        assert_eq!(outline.blocks[0].kind, BlockKind::Title);
        assert_eq!(outline.blocks[0].text, "谭轶天");
        assert_eq!(outline.blocks[0].style.size_pt, Some(24.0), "1/100 磅 → pt");
        assert!(outline.blocks[0].style.bold);

        assert_eq!(outline.blocks[1].kind, BlockKind::Paragraph, "body 占位符不是标题");
        assert_eq!(outline.blocks[1].style.align, Align::Center);
        assert_eq!(outline.blocks[1].style.size_pt, Some(12.0));
    }

    #[test]
    fn pptx_slides_are_read_in_numeric_order_and_counted() {
        let slide = |text: &str| {
            format!(
                r#"{SLIDE_HEAD}<p:sp><p:nvSpPr><p:nvPr><p:ph type="ctrTitle"/></p:nvPr></p:nvSpPr>
<p:txBody><a:p><a:r><a:t>{text}</a:t></a:r></a:p></p:txBody></p:sp>{SLIDE_TAIL}"#
            )
        };
        let bytes = zip_of(&[
            ("ppt/slides/slide10.xml", &slide("第十张")),
            ("ppt/slides/slide2.xml", &slide("第二张")),
            ("ppt/slides/slide1.xml", &slide("第一张")),
        ]);
        let outline = parse("pptx", &bytes).unwrap();
        assert_eq!(outline.pages, Some(3));
        let texts: Vec<&str> = outline.blocks.iter().map(|b| b.text.as_str()).collect();
        assert_eq!(texts, ["第一张", "第二张", "第十张"], "按数字序，不是字典序");
        assert!(outline.blocks.iter().all(|b| b.kind == BlockKind::Title));
    }

    #[test]
    fn pptx_table_is_a_table() {
        let slide = format!(
            r#"{SLIDE_HEAD}<p:graphicFrame><a:graphic><a:graphicData><a:tbl>
<a:tr><a:tc><a:txBody><a:p><a:r><a:t>技能</a:t></a:r></a:p></a:txBody></a:tc>
      <a:tc><a:txBody><a:p><a:r><a:t>Rust</a:t></a:r></a:p></a:txBody></a:tc></a:tr>
</a:tbl></a:graphicData></a:graphic></p:graphicFrame>{SLIDE_TAIL}"#
        );
        let bytes = zip_of(&[("ppt/slides/slide1.xml", &slide)]);
        let outline = parse("pptx", &bytes).unwrap();
        assert_eq!(outline.blocks.len(), 1);
        assert_eq!(outline.blocks[0].kind, BlockKind::Table);
        assert_eq!(outline.blocks[0].table.as_ref().unwrap()[0], ["技能", "Rust"]);
    }

    /* ---------- 入口与 zip 的边界 ---------- */

    #[test]
    fn junk_and_unknown_containers_are_none() {
        assert!(parse("docx", b"not a zip at all").is_none());
        assert!(parse("docx", b"PK\x03\x04truncated").is_none());
        assert!(parse("docx", &zip_of(&[("word/other.xml", "<x/>")])).is_none());
        assert!(parse("doc", &zip_of(&[])).is_none(), "未知扩展名不猜");
    }

    #[test]
    fn zip_entries_are_found_by_name_and_decompressed() {
        let bytes = zip_with(&[("a.txt", "hello 世界"), ("b.txt", "second")], 8);
        let zip = Zip::new(&bytes).unwrap();
        assert_eq!(zip.text("a.txt").unwrap(), "hello 世界");
        assert_eq!(zip.text("b.txt").unwrap(), "second");
        assert!(zip.text("missing.txt").is_none());
    }

    #[test]
    fn table_tidying_drops_trailing_empties_without_shifting_cells() {
        let rows = vec![
            vec!["a".to_string(), String::new(), String::new()],
            vec![String::new(), "b".to_string(), String::new()],
            vec![String::new(); 3],
        ];
        let (tidy, truncated) = tidy_table(rows);
        assert!(!truncated);
        assert_eq!(tidy, [["a", ""], ["", "b"]], "右侧空列要收掉，行内空格要留");
    }
}
