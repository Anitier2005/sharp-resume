//! 通讯层：所有对模型的请求都在这里发出，不经过 WebView。
//!
//! 为什么不让前端 fetch：
//!   1. WebView2 会把 `https://...` 当跨域，tokenhub / DeepSeek 都不返回 CORS 头，直接失败
//!   2. API key 一旦在前端，就存在 WebView 的 localStorage 里，任何注入脚本都能读
//!   3. 并发：改写多条 bullet 时前端会同时发 N 个请求，这里一个 Client 共享连接池
//!
//! 流式输出走 Tauri 的 Channel：Rust 解 SSE，逐块推给前端，前端只管渲染。

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use std::time::Duration;
use tauri::ipc::Channel;

/// 一条对话消息。比原来的 {role, content} 多了工具相关字段：
///   - assistant 带 `tool_calls`：模型要求调工具
///   - role = "tool" 带 `tool_call_id`：工具执行结果回灌
/// 两个可选字段都 skip_serializing_if，普通对话发出去的 JSON 和以前完全一致。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatMessage {
    pub role: String,
    /// 字符串（普通消息）或数组（视觉消息：`[{type:"text"},{type:"image_url"}]`）。
    /// 原样透传 —— 各家兼容端对段类型的校验不同，在这里建模反而束手束脚。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    /// 正文文本。视觉消息的 content 是数组，取其中 text 段拼接；
    /// 工具调用场景基本是纯文本，读字符串即可。
    pub fn text(&self) -> String {
        match &self.content {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(serde_json::Value::Array(parts)) => parts
                .iter()
                .filter_map(|part| part.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        }
    }

    pub fn system(text: impl Into<String>) -> Self {
        Self { role: "system".into(), content: Some(serde_json::Value::String(text.into())), tool_calls: None, tool_call_id: None }
    }
    pub fn user(text: impl Into<String>) -> Self {
        Self { role: "user".into(), content: Some(serde_json::Value::String(text.into())), tool_calls: None, tool_call_id: None }
    }
    /// 带图片的用户消息。`content` 走协议的多模态数组：
    /// 一个 text 段 + 若干 image_url 段（`data:` 开头的 base64）。
    ///
    /// 为什么图片不拼进字符串：一张 A4 页面渲染成 JPEG 也就 90KB，
    /// 但 base64 之后塞进正文会让 token 计数失真到没法估算预算。
    pub fn user_with_images(text: impl Into<String>, images: &[String]) -> Self {
        let mut parts = vec![serde_json::json!({ "type": "text", "text": text.into() })];
        for url in images {
            parts.push(serde_json::json!({ "type": "image_url", "image_url": { "url": url } }));
        }
        Self {
            role: "user".into(),
            content: Some(serde_json::Value::Array(parts)),
            tool_calls: None,
            tool_call_id: None,
        }
    }
    pub fn assistant(text: impl Into<String>) -> Self {
        Self { role: "assistant".into(), content: Some(serde_json::Value::String(text.into())), tool_calls: None, tool_call_id: None }
    }
    /// 模型这一轮只要调工具、不说话
    pub fn assistant_calls(calls: Vec<ToolCall>, text: String) -> Self {
        Self {
            role: "assistant".into(),
            content: if text.is_empty() { None } else { Some(serde_json::Value::String(text)) },
            tool_calls: Some(calls),
            tool_call_id: None,
        }
    }
    /// 工具结果。`content` 必须是字符串（OpenAI 协议），读不出来也要给个错误文本
    pub fn tool_result(call_id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            role: "tool".into(),
            content: Some(serde_json::Value::String(text.into())),
            tool_calls: None,
            tool_call_id: Some(call_id.into()),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FunctionCall {
    pub name: String,
    /// 模型给的是 JSON 字符串，不是对象；这里保持原样，解析失败由工具层兜
    pub arguments: String,
}

/// 前端传过来的请求。字段刻意保持扁平，好和 `AiSettings` 对上。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiRequest {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub temperature: Option<f64>,
    /// 思维强度档位（off/low/medium/high）；不是所有模型都认，原样透传
    #[serde(default)]
    pub reasoning: Option<String>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// OpenAI 风格的 tools 数组；None 时请求体里不出现这个字段
    #[serde(default)]
    pub tools: Option<serde_json::Value>,
}

/// 流式事件：一块增量、结束、或错误。前端按 kind 分支处理。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamChunk {
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<String>,
    /// 部分模型会单独给思维链内容（如 reasoning_content）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// kind = tool_start / tool_end 时：工具名
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// 工具的人话说明（读了第几页 / 结果有多大）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl StreamChunk {
    fn delta(text: String) -> Self {
        Self { kind: "delta", delta: Some(text), reasoning: None, message: None, tool: None, detail: None }
    }
    fn reasoning(text: String) -> Self {
        Self { kind: "reasoning", delta: None, reasoning: Some(text), message: None, tool: None, detail: None }
    }
    fn done() -> Self {
        Self { kind: "done", delta: None, reasoning: None, message: None, tool: None, detail: None }
    }
    fn error(message: String) -> Self {
        Self { kind: "error", delta: None, reasoning: None, message: Some(message), tool: None, detail: None }
    }
    pub fn tool_event(kind: &'static str, tool: String, detail: String) -> Self {
        Self { kind, delta: None, reasoning: None, message: None, tool: Some(tool), detail: Some(detail) }
    }
}

