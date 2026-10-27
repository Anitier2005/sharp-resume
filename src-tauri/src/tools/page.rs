//! 页面控制工具层。
//!
//! 这一层不碰简历内容，只决定「用户眼前看到什么」：当前页、编辑还是预览、
//! 快捷标签有哪些、界面长什么样（配色 / 字号 / 栏宽）。
//!
//! 它和内容工具走同一条边界：**工具不直接改前端状态**，只往 `ctx.actions`
//! 里追加一条指令，随本轮回复一起回传，由前端 reducer 应用。
//! 好处是「判断在 Rust、应用在前端」这条规则对界面和内容一致，
//! 而且用户能立刻看到变化，不会出现后端改了、界面没反应的割裂。
//!
//! 每个工具都必须校验参数再下发：模型经常把页名当 id 传，直接把无效指令
//! 丢给前端，用户看到的是「点了没反应」，比报错难查得多。

use serde_json::{json, Value};

use crate::tools::{push_action, ToolCtx};

/// 当前简历的页面列表
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

fn label_of(page: &Value) -> String {
    page.get("label")
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .unwrap_or("未命名页")
        .to_string()
}

fn id_of(page: &Value) -> String {
    page.get("id").and_then(Value::as_str).unwrap_or("").to_string()
}

/// 第几页（1 起），找不到返回 0
fn index_of(pages: &[Value], page_id: &str) -> usize {
    pages
        .iter()
        .position(|page| id_of(page) == page_id)
        .map(|index| index + 1)
        .unwrap_or(0)
}

pub fn open_page_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "pageId": { "type": "string", "description": "页面 id（以 pg- 开头）。最精确，优先用它" },
            "index": { "type": "integer", "description": "第几页，从 1 开始。用户说「第二页」时用它" },
            "label": { "type": "string", "description": "页名，可以只写一部分（如「英文」）。不知道 id 时用它" }
        },
        "additionalProperties": false
    })
}

pub fn view_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "view": {
                "type": "string",
                "enum": ["edit", "preview"],
                "description": "edit = 表单编辑态；preview = 最终排版成品"
            }
        },
        "required": ["view"],
        "additionalProperties": false
    })
}

pub fn open_pages_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "pageIds": {
                "type": "array",
                "items": { "type": "string" },
                "description": "要摆在右侧快捷栏里的页面 id，按你希望的顺序。最多 8 个，超出的会排进折叠菜单"
            }
        },
        "required": ["pageIds"],
        "additionalProperties": false
    })
}

pub fn ui_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "accent": {
                "type": "string",
                "description": "预设主题色：white（中性）/ ink（靛）/ pine（松）/ verm（朱）"
            },
            "accentHex": { "type": "string", "description": "自定义强调色的十六进制，如 #3A6EA5" },
            "mode": { "type": "string", "enum": ["light", "dark"] },
            "textScale": { "type": "number", "description": "界面字号倍数，1.0–1.6，默认 1.2" },
            "sideW": { "type": "number", "description": "左侧栏宽度（像素），180–480" },
            "docW": { "type": "number", "description": "右侧简历区宽度（像素），320–900" },
            "showSide": { "type": "boolean", "description": "左侧栏显示 / 收起" },
            "showDoc": { "type": "boolean", "description": "右侧简历区显示 / 收起" }
        },
        "additionalProperties": false
    })
}

pub fn open_page(args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let pages = pages_of(ctx)?;
    if pages.is_empty() {
        return Err("这份简历还没有页面".into());
    }

    let mut target: Option<&Value> = None;
    if let Some(id) = args.get("pageId").and_then(Value::as_str) {
        let id = id.trim();
        if !id.is_empty() {
            target = pages.iter().find(|page| id_of(page) == id);
            if target.is_none() {
                return Err(format!(
                    "没有 id 为 {id} 的页面。可用的页：{}",
                    pages
                        .iter()
                        .map(|page| format!("{}({})", label_of(page), id_of(page)))
                        .collect::<Vec<_>>()
                        .join("、")
                ));
            }
        }
    }
    if target.is_none() {
        if let Some(index) = args.get("index").and_then(Value::as_u64) {
            let index = index as usize;
            if index == 0 || index > pages.len() {
                return Err(format!("页码超范围：这份简历共 {} 页", pages.len()));
            }
            target = pages.get(index - 1);
        }
    }
    if target.is_none() {
        if let Some(label) = args.get("label").and_then(Value::as_str) {
            let label = label.trim();
            if !label.is_empty() {
                target = pages
                    .iter()
                    .find(|page| label_of(page) == label)
                    .or_else(|| pages.iter().find(|page| label_of(page).contains(label)));
            }
            if target.is_none() {
                return Err(format!("没有页名包含「{label}」的页面"));
            }
        }
    }

    let page = target.ok_or_else(|| "要打开哪一页？请给出 pageId、index 或 label".to_string())?;
    let page_id = id_of(page);
    push_action(ctx, json!({ "type": "openPage", "pageId": page_id }))?;
    Ok(format!(
        "已切换到第 {} 页「{}」（id: {page_id}）",
        index_of(pages, &page_id),
        label_of(page)
    ))
}

