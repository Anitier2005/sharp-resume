//! PDF 的带格式解析。
//!
//! `pdf_extract` 只能给纯文本：姓名那行的「居中」是 PDF 里的**坐标**，字号、行距、
//! 分栏全在提取时丢掉了。要拿回它们只能自己走 content stream：
//! `Tf` 给字号，`Tm`/`Td`/`cm` 给位置，`Tj`/`TJ` 给字。
//!
//! ## 文字怎么还原成 Unicode
//!
//! PDF 里的字符是**字形码**（CID），不是 Unicode。中文 PDF 几乎全是
//! `/Type0` + `/Identity-H`，`<186A>` 这样的两字节码只能靠字体的 `/ToUnicode`
//! CMap（`beginbfchar` / `beginbfrange`）反查。
//!
//! 实测结论（用户那份 WPS 导出的简历）：**ToUnicode 可行** —— 字体都带
//! `/ToUnicode`，`<186A>` → 「谭」。所以这里走正统路线，不做「pdf_extract 出字
//! + lopdf 出坐标」的两路对齐：那套要靠字符串比对猜对应，遇到重字、换行就会错位。
//!
//! 只保留一条退路：**简单字体**（Type1/TrueType，无 ToUnicode）按 ASCII 直通
//! 解码 —— 那类字体本来就是单字节编码，`0x20..0x7e` 与 ASCII 完全重合。
//! 映射率低于 [`MIN_MAPPED`] 就判定「这份文件我读不了」，返回 `None` 让调用方
//! 回落，**绝不吐乱码，也绝不返回空大纲**。
//!
//! ## 分层
//!
//! - [`interpret`]：content stream → `Vec<Run>`（带坐标与字号）。碰 lopdf 的部分
//!   全在这里，**没法单测**（仓库里不放用户样本）；
//! - 纯函数层：[`group_lines`] 聚行 → [`group_paragraphs`] 归段 →
//!   [`build_blocks`] 分级 → [`align_of`] 对齐。全是 `Vec<Run>` / `Vec<Line>` 进出，
//!   用合成数据单测（居中、左、右、一级/二级标题、列表、段间距、跨页、表格）。

use std::collections::HashMap;

use lopdf::content::Content;
use lopdf::{Dictionary, Document, Object, ObjectId};

use super::{Align, BlockKind, BlockStyle, DocBlock, DocOutline};

/* ==========================================================================
   阈值：全部集中在这里，单测与实现共用一份，避免「测试按 A 写、代码按 B 跑」
   ========================================================================== */

/// 同一行的 y 容差（相对字号，再夹到下面的 pt 区间）
const LINE_TOL_RATIO: f32 = 0.35;
const LINE_TOL_MIN: f32 = 1.2;
const LINE_TOL_MAX: f32 = 3.0;
/// 行内两段字的水平间距超过这个倍数（相对字号）就补一个空格
const WORD_GAP: f32 = 0.35;
/// 单元格内允许的最大间隙倍数；超过就当成「另起一列」
const CELL_GAP: f32 = 2.0;
/// 段内行距上限（相对字号）：超过就是另一段
const PARA_GAP: f32 = 1.95;
/// 段落内字号允许的抖动（相对）
const SIZE_TOL: f32 = 0.08;
/// 对齐判定：中线偏离页心超过这个比例（相对页宽）就不算居中
const CENTER_TOL: f32 = 0.03;
/// 右对齐：右边缘离页右边小于这个比例（相对页宽）才算贴右边。
/// 12% 是因为正常页边距就有 8~10%，卡 5% 会漏掉「贴着自己右边距」的行。
const RIGHT_TOL: f32 = 0.12;
/// 大字号标题判定（相对正文）
const TITLE_RATIO: f32 = 1.45;
/// 小字号标题判定（相对正文）
const HEADING_RATIO: f32 = 1.12;
/// 表格骨架：列左边缘跨行允许的抖动（pt）
const COL_TOL: f32 = 5.0;
/// 表格骨架：至少要几列几行
const TABLE_COLS: usize = 3;
const TABLE_ROWS: usize = 2;
/// 表格相邻行的行距上限（相对字号）：比正文宽松，表行本来就排得开
const TABLE_GAP: f32 = 4.0;
/// 文字映射率下限：低于它说明 ToUnicode 大面积缺失，宁可整份放弃
const MIN_MAPPED: f32 = 0.6;
/// Form XObject 递归上限（自引用的坏文档在这里停手）
const MAX_FORM_DEPTH: u8 = 4;

/* ==========================================================================
   入口
   ========================================================================== */

pub fn parse(bytes: &[u8]) -> Option<DocOutline> {
    let mut doc = Document::load_mem(bytes).ok()?;
    // 加密文档的 content stream 是密文，硬解只会得到乱码
    if doc.is_encrypted() && doc.decrypt("").is_err() {
        return None;
    }

    let pages = doc.get_pages();
    if pages.is_empty() {
        return None;
    }

    let mut runs: Vec<Run> = Vec::new();
    let mut stats = Stats::default();
    let mut first_w = 0.0f32;
    let mut first_h = 0.0f32;

    for (index, (page_no, page_id)) in pages.iter().enumerate() {
        let (w, h) = media_size(&doc, *page_id);
        if index == 0 {
            first_w = w;
            first_h = h;
        }
        let page = PageCtx::load(&doc, *page_id);
        let Ok(content) = doc.get_page_content(*page_id) else { continue };
        interpret(&content, &page, *page_no, h as f32, &mut runs, &mut stats, 0);
    }

    // 没有文字层（扫描件）→ 交给调用方回落到「读不到」，不要伪造结构
    if runs.is_empty() || stats.total == 0 || stats.ratio() < MIN_MAPPED {
        return None;
    }

    let lines = group_lines(&runs);
    if lines.is_empty() {
        return None;
    }
    let groups = group_paragraphs(&lines, first_w);
    let body = body_size(&groups, &lines);
    let blocks = build_blocks(&groups, &lines, first_w, body);
    if blocks.is_empty() {
        return None;
    }

    Some(DocOutline {
        pages: Some(pages.len() as u32),
        margin_pt: margins(&lines, first_w, first_h),
        body_pt: Some(round1(body)),
        blocks,
    })
}

/* ==========================================================================
   几何归纳（纯函数，可单测）
   ========================================================================== */

/// 一段已定位的文字。坐标一律是「从页顶往下量」的 pt，`page` 从 1 开始。
#[derive(Debug, Clone, PartialEq)]
struct Run {
    page: u32,
    x: f32,
    /// 基线（从页顶往下）
    y: f32,
    size: f32,
    /// 估宽（pt）：只用于对齐判断，不需要精确字宽
    w: f32,
    bold: bool,
    italic: bool,
    text: String,
}

/// 聚出来的一行。
#[derive(Debug, Clone, PartialEq)]
struct Line {
    page: u32,
    x: f32,
    y: f32,
    /// 右边缘（含末字宽度）
    right: f32,
    size: f32,
    bold: bool,
    italic: bool,
    text: String,
    /// 按大间隙切出来的单元格（表格骨架用）：(左边缘, 文字)
    cells: Vec<(f32, String)>,
}

impl Line {
    fn width(&self) -> f32 {
        (self.right - self.x).max(0.0)
    }
}

fn line_tol(size: f32) -> f32 {
    (size * LINE_TOL_RATIO).clamp(LINE_TOL_MIN, LINE_TOL_MAX)
}

/// CJK 与全角标点按 1.0em 计宽，其余 0.5em。**够用就好** ——
/// 对齐判断只需要「行有多宽」，不需要和渲染器逐像素一致。
fn em_width(ch: char) -> f32 {
    let c = ch as u32;
    let wide = (0x1100..=0x115F).contains(&c)
        || (0x2E80..=0xA4CF).contains(&c)
        || (0xAC00..=0xD7A3).contains(&c)
        || (0xF900..=0xFAFF).contains(&c)
        || (0xFE30..=0xFE6F).contains(&c)
        || (0xFF00..=0xFF60).contains(&c)
        || (0xFFE0..=0xFFE6).contains(&c)
        || (0x20000..=0x3FFFD).contains(&c);
    if wide {
        1.0
    } else {
        0.5
    }
}

