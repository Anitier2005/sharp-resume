//! 排版工具（会话 agent 直接调用，没有第二个 agent 可转交）。
//!
//! 三个入口，按力度从轻到重：
//!   `read_layout_guide` —— 组件手册：有哪些 class、能写哪些标签，按需取、不占常驻上下文。
//!   `set_page_layout`   —— 改这一页的成品版面（预设 / 块级手写 / 整页接管）。
//!   `note_on_page`      —— 把一句话贴在纸面上给用户看（不进导出、不进会话流）。
//!
//! 这里的工具只改版面，**不改内容**：任何入口都过文字锁（见下），改字请回到内容工具。
//!
//! 为什么拿掉旧的 `set_page_style`：那是「参数层」——只能从疏密三档、边距倍数、
//! 对齐两项里挑，本质是替 agent 做了排版决策。现在给它的是成品层：
//! 预设（一次性切换整体疏密）、块级手写（改一段）、整页接管（换一种版面语言）。

use serde_json::{json, Value};
use unicode_normalization::UnicodeNormalization;

use crate::render;
use crate::tools::{push_action, ToolCtx};

fn pages_of<'a>(ctx: &'a ToolCtx) -> Result<&'a Vec<Value>, String> {
    let people = ctx
        .workspace
        .get("people")
        .and_then(Value::as_array)
        .ok_or_else(|| "工作区快照里没有 people".to_string())?;
    let person = people
        .iter()
        .find(|item| item.get("id").and_then(Value::as_str) == Some(ctx.person_id))
        .ok_or_else(|| format!("找不到候选人 {}", ctx.person_id))?;
    let resumes = person
        .get("resumes")
        .and_then(Value::as_array)
        .ok_or_else(|| "候选人下没有简历".to_string())?;
    let resume = resumes
        .iter()
        .find(|item| item.get("id").and_then(Value::as_str) == Some(ctx.resume_id))
        .ok_or_else(|| format!("找不到简历 {}", ctx.resume_id))?;
    resume
        .get("pages")
        .and_then(Value::as_array)
        .ok_or_else(|| "简历下没有页面".to_string())
}

fn page_id_of(args: &Value, ctx: &ToolCtx) -> String {
    args.get("pageId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(ctx.page_id)
        .to_string()
}

/// 单块手写与整页手写的长度上限。
///
/// 不是防模型乱写，是防「一页塞进一本书」：整页 30000 字符已经相当于排版很密的
/// 两页 A4，再多就不是简历了。超了直接拒绝，让模型自己删减 ——
/// 悄悄截断会产出一份半截的版面，比报错更难查。
const MAX_PART_CHARS: usize = 6_000;
const MAX_HTML_CHARS: usize = 30_000;

/// 页头版式的合法值，直接问生成器要 —— 校验与生成必须是同一份清单，
/// 否则会出现「工具认了、`render_head` 不认」的空操作。
const HEAD_STYLES: [&str; 5] = render::HEAD_STYLES;

/* ---------- 文字锁 ---------- */

/// 排版模型能改版面，不能改内容。
///
/// 这条锁是**比对纯文本**而不是比对 HTML：class、标签、包裹方式随便换，
/// 只要印在纸上的字一个不多一个不少就放行。它挡的是真实发生过的事 ——
/// 排版模型为了「压进一页」把要点里的半句话删掉／改写，用户看到的是
/// 一份自己没写过的简历。
///
/// 归一规则（两边用同一套，否则会误伤正当的排版）：
///   1. 去掉零宽字符、软连字符（复制粘贴带进来的，纸上根本看不见）
///   2. NFKC：全角字母、康熙部首、带圈数字这些「长得一样、码位不同」的字符归一
///   3. 标点邻接的空白一律抹掉（见 `squeeze_punctuation`）
///   4. 压空白：换行、多空格、制表符都变一个空格
fn normalize_text(raw: &str) -> String {
    let visible: String = raw.chars().filter(|ch| !is_invisible(*ch)).collect();
    collapse(&squeeze_punctuation(&visible.nfkc().collect::<String>()))
}

/// 会被「空格式排版噪声」贴上来的一类字符。
///
/// 中文排版里「标题：值」「（备注）」两侧的空格是随手敲出来的，Word 与网页
/// 又各有各的习惯；文字锁不该因为这种差别拒掉一次正当的重排。
const PUNCT: &str = "：:，,。.（()）【】[]、；;！!？?「」『』《》〈〉“”‘’…—";

/// 抹掉紧贴在标点两侧的空白。
///
/// `A B:C` / `A B : C` / `A B：C` 归一后必须是同一个东西 —— 模型换个写法排版，
/// 不该被判成改了字。但字母之间的空格一律保留：`Java Script` 与 `JavaScript`
/// 是两个词，抹平就等于放过一次真正的改字。
fn squeeze_punctuation(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if !ch.is_whitespace() {
            out.push(ch);
            continue;
        }
        // 一段空白：看它与左右两侧是否贴着标点
        let before = out.chars().next_back();
        let mut after = None;
        while let Some(&next) = chars.peek() {
            if next.is_whitespace() {
                chars.next();
            } else {
                after = Some(next);
                break;
            }
        }
        let touches_punct = before.is_some_and(|ch| PUNCT.contains(ch))
            || after.is_some_and(|ch| PUNCT.contains(ch));
        if touches_punct {
            continue;
        }
        out.push(' ');
    }
    out
}

