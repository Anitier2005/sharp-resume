//! 会话历史检索：给模型一个「翻旧账」的入口。
//!
//! 为什么需要它：上下文窗口是有上限的，历史被挤出窗口之后模型就「失忆」了。
//! 滚动摘要（`session.rs`）能保住主线，但摘要必然丢细节 ——
//! 「第三轮里那个 QPS 数字」「上周说过的薪资口径」这类精确信息，
//! 只有回到原文才找得到。所以配一个按键检索：模型自己决定要不要翻。
//!
//! 数据源就是 `chat` 表：消息本来就全量落盘，这里只做只读检索，不改任何状态。

use rusqlite::params;
use serde_json::{json, Value};
use tauri::State;

use crate::db::Shared;

/// 最多看多少条消息：单人会话再长也就几千条，一次全读比按页翻简单
const SCAN_LIMIT: usize = 4_000;

struct Scored {
    id: String,
    role: String,
    time: String,
    snippet: String,
    score: f32,
}

fn excerpt(content: &str, needles: &[String]) -> String {
    let flat = content.replace('\n', " ");
    let lower = flat.to_lowercase();
    // 用第一个命中的词定位，取它前后各一段
    let mut hit_at = None;
    for needle in needles {
        if let Some(pos) = lower.find(needle) {
            hit_at = Some(pos);
            break;
        }
    }
    let Some(at) = hit_at else {
        return trim_chars(&flat, 0, 240);
    };
    // 字节位置 → 字符位置不需要精确：按字节切再往回收，避免切坏汉字
    let start = floor_char_boundary(&flat, at.saturating_sub(120));
    let end = ceil_char_boundary(&flat, (at + 120).min(flat.len()));
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.push_str(flat[start..end].trim());
    if end < flat.len() {
        out.push('…');
    }
    out
}

fn trim_chars(text: &str, start: usize, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let slice: String = chars.iter().skip(start).take(max).collect();
    if chars.len() > start + max {
        format!("{slice}…")
    } else {
        slice
    }
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn ceil_char_boundary(text: &str, mut index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

/// 在给定连接上检索某个会话。
fn search(
    conn: &rusqlite::Connection,
    scope: &str,
    query: &str,
    role: Option<&str>,
    limit: usize,
) -> Result<Vec<Scored>, String> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let needles: Vec<String> = {
        let mut list = crate::knowledge::tokens(query);
        // 整句也当一个「词」：原话命中最可信
        list.push(query.to_lowercase());
        list
    };

    let mut stmt = conn
        .prepare(
            "SELECT body FROM chat WHERE scope = ?1 ORDER BY ord DESC LIMIT ?2",
        )
        .map_err(|error| format!("读取会话历史失败：{error}"))?;
    let bodies: Vec<String> = stmt
        .query_map(params![scope, SCAN_LIMIT as i64], |row| row.get::<_, String>(0))
        .map_err(|error| format!("读取会话历史失败：{error}"))?
        .flatten()
        .collect();

    let mut out: Vec<Scored> = Vec::new();
    for body in bodies {
        let Ok(message) = serde_json::from_str::<Value>(&body) else { continue };
        let message_role = message.get("role").and_then(Value::as_str).unwrap_or("user");
        // 工具内部轮（internal）不进检索：它们是排版 agent 的往返，翻出来只会误导
        if message.get("internal").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        if let Some(want) = role {
            if !want.is_empty() && want != message_role {
                continue;
            }
        }
        let content = message.get("content").and_then(Value::as_str).unwrap_or("");
        if content.trim().is_empty() {
            continue;
        }
        let lower = content.to_lowercase();
        let mut score = 0.0f32;
        for needle in &needles {
            if needle.chars().count() < 2 {
                continue;
            }
            let mut from = 0;
            while let Some(pos) = lower[from..].find(needle.as_str()) {
                score += if needle.as_str() == query.to_lowercase() { 3.0 } else { 1.0 };
                from += pos + needle.len();
                if from >= lower.len() {
                    break;
                }
            }
        }
        if score <= 0.0 {
            continue;
        }
        // 长消息不因为长而占便宜：按「每千字命中数」归一
        let scale = (content.chars().count() as f32 / 1000.0).max(0.35);
        out.push(Scored {
            id: message
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            role: message_role.to_string(),
            time: message.get("time").and_then(Value::as_str).unwrap_or("").to_string(),
            snippet: excerpt(content, &needles),
            score: score / scale,
        });
    }

    out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    out.truncate(limit);
    Ok(out)
}

/// tools::search_history 的实现：模型自己翻旧账
pub fn search_tool(args: &Value, ctx: &crate::tools::ToolCtx) -> Result<String, String> {
    let query = args.get("query").and_then(Value::as_str).unwrap_or("").trim();
    if query.is_empty() {
        return Err("query 不能为空".into());
    }
    let role = args.get("role").and_then(Value::as_str).unwrap_or("");
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(6)
        .clamp(1, 20);

    let Some(root) = ctx.root else {
        return Err("当前环境读不到会话历史".into());
    };
    let conn = crate::db::open(&root.join("app.db"))?;
    let hits = search(&conn, ctx.person_id, query, Some(role), limit)?;
    if hits.is_empty() {
        return Ok(json!({
            "hits": [],
            "note": "这段对话里没有找到相关内容。别硬编：没聊过的事就照实说没聊过。"
        })
        .to_string());
    }
    Ok(json!({
        "hits": hits.iter().map(|hit| json!({
            "id": hit.id,
            "role": hit.role,
            "time": hit.time,
            "excerpt": hit.snippet,
        })).collect::<Vec<_>>(),
        "note": "excerpt 是原话摘录，可以据此引用；时间与角色用来判断是哪一轮说的。"
    })
    .to_string())
}

/// 面板/调试用：前端也可以直接翻历史（目前没接界面，留着给以后的「会话搜索」）
#[tauri::command]
pub fn history_search(
    state: State<'_, Shared>,
    key: String,
    query: String,
    role: Option<String>,
    limit: Option<usize>,
) -> Result<Value, String> {
    let role = role.unwrap_or_default();
    let limit = limit.unwrap_or(6).clamp(1, 50);
    state.with(|conn| {
        let hits = search(conn, &key, &query, Some(role.as_str()), limit)?;
        Ok(json!({
            "hits": hits.iter().map(|hit| json!({
                "id": hit.id,
                "role": hit.role,
                "time": hit.time,
                "excerpt": hit.snippet,
                "score": hit.score,
            })).collect::<Vec<_>>()
        }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excerpt_is_centred_on_the_hit() {
        let long = format!("{}QPS 3000{}", "前置内容。".repeat(40), "后面还有。".repeat(40));
        let text = excerpt(&long, &["qps".to_string()]);
        assert!(text.contains("QPS 3000"));
        assert!(text.starts_with('…'));
        assert!(text.ends_with('…'));
    }

    #[test]
    fn excerpt_handles_missing_needle() {
        let text = excerpt("一句很短的话", &["不存在".to_string()]);
        assert_eq!(text, "一句很短的话");
    }

    #[test]
    fn char_boundaries_never_split_hanzi() {
        let text = "中文内容测试";
        let index = floor_char_boundary(text, 4); // 落在「内」中间
        assert!(text.is_char_boundary(index));
        let index = ceil_char_boundary(text, 4);
        assert!(text.is_char_boundary(index));
    }
}
