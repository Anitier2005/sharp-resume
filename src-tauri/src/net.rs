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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
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
}

impl StreamChunk {
    fn delta(text: String) -> Self {
        Self { kind: "delta", delta: Some(text), reasoning: None, message: None }
    }
    fn reasoning(text: String) -> Self {
        Self { kind: "reasoning", delta: None, reasoning: Some(text), message: None }
    }
    fn done() -> Self {
        Self { kind: "done", delta: None, reasoning: None, message: None }
    }
    fn error(message: String) -> Self {
        Self { kind: "error", delta: None, reasoning: None, message: Some(message) }
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
    map
}

fn describe(status: u16, body: &str) -> String {
    let detail = serde_json::from_str::<serde_json::Value>(body)
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

/// 一次性请求（设置页的「测试连接」、以及不需要流式的场合）
#[tauri::command]
pub async fn chat_once(req: AiRequest) -> Result<String, String> {
    let response = client()
        .post(endpoint(&req.base_url))
        .bearer_auth(req.api_key.trim())
        .json(&body(&req, false))
        .send()
        .await
        .map_err(|error| format!("请求失败：{error}"))?;

    let status = response.status().as_u16();
    let text = response
        .text()
        .await
        .map_err(|error| format!("读取响应失败：{error}"))?;

    if !(200..300).contains(&status) {
        return Err(describe(status, &text));
    }

    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| format!("响应不是合法 JSON：{error}"))?;
    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let total = json["usage"]["total_tokens"].as_u64();
    Ok(match total {
        Some(tokens) => format!("{content} · {tokens} tokens"),
        None => content,
    })
}

/// 流式请求：按 SSE 解出 delta，逐块推给前端，返回累加后的全文。
///
/// `cancel` 由上层（session）持有：置位后在下一块就会退出，连接随 Response 释放而关闭。
pub async fn stream(
    req: &AiRequest,
    cancel: &std::sync::atomic::AtomicBool,
    on_event: &Channel<StreamChunk>,
) -> Result<String, String> {
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
        let message = describe(status, &text);
        let _ = on_event.send(StreamChunk::error(message.clone()));
        return Err(message);
    }

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut full = String::new();

    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::SeqCst) {
            return Ok(full);
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
                let _ = on_event.send(StreamChunk::done());
                return Ok(full);
            }
            let Ok(json) = serde_json::from_str::<serde_json::Value>(payload) else {
                continue;
            };
            let delta = &json["choices"][0]["delta"];
            if let Some(text) = delta["content"].as_str() {
                if !text.is_empty() {
                    full.push_str(text);
                    let _ = on_event.send(StreamChunk::delta(text.to_string()));
                }
            }
            if let Some(thought) = delta["reasoning_content"].as_str() {
                if !thought.is_empty() {
                    let _ = on_event.send(StreamChunk::reasoning(thought.to_string()));
                }
            }
        }
    }

    let _ = on_event.send(StreamChunk::done());
    Ok(full)
}

/// 无状态的一次性流式（设置页的直连测试用；正式对话走 session::chat_send）
#[tauri::command]
pub async fn chat_stream(req: AiRequest, on_event: Channel<StreamChunk>) -> Result<String, String> {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    stream(&req, &cancel, &on_event).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live_request() -> Option<AiRequest> {
        // 只在本机存在 config/ai.local.json 时跑，CI 上没有就跳过
        let path = std::path::Path::new("../config/ai.local.json");
        let raw = std::fs::read_to_string(path).ok()?;
        let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
        Some(AiRequest {
            base_url: json["baseUrl"].as_str()?.to_string(),
            api_key: json["apiKey"].as_str()?.to_string(),
            model: json["model"].as_str()?.to_string(),
            temperature: Some(0.0),
            reasoning: None,
            max_tokens: Some(64),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: "只回复两个字：收到".into(),
            }],
        })
    }

    #[test]
    fn endpoint_is_normalized() {
        assert_eq!(
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
            eprintln!("跳过：没有 config/ai.local.json");
            return;
        };
        let text = chat_once(req).await.expect("请求应成功");
        assert!(!text.is_empty(), "回复不该为空");
        eprintln!("live reply: {text}");
    }
}