fn text_width(text: &str, size: f32) -> f32 {
    text.chars().map(|ch| em_width(ch) * size).sum()
}

fn is_wide(ch: char) -> bool {
    em_width(ch) >= 1.0
}

/// 按 y 聚行（容差随字号缩放），再按 x 排字、补词间空格、切单元格。
fn group_lines(runs: &[Run]) -> Vec<Line> {
    let mut sorted: Vec<&Run> = runs.iter().filter(|r| !r.text.trim().is_empty()).collect();
    sorted.sort_by(|a, b| {
        a.page
            .cmp(&b.page)
            .then(a.y.total_cmp(&b.y))
            .then(a.x.total_cmp(&b.x))
    });

    let mut lines: Vec<Line> = Vec::new();
    let mut bucket: Vec<&Run> = Vec::new();
    for run in sorted {
        let same = bucket.first().is_some_and(|first: &&Run| {
            first.page == run.page
                && (run.y - first.y).abs() <= line_tol(first.size.max(run.size))
        });
        if same {
            bucket.push(run);
        } else {
            if !bucket.is_empty() {
                lines.push(finish_line(&bucket));
            }
            bucket = vec![run];
        }
    }
    if !bucket.is_empty() {
        lines.push(finish_line(&bucket));
    }
    lines
}

fn finish_line(runs: &[&Run]) -> Line {
    let mut sorted = runs.to_vec();
    sorted.sort_by(|a, b| a.x.total_cmp(&b.x));

    let size = sorted.iter().fold(0.0f32, |m, r| m.max(r.size));
    let mut text = String::new();
    let mut cells: Vec<(f32, String)> = Vec::new();
    let mut cell_x = sorted.first().map(|r| r.x).unwrap_or(0.0);
    let mut cell = String::new();
    let mut prev_right: Option<f32> = None;
    let mut prev_char: Option<char> = None;
    let mut bold = 0usize;
    let mut chars = 0usize;

    for run in &sorted {
        if let Some(prev) = prev_right {
            let gap = run.x - prev;
            let first = run.text.chars().next();
            // 两个汉字之间不补空格（导出器常把每个字单独定位，补了就成「一 份 简 历」）
            let cjk_pair = prev_char.is_some_and(is_wide) && first.is_some_and(is_wide);
            if gap > size * WORD_GAP && !cjk_pair {
                text.push(' ');
                if !cell.is_empty() {
                    cell.push(' ');
                }
            }
            if gap > size * CELL_GAP {
                if !cell.trim().is_empty() {
                    cells.push((cell_x, cell.trim().to_string()));
                }
                cell.clear();
                cell_x = run.x;
            }
        }
        text.push_str(&run.text);
        cell.push_str(&run.text);
        let count = run.text.chars().count();
        chars += count;
        if run.bold {
            bold += count;
        }
        prev_right = Some(run.x + run.w);
        prev_char = run.text.chars().last().or(prev_char);
    }
    if !cell.trim().is_empty() {
        cells.push((cell_x, cell.trim().to_string()));
    }

    let x = sorted.first().map(|r| r.x).unwrap_or(0.0);
    let right = sorted
        .iter()
        .map(|r| r.x + r.w)
        .fold(f32::NEG_INFINITY, f32::max);

    Line {
        page: sorted[0].page,
        x,
        y: sorted[0].y,
        right: if right.is_finite() { right } else { x },
        size,
        bold: chars > 0 && bold * 2 >= chars,
        italic: sorted.iter().filter(|r| r.italic).count() * 2 >= sorted.len(),
        text: text.trim().to_string(),
        cells,
    }
}

/// 归段：同页、同字号、同对齐、行距在 [`PARA_GAP`] 以内，才算同一段。
/// 返回的是**行下标**，顺序即阅读顺序。
fn group_paragraphs(lines: &[Line], page_w: f32) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let group = paragraph_from(lines, index, page_w);
        index = group.last().map(|last| last + 1).unwrap_or(index + 1);
        groups.push(group);
    }
    groups
}

/// 以 `start` 为起点的最大段落（连续、可合并的行下标）
fn paragraph_from(lines: &[Line], start: usize, page_w: f32) -> Vec<usize> {
    let mut group = vec![start];
    let mut index = start + 1;
    while index < lines.len() {
        if !mergeable(&lines[index - 1], &lines[index], page_w) {
            break;
        }
        group.push(index);
        index += 1;
    }
    group
}

fn mergeable(prev: &Line, line: &Line, page_w: f32) -> bool {
    if prev.page != line.page {
        return false;
    }
    // 字号差得多 → 是标题在换正文
    if (prev.size - line.size).abs() > prev.size.max(line.size) * SIZE_TOL {
        return false;
    }
    // 对齐方式变了 → 另一块
    if align_of(prev, page_w) != align_of(line, page_w) {
        return false;
    }
    let gap = line.y - prev.y;
    if gap <= 0.0 || gap > prev.size.max(line.size) * PARA_GAP {
        return false;
    }
    // 列表项自成一块
    !starts_item(&line.text) && !starts_item(&prev.text)
}

/// 列表项的记号：原件里就是这些字符开头
const ITEM_MARKS: [char; 12] = ['·', '•', '●', '○', '◆', '◇', '▪', '■', '□', '◦', '*', '＊'];

fn starts_item(text: &str) -> bool {
    let trimmed = text.trim_start();
    let Some(first) = trimmed.chars().next() else { return false };
    if ITEM_MARKS.contains(&first) {
        return true;
    }
    // 「- xxx」；单独一个连字符不算（那是分隔线）
    if (first == '-' || first == '–' || first == '—')
        && trimmed.chars().nth(1).is_some_and(char::is_whitespace)
    {
        return true;
    }
    // 「1. xxx」「1、xxx」「1) xxx」
    starts_numbered(trimmed)
}

fn starts_numbered(text: &str) -> bool {
    let mut chars = text.chars().peekable();
    let mut digits = 0;
    while let Some(ch) = chars.peek().copied() {
        if ch.is_ascii_digit() {
            digits += 1;
            chars.next();
        } else {
            break;
        }
    }
    if digits == 0 || digits > 2 {
        return false;
    }
    chars
        .peek()
        .copied()
        .is_some_and(|ch| ch == '.' || ch == '、' || ch == ')' || ch == '．')
}

/// 对齐：先看行中线离页心多远，再看右边缘是否贴右页边距。
fn align_of(line: &Line, page_w: f32) -> Align {
    if page_w <= 0.0 {
        return Align::Left;
    }
    let mid = (line.x + line.right) / 2.0;
    if (mid - page_w / 2.0).abs() < page_w * CENTER_TOL {
        return Align::Center;
    }
    // 只有整行都落在右半页、且右边缘贴边，才算右对齐
    // （顶格撑满的正文右边缘也贴边，但它的左边缘在左边距）
    if line.x > page_w * 0.5 && page_w - line.right < page_w * RIGHT_TOL {
        return Align::Right;
    }
    Align::Left
}

/// 正文基准字号：按**字数**加权取众数（按块数会被短标题带偏）。
fn body_size(groups: &[Vec<usize>], lines: &[Line]) -> f32 {
    let mut weights: Vec<(f32, usize)> = Vec::new();
    for group in groups {
        let Some(first) = group.first().map(|i| &lines[*i]) else { continue };
        let chars: usize = group.iter().map(|i| lines[*i].text.chars().count()).sum();
        let key = round1(first.size);
        match weights.iter_mut().find(|(size, _)| (*size - key).abs() < 0.1) {
            Some((_, weight)) => *weight += chars,
            None => weights.push((key, chars)),
        }
    }
    weights.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.total_cmp(&b.0)));
    weights.first().map(|(size, _)| *size).unwrap_or(12.0)
}

