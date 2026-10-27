//! 页排版 agent：一个页面一条会话，只排版不改内容。
//!
//! 为什么单独开一个 agent 而不是让会话 agent 顺手调样式：
//!   1. 排版是「局部最优」问题——它要盯着这一页的内容量和留白反复权衡，
//!      和「用户到底想表达什么」是两件事，混在一个上下文里会互相干扰。
//!   2. 会话按候选人隔离、跟着对话走；排版要跟着**页**走。生命周期不同，
//!      用同一份会话就会互相污染。
//!   3. 权限必须收窄：会话 agent 只有 `request_layout`，真正写样式的
//!      `set_page_style` 只在这一层可用。
//!
//! 会话 key：`layout:<resumeId>:<pageId>`，与人的会话共用 sessions.json。

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::ipc::Channel;

use crate::net::{AiRequest, ChatMessage, StreamChunk};
use crate::session::{self, SendResult, Sessions};
use crate::tools::{Scope, ToolCtx};

/// 与会话 agent 共用一套「每轮注入定位」的做法：页面的形状每轮都可能变
const SYSTEM: &str = include_str!("../../../prompts/layout.md");

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutArgs {
    pub person_id: String,
    pub resume_id: String,
    pub page_id: String,
    /// 用户或会话 agent 给的意图，例如「内容超了一页，压到一页内」
    #[serde(default)]
    pub brief: String,
    pub workspace: Value,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub temperature: Option<f64>,
    pub reasoning: Option<String>,
    pub context_window: Option<usize>,
}