/// 全局共享的 Client：连接池在这里，并发请求复用它。
/// 超时按「对话」这个场景给：连接 10s、整体 5min（长文改写可能跑很久）。
fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(300))
            .user_agent("sharp-resume/0.1")
            .build()
            .expect("构造 HTTP client 失败")
    })
}

/// baseUrl 可能带或不带 `/chat/completions`，也可能带尾斜杠
fn endpoint(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if base.ends_with("/chat/completions") {
        base.to_string()
    } else {
        format!("{base}/chat/completions")
    }
}

fn body(req: &AiRequest, stream: bool) -> serde_json::Value {
    let mut map = serde_json::json!({
        "model": req.model,
        "messages": req.messages,
        "stream": stream,
    });
    if let Some(temperature) = req.temperature {
        map["temperature"] = serde_json::json!(temperature);
    }
    if let Some(max_tokens) = req.max_tokens {
        map["max_tokens"] = serde_json::json!(max_tokens);
    }
    // 思维强度：OpenAI 风格是 reasoning_effort，部分网关用 reasoning。
    // 两个都带上，不认的一方会忽略未知字段。
    if let Some(effort) = &req.reasoning {
        if effort != "off" {
            map["reasoning_effort"] = serde_json::json!(effort);
        }
    }
    // 有工具就挂上；措辞固定 auto —— 强制调用会让「随便聊聊」也去读简历
    if let Some(tools) = &req.tools {
        map["tools"] = tools.clone();
        map["tool_choice"] = serde_json::json!("auto");
    }
    map
}

/// 失败的类别。只有 `ImageRejected` 会触发一个动作（改判能力），其余六支只影响措辞。
///
/// 为什么不把它传遍调用栈：为了一个枚举去把 `Result<String, String>` 改成
/// `Result<_, TurnError>` 会波及 `session.rs` / `agent/layout.rs` / `agent/turn.rs`
/// 三处签名与十几处 `?`。唯一需要知道 status 的地方就是这里（我们手里正拿着响应），
/// 就地判、就地改判，是最小改动。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Auth,
    Quota,
    NotFound,
    Offline,
    Timeout,
    /// 400 且错误体明确指向图像字段：**强证据**，但要可逆
    ImageRejected,
    BadRequest,
    Server,
}