/// 组装块：表格骨架 → 列表项 → 标题分级 → 段落。
fn build_blocks(groups: &[Vec<usize>], lines: &[Line], page_w: f32, body: f32) -> Vec<DocBlock> {
    // 文档标题的字号：最上方那个比正文大一截的块。它不参与标题分级
    // （否则「姓名 20pt」会把 14pt 的一级标题挤成二级）
    let title_size = groups
        .iter()
        .take(3)
        .filter_map(|group| group.first().map(|i| round1(lines[*i].size)))
        .find(|size| *size >= body * TITLE_RATIO);

    // 其余的「比正文大」的字号排名：最大的一级标题
    let mut sizes: Vec<f32> = groups
        .iter()
        .filter_map(|group| group.first().map(|i| round1(lines[*i].size)))
        .filter(|size| *size >= body * HEADING_RATIO)
        .filter(|size| !title_size.is_some_and(|title| (title - *size).abs() < 0.1))
        .collect();
    sizes.sort_by(|a, b| b.total_cmp(a));
    sizes.dedup_by(|a, b| (*a - *b).abs() < 0.1);

    let mut blocks: Vec<DocBlock> = Vec::new();
    let mut seen_title = false;
    let min_left = lines.iter().map(|l| l.x).fold(f32::INFINITY, f32::min);
    let indent_step = (body * 1.5).max(1.0);

    let mut index = 0;
    while index < lines.len() {
        // 表格先判：它的行距往往比正文还大，按「段落」切会一行一段
        if let Some(rows) = table_at(lines, index) {
            let span = &lines[index..index + rows];
            let Some(cells) = as_table(span) else { break };
            blocks.push(DocBlock {
                kind: BlockKind::Table,
                text: span
                    .iter()
                    .map(|line| line.cells.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join("\t"))
                    .collect::<Vec<_>>()
                    .join("\n"),
                style: BlockStyle {
                    size_pt: Some(round1(span[0].size)),
                    align: align_of(&span[0], page_w),
                    ..Default::default()
                },
                table: Some(cells),
            });
            index += rows;
            continue;
        }

        let group = paragraph_from(lines, index, page_w);
        index = group.last().map(|last| last + 1).unwrap_or(index + 1);

        let head = &lines[group[0]];
        let text = group
            .iter()
            .map(|i| lines[*i].text.trim())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if text.is_empty() {
            continue;
        }
        // 对齐取最宽的那一行：短行（「2019」这种）夹在段落里代表不了整块
        let align = group
            .iter()
            .map(|i| &lines[*i])
            .max_by(|a, b| a.width().total_cmp(&b.width()))
            .map(|line| align_of(line, page_w))
            .unwrap_or(Align::Left);

        let size = round1(head.size);
        let ratio = if body > 0.0 { size / body } else { 1.0 };
        let level = sizes
            .iter()
            .position(|s| (*s - size).abs() < 0.1)
            .map(|rank| (rank + 1).min(6) as u8);

        let (kind, level) = if ratio >= TITLE_RATIO && !seen_title && blocks.len() < 3 {
            // 最上方的那个大字是「文档标题」（简历里就是姓名）
            seen_title = true;
            (BlockKind::Title, None)
        } else if ratio >= HEADING_RATIO {
            (BlockKind::Heading, level.or(Some(1)))
        } else if starts_item(&head.text) {
            (BlockKind::ListItem, None)
        } else {
            (BlockKind::Paragraph, None)
        };

        let indent = ((head.x - min_left) / indent_step).round().clamp(0.0, 6.0) as u8;
        blocks.push(DocBlock {
            kind,
            text,
            style: BlockStyle {
                size_pt: Some(size),
                // 字号大于正文的标题直接算粗体：视觉权重比字体名可靠
                bold: head.bold || ratio >= HEADING_RATIO,
                italic: head.italic,
                align,
                indent,
                level,
            },
            table: None,
        });
    }
    blocks
}

/// 从 `start` 起有几行构成表格骨架（0 = 不是表格）。
///
/// 表格的行距普遍大于正文行距，所以**不能**按段落切了再判 —— 那样一行一段，
/// 永远凑不够两行。这里直接看行：列数一致、列左边缘跨行对齐、纵向不跳页。
fn table_at(lines: &[Line], start: usize) -> Option<usize> {
    let first = lines.get(start)?;
    let cols = first.cells.len();
    if cols < TABLE_COLS {
        return None;
    }
    let mut rows = 1;
    while let Some(next) = lines.get(start + rows) {
        let prev = &lines[start + rows - 1];
        if next.page != prev.page || next.cells.len() != cols {
            break;
        }
        let gap = next.y - prev.y;
        if gap <= 0.0 || gap > prev.size.max(next.size) * TABLE_GAP {
            break;
        }
        let aligned = (0..cols)
            .all(|col| (next.cells[col].0 - first.cells[col].0).abs() <= COL_TOL);
        if !aligned {
            break;
        }
        rows += 1;
    }
    (rows >= TABLE_ROWS).then_some(rows)
}

/// 表格骨架：行 × 列的文字。不做合并单元格还原 —— 多的给不了，
/// 给错了比不给更坏。
fn as_table(rows: &[Line]) -> Option<Vec<Vec<String>>> {
    if rows.len() < TABLE_ROWS {
        return None;
    }
    let cols = rows[0].cells.len();
    if cols < TABLE_COLS || rows.iter().any(|line| line.cells.len() != cols) {
        return None;
    }
    Some(
        rows.iter()
            .map(|line| line.cells.iter().map(|(_, text)| text.clone()).collect())
            .collect(),
    )
}

/// 版心：所有行的最小左 / 最小上 / 右留白 / 下留白。
fn margins(lines: &[Line], page_w: f32, page_h: f32) -> Option<[f32; 4]> {
    if lines.is_empty() || page_w <= 0.0 || page_h <= 0.0 {
        return None;
    }
    let left = lines.iter().map(|l| l.x).fold(f32::INFINITY, f32::min);
    let top = lines.iter().map(|l| l.y - l.size).fold(f32::INFINITY, f32::min);
    let right = page_w - lines.iter().map(|l| l.right).fold(f32::NEG_INFINITY, f32::max);
    let bottom = page_h - lines.iter().map(|l| l.y).fold(f32::NEG_INFINITY, f32::max);
    Some([
        round1(left.max(0.0)),
        round1(top.max(0.0)),
        round1(right.max(0.0)),
        round1(bottom.max(0.0)),
    ])
}

fn round1(value: f32) -> f32 {
    (value * 10.0).round() / 10.0
}

/* ==========================================================================
   content stream 解释器
   ========================================================================== */

#[derive(Debug, Default)]
struct Stats {
    /// 发出的字形数
    total: usize,
    /// 成功映射到 Unicode 的字形数
    mapped: usize,
}

impl Stats {
    fn ratio(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            self.mapped as f32 / self.total as f32
        }
    }
}

/// PDF 的 3×2 变换矩阵：`x' = a·x + c·y + e`，`y' = b·x + d·y + f`。
#[derive(Debug, Clone, Copy)]
struct Matrix {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl Matrix {
    const ID: Matrix = Matrix { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    fn new(values: [f64; 6]) -> Self {
        Matrix { a: values[0], b: values[1], c: values[2], d: values[3], e: values[4], f: values[5] }
    }

    fn translate(tx: f64, ty: f64) -> Self {
        Matrix { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: tx, f: ty }
    }

    /// `self` 先作用、`outer` 后作用
    fn then(&self, outer: &Matrix) -> Matrix {
        Matrix {
            a: self.a * outer.a + self.b * outer.c,
            b: self.a * outer.b + self.b * outer.d,
            c: self.c * outer.a + self.d * outer.c,
            d: self.c * outer.b + self.d * outer.d,
            e: self.e * outer.a + self.f * outer.c + outer.e,
            f: self.e * outer.b + self.f * outer.d + outer.f,
        }
    }

    /// 纵向缩放（字号要乘它）：`(0,1)` 变换后的长度
    fn y_scale(&self) -> f64 {
        (self.c * self.c + self.d * self.d).sqrt()
    }
}

/// 一个字体的解码表。`code_bytes == 2` 是 Identity-H 那种 CID 字体。
#[derive(Debug, Clone)]
struct Font {
    map: HashMap<u32, String>,
    code_bytes: usize,
    bold: bool,
    italic: bool,
}

impl Font {
    fn is_cid(&self) -> bool {
        self.code_bytes == 2
    }