/// 把这一页的「形状」写成人话：模型据此判断挤还是空
fn describe_page(args: &LayoutArgs) -> String {
    let Some(person) = args
        .workspace
        .get("people")
        .and_then(Value::as_array)
        .and_then(|list| {
            list.iter()
                .find(|item| item.get("id").and_then(Value::as_str) == Some(args.person_id.as_str()))
        })
    else {
        return "（工作区快照里找不到这一页）".to_string();
    };

    let Some(resume) = person
        .get("resumes")
        .and_then(Value::as_array)
        .and_then(|list| {
            list.iter()
                .find(|item| item.get("id").and_then(Value::as_str) == Some(args.resume_id.as_str()))
        })
    else {
        return "（找不到这份简历）".to_string();
    };

    let pages = resume
        .get("pages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    // 找不到就必须明确报错：早先用「0 号页兜底」会让排版 agent 去改第 1 页
    let Some(offset) = pages
        .iter()
        .position(|page| page.get("id").and_then(Value::as_str) == Some(args.page_id.as_str()))
    else {
        return "（找不到这一页）".to_string();
    };
    let page = &pages[offset];
    let index = offset + 1;

    let label = page
        .get("label")
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .unwrap_or("未命名页");
    let header = page
        .get("header")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let sections = page
        .get("sections")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let entries: usize = sections
        .iter()
        .map(|section| {
            section
                .get("items")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or(0)
        })
        .sum();
    let bullets: usize = sections
        .iter()
        .flat_map(|section| {
            section
                .get("items")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .map(|entry| {
            entry
                .get("bullets")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or(0)
        })
        .sum();

    let style = page
        .get("style")
        .cloned()
        .unwrap_or_else(|| json!({ "density": "normal", "margin": 1.0, "align": "left" }));

    format!(
        "第 {index} 页 / 共 {} 页，页名「{label}」\n\
         页头字段 {header} 个，分区 {} 个，条目 {entries} 个，要点 {bullets} 条\n\
         当前版式：{style}",
        pages.len(),
        sections.len()
    )
}

#[tauri::command]
pub async fn layout_send(
    app: tauri::AppHandle,
    state: tauri::State<'_, Sessions>,
    args: LayoutArgs,
    on_event: Channel<StreamChunk>,
) -> Result<SendResult, String> {
    let key = format!("layout:{}:{}", args.resume_id, args.page_id);
    let system = SYSTEM.replace("{{page}}", &describe_page(&args));

    let budget_window = args.context_window.unwrap_or(64_000);
    let budget = budget_window.saturating_sub(session::estimate_tokens(&system) + 2_000) * 6 / 10;
    let history = session::trim_history(&state.history_of(&app, &key), budget);

    let mut messages = vec![ChatMessage::system(system)];
    for message in &history {
        let role = message.get("role").and_then(Value::as_str).unwrap_or("assistant");
        let content = message.get("content").and_then(Value::as_str).unwrap_or("");
        if content.trim().is_empty() {
            continue;
        }
        messages.push(match role {
            "user" => ChatMessage::user(content),
            _ => ChatMessage::assistant(content),
        });
    }

    let brief = args.brief.trim();
    messages.push(ChatMessage::user(if brief.is_empty() {
        "用户切换到了预览。请检查这一页的版式是否合适，不合适就调整。"
    } else {
        brief
    }));

    let template = AiRequest {
        base_url: args.base_url,
        api_key: args.api_key,
        model: args.model.clone(),
        messages: Vec::new(),
        temperature: args.temperature,
        reasoning: args.reasoning,
        max_tokens: None,
        tools: None,
    };

    let actions = std::sync::Mutex::new(Vec::new());
    let ctx = ToolCtx {
        workspace: &args.workspace,
        person_id: &args.person_id,
        resume_id: &args.resume_id,
        page_id: &args.page_id,
        actions: &actions,
    };

    let cancel = std::sync::atomic::AtomicBool::new(false);
    let turn = crate::agent::turn::run(&template, messages, &ctx, &cancel, &on_event, Scope::Layout)
        .await?;

    let reply_id = format!("l-{}", session::uuid_like());
    state.append(
        &app,
        &key,
        json!({
            "id": reply_id,
            "role": "assistant",
            "content": turn.content,
            "time": session::chrono_like(),
            "tool": if turn.used.is_empty() {
                format!("{} · 排版", args.model)
            } else {
                format!("{} · 用了 {}", args.model, turn.used.join(" / "))
            },
        }),
    );

    Ok(SendResult { id: reply_id, actions: turn.actions })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> LayoutArgs {
        LayoutArgs {
            person_id: "p1".into(),
            resume_id: "r1".into(),
            page_id: "pg2".into(),
            brief: String::new(),
            workspace: json!({
                "people": [{
                    "id": "p1",
                    "resumes": [{
                        "id": "r1",
                        "pages": [
                            { "id": "pg1", "label": "第 1 页", "header": [], "sections": [] },
                            {
                                "id": "pg2",
                                "label": "English",
                                "header": [{ "id": "f1" }],
                                "sections": [{
                                    "id": "s1",
                                    "items": [
                                        { "id": "e1", "bullets": ["a", "b"] },
                                        { "id": "e2", "bullets": ["c"] }
                                    ]
                                }],
                                "style": { "density": "roomy", "margin": 1.1, "align": "left" }
                            }
                        ]
                    }]
                }]
            }),
            base_url: String::new(),
            api_key: String::new(),
            model: String::new(),
            temperature: None,
            reasoning: None,
            context_window: None,
        }
    }

    #[test]
    fn page_brief_counts_content_and_keeps_style() {
        let text = describe_page(&sample());
        assert!(text.contains("第 2 页 / 共 2 页"), "{text}");
        assert!(text.contains("「English」"), "{text}");
        assert!(text.contains("条目 2 个"), "{text}");
        assert!(text.contains("要点 3 条"), "{text}");
        // 已有版式必须原样带上，否则模型每次都当第一次排
        assert!(text.contains("roomy"), "{text}");
    }

    #[test]
    fn missing_page_is_reported_not_panicked() {
        let mut args = sample();
        args.page_id = "pg-nope".into();
        assert!(describe_page(&args).contains("找不到这一页"));
    }
}
