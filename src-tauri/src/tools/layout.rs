//! 排版工具：**只有页排版 agent 能用**。
//!
//! 会话 agent 想改排版必须调 `request_layout`（把意图告诉页 agent），
//! 自己不能直接写样式 —— 否则「谁负责排版」这条边界就形同虚设。
//!
//! 可调的三项都是页级、可逆、不影响内容的量：
//!   density 疏密（行高）
//!   margin  页边距倍数
//!   align   页头对齐
//! 刻意不允许改字号：纸面是 A4 等比缩略，字号一变，分页虚线就和真实打印对不上。

use serde_json::{json, Value};

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

pub fn page_style_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "pageId": { "type": "string", "description": "要排版的页；不传就是当前页" },
            "density": {
                "type": "string",
                "enum": ["compact", "normal", "roomy"],
                "description": "紧凑 = 内容多、要一页装下；宽松 = 内容少、可以透气；normal 是默认"
            },
            "margin": { "type": "number", "description": "页边距倍数，0.8–1.6，1 是默认" },
            "align": { "type": "string", "enum": ["left", "center"], "description": "页头对齐" }
        },
        "additionalProperties": false
    })
}

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

pub fn set_page_style(args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let pages = pages_of(ctx)?;
    let page_id = args
        .get("pageId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(ctx.page_id)
        .to_string();

    let page = pages
        .iter()
        .find(|item| item.get("id").and_then(Value::as_str) == Some(page_id.as_str()))
        .ok_or_else(|| format!("没有 id 为 {page_id} 的页面"))?;

    // 在旧样式上合并：只传 density 时不该把 margin 清掉
    let mut style = page
        .get("style")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut notes: Vec<String> = Vec::new();

    if let Some(density) = args.get("density").and_then(Value::as_str) {
        let density = density.trim();
        if !matches!(density, "compact" | "normal" | "roomy") {
            return Err("density 只能是 compact / normal / roomy".into());
        }
        style.insert("density".into(), json!(density));
        notes.push(format!("疏密 {density}"));
    }

    if let Some(margin) = args.get("margin").and_then(Value::as_f64) {
        // 0.8 以下边距会贴边，1.6 以上正文区太窄会频繁换行
        let margin = margin.clamp(0.8, 1.6);
        let margin = (margin * 100.0).round() / 100.0;
        style.insert("margin".into(), json!(margin));
        notes.push(format!("边距 {margin:.2}×"));
    }

    if let Some(align) = args.get("align").and_then(Value::as_str) {
        let align = align.trim();
        if align != "left" && align != "center" {
            return Err("align 只能是 left 或 center".into());
        }
        style.insert("align".into(), json!(align));
        notes.push(format!("页头 {align}"));
    }

    if notes.is_empty() {
        return Err("没有要改的项：至少给 density / margin / align 之一".into());
    }

    push_action(
        ctx,
        json!({
            "type": "patchPageStyle",
            "pageId": page_id,
            "style": Value::Object(style),
        }),
    )?;
    Ok(format!("排版已更新：{}", notes.join("、")))
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
    let page_id = args
        .get("pageId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(ctx.page_id)
        .to_string();
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
    Ok("已把排版要求转给这一页的排版 agent；它会自己决定具体留白与疏密".into())
}