fn is_invisible(ch: char) -> bool {
    matches!(
        ch,
        '\u{00ad}' | '\u{180e}' | '\u{200b}'..='\u{200f}' | '\u{2060}'..='\u{2064}' | '\u{feff}'
    )
}

/// 一段 HTML 印在纸上的样子：去标签、解实体、归一。
///
/// 这是文字的**定义**，不只是内部工具：单测与以后的差异高亮都从这里取。
#[allow(dead_code)]
pub(crate) fn text_of_html(html: &str) -> String {
    normalize_text(&strip_html(html))
}

/// 去标签 + 解实体 + 压空白，但**不做 NFKC**。
///
/// 比对走归一后的文本；报错里引原文要走这个 —— 归一会把全角冒号压成半角，
/// 「原文：职位:实习生」这种字面量模型认不出来是自己写的哪一句。
fn strip_html(html: &str) -> String {
    let mut raw = String::with_capacity(html.len());
    let mut chars = html.chars().peekable();
    let mut in_tag = false;
    while let Some(ch) = chars.next() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                // 标签两侧本来是两个词：`<h2>工作</h2><li>要点</li>` 直接拆会粘成
                // 「工作要点」。补空格再压空白，两边才对得上
                raw.push(' ');
            }
            _ if in_tag => {}
            '&' => raw.push_str(&take_entity(&mut chars)),
            _ => raw.push(ch),
        }
    }
    collapse(&raw)
}

/// 换行、多空格、制表符都压成一个空格
fn collapse(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 读一个 `&…;` 实体。认不出来就原样吐回 `&name` —— 宁可多留一个 & 也不吞字。
fn take_entity(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut name = String::new();
    let mut closed = false;
    while let Some(&ch) = chars.peek() {
        if ch == ';' {
            chars.next();
            closed = true;
            break;
        }
        if name.len() >= 8 || !(ch.is_ascii_alphanumeric() || ch == '#') {
            break;
        }
        name.push(ch);
        chars.next();
    }
    if closed {
        if let Some(text) = decode_entity(&name) {
            return text;
        }
    }
    format!("&{name}")
}

fn decode_entity(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    let named = match lower.as_str() {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        "nbsp" => " ",
        "middot" => "·",
        "hellip" => "…",
        "mdash" => "—",
        "ndash" => "–",
        "ldquo" => "“",
        "rdquo" => "”",
        "lsquo" => "‘",
        "rsquo" => "’",
        "bull" => "•",
        "times" => "×",
        _ => "",
    };
    if !named.is_empty() {
        return Some(named.to_string());
    }
    let code = if let Some(hex) = lower.strip_prefix("#x") {
        u32::from_str_radix(hex, 16).ok()?
    } else if let Some(decimal) = lower.strip_prefix('#') {
        decimal.parse::<u32>().ok()?
    } else {
        return None;
    };
    char::from_u32(code).map(|ch| ch.to_string())
}

/// 一个字段印在纸上的样子。
///
/// `lead` = 首字段：页头与条目的第一个字段只印值、不印标签（「张三」而不是
/// 「姓名：张三」）。其余按「标签：值」印，与前端 `fieldText` 同一规则。
/// 图片字段返回空串：它的文字是图注，页头那张证件照更是一个字都不印。
fn field_text(field: &Value, lead: bool) -> String {
    if field
        .get("assetId")
        .and_then(Value::as_str)
        .is_some_and(|id| !id.trim().is_empty())
    {
        return String::new();
    }
    let value = field.get("value").and_then(Value::as_str).unwrap_or("").trim();
    let label = field.get("label").and_then(Value::as_str).unwrap_or("").trim();
    if lead {
        return if value.is_empty() {
            label.to_string()
        } else {
            value.to_string()
        };
    }
    match (label.is_empty(), value.is_empty()) {
        (true, _) => value.to_string(),
        (false, true) => format!("{label}："),
        (false, false) => format!("{label}：{value}"),
    }
}

fn strings_of(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// 块的结构化纯文本（归一后）。
///
/// 生成器对同一份内容有几种排法（页头把首字段当大标题、字段组全部带标签、
/// tags/kv/table 每条只取第一个字段），纯文本因此不止一种合法形态。
/// 这里给出**一种**（页头/分区那种最常见的），比对时用 `text_variants` 取全集。
pub(crate) fn text_of_block(block: &Value) -> String {
    text_variants(block).remove(0)
}

/// 整页的纯文本：各块按顺序拼，图片块跳过（它没有文字）。
///
/// 这是「整页接管」那条路的判据。整页是一个整体，逐块比没意义 ——
/// 模型本来就要把内容重新分栏、挪位置，只要一个字没多没少就该放行。
fn page_text(blocks: &[&Value]) -> String {
    let parts: Vec<String> = blocks
        .iter()
        .filter(|block| block.get("kind").and_then(Value::as_str) != Some("media"))
        .map(|block| text_of_block(block))
        .filter(|text| !text.is_empty())
        .collect();
    collapse(&parts.join(" "))
}

/// 这一块的纯文本可能长成的所有样子（归一后）。命中任意一种就算没改字。
pub(crate) fn text_variants(block: &Value) -> Vec<String> {
    raw_variants(block)
        .iter()
        .map(|text| normalize_text(text))
        .collect()
}

/// 同上，但不做 NFKC —— 报错里引原文要用它，见 `strip_html` 的注释。
fn raw_variants(block: &Value) -> Vec<String> {
    let title = block.get("title").and_then(Value::as_str).unwrap_or("").trim();
    let fields = block.get("fields").and_then(Value::as_array);
    let items = block.get("items").and_then(Value::as_array);

    // 三种形态：页头（首字段只取值，条目全展开）/ 字段组（全部带标签）/ 每条只取首字段
    // （tags、kv、table 三档每条只印第一个字段）
    let shapes: Vec<(bool, bool)> = vec![(true, true), (false, true), (true, false)];

    let mut out = Vec::new();
    for (lead_fields, all_entry_fields) in shapes {
        let mut parts: Vec<String> = Vec::new();
        if !title.is_empty() {
            parts.push(title.to_string());
        }
        if let Some(fields) = fields {
            for (index, field) in fields.iter().enumerate() {
                let lead = lead_fields && index == 0;
                let text = field_text(field, lead);
                if !text.is_empty() {
                    parts.push(text);
                }
            }
        }
        if let Some(items) = items {
            for item in items {
                if let Some(fields) = item.get("fields").and_then(Value::as_array) {
                    for (index, field) in fields.iter().enumerate() {
                        // 形态三：每条只有首字段进纸面（tags / kv / table）
                        if !all_entry_fields && index > 0 {
                            continue;
                        }
                        let text = field_text(field, index == 0);
                        if !text.is_empty() {
                            parts.push(text);
                        }
                    }
                }
                parts.extend(strings_of(item.get("bullets")));
            }
        }
        parts.extend(strings_of(block.get("caption")));
        out.push(collapse(&parts.join(" ")));
    }
    out.dedup();
    out
}

/// 报错里引原文时截前 40 字：够认出是哪一段，又不至于把一整段回显给模型。
fn preview(text: &str) -> String {
    let head: String = text.chars().take(40).collect();
    format!("{head}…")
}

/// 块的「标题」：报错时要说清是哪一块。分区有 title，字段组只能拿第一个值。
fn block_label(block: &Value) -> String {
    let title = block.get("title").and_then(Value::as_str).unwrap_or("").trim();
    if !title.is_empty() {
        return title.to_string();
    }
    if let Some(value) = block
        .get("fields")
        .and_then(Value::as_array)
        .and_then(|fields| fields.first())
    {
        let text = field_text(value, true);
        if !text.is_empty() {
            return text;
        }
    }
    block.get("id").and_then(Value::as_str).unwrap_or("").to_string()
}


/* ---------- 组件手册 ---------- */

pub fn guide_params() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    })
}

