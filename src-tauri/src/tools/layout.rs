//! 排版工具。
//!
//! 这里有两条完全不同的路径，别混：
//!   `request_layout` —— **会话 agent** 用。它不排版，只把意图转给该页的排版 agent。
//!   `set_page_layout` —— **排版 agent** 用。它拥有这一页版面的全部决定权。
//!
//! 为什么拿掉旧的 `set_page_style`：那是「参数层」——只能从疏密三档、边距倍数、
//! 对齐两项里挑，本质是替 agent 做了排版决策。现在给它的是成品层：
//! 预设（一次性切换整体疏密）、块级手写（改一段）、整页接管（换一种版面语言）。
//! 组件清单见 `render::GUIDE`，由 `read_layout_guide` 按需取，不占常驻上下文。

use serde_json::{json, Value};

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

/* ---------- 会话 agent：转交 ---------- */

pub fn request_layout_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "pageId": { "type": "string", "description": "要让哪一页重新排版；不传就是当前页" },
            "brief": {
                "type": "string",
                "description": "给排版 agent 的要求，比如「内容超了一页，压到一页内」「留白太多，显得空」。写意图，不要写具体像素"
            }
        },
        "required": ["brief"],
        "additionalProperties": false
    })
}

pub fn request_layout(args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let brief = args
        .get("brief")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if brief.is_empty() {
        return Err("brief 不能为空：要说清楚想让版式变成什么样".into());
    }

    let pages = pages_of(ctx)?;
    let page_id = page_id_of(args, ctx);
    if !pages
        .iter()
        .any(|item| item.get("id").and_then(Value::as_str) == Some(page_id.as_str()))
    {
        return Err(format!("没有 id 为 {page_id} 的页面"));
    }

    push_action(
        ctx,
        json!({ "type": "requestLayout", "pageId": page_id, "brief": brief }),
    )?;
    Ok("已把排版要求转给这一页的排版 agent；它会自己决定具体版面".into())
}

/* ---------- 排版 agent：组件手册 ---------- */

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

/* ---------- 排版 agent：写成品 ---------- */

pub fn set_layout_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "pageId": { "type": "string", "description": "要排的页；不传就是当前页" },
            "preset": {
                "type": "string",
                "enum": ["classic", "compact", "roomy", "custom"],
                "description": "整体疏密。compact 把内容压进一页、roomy 更透气、classic 是默认；custom 表示字号间距都被你手改过"
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
            layout.insert("html".into(), json!(html));
            notes.push(format!("整页手写 {len} 字（parts 与 preset 不再生效）"));
        }
    }

    if let Some(parts) = args.get("parts").and_then(Value::as_object) {
        if parts.is_empty() {
            return Err("parts 是空的：要么给至少一个块，要么用 reset 清空".into());
        }
        let known: Vec<&str> = page
            .get("blocks")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|block| block.get("id").and_then(Value::as_str))
                    .collect()
            })
            .unwrap_or_default();

        let mut merged = layout
            .get("parts")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();

        for (block_id, raw) in parts {
            if !known.contains(&block_id.as_str()) {
                return Err(format!(
                    "没有 id 为 {block_id} 的块。这一页的块是：{}（先用 read_page 看准确 id）",
                    if known.is_empty() {
                        "（没有块）".to_string()
                    } else {
                        known.join(" / ")
                    }
                ));
            }
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
        return Err("没有要改的项：至少给 preset / parts / html / reset 之一".into());
    }

    push_action(
        ctx,
        json!({
            "type": "patchPageLayout",
            "pageId": page_id,
            "layout": Value::Object(layout),
        }),
    )?;

    Ok(format!("这一页的版面已更新：{}", notes.join("、")))
}