/// 从 `(status, body)` 判类别。
///
/// `status == 0` 表示「请求没走到 HTTP」——连接失败、DNS、超时都在这一支，
/// 调用方把 reqwest 的错误文本当 body 传进来。
///
/// `ImageRejected` 的判据必须**两条同时**命中（图像字段 + 拒绝措辞）：
/// 只命中一半一律 `BadRequest`。宁可少判一次支持，也不要多判一次不支持 ——
/// 前者只是少发一次图，后者是永久拉黑用户的看图能力。
pub fn classify_error(status: u16, body: &str) -> ErrorKind {
    let lowered = body.to_lowercase();
    match status {
        0 => {
            if lowered.contains("timed out")
                || lowered.contains("timeout")
                || lowered.contains("超时")
            {
                ErrorKind::Timeout
            } else {
                ErrorKind::Offline
            }
        }
        401 | 403 => ErrorKind::Auth,
        402 | 429 => ErrorKind::Quota,
        404 => ErrorKind::NotFound,
        408 => ErrorKind::Timeout,
        400 if looks_like_image_reject(&lowered) => ErrorKind::ImageRejected,
        400 => ErrorKind::BadRequest,
        code if (500..600).contains(&code) => ErrorKind::Server,
        _ => ErrorKind::BadRequest,
    }
}

/// 图像字段 + 拒绝措辞，两条同时命中才算。
fn looks_like_image_reject(lowered: &str) -> bool {
    const FIELD: [&str; 5] = ["image_url", "image", "vision", "multimodal", "content[].type"];
    const REFUSE: [&str; 5] = ["not support", "unsupported", "invalid", "unknown", "unrecognized"];
    // 尺寸/体积类的话即使带「invalid」也不判能力：那是「这张太大」，
    // 压缩之后就能过，不代表模型不能看图
    const SIZE: [&str; 5] = ["too large", "size", "dimension", "resolution", "payload"];
    if SIZE.iter().any(|word| lowered.contains(word)) {
        return false;
    }
    FIELD.iter().any(|word| lowered.contains(word))
        && REFUSE.iter().any(|word| lowered.contains(word))
}

/// 这条请求里到底有没有图。
///
/// 只有带了图，400 才可能是「拒图」—— 纯文本请求被 400 拒绝跟能力无关，
/// 不许顺手把一个模型判成看不了图。
fn has_images(req: &AiRequest) -> bool {
    req.messages.iter().any(|message| {
        message
            .content
            .as_ref()
            .and_then(|content| content.as_array())
            .map(|parts| {
                parts
                    .iter()
                    .any(|part| part.get("type").and_then(|kind| kind.as_str()) == Some("image_url"))
            })
            .unwrap_or(false)
    })
}

fn describe(status: u16, body: &str) -> String {    let detail = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|json| {
            json.get("error")
                .and_then(|error| error.get("message"))
                .and_then(|message| message.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| body.chars().take(300).collect());
    if detail.trim().is_empty() {
        format!("HTTP {status}")
    } else {
        format!("HTTP {status} · {detail}")
    }
}

/// 一次性请求，**把状态码原样带出来**。
///
/// 只给两处用：能力探针（要区分「400 拒图」与「400 别的原因」），以及 `chat_once`。
/// 其它调用方一律用 `chat_once` —— 它们只需要一句人话。
pub async fn chat_once_status(req: AiRequest) -> Result<(String, Option<u64>), (u16, String)> {
    let response = client()
        .post(endpoint(&req.base_url))
        .bearer_auth(req.api_key.trim())
        .json(&body(&req, false))
        .send()
        .await
        // 没走到 HTTP：status 用 0 表示，分类函数据此判 Offline / Timeout
        .map_err(|error| (0u16, format!("请求失败：{error}")))?;

    let status = response.status().as_u16();
    let text = response
        .text()
        .await
        .map_err(|error| (status, format!("读取响应失败：{error}")))?;

    if !(200..300).contains(&status) {
        return Err((status, describe(status, &text)));
    }

    let json: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| (status, format!("响应不是合法 JSON：{error}")))?;
    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();
    Ok((content, json["usage"]["total_tokens"].as_u64()))
}

