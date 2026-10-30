//! 文档的「带格式解析」：把 PDF / docx / pptx / xlsx 归一成同一种结构。
//!
//! 为什么要有这一层：原来 PDF 只走 `pdf_extract::extract_text_from_mem`，出来是一串
//! 纯字符 —— 姓名那行的「居中」是 PDF 里的**坐标**，字距、字号、段落对齐、表格结构
//! 全在提取时丢掉了。模型拿到的是一篇文章，而不是一份排版结果，于是「忠实还原原件」
//! 这件事从一开始就没有依据，只能靠看图猜。
//!
//! 这里定义的 `DocOutline` 是**两个消费者共用的一份事实**：
//!   - 会话 agent 的材料段（它要知道原件写了什么、哪句是标题、有没有表格）；
//!   - 排版 agent 的 `describe_page`（它要知道原件长什么样：字号层级、对齐、页边距）。
//!
//! 一格式一实现，互不影响：
//!   - `pdf::parse`：lopdf 遍历 content stream，取 `Tf` 字号与 `Tm`/`Td` 位置；
//!   - `office::parse`：docx/pptx/xlsx 都是 zip + XML，共用一套解压 + quick-xml。

pub mod office;
pub mod pdf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// 归一结果：一份文档的「带格式大纲」。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DocOutline {
    /// 文档级元信息：页数、纸张、页边距……只有能读出来的格式才有
    #[serde(default)]
    pub pages: Option<u32>,
    /// 版心（pt）：左、上、右、下。读不出就不给 —— 宁可缺，不要编
    #[serde(default)]
    pub margin_pt: Option<[f32; 4]>,
    /// 正文基准字号（pt）：取出现次数最多的字号。所有相对大小都以它为 1.0
    #[serde(default)]
    pub body_pt: Option<f32>,
    #[serde(default)]
    pub blocks: Vec<DocBlock>,
}

/// 一个结构块。不追求还原 PDF 的每个 run，只保留「排版能用到的那几维」。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DocBlock {
    pub kind: BlockKind,
    /// 纯文本。表格见 `table`，这里的 text 是给搜索/摘要用的扁平版
    pub text: String,
    #[serde(default)]
    pub style: BlockStyle,
    /// 只有 `kind == Table` 时有值：行 × 列
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<Vec<Vec<String>>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum BlockKind {
    /// 文档标题 / 姓名那种最大的字
    Title,
    /// 章节标题，`level` 从 1 开始（1 = 一级）
    Heading,
    Paragraph,
    /// 列表项（原件里的「· xxx」）
    ListItem,
    Table,
    /// 图片占位：位置能读到就记，读不到就只有这一条
    Image,
    /// 解析器想交代的一句话（「这一页没有文字层」等）
    Note,
}

/// 排版相关的样式维度。**只放能被下游用上的**，不追求完整还原。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct BlockStyle {
    /// 字号（pt）。未知就不给
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_pt: Option<f32>,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub align: Align,
    /// 缩进层级（0 = 顶格）。列表项与多级标题用
    #[serde(default)]
    pub indent: u8,
    /// 标题层级（1..=6），只有 `kind == Heading` 有意义
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<u8>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

impl Align {
    fn of(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "center" | "centre" | "ctr" => Align::Center,
            "right" | "end" | "r" => Align::Right,
            "justify" | "both" => Align::Justify,
            _ => Align::Left,
        }
    }
}

/// 统一入口：按扩展名分派。未知格式返回 `None`（调用方回落到「读不到」的说法，
/// 不要伪造一个空大纲 —— 空大纲会让人以为「这份文件是空的」）。
pub fn parse(ext: &str, bytes: &[u8]) -> Option<DocOutline> {
    let ext = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    let outline = match ext.as_str() {
        "pdf" => pdf::parse(bytes),
        "docx" | "pptx" | "xlsx" => office::parse(&ext, bytes),
        _ => None,
    }?;
    Some(rank(outline))
}

/// 收口：给标题分级别、给正文定基准字号。
///
/// 放在公共层而不是各解析器里：四个格式各自聚类字号，规则一定会漂移；
/// 而「谁是一级标题」这件事全文档只该有一套算法。
fn rank(outline: DocOutline) -> DocOutline {
    // 基准字号 = 出现最多的字号（按块数加权）。取不到就留空，别猜
    let mut counts: Vec<(f32, usize)> = Vec::new();
    for block in &outline.blocks {
        let Some(size) = block.style.size_pt else { continue };
        let key = (size * 10.0).round() / 10.0;
        match counts.iter_mut().find(|(value, _)| (*value - key).abs() < 0.05) {
            Some((_, count)) => *count += 1,
            None => counts.push((key, 1)),
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.total_cmp(&b.0)));

    let mut out = outline;
    if out.body_pt.is_none() {
        // 解析器自己给了就信它（它知道正文段落是哪些）；没给才用众数兜底
        out.body_pt = counts.first().map(|(size, _)| *size);
    }
    out
}