    /// 字节串 → (文字, 字形数, 映射成功数)
    fn decode(&self, bytes: &[u8]) -> (String, usize, usize) {
        let mut out = String::new();
        let mut glyphs = 0usize;
        let mut mapped = 0usize;
        if self.is_cid() {
            for pair in bytes.chunks(2) {
                let code = ((pair[0] as u32) << 8) | *pair.get(1).unwrap_or(&0) as u32;
                glyphs += 1;
                if let Some(text) = self.map.get(&code) {
                    out.push_str(text);
                    mapped += 1;
                }
                // 缺映射就不出字：宁可少一个字，也不要吐一个方块
            }
        } else {
            for byte in bytes {
                glyphs += 1;
                if let Some(text) = self.map.get(&(*byte as u32)) {
                    out.push_str(text);
                    mapped += 1;
                } else if (0x20..0x7f).contains(byte) {
                    // Type1/TrueType 的 WinAnsi 与 ASCII 在 0x20..0x7e 完全重合
                    out.push(*byte as char);
                    mapped += 1;
                }
            }
        }
        (out, glyphs, mapped)
    }
}

/// 一页要用到的东西：字体表 + Form XObject 内容流
#[derive(Debug, Default)]
struct PageCtx {
    fonts: HashMap<Vec<u8>, Font>,
    forms: HashMap<Vec<u8>, Vec<u8>>,
}

impl PageCtx {
    fn load(doc: &Document, page_id: ObjectId) -> Self {
        let mut ctx = PageCtx::default();
        let Ok((own, chain)) = doc.get_page_resources(page_id) else {
            return ctx;
        };
        // 页自己没有 /Resources 时字体挂在父节点上（生成的 PDF 里极常见）
        let mut all: Vec<&Dictionary> = own.into_iter().collect();
        for id in chain {
            if let Ok(dict) = doc.get_dictionary(id) {
                all.push(dict);
            }
        }
        for resources in all {
            if let Ok(fonts) = resources.get(b"Font") {
                if let Some(fonts) = dict_of(doc, fonts) {
                    for (name, value) in fonts.iter() {
                        if let Some(dict) = dict_of(doc, value) {
                            ctx.fonts.insert(name.clone(), build_font(doc, dict));
                        }
                    }
                }
            }
            if let Ok(xobjects) = resources.get(b"XObject") {
                if let Some(xobjects) = dict_of(doc, xobjects) {
                    for (name, value) in xobjects.iter() {
                        let Ok(stream) = resolve(doc, value).as_stream() else { continue };
                        let is_form = stream
                            .dict
                            .get(b"Subtype")
                            .ok()
                            .and_then(as_name)
                            .is_some_and(|subtype| subtype == b"Form");
                        if !is_form {
                            continue;
                        }
                        if let Ok(data) = stream.decompressed_content() {
                            ctx.forms.insert(name.clone(), data);
                        }
                    }
                }
            }
        }
        ctx
    }
}

/// 图形状态（`q`/`Q` 保存恢复的就是它 + 文字状态）
#[derive(Clone)]
struct GState {
    ctm: Matrix,
    font: Option<Font>,
    size: f64,
    leading: f64,
    h_scale: f64,
    char_space: f64,
    word_space: f64,
    render: i64,
}

impl Default for GState {
    fn default() -> Self {
        GState {
            ctm: Matrix::ID,
            font: None,
            size: 0.0,
            leading: 0.0,
            h_scale: 1.0,
            char_space: 0.0,
            word_space: 0.0,
            render: 0,
        }
    }
}

/// 解释一段 content stream，把可见文字按位置塞进 `out`。
fn interpret(
    content: &[u8],
    page: &PageCtx,
    page_no: u32,
    page_h: f32,
    out: &mut Vec<Run>,
    stats: &mut Stats,
    depth: u8,
) {
    let Ok(parsed) = Content::decode(content) else { return };
    let mut stack: Vec<GState> = Vec::new();
    let mut gs = GState::default();
    let mut tm = Matrix::ID;
    let mut tlm = Matrix::ID;

    for op in &parsed.operations {
        match op.operator.as_str() {
            "q" => stack.push(gs.clone()),
            "Q" => {
                if let Some(prev) = stack.pop() {
                    gs = prev;
                }
            }
            "cm" => {
                if let Some(m) = matrix_of(&op.operands) {
                    gs.ctm = m.then(&gs.ctm);
                }
            }
            "BT" => {
                tm = Matrix::ID;
                tlm = Matrix::ID;
            }
            "Tf" => {
                if let Some(name) = op.operands.first().and_then(as_name) {
                    gs.font = page.fonts.get(name).cloned();
                }
                if let Some(size) = op.operands.get(1).and_then(as_f64) {
                    gs.size = size.abs();
                }
            }
            "TL" => gs.leading = op.operands.first().and_then(as_f64).unwrap_or(0.0),
            "Tc" => gs.char_space = op.operands.first().and_then(as_f64).unwrap_or(0.0),
            "Tw" => gs.word_space = op.operands.first().and_then(as_f64).unwrap_or(0.0),
            "Tz" => gs.h_scale = op.operands.first().and_then(as_f64).unwrap_or(100.0) / 100.0,
            "Tr" => gs.render = op.operands.first().and_then(as_f64).unwrap_or(0.0) as i64,
            "Tm" => {
                if let Some(m) = matrix_of(&op.operands) {
                    tm = m;
                    tlm = m;
                }
            }
            "Td" => {
                let tx = op.operands.first().and_then(as_f64).unwrap_or(0.0);
                let ty = op.operands.get(1).and_then(as_f64).unwrap_or(0.0);
                tlm = Matrix::translate(tx, ty).then(&tlm);
                tm = tlm;
            }
            "TD" => {
                let tx = op.operands.first().and_then(as_f64).unwrap_or(0.0);
                let ty = op.operands.get(1).and_then(as_f64).unwrap_or(0.0);
                gs.leading = -ty;
                tlm = Matrix::translate(tx, ty).then(&tlm);
                tm = tlm;
            }
            "T*" => {
                tlm = Matrix::translate(0.0, -gs.leading).then(&tlm);
                tm = tlm;
            }
            "Tj" => {
                if let Some(bytes) = op.operands.first().and_then(as_bytes) {
                    show(&gs, &mut tm, bytes, page_no, page_h, out, stats);
                }
            }
            "TJ" => {
                if let Some(Object::Array(items)) = op.operands.first() {
                    for item in items {
                        match item {
                            Object::String(bytes, _) => {
                                show(&gs, &mut tm, bytes, page_no, page_h, out, stats)
                            }
                            other => {
                                if let Some(n) = as_f64(other) {
                                    // TJ 里的数字是「往前挪 -n/1000 em」
                                    let shift = -n / 1000.0 * gs.size * gs.h_scale;
                                    tm = Matrix::translate(shift, 0.0).then(&tm);
                                }
                            }
                        }
                    }
                }
            }
            "'" => {
                tlm = Matrix::translate(0.0, -gs.leading).then(&tlm);
                tm = tlm;
                if let Some(bytes) = op.operands.first().and_then(as_bytes) {
                    show(&gs, &mut tm, bytes, page_no, page_h, out, stats);
                }
            }
            "\"" => {
                if let Some(value) = op.operands.first().and_then(as_f64) {
                    gs.word_space = value;
                }
                if let Some(value) = op.operands.get(1).and_then(as_f64) {
                    gs.char_space = value;
                }
                tlm = Matrix::translate(0.0, -gs.leading).then(&tlm);
                tm = tlm;
                if let Some(bytes) = op.operands.get(2).and_then(as_bytes) {
                    show(&gs, &mut tm, bytes, page_no, page_h, out, stats);
                }
            }
            "Do" => {
                if depth < MAX_FORM_DEPTH {
                    if let Some(name) = op.operands.first().and_then(as_name) {
                        if let Some(data) = page.forms.get(name) {
                            let data = data.clone();
                            interpret(&data, page, page_no, page_h, out, stats, depth + 1);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// 出一段字：定位、判粗体、把光标往前推。
fn show(
    gs: &GState,
    tm: &mut Matrix,
    bytes: &[u8],
    page_no: u32,
    page_h: f32,
    out: &mut Vec<Run>,
    stats: &mut Stats,
) {
    let Some(font) = gs.font.as_ref() else { return };
    let (text, glyphs, mapped) = font.decode(bytes);
    stats.total += glyphs;
    stats.mapped += mapped;

    let trm = tm.then(&gs.ctm);
    // 字号要乘上 Tm/cm 的缩放（WPS 导出的 Tm 是 `0.05 0 0 -0.05 x y`）
    let size = (gs.size * trm.y_scale()) as f32;
    // 累进宽度用实际解出来的字算，缺映射的字按 1em 记（总比停在原地好）
    let advance: f32 = text
        .chars()
        .map(|ch| em_width(ch) * size + gs.char_space as f32)
        .sum();
    if size > 0.0 && !text.is_empty() {
        // PDF 的 y 轴朝上，这里翻成「从页顶往下」，读代码时不用每次心算
        let y = page_h - trm.f as f32;
        // 「2 Tr」= 填充+描边，是导出器伪造粗体的惯用手法
        let bold = font.bold || gs.render == 2 || gs.render == 6;
        out.push(Run {
            page: page_no,
            x: trm.e as f32,
            y,
            size,
            w: text_width(&text, size),
            bold,
            italic: font.italic,
            text,
        });
    }

    // 累进：即便这段没出字，光标也要往前走，否则同一 Tm 下的后续文字会叠字
    if size > 0.0 && advance > 0.0 {
        *tm = Matrix::translate(advance as f64, 0.0).then(tm);
    }
}

/* ==========================================================================
   lopdf 取值小工具
   ========================================================================== */

fn as_f64(object: &Object) -> Option<f64> {
    match object {
        Object::Integer(value) => Some(*value as f64),
        Object::Real(value) => Some(*value as f64),
        _ => None,
    }
}

fn as_name(object: &Object) -> Option<&[u8]> {
    match object {
        Object::Name(name) => Some(name),
        _ => None,
    }
}

fn as_bytes(object: &Object) -> Option<&[u8]> {
    match object {
        Object::String(bytes, _) => Some(bytes),
        _ => None,
    }
}

fn matrix_of(operands: &[Object]) -> Option<Matrix> {
    if operands.len() < 6 {
        return None;
    }
    let mut values = [0.0f64; 6];
    for (index, slot) in values.iter_mut().enumerate() {
        *slot = as_f64(&operands[index])?;
    }
    Some(Matrix::new(values))
}

fn dict_of<'a>(doc: &'a Document, object: &'a Object) -> Option<&'a Dictionary> {
    match object {
        Object::Dictionary(dict) => Some(dict),
        Object::Reference(id) => doc.get_object(*id).ok().and_then(|obj| obj.as_dict().ok()),
        _ => None,
    }
}

fn resolve<'a>(doc: &'a Document, object: &'a Object) -> &'a Object {
    match object {
        Object::Reference(id) => doc.get_object(*id).unwrap_or(object),
        _ => object,
    }
}

/// 页面尺寸（pt）。`MediaBox` 读不出就退回 A4 —— 对齐判断需要一个页宽。
fn media_size(doc: &Document, page_id: ObjectId) -> (f32, f32) {
    let mut node = doc.get_object(page_id).ok().and_then(|obj| obj.as_dict().ok());
    let mut hops = 0;
    while let Some(dict) = node {
        if let Ok(boxed) = dict.get(b"MediaBox") {
            if let Object::Array(items) = resolve(doc, boxed) {
                let nums: Vec<f64> = items.iter().filter_map(as_f64).collect();
                if nums.len() == 4 {
                    let w = (nums[2] - nums[0]).abs();
                    let h = (nums[3] - nums[1]).abs();
                    if w > 1.0 && h > 1.0 {
                        return (w as f32, h as f32);
                    }
                }
            }
        }
        hops += 1;
        if hops > 32 {
            break;
        }
        node = dict.get(b"Parent").ok().and_then(|parent| dict_of(doc, parent));
    }
    (595.28, 841.89)
}

fn build_font(doc: &Document, dict: &Dictionary) -> Font {
    let subtype = dict
        .get(b"Subtype")
        .ok()
        .and_then(as_name)
        .map(|name| String::from_utf8_lossy(name).to_string())
        .unwrap_or_default();
    let base = dict
        .get(b"BaseFont")
        .ok()
        .and_then(as_name)
        .map(|name| String::from_utf8_lossy(name).to_ascii_lowercase())
        .unwrap_or_default();

    let parsed = dict.get(b"ToUnicode").ok().and_then(|object| {
        let stream = resolve(doc, object).as_stream().ok()?;
        let data = stream.decompressed_content().ok()?;
        Some(parse_cmap(&data))
    });
    let (map, code_bytes) = parsed.unwrap_or_default();
    // Identity-H 的字体一定是 2 字节码；有 CMap 就听 CMap 的码宽
    let cid = subtype.contains("Type0") || code_bytes == 2;

    Font {
        map,
        code_bytes: if cid { 2 } else { 1 },
        bold: base.contains("bold") || base.contains("heavy") || base.contains("black"),
        italic: base.contains("italic") || base.contains("oblique"),
    }
}

/* ==========================================================================
   ToUnicode CMap
   ========================================================================== */

/// 解析 `/ToUnicode` CMap：`bfchar` 与 `bfrange`（含数组形式）。
fn parse_cmap(data: &[u8]) -> (HashMap<u32, String>, usize) {
    let mut map: HashMap<u32, String> = HashMap::new();
    let mut code_bytes = 2usize;
    let text = String::from_utf8_lossy(data);

    // 码宽由 codespacerange 决定：`<0000> <FFFF>` 是 2 字节
    if let Some(block) = section(&text, "begincodespacerange", "endcodespacerange") {
        if let Some(first) = block.iter().find_map(|line| hex_token(line, 0)) {
            // hex_token 已经按半字节凑成字节了，长度就是码宽
            code_bytes = first.len().clamp(1, 4);
        }
    }

    if let Some(block) = section(&text, "beginbfchar", "endbfchar") {
        for line in block {
            let tokens = hex_tokens(line);
            if tokens.len() >= 2 {
                map.insert(decode_code(&tokens[0]), utf16(&tokens[1]));
            }
        }
    }

    if let Some(block) = section(&text, "beginbfrange", "endbfrange") {
        for line in block {
            let tokens = hex_tokens(line);
            if tokens.len() < 2 {
                continue;
            }
            let low = decode_code(&tokens[0]);
            let high = decode_code(&tokens[1]);
            if high < low || high - low > 0xFFFF {
                continue;
            }
            if let Some(dst) = tokens.get(2) {
                // `<lo> <hi> <dst>`：目的串的最后一码位递增
                let base = utf16(dst);
                for offset in 0..=(high - low) {
                    map.insert(low + offset, bump_last(&base, offset));
                }
            } else if let Some(array) = bracket_array(line) {
                for (offset, dst) in array.iter().enumerate() {
                    if low + offset as u32 > high {
                        break;
                    }
                    map.insert(low + offset as u32, utf16(dst));
                }
            }
        }
    }

    (map, code_bytes)
}

/// 取出 `begin…end` 之间的行
fn section<'a>(text: &'a str, begin: &str, end: &str) -> Option<Vec<&'a str>> {
    let start = text.find(begin)? + begin.len();
    let stop = text[start..].find(end)? + start;
    Some(text[start..stop].lines().collect())
}

/// 一行里第 `index` 个 `<...>` 的字节
fn hex_token(line: &str, index: usize) -> Option<Vec<u8>> {
    hex_tokens(line).into_iter().nth(index)
}

fn hex_tokens(line: &str) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut current: Option<Vec<u8>> = None;
    for ch in line.chars() {
        match ch {
            '<' => current = Some(Vec::new()),
            '>' => {
                if let Some(nibbles) = current.take() {
                    out.push(pack(&nibbles));
                }
            }
            other => {
                if let Some(nibbles) = current.as_mut() {
                    if let Some(digit) = other.to_digit(16) {
                        nibbles.push(digit as u8);
                    }
                }
            }
        }
    }
    out
}

/// 半字节凑成字节（奇数位补 0）
fn pack(nibbles: &[u8]) -> Vec<u8> {
    nibbles
        .chunks(2)
        .map(|pair| (pair[0] << 4) | *pair.get(1).unwrap_or(&0))
        .collect()
}

/// `<0001> <0005> [<0041> <0042>]` 里的方括号数组
fn bracket_array(line: &str) -> Option<Vec<Vec<u8>>> {
    let start = line.find('[')?;
    let stop = line[start..].find(']')? + start;
    let tokens = hex_tokens(&line[start..=stop]);
    if tokens.is_empty() {
        None
    } else {
        Some(tokens)
    }
}

fn decode_code(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0u32, |acc, byte| (acc << 8) | *byte as u32)
}

/// UTF-16BE（含代理对）→ String
fn utf16(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks(2)
        .map(|pair| ((pair[0] as u16) << 8) | *pair.get(1).unwrap_or(&0) as u16)
        .collect();
    String::from_utf16_lossy(&units)
}

/// bfrange 的递增：最后一码位 + offset
fn bump_last(text: &str, offset: u32) -> String {
    let mut chars: Vec<char> = text.chars().collect();
    if let Some(last) = chars.last_mut() {
        *last = char::from_u32(*last as u32 + offset).unwrap_or(*last);
    }
    chars.into_iter().collect()
}

/* ==========================================================================
   单测：几何归纳全用合成数据（仓库里不放用户样本）
   ========================================================================== */

#[cfg(test)]
mod tests {
    use super::*;

    /// 一行字：按 CJK 1em / ASCII 0.5em 估宽，和实现里同一套算法
    fn run(page: u32, x: f32, y: f32, size: f32, text: &str) -> Run {
        Run {
            page,
            x,
            y,
            size,
            w: text_width(text, size),
            bold: false,
            italic: false,
            text: text.to_string(),
        }
    }

    /// A4 页宽
    const W: f32 = 595.28;

    /* ---------- 1. 聚行 ---------- */

    #[test]
    fn glyphs_on_the_same_baseline_merge_into_one_line() {
        // WPS 导出会把每个字单独定位，y 完全相同
        let runs = vec![
            run(1, 258.0, 40.0, 20.0, "谭"),
            run(1, 278.0, 40.0, 20.0, "轶"),
            run(1, 298.0, 40.0, 20.0, "天"),
        ];
        let lines = group_lines(&runs);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "谭轶天", "汉字之间不该补空格");
        assert!((lines[0].right - 318.0).abs() < 0.05);
    }

    #[test]
    fn ascii_runs_with_a_gap_get_a_space_between_them() {
        let runs = vec![
            run(1, 40.0, 100.0, 10.0, "Rust"),
            run(1, 70.0, 100.0, 10.0, "TypeScript"),
        ];
        assert_eq!(group_lines(&runs)[0].text, "Rust TypeScript");
    }

    #[test]
    fn baselines_within_tolerance_are_the_same_line() {
        let one = vec![run(1, 40.0, 100.0, 10.0, "甲"), run(1, 50.0, 101.4, 10.0, "乙")];
        assert_eq!(group_lines(&one).len(), 1, "1.4pt 的基线抖动仍是同一行");
        let two = vec![run(1, 40.0, 100.0, 10.0, "甲"), run(1, 50.0, 108.0, 10.0, "乙")];
        assert_eq!(group_lines(&two).len(), 2, "8pt 就该是两行了");
    }

    #[test]
    fn lines_are_ordered_top_down_left_right_not_by_input_order() {
        let runs = vec![
            run(1, 200.0, 200.0, 10.0, "第二行"),
            run(1, 60.0, 100.0, 10.0, "第二字"),
            run(1, 40.0, 100.0, 10.0, "第一行首字"),
        ];
        let lines = group_lines(&runs);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "第一行首字第二字", "行内按 x 排");
    }

    /* ---------- 2. 对齐 ---------- */

    #[test]
    fn a_line_centred_on_the_page_is_centre_aligned() {
        let line = &group_lines(&[run(1, 258.0, 40.0, 20.0, "谭轶天")])[0];
        assert_eq!(align_of(line, W), Align::Center);
    }

    #[test]
    fn a_line_starting_at_the_left_margin_is_left_aligned() {
        let line = &group_lines(&[run(1, 56.0, 200.0, 10.0, "工作经历：某公司后端开发")])[0];
        assert_eq!(align_of(line, W), Align::Left);
    }

    #[test]
    fn a_line_ending_on_the_right_margin_is_right_aligned() {
        let text = "2021.03 - 2023.06";
        let width = text_width(text, 10.0);
        let line = &group_lines(&[run(1, W - 56.0 - width, 200.0, 10.0, text)])[0];
        assert!(line.right > W * 0.9);
        assert_eq!(align_of(line, W), Align::Right);
    }

    #[test]
    fn a_wide_paragraph_is_not_mistaken_for_right_aligned() {
        // 顶格、撑满整行的正文：右边缘也贴右边，但左边缘在左边距
        let text = "负责订单系统的下单、支付、退款三条链路的重构，把平均响应时间从 800ms 降到 120ms，\
并把可用性从 99.5% 提到 99.95%，同时补齐了灰度与回滚方案，线上零事故运行一年。";
        let line = &group_lines(&[run(1, 56.0, 200.0, 10.0, text)])[0];
        assert!(line.right > W * 0.9, "样本要真的撑到右边：{}", line.right);
        assert_eq!(align_of(line, W), Align::Left);
    }

    /* ---------- 3. 归段 ---------- */

    #[test]
    fn lines_one_and_a_half_lines_apart_belong_to_the_same_paragraph() {
        let runs = vec![
            run(1, 56.0, 300.0, 10.0, "负责订单系统重构，"),
            run(1, 56.0, 315.0, 10.0, "把响应时间降到 120ms。"),
        ];
        let lines = group_lines(&runs);
        assert_eq!(group_paragraphs(&lines, W).len(), 1, "1.5 倍行距是同一段");
    }

    #[test]
    fn a_big_gap_starts_a_new_paragraph() {
        let runs = vec![
            run(1, 56.0, 300.0, 10.0, "负责订单系统重构。"),
            run(1, 56.0, 330.0, 10.0, "负责支付网关接入。"),
        ];
        let lines = group_lines(&runs);
        assert_eq!(group_paragraphs(&lines, W).len(), 2, "30pt（3 倍字号）是另一段");
    }

    #[test]
    fn a_size_change_splits_the_paragraph() {
        let runs = vec![
            run(1, 56.0, 300.0, 10.0, "教育背景"),
            run(1, 56.0, 313.0, 14.0, "某大学 计算机科学与技术"),
        ];
        let lines = group_lines(&runs);
        assert_eq!(group_paragraphs(&lines, W).len(), 2);
    }

    #[test]
    fn a_list_marker_always_starts_its_own_paragraph() {
        let runs = vec![
            run(1, 56.0, 300.0, 10.0, "技术栈："),
            run(1, 56.0, 314.0, 10.0, "· Rust / TypeScript"),
        ];
        let lines = group_lines(&runs);
        assert_eq!(group_paragraphs(&lines, W).len(), 2);
    }

    #[test]
    fn a_new_page_never_merges_with_the_previous_one() {
        // 跨页：上一页 y=780 是页脚，下一页 y=40 是页眉，坐标会重置
        let runs = vec![
            run(1, 56.0, 780.0, 10.0, "第 1 页页脚"),
            run(2, 56.0, 40.0, 10.0, "第 2 页页眉"),
        ];
        let lines = group_lines(&runs);
        assert_eq!(lines.len(), 2);
        assert_eq!(group_paragraphs(&lines, W).len(), 2);
    }

    /* ---------- 4. 分级 ---------- */

    #[test]
    fn the_big_centred_line_at_the_top_is_the_title() {
        let runs = vec![
            run(1, 258.0, 60.0, 22.0, "谭轶天"),
            run(1, 56.0, 100.0, 10.0, "性别：男 年龄：20 电话：138****0000"),
            run(1, 56.0, 130.0, 10.0, "熟悉 Rust、TypeScript 与 React，做过三年后端。"),
            run(1, 56.0, 145.0, 10.0, "熟悉 MySQL、Redis 与消息队列，能独立排查线上问题。"),
        ];
        let lines = group_lines(&runs);
        let groups = group_paragraphs(&lines, W);
        let body = body_size(&groups, &lines);
        assert_eq!(body, 10.0, "正文基准字号按字数加权取众数");

        let blocks = build_blocks(&groups, &lines, W, body);
        assert_eq!(blocks[0].kind, BlockKind::Title);
        assert_eq!(blocks[0].style.size_pt, Some(22.0));
        assert_eq!(blocks[0].style.align, Align::Center);
        assert_eq!(blocks[0].style.level, None);
        assert_eq!(blocks[1].kind, BlockKind::Paragraph);
    }

    #[test]
    fn two_section_sizes_become_level_one_and_level_two_headings() {
        let runs = vec![
            run(1, 56.0, 60.0, 20.0, "谭轶天"),
            run(1, 56.0, 100.0, 14.0, "工作经历"),
            run(1, 56.0, 120.0, 10.0, "某公司 后端开发 2021.03 - 2023.06"),
            run(1, 56.0, 140.0, 12.0, "教育背景"),
            run(1, 56.0, 158.0, 10.0, "某大学 计算机科学与技术 2017 - 2021"),
        ];
        let lines = group_lines(&runs);
        let groups = group_paragraphs(&lines, W);
        let body = body_size(&groups, &lines);
        let blocks = build_blocks(&groups, &lines, W, body);

        assert_eq!(blocks[0].kind, BlockKind::Title);
        assert_eq!(blocks[1].kind, BlockKind::Heading);
        assert_eq!(blocks[1].style.level, Some(1), "14pt 是最大的一档标题");
        assert_eq!(blocks[3].kind, BlockKind::Heading);
        assert_eq!(blocks[3].style.level, Some(2), "12pt 低一档");
    }

    #[test]
    fn only_one_block_is_ever_the_document_title() {
        let runs = vec![
            run(1, 56.0, 60.0, 20.0, "谭轶天"),
            run(1, 56.0, 100.0, 20.0, "个人简介"),
            run(1, 56.0, 140.0, 10.0, "正文一行。"),
        ];
        let lines = group_lines(&runs);
        let groups = group_paragraphs(&lines, W);
        let blocks = build_blocks(&groups, &lines, W, 10.0);
        assert_eq!(blocks[0].kind, BlockKind::Title);
        assert_eq!(blocks[1].kind, BlockKind::Heading, "第二个大字是标题，不是文档标题");
        assert_eq!(blocks[1].style.level, Some(1));
    }

    #[test]
    fn markers_are_recognised_as_list_items() {
        for text in ["· Rust", "• TypeScript", "- React", "1. 负责下单链路", "2、支付"] {
            assert!(starts_item(text), "{text} 应该被判成列表项");
        }
        for text in ["谭轶天", "熟悉 Rust 与 TypeScript", "2019 - 2023", "-"] {
            assert!(!starts_item(text), "{text} 不该被判成列表项");
        }
    }

    #[test]
    fn markers_turn_into_list_item_blocks() {
        let runs = vec![
            run(1, 56.0, 60.0, 10.0, "技术栈"),
            run(1, 56.0, 80.0, 10.0, "· Rust / TypeScript"),
            run(1, 56.0, 94.0, 10.0, "· MySQL / Redis"),
        ];
        let lines = group_lines(&runs);
        let groups = group_paragraphs(&lines, W);
        let blocks = build_blocks(&groups, &lines, W, 10.0);
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].kind, BlockKind::Paragraph);
        assert_eq!(blocks[1].kind, BlockKind::ListItem);
        assert_eq!(blocks[2].kind, BlockKind::ListItem);
    }