pub fn read_layout_guide(_args: &Value, _ctx: &ToolCtx) -> Result<String, String> {
    Ok(render::GUIDE.to_string())
}

/* ---------- 写成品 ---------- */

pub fn set_layout_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "pageId": { "type": "string", "description": "要排的页；不传就是当前页" },
            "preset": {
                "type": "string",
                "enum": ["classic", "compact", "dense", "roomy", "custom"],
                "description": "整体疏密。compact 把内容压进一页、dense 是原件级（字号行距都贴着原件）、roomy 更透气、classic 是默认；custom 表示字号间距都被你手改过"
            },
            "head": {
                "type": "string",
                "enum": ["", "center", "bare", "thick", "aside"],
                "description": "页头版式。aside 是原件最常见的排法：姓名与联系方式居中、照片贴右上角；center 整体居中；bare 不画分隔线；thick 粗分隔线。不传就维持现状，传空串回到生成器默认"
            },
            "parts": {
                "type": "object",
                "description": "只重写某几个块：块 id → 这一块的 HTML。块 id 从 read_page 的输出里取。值为空串表示撤销这一块的覆盖，回到自动生成",
                "additionalProperties": { "type": "string" }
            },
            "html": {
                "type": "string",
                "description": "整页接管：直接给出整页内容的 HTML（不含外层 article）。作品集、或结构化表达不了的版面才用。传空串撤销。设了它 parts 与 preset 都不再生效"
            },
            "reset": {
                "type": "boolean",
                "description": "清掉这一页所有手写覆盖与预设，回到纯结构化生成"
            }
        },
        "additionalProperties": false
    })
}

/// 同一轮里可能连着改几次版面。
///
/// 第二次必须以「上一次的产出」为基线：`workspace` 快照在这一轮内是不变的，
/// 拿它当基线就会把前一次刚写下的覆盖冲掉 —— 模型看到工具返回「已更新」，
/// 用户那边却少了一半改动，这种不一致极难查。
fn pending_layout(ctx: &ToolCtx, page_id: &str) -> Option<serde_json::Map<String, Value>> {
    let actions = ctx.out.actions.lock().ok()?;
    actions.iter().rev().find_map(|action| {
        if action.get("type").and_then(Value::as_str) != Some("patchPageLayout") {
            return None;
        }
        if action.get("pageId").and_then(Value::as_str) != Some(page_id) {
            return None;
        }
        action.get("layout").and_then(Value::as_object).cloned()
    })
}