/// 一次性请求（设置页的「测试连接」、以及不需要流式的场合）
#[tauri::command]
pub async fn chat_once(req: AiRequest) -> Result<String, String> {
    let (content, total) = chat_once_status(req).await.map_err(|(_, message)| message)?;
    Ok(match total {
        Some(tokens) => format!("{content} · {tokens} tokens"),
        None => content,
    })
}

/// 一轮流的产出：正文 + 模型要求的工具调用
#[derive(Debug, Default)]
pub struct StreamOutcome {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
}

/// SSE 里 tool_calls 是**分片**下发的（id / name / arguments 各来一段），
/// 必须按 index 累积拼起来，最后才是完整的 JSON 字符串。
#[derive(Debug, Default)]
struct CallBuf {
    id: String,
    name: String,
    args: String,
}

/// 流式请求：按 SSE 解出 delta，逐块推给前端。
///
/// `cancel` 由上层（session）持有：置位后在下一块就会退出，连接随 Response 释放而关闭。
pub async fn stream(
    req: &AiRequest,
    cancel: &std::sync::atomic::AtomicBool,
    on_event: &Channel<StreamChunk>,
) -> Result<StreamOutcome, String> {
    stream_with(req, cancel, &mut |chunk| {
        let _ = on_event.send(chunk);
    })
    .await
}

/// 与 `stream` 同一套逻辑，事件出口换成可注入的闭包 ——
/// 单测造不出 Channel（要有 webview），但闭环验证必须能收事件。
pub async fn stream_with<E: FnMut(StreamChunk)>(
    req: &AiRequest,
    cancel: &std::sync::atomic::AtomicBool,
    emit: &mut E,
) -> Result<StreamOutcome, String> {
    use std::sync::atomic::Ordering;

    let response = client()
        .post(endpoint(&req.base_url))
        .bearer_auth(req.api_key.trim())
        .json(&body(req, true))
        .send()
        .await
        .map_err(|error| format!("请求失败：{error}"))?;

    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        let text = response.text().await.unwrap_or_default();
        let mut message = describe(status, &text);
        // 协议层明确拒图（且这一轮确实带了图）→ 就地改判，下一轮起不再发图。
        // 为什么在这里改判而不是把错误码往上传：只有这里有 status + body，
        // 也是唯一知道「这条请求是不是真的带了图」的地方之一（`has_images`）。
        // 改判本身就让这条路径一次收敛 —— 下一轮 `resolve()` 给出 Unsupported，
        // 材料组装不再附图，不会再有第二次。不需要额外的计数器。
        if classify_error(status, &text) == ErrorKind::ImageRejected && has_images(req) {
            crate::capability::mark_protocol_reject(&req.base_url, &req.model, &req.api_key);
            message.push_str(
                "（这个模型不接受图像输入。去 设置 → 模型 点一次「测一次多模态」确认，或换一个能看图的模型。）",
            );
        }
        emit(StreamChunk::error(message.clone()));
        return Err(message);
    }

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut outcome = StreamOutcome::default();
    let mut calls: Vec<CallBuf> = Vec::new();

    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::SeqCst) {
            return Ok(outcome);
        }
        let bytes = chunk.map_err(|error| format!("读取流失败：{error}"))?;
        buffer.push_str(&String::from_utf8_lossy(&bytes));

        // SSE：一行一条，空行分隔事件
        while let Some(index) = buffer.find('\n') {
            let line = buffer[..index].trim_end_matches('\r').to_string();
            buffer.drain(..=index);
            if !line.starts_with("data:") {
                continue;
            }
            let payload = line[5..].trim();
            if payload.is_empty() {
                continue;
            }
            if payload == "[DONE]" {
                outcome.tool_calls = finish_calls(calls);
                emit(StreamChunk::done());
                return Ok(outcome);
            }
            let Ok(json) = serde_json::from_str::<serde_json::Value>(payload) else {
                continue;
            };
            let delta = &json["choices"][0]["delta"];
            if let Some(text) = delta["content"].as_str() {
                if !text.is_empty() {
                    outcome.content.push_str(text);
                    emit(StreamChunk::delta(text.to_string()));
                }
            }
            if let Some(thought) = delta["reasoning_content"].as_str() {
                if !thought.is_empty() {
                    emit(StreamChunk::reasoning(thought.to_string()));
                }
            }
            if let Some(parts) = delta["tool_calls"].as_array() {
                for part in parts {
                    let slot = part["index"].as_u64().unwrap_or(0) as usize;
                    while calls.len() <= slot {
                        calls.push(CallBuf::default());
                    }
                    let call = &mut calls[slot];
                    if let Some(id) = part["id"].as_str() {
                        if !id.is_empty() && call.id.is_empty() {
                            call.id = id.to_string();
                        }
                    }
                    if let Some(name) = part["function"]["name"].as_str() {
                        if !name.is_empty() && call.name.is_empty() {
                            call.name = name.to_string();
                        }
                    }
                    if let Some(args) = part["function"]["arguments"].as_str() {
                        call.args.push_str(args);
                    }
                }
            }
        }
    }

    outcome.tool_calls = finish_calls(calls);
    emit(StreamChunk::done());
    Ok(outcome)
}