    #[test]
    fn a_deeper_first_line_gets_an_indent() {
        let runs = vec![
            run(1, 56.0, 60.0, 10.0, "技术栈"),
            run(1, 86.0, 80.0, 10.0, "· Rust / TypeScript"),
        ];
        let lines = group_lines(&runs);
        let groups = group_paragraphs(&lines, W);
        let blocks = build_blocks(&groups, &lines, W, 10.0);
        assert_eq!(blocks[0].style.indent, 0);
        assert_eq!(blocks[1].style.indent, 2, "30pt ≈ 2 个字身");
    }

    /* ---------- 5. 表格骨架 ---------- */

    #[test]
    fn aligned_columns_of_text_become_a_table() {
        let row = |y: f32, cells: [&str; 3]| {
            let xs = [56.0f32, 260.0, 420.0];
            cells
                .iter()
                .enumerate()
                .map(|(index, text)| run(1, xs[index], y, 10.0, text))
                .collect::<Vec<_>>()
        };
        let mut runs = row(300.0, ["时间", "公司", "职位"]);
        runs.extend(row(320.0, ["2021-2023", "某科技", "后端开发"]));
        runs.extend(row(340.0, ["2019-2021", "某网络", "实习生"]));
        let lines = group_lines(&runs);
        // 表格行距比正文大，归段会一行一段 —— 表格判定必须直接看行
        assert_eq!(group_paragraphs(&lines, W).len(), 3, "每行各自成段");
        assert_eq!(table_at(&lines, 0), Some(3));
        let table = as_table(&lines[0..3]).expect("三列三行应该认成表格");
        assert_eq!(table.len(), 3);
        assert_eq!(table[0], vec!["时间", "公司", "职位"]);
        assert_eq!(table[2], vec!["2019-2021", "某网络", "实习生"]);

        let groups = group_paragraphs(&lines, W);
        let blocks = build_blocks(&groups, &lines, W, 10.0);
        assert_eq!(blocks.len(), 1, "整块表格只出一个块");
        assert_eq!(blocks[0].kind, BlockKind::Table);
    }