pub fn set_page_layout(args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let pages = pages_of(ctx)?;
    let page_id = page_id_of(args, ctx);

    let page = pages
        .iter()
        .find(|item| item.get("id").and_then(Value::as_str) == Some(page_id.as_str()))
        .ok_or_else(|| format!("没有 id 为 {page_id} 的页面"))?;

    // 这一页的块：parts 要按 id 找块，整页接管要按顺序拼出原文
    let blocks: Vec<&Value> = page
        .get("blocks")
        .and_then(Value::as_array)
        .map(|list| list.iter().collect())
        .unwrap_or_default();

    // 在现有成品上合并，不是整体替换：只改一块时不该把别的块和预设清掉。
    // 基线优先取同一轮里已经产出的那份，其次才是快照里的
    let mut layout = pending_layout(ctx, &page_id)
        .or_else(|| page.get("layout").and_then(Value::as_object).cloned())
        .unwrap_or_default();
    let mut notes: Vec<String> = Vec::new();

    if args.get("reset").and_then(Value::as_bool) == Some(true) {
        layout.clear();
        notes.push("清空所有手写，回到结构化生成".to_string());
    }

    if let Some(preset) = args.get("preset").and_then(Value::as_str) {
        let preset = preset.trim();
        if !render::PRESETS.contains(&preset) {
            return Err(format!("preset 只能是 {}", render::PRESETS.join(" / ")));
        }
        layout.insert("preset".into(), json!(preset));
        notes.push(format!("预设 {preset}"));
    }

    if let Some(head) = args.get("head").and_then(Value::as_str) {
        let head = head.trim();
        if !HEAD_STYLES.contains(&head) {
            return Err(format!("head 只能是 {}", HEAD_STYLES[1..].join(" / ")));
        }
        if head.is_empty() {
            layout.remove("head");
            notes.push("页头回到生成器默认".to_string());
        } else {
            layout.insert("head".into(), json!(head));
            // aside 已经用上绝对定位（render::HEAD_ASIDE_ABSOLUTE = true）：照片贴版心右上角。
            // 模型看不见渲染结果，不说清「现在生效的是什么」，它就会按旧印象以为照片还在左边。
            if head == "aside" {
                notes.push("页头 aside：姓名居中、证件照贴右上角".to_string());
            } else {
                notes.push(format!("页头 {head}"));
            }
        }
    }

    if let Some(html) = args.get("html").and_then(Value::as_str) {
        let html = html.trim();
        if html.is_empty() {
            layout.remove("html");
            notes.push("撤销整页手写".to_string());
        } else {
            let len = html.chars().count();
            if len > MAX_HTML_CHARS {
                return Err(format!(
                    "整页 HTML 有 {len} 字，超过 {MAX_HTML_CHARS} 字上限。删掉不必要的内容再试，不要截断 —— 半截版面比报错更难查"
                ));
            }
            // 整页接管也是「不许改字」：换个入口写整页，不等于能改内容。
            // 只锁 parts 的话，模型绕一道就成了「排版模型替我改简历」。
            let original = page_text(&blocks);
            let written = normalize_text(&strip_html(html));
            if written != original {
                return Err(format!(
                    "整页接管不能改字：这一页原文共 {} 字，你写的是 {} 字。改字请用内容工具（set_field / update_entry），这里只改版面。",
                    original.chars().count(),
                    written.chars().count()
                ));
            }
            layout.insert("html".into(), json!(html));
            notes.push(format!("整页手写 {len} 字（parts 与 preset 不再生效）"));
        }
    }

    if let Some(parts) = args.get("parts").and_then(Value::as_object) {
        if parts.is_empty() {
            return Err("parts 是空的：要么给至少一个块，要么用 reset 清空".into());
        }
        let mut merged = layout
            .get("parts")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();

        for (block_id, raw) in parts {
            let index = blocks
                .iter()
                .position(|block| block.get("id").and_then(Value::as_str) == Some(block_id.as_str()))
                .ok_or_else(|| {
                    format!(
                        "块 {block_id} 不存在。这一页的块是：{}（先用 read_page 看准确 id）",
                        if blocks.is_empty() {
                            "（没有块）".to_string()
                        } else {
                            blocks
                                .iter()
                                .filter_map(|block| block.get("id").and_then(Value::as_str))
                                .collect::<Vec<_>>()
                                .join(" / ")
                        }
                    )
                })?;
            let block = blocks[index];
            let html = raw
                .as_str()
                .ok_or_else(|| format!("块 {block_id} 的 HTML 必须是字符串"))?;
            let html = html.trim();
            if html.is_empty() {
                merged.remove(block_id);
                notes.push(format!("撤销块 {block_id} 的手写"));
                continue;
            }
            let len = html.chars().count();
            if len > MAX_PART_CHARS {
                return Err(format!(
                    "块 {block_id} 的 HTML 有 {len} 字，超过 {MAX_PART_CHARS} 字上限。只写这一块真正需要的内容"
                ));
            }
            // 文字锁：只比印在纸上的字。图片块没有文字，跳过
            // （媒体块的 caption 不算 —— 它不是模型改的那部分内容）
            if block.get("kind").and_then(Value::as_str) != Some("media") {
                let written_raw = strip_html(html);
                let written = normalize_text(&written_raw);
                if !text_variants(block).iter().any(|text| text == &written) {
                    return Err(format!(
                        "第 {} 块「{}」的文字被改动了，这里只改版面、不改字。原文：{}；你写的是：{}。改字请用内容工具（set_field / update_entry）。",
                        index + 1,
                        block_label(block),
                        preview(&raw_variants(block).remove(0)),
                        preview(&written_raw)
                    ));
                }
            }
            merged.insert(block_id.clone(), json!(html));
            notes.push(format!("块 {block_id} 手写 {len} 字"));
        }

        if merged.is_empty() {
            layout.remove("parts");
        } else {
            layout.insert("parts".into(), Value::Object(merged));
        }
    }

    if notes.is_empty() {
        return Err("没有要改的项：至少给 preset / head / parts / html / reset 之一".into());
    }

    push_action(
        ctx,
        json!({
            "type": "patchPageLayout",
            "pageId": page_id,
            "layout": Value::Object(layout),
        }),
    )?;

    let mut reply = format!("这一页的版面已更新：{}", notes.join("、"));
    // 实测结论必须跟着返回：agent 不知道「这一版到底有没有进一页」就会
    // 接着说「已经压到一页了」。`ctx.facts` 为空（老调用点 / 测试）时不追加，
    // 免得在拿不到实测的情况下编一句。
    if let Some(facts) = ctx.facts {
        reply.push('\n');
        reply.push_str(&crate::agent::facts::layout_result_line(facts));
        // 这一页的实测是 unknown / stale 时，必须把「滞后一轮」说出来：
        // 模型同一轮连改两次、看到数字没变，会得出「版面无效、瓶颈在内容」，
        // 转头去劝用户删内容 —— 而它要的其实是还原原件版式。
        if matches!(
            crate::agent::facts::fit_of(facts.measure.as_ref()),
            crate::agent::facts::Fit::Unknown(_) | crate::agent::facts::Fit::Stale { .. }
        ) {
            reply.push('\n');
            reply.push_str(MEASURE_LAG_HINT);
        }
    }
    Ok(reply)
}