/* ==========================================================================
   给下游看的 JSON 形状
   ========================================================================== */

impl DocOutline {
    /// 材料段/事实块共用的紧凑描述。**不给全文**，只给层级与字号的轮廓 ——
    /// 全文走 `read_source`，这里的目标是让模型 30 秒内知道「原件长什么样」。
    pub fn outline_json(&self) -> Value {
        let blocks: Vec<Value> = self
            .blocks
            .iter()
            .take(120)
            .map(|block| {
                json!({
                    // 字号与边距都按 0.1 取整再给：f32 直出会变成 56.70000076293945，
                    // 既灌水又让「同一个值」没法用相等去断言
                    "k": match block.kind {
                        BlockKind::Title => "title",
                        BlockKind::Heading => "heading",
                        BlockKind::Paragraph => "p",
                        BlockKind::ListItem => "li",
                        BlockKind::Table => "table",
                        BlockKind::Image => "img",
                        BlockKind::Note => "note",
                    },
                    "lvl": block.style.level,
                    "pt": block.style.size_pt.map(round1),
                    "align": match block.style.align {
                        Align::Left => "left",
                        Align::Center => "center",
                        Align::Right => "right",
                        Align::Justify => "justify",
                    },
                    "bold": block.style.bold,
                    "ind": block.style.indent,
                    "text": clip(&block.text, 120),
                })
            })
            .collect();
        json!({
            "pages": self.pages,
            "marginPt": self.margin_pt.map(|side| side.map(round1)),
            "bodyPt": self.body_pt.map(round1),
            "blocks": blocks,
        })
    }
}

/// f32 → 0.1 取整的 f64。JSON 里不该出现 56.70000076293945 这种尾数。
///
/// 必须先 `as f64` 再乘除：f32 的 9.6 实际存的是 9.600000381，在 f32 里
/// 乘十、四舍五入、再除十，落到 f64 时尾数照样还在 —— 等于没取整。
fn round1(value: f32) -> f64 {
    let value = value as f64;
    (value * 10.0).round() / 10.0
}

fn clip(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let mut out: String = flat.chars().take(max).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(kind: BlockKind, text: &str, size: Option<f32>) -> DocBlock {
        DocBlock {
            kind,
            text: text.to_string(),
            style: BlockStyle { size_pt: size, ..Default::default() },
            table: None,
        }
    }

    #[test]
    fn unknown_extension_is_not_an_empty_outline() {
        // 空大纲会被读成「这份文件是空的」，那是假话
        assert!(parse("zip", b"PK\x03\x04").is_none());
        assert!(parse("", b"hello").is_none());
    }

    #[test]
    fn align_follows_the_common_spellings() {
        assert_eq!(Align::of("center"), Align::Center);
        assert_eq!(Align::of("CTR"), Align::Center);
        assert_eq!(Align::of("right"), Align::Right);
        assert_eq!(Align::of("both"), Align::Justify);
        assert_eq!(Align::of(""), Align::Left);
        assert_eq!(Align::of("left"), Align::Left);
    }

    #[test]
    fn outline_json_carries_the_dimensions_the_layout_agent_needs() {
        let outline = DocOutline {
            pages: Some(1),
            margin_pt: Some([56.7, 56.7, 56.7, 56.7]),
            body_pt: Some(9.6),
            blocks: vec![
                DocBlock {
                    kind: BlockKind::Title,
                    text: "谭轶天".into(),
                    style: BlockStyle {
                        size_pt: Some(19.2),
                        bold: true,
                        align: Align::Center,
                        ..Default::default()
                    },
                    table: None,
                },
                block(BlockKind::Paragraph, "性别：男 年龄：20", Some(9.6)),
            ],
        };
        let value = outline.outline_json();
        assert_eq!(value["pages"], 1);
        assert_eq!(value["bodyPt"], 9.6);
        assert_eq!(value["marginPt"][0], 56.7);
        assert_eq!(value["blocks"][0]["k"], "title");
        assert_eq!(value["blocks"][0]["align"], "center");
        assert_eq!(value["blocks"][0]["bold"], true);
        assert_eq!(value["blocks"][1]["k"], "p");
    }

    #[test]
    fn long_text_is_clipped_with_an_ellipsis() {
        let long = "一".repeat(500);
        let outline = DocOutline {
            blocks: vec![block(BlockKind::Paragraph, &long, None)],
            ..Default::default()
        };
        let text = outline.outline_json()["blocks"][0]["text"].as_str().unwrap().to_string();
        assert!(text.chars().count() <= 121, "{text:?}");
        assert!(text.ends_with('…'));
    }
}