    #[test]
    fn columns_that_do_not_line_up_are_not_a_table() {
        let runs = vec![
            run(1, 56.0, 300.0, 10.0, "时间"),
            run(1, 260.0, 300.0, 10.0, "公司"),
            run(1, 420.0, 300.0, 10.0, "职位"),
            run(1, 40.0, 320.0, 10.0, "2021"),
            run(1, 300.0, 320.0, 10.0, "某科技"),
            run(1, 500.0, 320.0, 10.0, "后端"),
        ];
        let lines = group_lines(&runs);
        assert_eq!(table_at(&lines, 0), None, "列没对齐就不是表格");
    }

    #[test]
    fn a_lonely_multi_column_line_is_not_a_table() {
        // 简历里的「性别：男  年龄：20  电话：…」就是一行三列，不能当表
        let runs = vec![
            run(1, 56.0, 300.0, 10.0, "性别：男"),
            run(1, 200.0, 300.0, 10.0, "年龄：20"),
            run(1, 340.0, 300.0, 10.0, "电话：138****0000"),
        ];
        let lines = group_lines(&runs);
        assert_eq!(table_at(&lines, 0), None);
    }

    /* ---------- 6. 版心 ---------- */

    #[test]
    fn margins_come_from_the_extremes_of_the_text() {
        let lines = group_lines(&[
            run(1, 56.0, 60.0, 20.0, "谭轶天"),
            run(1, 72.0, 200.0, 10.0, "正文"),
        ]);
        let margin = margins(&lines, W, 841.89).unwrap();
        assert!((margin[0] - 56.0).abs() < 0.05, "左留白 = 最左");
        assert!((margin[1] - 40.0).abs() < 0.05, "上留白 = 最高那行的顶");
        // 最右的是**标题那行**：谭轶天 3 字 × 20pt = 60，起点 56 → 右边缘 116；
        // 正文「正文」只有 2 × 10 = 20，右边缘 92。原来写成 W-92 是漏算了标题宽度。
        assert!((margin[2] - (W - 116.0)).abs() < 0.05, "右留白 = 页宽 - 最右（56 + 3 个 20pt 字）");
        assert!((margin[3] - (841.89 - 200.0)).abs() < 0.05, "下留白");
    }

