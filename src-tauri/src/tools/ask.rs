//! 提问表单：让会话 agent 主动向用户收集信息。
//!
//! 为什么不是「选择题工具」而是表单：真实需求里经常一次要问好几件不同性质的事
//! （岗位是什么、要不要英文版、期望几号前改完），单靠选择题问不完，
//! 而且模型会退化成「你想改哪一段？A/B/C」这种低效来回。
//!
//! 交互模型刻意做成「一轮问、下一轮答」：
//!   工具把表单推给前端，模型这轮到此为止；用户填完，前端把答案拼成一条普通
//!   用户消息发回来。这样不需要在 Rust 里挂起一个跨请求的 async 任务
//!   （那会在中断、切人、关窗口时留下悬挂状态），模型侧看到的也只是普通对话。

use serde_json::{json, Value};

use crate::tools::{push_action, ToolCtx};

/// 支持的控件类型。故意保守：多一个类型就多一处前端渲染与校验。
const KINDS: [&str; 7] = ["text", "textarea", "select", "multi", "confirm", "number", "date"];

pub fn params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "questions": {
                "type": "array",
                "description": "要问用户的题目，1–8 道",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "题目 id，答案回传时用它对应；不填自动编号" },
                        "label": { "type": "string", "description": "问题本身，写清楚要什么，别写「请填写」这种空话" },
                        "kind": {
                            "type": "string",
                            "enum": ["text", "textarea", "select", "multi", "confirm", "number", "date"],
                            "description": "text 单行；textarea 多行；select 单选；multi 多选；confirm 是/否；number 数字；date 日期"
                        },
                        "hint": { "type": "string", "description": "补充说明，可留空" },
                        "options": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "select / multi 必填"
                        },
                        "required": { "type": "boolean", "description": "是否必填，默认否" }
                    },
                    "required": ["label", "kind"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["questions"],
        "additionalProperties": false
    })
}

pub fn ask(args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let list = args
        .get("questions")
        .and_then(Value::as_array)
        .ok_or_else(|| "questions 必须是数组".to_string())?;
    if list.is_empty() {
        return Err("至少要有一道题".into());
    }
    if list.len() > 8 {
        return Err(format!("一次最多 8 道题，收到 {}", list.len()));
    }

    let mut questions: Vec<Value> = Vec::new();
    for (index, item) in list.iter().enumerate() {
        let object = item
            .as_object()
            .ok_or_else(|| format!("第 {} 题不是对象", index + 1))?;

        let label = object
            .get("label")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if label.is_empty() {
            return Err(format!("第 {} 题缺少 label", index + 1));
        }

        // 模型经常把 kind 写成 "string" / "choice"，这里直接拒绝并列出可选值，
        // 比默默当成 text 渲染更可控
        let kind = object
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("text")
            .trim()
            .to_string();
        if !KINDS.contains(&kind.as_str()) {
            return Err(format!(
                "第 {} 题的 kind 不认识：{kind}（可用：{}）",
                index + 1,
                KINDS.join(" / ")
            ));
        }

        let id = object
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("q{}", index + 1));

        let mut question = json!({ "id": id, "label": label, "kind": kind });
        if let Some(hint) = object.get("hint").and_then(Value::as_str) {
            if !hint.trim().is_empty() {
                question["hint"] = json!(hint.trim());
            }
        }
        if let Some(required) = object.get("required").and_then(Value::as_bool) {
            question["required"] = json!(required);
        }

        if kind == "select" || kind == "multi" {
            let options: Vec<String> = object
                .get("options")
                .and_then(Value::as_array)
                .map(|array| {
                    array
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            if options.is_empty() {
                return Err(format!("第 {} 题是 {kind}，必须给 options", index + 1));
            }
            question["options"] = json!(options);
        }

        questions.push(question);
    }

    push_action(ctx, json!({ "type": "ask", "questions": questions }))?;
    Ok(format!(
        "已向用户展示 {} 道题的表单。用户下一次发消息时会把答案给你；\
         这一轮不要再追问同一批问题，也不要自己编答案。",
        questions.len()
    ))
}