/// 碎片收口：没名字的丢掉（有些网关会发空占位），缺 id 的补一个
fn finish_calls(calls: Vec<CallBuf>) -> Vec<ToolCall> {
    calls
        .into_iter()
        .enumerate()
        .filter(|(_, call)| !call.name.is_empty())
        .map(|(index, call)| ToolCall {
            id: if call.id.is_empty() { format!("call_{index}") } else { call.id },
            kind: "function".into(),
            function: FunctionCall {
                name: call.name,
                // 空参数要给 "{}"，否则工具层解析会报错
                arguments: if call.args.trim().is_empty() { "{}".into() } else { call.args },
            },
        })
        .collect()
}

/// 无状态的一次性流式（设置页的直连测试用；正式对话走 session::chat_send）
#[tauri::command]
pub async fn chat_stream(req: AiRequest, on_event: Channel<StreamChunk>) -> Result<String, String> {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    Ok(stream(&req, &cancel, &on_event).await?.content)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 视觉消息的形状：一个 text 段 + 若干 image_url 段。
    /// 这条断言是给「图片发出去模型却看不到」这类问题兜底的 ——
    /// 段类型写错，网关不会报错，只会静默忽略图片。
    #[test]
    fn image_message_uses_the_multimodal_shape() {
        let message = ChatMessage::user_with_images(
            "看这一页",
            &["data:image/jpeg;base64,AAAA".to_string()],
        );
        let value = serde_json::to_value(&message).unwrap();
        let parts = value["content"].as_array().expect("content 要是数组");
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["type"], "text");
        assert_eq!(parts[0]["text"], "看这一页");
        assert_eq!(parts[1]["type"], "image_url");
        assert_eq!(parts[1]["image_url"]["url"], "data:image/jpeg;base64,AAAA");
        // 纯文本消息仍然是字符串：多数网关对数组的校验更严
        assert!(serde_json::to_value(ChatMessage::user("纯文本")).unwrap()["content"].is_string());
    }

    fn live_request() -> Option<AiRequest> {
        // 真联网的用例默认不跑：密钥可能在、但配额可能没了（402），
        // 那时红的不是代码。要验 TLS/代理就显式 `SHARP_LIVE=1 cargo test`。
        let (base_url, api_key, model) = live_settings()?;
        Some(AiRequest {
            base_url,
            api_key,
            model,
            temperature: Some(0.0),
            reasoning: None,
            max_tokens: Some(64),
            tools: None,
            messages: vec![ChatMessage::user("只回复两个字：收到")],
        })
    }

    /// 公开配置 + 本机密钥，和 scripts/package.mjs 里那套目录约定一致。
    /// 同时把「默认不联网」的开关收在这里，两条用例都走它。
    fn live_settings() -> Option<(String, String, String)> {
        if std::env::var("SHARP_LIVE").is_err() {
            return None;
        }
        let base: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string("../testdata/ai.json").ok()?).ok()?;
        let secret: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string("../testdata/secrets/ai.local.json").ok()?)
                .ok()?;
        Some((
            base["baseUrl"].as_str()?.to_string(),
            secret["apiKey"].as_str()?.to_string(),
            base["model"].as_str()?.to_string(),
        ))
    }

    /* ---------- classify_error：误判的代价不对称，所以逐条钉住 ---------- */

    #[test]
    fn a_model_that_refuses_images_is_recognized() {
        // 这是唯一会触发「改判」的一支：400 + 图像字段 + 拒绝措辞，三条同时命中
        assert_eq!(
            classify_error(400, r#"{"error":{"message":"image_url is not supported by this model"}}"#),
            ErrorKind::ImageRejected
        );
        assert_eq!(
            classify_error(400, "This model does not support vision input"),
            ErrorKind::ImageRejected
        );
        assert_eq!(
            classify_error(400, r#"{"error":"unknown variant `image_url`, expected `text`"}"#),
            ErrorKind::ImageRejected
        );
    }

    #[test]
    fn a_plain_400_never_changes_capability() {
        // 只有图像词、没有拒绝措辞 → 不是拒图
        assert_eq!(
            classify_error(400, r#"{"error":"messages must not be empty"}"#),
            ErrorKind::BadRequest
        );
        assert_eq!(
            classify_error(400, r#"{"error":"image_url is required"}"#),
            ErrorKind::BadRequest
        );
    }

    #[test]
    fn a_too_large_image_is_not_a_capability_problem() {
        // 「这张太大」压缩后就能过，判成不支持等于永久拉黑
        assert_eq!(
            classify_error(400, "image is too large, max 5MB"),
            ErrorKind::BadRequest
        );
        assert_eq!(
            classify_error(400, "invalid image size: 12000x12000 exceeds limit"),
            ErrorKind::BadRequest
        );
        assert_eq!(
            classify_error(400, "invalid image resolution"),
            ErrorKind::BadRequest
        );
    }

    #[test]
    fn transport_failures_are_not_verdicts() {
        assert_eq!(classify_error(0, "请求失败：timed out"), ErrorKind::Timeout);
        assert_eq!(classify_error(0, "请求失败：dns error"), ErrorKind::Offline);
        assert_eq!(classify_error(408, "request timeout"), ErrorKind::Timeout);
    }

    #[test]
    fn config_errors_are_classified_apart() {
        assert_eq!(classify_error(401, "bad key"), ErrorKind::Auth);
        assert_eq!(classify_error(403, "forbidden"), ErrorKind::Auth);
        assert_eq!(classify_error(402, "no balance"), ErrorKind::Quota);
        assert_eq!(classify_error(429, "rate limited"), ErrorKind::Quota);
        assert_eq!(classify_error(404, "model not found"), ErrorKind::NotFound);
        assert_eq!(classify_error(500, "boom"), ErrorKind::Server);
        assert_eq!(classify_error(503, "unavailable"), ErrorKind::Server);
    }

    #[test]
    fn only_a_request_carrying_images_can_be_blamed_on_images() {
        // 纯文本请求撞 400，就算错误体里提到 image，也不许判能力 ——
        // 这一条挡的是「把别的问题算到多模态头上」
        let text_only = AiRequest {
            base_url: "https://x/v1".into(),
            api_key: "k".into(),
            model: "m".into(),
            messages: vec![ChatMessage::system("s"), ChatMessage::user("纯文本")],
            temperature: None,
            reasoning: None,
            max_tokens: None,
            tools: None,
        };
        assert!(!has_images(&text_only));

        let with_image = AiRequest {
            messages: vec![ChatMessage::user_with_images(
                "看图",
                &["data:image/png;base64,AAAA".to_string()],
            )],
            ..text_only
        };
        assert!(has_images(&with_image));
    }

    #[test]
    fn endpoint_is_normalized() {        assert_eq!(
            endpoint("https://a.com/v1/"),
            "https://a.com/v1/chat/completions"
        );
        assert_eq!(
            endpoint("https://a.com/v1/chat/completions"),
            "https://a.com/v1/chat/completions"
        );
    }

    /// 真连一次：验证 TLS（Schannel）、代理、JSON 三段都对
    #[tokio::test]
    async fn live_chat_once() {
        let Some(req) = live_request() else {
            eprintln!("跳过：没有 testdata 配置");
            return;
        };
        let text = chat_once(req).await.expect("请求应成功");
        assert!(!text.is_empty(), "回复不该为空");
        eprintln!("live reply: {text}");
    }

    fn demo_workspace() -> serde_json::Value {
        serde_json::json!({
            "people": [{
                "id": "p1",
                "fields": [{ "id": "f0", "label": "姓名", "value": "张三" }],
                "items": [],
                "resumes": [{
                    "id": "r1",
                    "title": "中文简历",
                    "pages": [
                        { "id": "pg1", "label": "通用版", "header": [], "sections": [] },
                        { "id": "pg2", "label": "English", "header": [], "sections": [] }
                    ]
                }]
            }]
        })
    }

    /// 工具调用闭环：模型要工具 → 我们执行 → 结果回灌 → 它据此回答。
    /// 这是 P1 的核心验收点，必须真连模型，否则只是「协议长得像」。
    #[tokio::test]
    async fn live_tool_call_roundtrip() {
        let Some((base_url, api_key, model)) = live_settings() else {
            eprintln!("跳过：没有 testdata 配置");
            return;
        };

        let workspace = demo_workspace();
        let out = crate::tools::Outbox::default();
        let ctx = crate::tools::ToolCtx {
            workspace: &workspace,
            person_id: "p1",
            resume_id: "r1",
            page_id: "pg2",
            out: &out,
            root: None,
            facts: None,
        };

        let mut messages = vec![
            ChatMessage::system(
                "你是简历助手。用户问「第几页」的时候必须先调用 list_pages，不许凭猜回答。",
            ),
            ChatMessage::user("我现在在第几页？"),
        ];

        let cancel = std::sync::atomic::AtomicBool::new(false);
        let first = stream_with(
            &AiRequest {
                base_url: base_url.clone(),
                api_key: api_key.clone(),
                model: model.clone(),
                messages: messages.clone(),
                temperature: Some(0.0),
                reasoning: None,
                max_tokens: None,
                tools: Some(crate::tools::specs(crate::tools::Scope::Session)),
            },
            &cancel,
            &mut |_| {},
        )
        .await
        .expect("第一轮应成功");

        assert!(!first.tool_calls.is_empty(), "模型应该要求调工具，实际正文：{}", first.content);

        let call = &first.tool_calls[0];
        let args: serde_json::Value =
            serde_json::from_str(&call.function.arguments).unwrap_or_else(|_| serde_json::json!({}));
        let result = crate::tools::call(&call.function.name, &args, &ctx, crate::tools::Scope::Session)
            .unwrap_or_else(|error| panic!("工具 {} 执行失败：{error}", call.function.name));
        assert!(
            result.contains("← 当前") && result.contains("English"),
            "工具应标出当前页是 English，实际：\n{result}"
        );

        messages.push(ChatMessage::assistant_calls(first.tool_calls.clone(), first.content));
        messages.push(ChatMessage::tool_result(call.id.clone(), result));

        let second = stream_with(
            &AiRequest {
                base_url,
                api_key,
                model,
                messages,
                temperature: Some(0.0),
                reasoning: None,
                max_tokens: None,
                tools: Some(crate::tools::specs(crate::tools::Scope::Session)),
            },
            &cancel,
            &mut |_| {},
        )
        .await
        .expect("第二轮应成功");

        assert!(!second.content.trim().is_empty(), "工具回灌后应该给出正文");
        assert!(
            second.content.contains('2') || second.content.contains("二") || second.content.contains("English"),
            "回答里应提到第 2 页 / English，实际：{}",
            second.content
        );
        eprintln!("live tool reply: {}", second.content);
    }
}