    /* ---------- 7. CMap 解析 ---------- */

    #[test]
    fn cmap_bfchar_and_bfrange_are_both_understood() {
        let cmap = b"/CIDInit /ProcSet findresource begin\n\
1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n\
2 beginbfchar\n<0003> <0020>\n<186A> <8C2D>\nendbfchar\n\
1 beginbfrange\n<0004> <0006> <0061>\nendbfrange\n";
        let (map, bytes) = parse_cmap(cmap);
        assert_eq!(bytes, 2, "Identity-H 是两字节");
        assert_eq!(map.get(&0x186A).map(String::as_str), Some("谭"));
        assert_eq!(map.get(&0x0003).map(String::as_str), Some(" "));
        assert_eq!(map.get(&0x0004).map(String::as_str), Some("a"));
        assert_eq!(map.get(&0x0006).map(String::as_str), Some("c"));
    }

    #[test]
    fn a_broken_cmap_never_panics() {
        for junk in [
            &b""[..],
            b"beginbfchar\n<zz> <",
            b"beginbfrange\n<0005> <0001> <0041>\nendbfrange",
        ] {
            let (_, bytes) = parse_cmap(junk);
            assert!((1..=4).contains(&bytes), "码宽 {bytes} 不在 1..=4");
        }
    }

    #[test]
    fn a_cid_font_without_mappings_yields_nothing_rather_than_boxes() {
        let font = Font { map: HashMap::new(), code_bytes: 2, bold: false, italic: false };
        let (text, glyphs, mapped) = font.decode(&[0x18, 0x6A]);
        assert!(text.is_empty(), "缺 ToUnicode 时宁可不显字，也不吐方块");
        assert_eq!((glyphs, mapped), (1, 0));
    }