/// 实测滞后一轮的说明。只在量不到 / 旧数作废时追加，有实测值时一个字都不多说。
const MEASURE_LAG_HINT: &str = "页数要等这一轮结束、纸面重新排完才会更新 —— 同一轮里连改两次不会让数字变，别据此判断「版面没用」，也别凭这个去要求删内容。";

/* ---------- 留批注 ---------- */

/// 批注的长度上限。200 字大约是两行批注气泡；再长用户点开看到的是一段正文，
/// 那不如直接改版面。
const MAX_NOTE_CHARS: usize = 200;

/// 单轮批注条数上限。纸面就那么大：三条以上就从「提示」变成「涂鸦」了。
const MAX_NOTES_PER_TURN: usize = 2;

/// 占一个本轮名额，超了报错。
///
/// 必须报错而不是静默丢弃：静默丢会让模型以为话已经说出去了，下一轮不会再提，
/// 用户永远看不到那条批注 —— 而模型在总结里还会说「我已经在纸面上标注了」。
fn take_slot(
    counter: &std::sync::atomic::AtomicUsize,
    limit: usize,
    what: &str,
) -> Result<(), String> {
    if counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) >= limit {
        return Err(format!(
            "这一轮已经留下 {limit} 条{what}了。先把版排完，剩下的一轮再说 —— 别再试，重试也不会成功"
        ));
    }
    Ok(())
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs() as i64)
        .unwrap_or(0)
}

/// 当前页 + 这一页的块 id 清单。批注贴错块 = 用户点了没反应，所以先校验。
fn page_blocks<'a>(ctx: &'a ToolCtx) -> Result<(String, Vec<String>), String> {
    let pages = pages_of(ctx)?;
    let page = pages
        .iter()
        .find(|item| item.get("id").and_then(Value::as_str) == Some(ctx.page_id))
        .ok_or_else(|| format!("找不到当前页 {}", ctx.page_id))?;
    let blocks = page
        .get("blocks")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|block| block.get("id").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    Ok((ctx.page_id.to_string(), blocks))
}

fn block_arg(args: &Value, blocks: &[String]) -> Result<Option<String>, String> {
    let block_id = args
        .get("blockId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(block_id) = block_id else {
        return Ok(None);
    };
    if !blocks.iter().any(|known| known == block_id) {
        return Err(format!(
            "这一页没有 id 为 {block_id} 的块。可用的块：{}（先用 read_page 看准确 id）",
            if blocks.is_empty() {
                "（这一页还没有块）".to_string()
            } else {
                blocks.join(" / ")
            }
        ));
    }
    Ok(Some(block_id.to_string()))
}

