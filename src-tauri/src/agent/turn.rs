//! 一轮对话的循环。
//!
//! 请求 → 模型可能要求调工具 → 就地执行 → 把结果作为 tool 消息回灌 → 再请求，
//! 直到它不再要工具、给出正文为止。
//!
//! 几个刻意的约束：
//!   - **轮次上限 6**：模型偶尔会反复读同一页，没有上限就是无限烧 token
//!   - **工具出错不中断**：错误文本当作结果喂回去，让它下一轮自己纠正；
//!     直接失败会把一次小失误变成整轮报废
//!   - **取消**：每轮开始前检查标记，中断时保留已经说出去的话

use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use serde_json::Value;

use crate::net::{self, AiRequest, ChatMessage, StreamChunk};
use crate::tools::{Scope, ToolCtx};

const MAX_ROUNDS: usize = 6;

pub struct TurnResult {
    /// 这一轮（可能跨多次请求）累计说出来的正文
    pub content: String,
    /// 调用过的工具名，按顺序
    pub used: Vec<String>,
    /// 页面控制工具产出的界面指令，按产出顺序。前端拿到后依次应用。
    pub actions: Vec<Value>,
}

fn drain(sink: &Mutex<Vec<Value>>) -> Vec<Value> {
    sink.lock().map(|guard| guard.clone()).unwrap_or_default()
}

/// 把工具调用翻译成一句给人看的状态
fn describe(name: &str, args: &Value) -> String {
    match name {
        "read_page" => match args["pageId"].as_str() {
            Some(id) if !id.trim().is_empty() => format!("读取页面 {id}"),
            _ => "读取当前页".to_string(),
        },
        "list_pages" => "列出所有页".to_string(),
        "read_library" => match args["keyword"].as_str() {
            Some(word) if !word.trim().is_empty() => format!("在经历库里找「{word}」"),
            _ => "翻经历库".to_string(),
        },
        "read_person" => "读取候选人信息".to_string(),
        "open_page" => match args["index"].as_u64() {
            Some(index) => format!("切到第 {index} 页"),
            None => "切换页面".to_string(),
        },
        "set_view" => match args["view"].as_str() {
            Some("preview") => "切到预览".to_string(),
            _ => "切到编辑".to_string(),
        },
        "set_open_pages" => "调整快捷标签".to_string(),
        "set_ui" => "调整界面".to_string(),
        other => format!("调用 {other}"),
    }
}

fn summarize(result: &str) -> String {
    format!("返回 {} 字", result.chars().count())
}

/// 有些网关 / 小模型不认 tools 字段，会直接 400。
/// 认出来就降级成普通对话，而不是让整个会话报废。
fn looks_like_tool_rejection(message: &str) -> bool {
    let lower = message.to_lowercase();
    if !(lower.contains("tool") || lower.contains("function")) {
        return false;
    }
    lower.contains("400")
        || lower.contains("not support")
        || lower.contains("unsupported")
        || lower.contains("invalid")
        || lower.contains("unknown")
}

/// 事件出口用闭包而不是 `Channel`：
/// 会话是「前端发起、边跑边推给它」，排版是「后端自己发起、没人等着」，
/// 后一种情况根本造不出 Channel（它需要一个 webview 端），
/// 硬要一个就会逼出「伪造通道」这种假东西。
pub async fn run<E: FnMut(StreamChunk) + Send>(
    template: &AiRequest,
    messages: Vec<ChatMessage>,
    ctx: &ToolCtx<'_>,
    cancel: &AtomicBool,
    mut on_event: E,
    scope: Scope,
) -> Result<TurnResult, String> {
    use std::sync::atomic::Ordering;

    let specs = crate::tools::specs(scope);
    let mut messages = messages;
    let mut speech = String::new();
    let mut used: Vec<String> = Vec::new();
    let mut with_tools = true;

    for _ in 0..MAX_ROUNDS {
        if cancel.load(Ordering::SeqCst) {
            break;
        }

        let mut request = AiRequest {
            base_url: template.base_url.clone(),
            api_key: template.api_key.clone(),
            model: template.model.clone(),
            messages: messages.clone(),
            temperature: template.temperature,
            reasoning: template.reasoning.clone(),
            max_tokens: template.max_tokens,
            tools: if with_tools { Some(specs.clone()) } else { None },
        };

        let outcome = match net::stream_with(&request, cancel, &mut |chunk| on_event(chunk)).await {
            Ok(value) => value,
            Err(error) if with_tools && looks_like_tool_rejection(&error) => {
                // 这个模型不认工具：这次及以后都不再带，退回纯对话
                with_tools = false;
                request.tools = None;
                net::stream_with(&request, cancel, &mut |chunk| on_event(chunk)).await?
            }
            Err(error) => return Err(error),
        };
        // 模型在调工具之前往往会先说一句「我看一下」，那句也要留着
        speech.push_str(&outcome.content);

        if outcome.tool_calls.is_empty() {
            return Ok(TurnResult { content: speech, used, actions: drain(ctx.actions) });
        }
        if cancel.load(Ordering::SeqCst) {
            break;
        }

        messages.push(ChatMessage::assistant_calls(outcome.tool_calls.clone(), outcome.content));

        for call in &outcome.tool_calls {
            let name = call.function.name.clone();
            let args: Value = serde_json::from_str(&call.function.arguments)
                .unwrap_or_else(|_| Value::Object(Default::default()));

            on_event(StreamChunk::tool_event(
                "tool_start",
                name.clone(),
                describe(&name, &args),
            ));

            let result = match crate::tools::call(&name, &args, ctx, scope) {
                Ok(text) => text,
                Err(error) => format!("工具执行失败：{error}"),
            };

            on_event(StreamChunk::tool_event(
                "tool_end",
                name.clone(),
                summarize(&result),
            ));

            used.push(name);
            messages.push(ChatMessage::tool_result(call.id.clone(), result));
        }
    }

    if speech.is_empty() {
        speech.push_str("（这一轮没有产出正文。可能是模型一直在调工具，或请求被中断。）");
    }
    Ok(TurnResult { content: speech, used, actions: drain(ctx.actions) })
}