    #[test]
    fn a_simple_font_lets_ascii_through_untouched() {
        let font = Font { map: HashMap::new(), code_bytes: 1, bold: false, italic: false };
        let (text, glyphs, mapped) = font.decode(b"Hello");
        assert_eq!(text, "Hello");
        assert_eq!((glyphs, mapped), (5, 5));
    }

    /* ---------- 8. 端到端（自己造 PDF，不碰用户样本） ---------- */

    use lopdf::{dictionary, Stream};

    /// 一页 PDF：Helvetica-Bold（单字节、无 ToUnicode，走 ASCII 直通）+ 给定 content
    fn sample_pdf(content: &str, width: f32, height: f32) -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica-Bold",
        });
        let resources = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => Object::Reference(font_id) },
        });
        let contents =
            doc.add_object(Stream::new(Dictionary::new(), content.as_bytes().to_vec()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => Object::Reference(pages_id),
            "MediaBox" => Object::Array(vec![
                Object::Real(0.0),
                Object::Real(0.0),
                Object::Real(width),
                Object::Real(height),
            ]),
            "Resources" => Object::Reference(resources),
            "Contents" => Object::Reference(contents),
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => Object::Array(vec![Object::Reference(page)]),
                "Count" => Object::Integer(1),
            }),
        );
        let catalog = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => Object::Reference(pages_id),
        });
        doc.trailer.set("Root", Object::Reference(catalog));
        let mut out = Vec::new();
        doc.save_to(&mut out).expect("测试用 PDF 要能写出");
        out
    }

    #[test]
    fn end_to_end_reads_size_position_and_alignment_out_of_a_content_stream() {
        // 页宽 600：标题居中（24pt），两行正文（10pt）顶格
        let content = "\
BT /F1 24 Tf 1 0 0 1 225 700 Tm (Tan Yitian) Tj ET\n\
BT /F1 10 Tf 1 0 0 1 50 600 Tm (Rust / TypeScript / React) Tj ET\n\
BT /F1 10 Tf 1 0 0 1 50 586 Tm (MySQL / Redis / Kafka) Tj ET\n";
        let bytes = sample_pdf(content, 600.0, 800.0);
        let outline = parse(&bytes).expect("这份 PDF 有完整文字层");
        assert_eq!(outline.pages, Some(1));
        let body = outline.body_pt.expect("要能给出正文基准字号");
        assert!((body - 10.0).abs() < 0.05, "body_pt = {body}");

        let title = &outline.blocks[0];
        assert_eq!(title.kind, BlockKind::Title);
        assert_eq!(title.style.size_pt, Some(24.0));
        assert_eq!(title.style.align, Align::Center);
        assert!(title.style.bold, "Helvetica-Bold 要认出粗体");
        assert_eq!(title.text, "Tan Yitian");

        assert_eq!(outline.blocks.len(), 2, "两行 14pt 行距的正文归成一段");
        assert_eq!(outline.blocks[1].kind, BlockKind::Paragraph);
        assert_eq!(outline.blocks[1].style.align, Align::Left);
        assert!(outline.blocks[1].text.contains("Kafka"));

        let margin = outline.margin_pt.expect("要能给出页边距");
        assert!((margin[0] - 50.0).abs() < 0.1, "左留白 = {margin:?}");
    }

    #[test]
    fn a_scaled_text_matrix_reports_the_effective_font_size() {
        // WPS 那种 `0.05 0 0 -0.05 x y`：字号 480 × 0.05 = 24
        let content = "BT /F1 480 Tf 0.05 0 0 -0.05 225 100 Tm (Tan Yitian) Tj ET\n";
        let bytes = sample_pdf(content, 600.0, 800.0);
        let outline = parse(&bytes).expect("有文字");
        assert_eq!(outline.blocks[0].style.size_pt, Some(24.0));
    }

    #[test]
    fn a_page_without_a_text_layer_is_not_an_empty_outline() {
        // 空 content：读不出结构就该是 None，让调用方说「读不到」
        let bytes = sample_pdf("", 600.0, 800.0);
        assert!(parse(&bytes).is_none());
    }

    #[test]
    fn garbage_in_place_of_a_pdf_is_none_not_a_panic() {
        assert!(parse(b"not a pdf at all").is_none());
        assert!(parse(b"%PDF-1.4\n").is_none());
    }

    #[test]
    fn tj_arrays_advance_the_pen_without_losing_the_text() {
        let content = "BT /F1 10 Tf 1 0 0 1 50 600 Tm [(Rust) -120 (TypeScript)] TJ ET\n";
        let bytes = sample_pdf(content, 600.0, 800.0);
        let outline = parse(&bytes).expect("有文字");
        let text = &outline.blocks[0].text;
        assert!(text.contains("Rust") && text.contains("TypeScript"), "{text:?}");
    }
}