pub fn set_view(args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let view = args
        .get("view")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if view != "edit" && view != "preview" {
        return Err("view 只能是 edit 或 preview".into());
    }
    push_action(ctx, json!({ "type": "setView", "view": view }))?;
    Ok(match view.as_str() {
        "preview" => "已切到预览（用户现在看到的是最终排版）".to_string(),
        _ => "已切回编辑（用户现在看到的是表单）".to_string(),
    })
}

pub fn set_open_pages(args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let pages = pages_of(ctx)?;
    let wanted = args
        .get("pageIds")
        .and_then(Value::as_array)
        .ok_or_else(|| "pageIds 必须是数组".to_string())?;

    let mut ordered: Vec<String> = Vec::new();
    let mut unknown: Vec<String> = Vec::new();
    for value in wanted {
        let id = value.as_str().unwrap_or("").trim().to_string();
        if id.is_empty() || ordered.contains(&id) {
            continue;
        }
        if pages.iter().any(|page| id_of(page) == id) {
            ordered.push(id);
        } else {
            unknown.push(id);
        }
    }
    if ordered.is_empty() {
        return Err("pageIds 里没有一个属于当前简历的页".into());
    }
    // 不变量：当前页永远在快捷栏里，否则用户会「看不见自己正在看的页」
    if !ctx.page_id.is_empty() && !ordered.iter().any(|id| id == ctx.page_id) {
        ordered.insert(0, ctx.page_id.to_string());
    }

    let names: Vec<String> = ordered
        .iter()
        .map(|id| {
            pages
                .iter()
                .find(|page| &id_of(page) == id)
                .map(label_of)
                .unwrap_or_else(|| id.clone())
        })
        .collect();

    push_action(ctx, json!({ "type": "setOpenPages", "pageIds": ordered }))?;
    let mut text = format!("快捷栏现在是：{}", names.join(" | "));
    if !unknown.is_empty() {
        text.push_str(&format!("\n（忽略了不存在的页：{}）", unknown.join("、")));
    }
    Ok(text)
}

pub fn set_ui(args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let mut patch = serde_json::Map::new();
    let mut notes: Vec<String> = Vec::new();

    if let Some(accent) = args.get("accent").and_then(Value::as_str) {
        let accent = accent.trim();
        if matches!(accent, "white" | "ink" | "pine" | "verm") {
            patch.insert("accent".into(), json!(accent));
            notes.push(format!("主题色 {accent}"));
        } else {
            return Err("accent 只能是 white / ink / pine / verm".into());
        }
    }

    if let Some(hex) = args.get("accentHex").and_then(Value::as_str) {
        let hex = hex.trim();
        let ok = hex.len() == 7
            && hex.starts_with('#')
            && hex[1..].chars().all(|c| c.is_ascii_hexdigit());
        if !ok {
            return Err("accentHex 要写成 #RRGGBB".into());
        }
        patch.insert("accentHex".into(), json!(hex.to_lowercase()));
        notes.push(format!("自定义色 {hex}"));
    }

    if let Some(mode) = args.get("mode").and_then(Value::as_str) {
        let mode = mode.trim();
        if mode != "light" && mode != "dark" {
            return Err("mode 只能是 light 或 dark".into());
        }
        patch.insert("mode".into(), json!(mode));
        notes.push(if mode == "dark" { "深色模式".into() } else { "浅色模式".to_string() });
    }

    if let Some(scale) = args.get("textScale").and_then(Value::as_f64) {
        // 夹在 1.0–1.6：低于 1.0 会让 9px 的辅助文字不可读，高于 1.6 版式会挤爆
        let scale = scale.clamp(1.0, 1.6);
        patch.insert("textScale".into(), json!((scale * 100.0).round() / 100.0));
        notes.push(format!("字号 {scale:.2}×"));
    }

    if let Some(width) = args.get("sideW").and_then(Value::as_f64) {
        // 圆整成整数再下发：前端按像素写 style，480.0 和 480 在 JSON 里是两种值，
        // reducer 的相等判断会因此认为"变了"，白触发一次重渲染
        let width = width.clamp(180.0, 480.0).round() as i64;
        patch.insert("sideW".into(), json!(width));
        notes.push(format!("左栏 {width}px"));
    }

    if let Some(width) = args.get("docW").and_then(Value::as_f64) {
        let width = width.clamp(320.0, 900.0).round() as i64;
        patch.insert("docW".into(), json!(width));
        notes.push(format!("右栏 {width}px"));
    }

    if let Some(show) = args.get("showSide").and_then(Value::as_bool) {
        patch.insert("showSide".into(), json!(show));
        notes.push(if show { "展开左栏".into() } else { "收起左栏".to_string() });
    }

    if let Some(show) = args.get("showDoc").and_then(Value::as_bool) {
        patch.insert("showDoc".into(), json!(show));
        notes.push(if show { "展开右栏".into() } else { "收起右栏".to_string() });
    }

    if patch.is_empty() {
        return Err("没有要改的项。可用参数：accent/accentHex/mode/textScale/sideW/docW/showSide/showDoc".into());
    }

    push_action(ctx, json!({ "type": "patchUi", "patch": Value::Object(patch) }))?;
    Ok(format!("界面已更新：{}", notes.join("、")))
}