pub fn note_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "text": {
                "type": "string",
                "description": "写给用户看的一句话，1–200 字。说清「哪里、为什么」，别写「这里有问题」这种没法接的话"
            },
            "blockId": {
                "type": "string",
                "description": "贴在哪个块上（块 id 从 read_page 取）。不传就贴在页边，表示整页的批注"
            }
        },
        "required": ["text"],
        "additionalProperties": false
    })
}

/// 在纸面上留一条给用户看的批注。
///
/// `id` 与 `at` 在 Rust 生成而不是交给前端：前端要做去重与「已读」记忆，
/// 让它自己造 id 就没法区分「同一条批注重渲染」和「又贴了一条」。
pub fn note_on_page(args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let text = args
        .get("text")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if text.is_empty() {
        return Err("text 不能为空：批注是给人看的一句话，不是占位符".into());
    }
    let len = text.chars().count();
    if len > MAX_NOTE_CHARS {
        return Err(format!(
            "批注有 {len} 字，超过 {MAX_NOTE_CHARS} 字上限。批注是一句话，不是一段说明 —— 说清要紧的那一点就好"
        ));
    }

    let (page_id, blocks) = page_blocks(ctx)?;
    let block_id = block_arg(args, &blocks)?;

    // 参数都验过了才占名额：校验失败不该消耗一次机会
    take_slot(&ctx.out.notes, MAX_NOTES_PER_TURN, "批注")?;

    let mut note = json!({
        "id": format!("n-{}", crate::session::uuid_like()),
        "text": text,
        "at": now_secs(),
    });
    if let Some(block_id) = &block_id {
        note["blockId"] = json!(block_id);
    }

    push_action(
        ctx,
        json!({
            "type": "pageNote",
            "personId": ctx.person_id,
            "resumeId": ctx.resume_id,
            "pageId": page_id,
            "note": note,
        }),
    )?;

    Ok(format!(
        "批注已贴在{}，用户会直接在纸面上看到。\n\
         它是给用户看的，不是给自己记笔记 —— 批注不改任何内容，该改的字用内容工具改。这一轮还能再留 {} 条，说完继续，不用停下来等。",
        if block_id.is_some() { "这一块旁边" } else { "页边" },
        MAX_NOTES_PER_TURN.saturating_sub(
            ctx.out.notes.load(std::sync::atomic::Ordering::SeqCst)
        )
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::facts::{PageFacts, PageMeasure, Target, TargetSource};
    use crate::tools::Outbox;

    fn workspace() -> Value {
        json!({ "people": [{ "id": "p1", "resumes": [{
            "id": "r1",
            "pages": [{ "id": "pg1", "label": "第一页", "blocks": [
                { "id": "b1", "kind": "fields", "fields": [
                    { "id": "f1", "label": "姓名", "value": "谭轶天" },
                    { "id": "f2", "label": "联系", "value": "男 · 20 岁 · 159****1234" },
                    { "id": "f3", "label": "照片", "value": "", "assetId": "a1" }
                ]},
                { "id": "b2", "kind": "section", "title": "实习与项目经历", "items": [{
                    "id": "e1",
                    "fields": [
                        { "id": "e1a", "label": "公司", "value": "铁未来" },
                        { "id": "e1b", "label": "职位", "value": "实习生" }
                    ],
                    "bullets": ["通过定性方法定位入门难点并改进赛制，同时调整模型参数增强经济模拟的真实性"]
                }]},
                { "id": "b3", "kind": "media", "assetId": "a1", "caption": "作品" }
            ] }]
        }]}] })
    }

    /// 文字锁：改一个字（定性 → 定量）必须被拒，且文案指到具体块
    #[test]
    fn text_lock_rejects_a_single_changed_word() {
        let err = run(
            None,
            json!({ "parts": { "b2": "<section class=\"sec\"><h2>实习与项目经历</h2><div class=\"entry\"><b>铁未来</b><div class=\"meta\">职位：实习生</div><ul><li>通过定量方法定位入门难点并改进赛制，同时调整模型参数增强经济模拟的真实性</li></ul></div></section>" } }),
        )
        .unwrap_err();
        assert!(err.starts_with("第 2 块「实习与项目经历」的文字被改动了"), "{err}");
        assert!(err.contains("这里只改版面、不改字"), "{err}");
        assert!(err.contains("原文：实习与项目经历 铁未来 职位：实习生 通过定性方法"), "{err}");
        assert!(err.contains("你写的是：实习与项目经历 铁未来 职位：实习生 通过定量方法"), "{err}");
        assert!(err.contains("改字请用内容工具"), "{err}");
    }

    /// 文字锁：只换 class / 结构 / 标签，一个字不改 —— 必须放行
    #[test]
    fn text_lock_lets_a_pure_relayout_through() {
        let text = run(
            None,
            json!({ "parts": { "b2": "<section class=\"sec tight\"><h2 class=\"t\">实习与项目经历</h2><div class=\"entry row\"><b>铁未来</b><span class=\"meta\">职位：实习生</span><ul class=\"bullets\"><li>通过定性方法定位入门难点并改进赛制，同时调整模型参数增强经济模拟的真实性</li></ul></div></section>" } }),
        )
        .unwrap();
        assert!(text.contains("这一页的版面已更新"), "{text}");
        assert!(text.contains("块 b2 手写"), "{text}");
    }

    /// 图片块没有文字，不该被文字锁拦住
    #[test]
    fn text_lock_skips_media_blocks() {
        let text = run(
            None,
            json!({ "parts": { "b3": "<figure class=\"fig wide\"><img src=\"asset:a1\" alt=\"\"><figcaption>作品</figcaption></figure>" } }),
        )
        .unwrap();
        assert!(text.contains("块 b3 手写"), "{text}");
    }

    /// 不存在的块要报「块 x 不存在」，不要静默放行
    #[test]
    fn unknown_block_is_an_error() {
        let err = run(None, json!({ "parts": { "b9": "<p>随便写</p>" } })).unwrap_err();
        assert!(err.contains("块 b9 不存在"), "{err}");
    }

    /// 整页接管：文字一个字没变，只是重新分栏 —— 必须放行（这才是「完整排版权」）
    #[test]
    fn whole_page_html_with_the_same_text_passes() {
        let text = run(
            None,
            json!({ "html": "<header class=\"head\"><div class=\"txt\"><h1 class=\"name\">谭轶天</h1><div class=\"meta\">联系：男 · 20 岁 · 159****1234</div></div></header><section class=\"sec\"><h2>实习与项目经历</h2><div class=\"entry\"><b>铁未来</b><div class=\"meta\">职位：实习生</div><ul><li>通过定性方法定位入门难点并改进赛制，同时调整模型参数增强经济模拟的真实性</li></ul></div></section>" }),
        )
        .unwrap();
        assert!(text.contains("整页手写"), "{text}");
    }

    /// 整页接管也不能多写一句话 —— 换个入口不等于能改内容
    #[test]
    fn whole_page_html_cannot_add_a_sentence() {
        let err = run(
            None,
            json!({ "html": "<header class=\"head\"><div class=\"txt\"><h1 class=\"name\">谭轶天</h1><div class=\"meta\">联系：男 · 20 岁 · 159****1234</div></div></header><section class=\"sec\"><h2>实习与项目经历</h2><div class=\"entry\"><b>铁未来</b><div class=\"meta\">职位：实习生</div><ul><li>通过定性方法定位入门难点并改进赛制，同时调整模型参数增强经济模拟的真实性</li></ul></div></section><p>另外我还写过一篇论文</p>" }),
        )
        .unwrap_err();
        assert!(err.starts_with("整页接管不能改字：这一页原文共"), "{err}");
        assert!(err.contains("你写的是"), "{err}");
        assert!(err.contains("改字请用内容工具"), "{err}");
        assert!(!err.contains("第 1 块"), "整页这条路不该报块号：{err}");
    }

    /// 只有「标点邻接」的空白算排版噪声；字母之间的空格是两个词，不能抹
    #[test]
    fn only_whitespace_next_to_punctuation_is_squeezed() {
        // 三种写法（紧贴 / 两侧都带空格 / 全角冒号）互等
        for html in [
            "<p>A B:C</p>",
            "<p>A B : C</p>",
            "<p>A B：C</p>",
        ] {
            assert_eq!(text_of_html(html), "A B:C", "{html}");
        }
        // 字母之间的空格保留：抹掉就等于放过一次真正的改字
        assert_ne!(text_of_html("<p>Java Script</p>"), text_of_html("<p>JavaScript</p>"));
        assert_eq!(text_of_html("<p>Java Script</p>"), "Java Script");
    }

    #[test]
    fn text_of_html_strips_tags() {
        assert_eq!(
            text_of_html("<section><h2>标题</h2><ul><li>要点</li></ul></section>"),
            "标题 要点"
        );
    }

    #[test]
    fn text_of_html_collapses_whitespace() {
        assert_eq!(text_of_html("  <p>工作\n\n   经历\t·  实习 </p> "), "工作 经历 · 实习");
    }

    /// 全角字母、零宽字符都是「看着一样、码位不同」，不归一会误伤正当排版
    #[test]
    fn text_of_html_normalizes_fullwidth_and_zero_width() {
        assert_eq!(text_of_html("<b>ＡＢＣ１２３</b>"), "ABC123");
        assert_eq!(text_of_html("工\u{200b}作"), "工作");
    }

    /// 归一必须**两侧对称**：同一套归一作用在「模型写的 HTML」和「块的结构化文本」上，
    /// 各做一次。否则全角冒号、全角括号、不间断空格这些纸上完全看不出差别的东西，
    /// 会把一次正当的排版判成改字 —— 然后模型被无意义地拒绝，还不知道为什么。
    ///
    /// 三组最常撞的：`：`/`:`、`（`/`(`、`\u{00A0}`/空格。
    #[test]
    fn normalization_is_symmetric_on_both_sides() {
        let block = json!({
            "id": "b1", "kind": "section", "title": "实习（项目）",
            "items": [{ "id": "e1", "bullets": ["A\u{00a0}B：C"] }]
        });
        let block_text = text_of_block(&block);
        // 标点旁的空白也被抹平：`B：C` / `B : C` / `B:C` 是同一个东西，
        // `）` 后面那个空格同理 —— 两侧同一套规则，所以这条锁不会误伤
        assert_eq!(block_text, "实习(项目)A B:C");

        for html in [
            // 原样照抄
            "<h2>实习（项目）</h2><li>A\u{00a0}B：C</li>",
            // 全角换半角 + 不间断空格换普通空格
            "<h2>实习(项目)</h2><li>A B: C</li>",
            // 写成实体
            "<h2>实习（项目）</h2><li>A&nbsp;B&#xFF1A;C</li>",
        ] {
            assert_eq!(text_of_html(html), block_text, "{html}");
        }
    }

    /// 块文本按「首字段取值、其余标签：值、要点照抄」拼。
    /// 注意 NFKC 会把全角冒号压成半角 —— 比对两边都归一，所以锁不受影响；
    /// 报错里引的原文走的是不归一的那份（见 `strip_html`）。
    #[test]
    fn text_of_block_follows_the_generator_shape() {
        let ws = workspace();
        let page = &ws["people"][0]["resumes"][0]["pages"][0];
        let head = &page["blocks"][0];
        assert_eq!(text_of_block(head), "谭轶天 联系:男 · 20 岁 · 159****1234");
        let section = &page["blocks"][1];
        assert!(text_of_block(section).starts_with("实习与项目经历 铁未来 职位:实习生 通过定性方法"));
    }

    /// 页头 aside 现在真的把证件照放到版心右上角，返回里不许再提「降级」
    #[test]
    fn head_aside_reports_the_absolute_photo() {
        let text = run(None, json!({ "head": "aside" })).unwrap();
        assert!(text.contains("证件照贴右上角"), "{text}");
        assert!(!text.contains("已降级"), "{text}");
        assert!(!text.contains("无法绝对定位"), "{text}");
    }

    /// 量不到时把「实测要等一轮」说清楚：堵住「版面没用 → 去删内容」的误判
    #[test]
    fn unknown_measure_says_the_reading_lags_one_round() {
        let unknown = PageFacts::default();
        let text = run(Some(&unknown), json!({ "preset": "compact" })).unwrap();
        assert!(text.contains("尚未实测"), "{text}");
        assert!(text.contains("等这一轮结束"), "{text}");
        assert!(text.contains("不会让数字变"), "{text}");
        assert!(text.contains("别据此判断"), "{text}");
    }

    /// 有实测值时不多嘴：「等一轮」那句只属于量不到 / 旧数作废
    #[test]
    fn measured_reply_does_not_mention_the_lag() {
        let text = run(Some(&facts(1000.0)), json!({ "preset": "compact" })).unwrap();
        assert!(!text.contains("等这一轮结束"), "{text}");
    }

    #[test]
    fn dense_is_a_legal_preset() {
        let text = run(None, json!({ "preset": "dense" })).unwrap();
        assert!(text.contains("预设 dense"), "{text}");
    }

    /// 目标 1 页；传 1000.0 = 实测 1.00 页，1180.0 = 1.18 页溢出
    fn facts(content_px: f64) -> PageFacts {
        PageFacts {
            target: Target {
                pages: Some(1),
                source: TargetSource::UserBefore,
            },
            measure: Some(PageMeasure {
                page_id: "pg1".into(),
                status: "ok".into(),
                version: 12,
                content: "v37".into(),
                one_page: Some(1000.0),
                content_px: Some(content_px),
                line_height: Some(17.0),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn run(facts: Option<&PageFacts>, args: Value) -> Result<String, String> {
        let ws = workspace();
        let out = Outbox::default();
        let ctx = ToolCtx {
            workspace: &ws,
            person_id: "p1",
            resume_id: "r1",
            page_id: "pg1",
            out: &out,
            root: None,
            facts,
        };
        set_page_layout(&args, &ctx)
    }

    #[test]
    fn reply_appends_the_measured_verdict() {
        let facts = facts(1000.0);
        let text = run(Some(&facts), json!({ "preset": "compact" })).unwrap();
        assert!(text.contains("这一页的版面已更新"), "{text}");
        // 结论行必须来自 facts 那一处（这里断言「同一份」，不重抄句子）
        assert!(
            text.contains(&crate::agent::facts::layout_result_line(&facts)),
            "{text}"
        );
        assert!(text.contains("（目标 1 页，达标）"), "{text}");
    }

    #[test]
    fn overflow_reply_says_target_is_still_missed() {
        let text = run(Some(&facts(1180.0)), json!({ "preset": "compact" })).unwrap();
        assert!(text.contains("仍未达标"), "{text}");
        assert!(text.contains("超出 0.18 页"), "{text}");
    }

    /// 没有 facts（老调用点 / 测试）→ 返回与今天逐字相同，绝不编一句实测
    #[test]
    fn without_facts_the_reply_does_not_mention_measurement() {
        let text = run(None, json!({ "preset": "compact" })).unwrap();
        assert_eq!(text, "这一页的版面已更新：预设 compact");
        assert!(!text.contains("实测"), "{text}");
    }

    /// 只有改成功了才追加结论：报错路径不该顺带说一句「实测多少页」
    #[test]
    fn failed_edit_returns_only_the_error() {
        let facts = facts(1000.0);
        let err = run(Some(&facts), json!({ "preset": "不存在的预设" })).unwrap_err();
        assert!(err.contains("preset"), "{err}");
        assert!(!err.contains("实测"), "{err}");
    }
}
