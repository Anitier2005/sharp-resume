//! 会话与上下文：真相源在 Rust。
//!
//! 为什么把会话搬到 Rust：
//!   1. **内存**：长对话如果整份留在 WebView 里（React state + JSON），内存只涨不落。
//!      这里按 token 预算裁剪，前端只拿到「要显示的那一段」。
//!   2. **一致性**：上下文窗口、裁剪策略、system 模板都在一处，换模型不用改前端。
//!   3. **可恢复**：消息落盘在 SharpResumeData/sessions.json，崩溃不丢。
//!
//! 裁剪策略（务实版）：
//!   - system 模板与材料永远保留
//!   - 历史从最新往回留，直到用掉约 60% 的上下文窗口（给回复和输出留余量）
//!   - 至少保留最近一条 user 消息，否则模型会答非所问

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::ipc::Channel;

use crate::net::{AiRequest, ChatMessage, StreamChunk};

/// system 模板随包硬编码：改 `prompts/system.md` 重新打包即可生效
const SYSTEM_TEMPLATE: &str = include_str!("../../prompts/system.md");

/// 前端最多拿到多少条消息用于渲染（再往上滚就不给了，避免 DOM 无限长）
const DISPLAY_LIMIT: usize = 80;

#[derive(Default)]
pub struct Sessions {
    /// key = personId（会话按人隔离；`layout:` 前缀是页排版 agent 自己的会话）
    store: Mutex<HashMap<String, Vec<Value>>>,
    /// 正在跑的请求：key → 取消标记
    running: Mutex<HashMap<String, Arc<AtomicBool>>>,
    /// 旧版 sessions.json：只在首次启动时读一次，导进数据库后改名归档
    file: OnceLock<PathBuf>,
    /// 导入只做一次
    booted: OnceLock<()>,
}

impl Sessions {
    fn path(&self, app: &tauri::AppHandle) -> PathBuf {
        self.file
            .get_or_init(|| {
                // 与 workspace 同目录：便携版整目录拷走时对话跟着走
                crate::data_dir(app).unwrap_or_else(|_| PathBuf::from("."))
                    .join("sessions.json")
            })
            .clone()
    }

    /// 每次操作开一次连接。
    ///
    /// 会话是「一条消息写一次」，频率很低；反过来把连接长期挂在 Sessions 里，
    /// 导出、资产池那些命令就得多一条路径去拿它，得不偿失。
    fn with_db<T>(
        &self,
        app: &tauri::AppHandle,
        f: impl FnOnce(&rusqlite::Connection) -> Result<T, String>,
    ) -> Result<T, String> {
        let path = crate::data_dir(app)?.join("app.db");
        let conn = crate::db::open(&path)?;
        f(&conn)
    }

    /// 把旧的 sessions.json 导进 chat 表，成功就改名归档。
    ///
    /// 必须归档：不改名的话每次启动都会把旧消息再插一遍，用户删掉的对话会复活。
    fn bootstrap(&self, app: &tauri::AppHandle) {
        self.booted.get_or_init(|| {
            let path = self.path(app);
            if !path.exists() {
                return;
            }
            let Ok(raw) = std::fs::read_to_string(&path) else {
                return;
            };
            // 文件坏了就留在原地：用户还能自己捞，写库反而可能盖掉能用的数据
            let saved = self.with_db(app, |conn| import_legacy(conn, &raw));
            if saved.is_ok() {
                let _ = std::fs::rename(&path, path.with_file_name("sessions.json.imported"));
            }
        });
    }

    /// 按 key 懒加载：一个 scope 只从库里读一次
    fn ensure_loaded(&self, app: &tauri::AppHandle, key: &str) {
        self.bootstrap(app);
        if self.store.lock().map(|s| s.contains_key(key)).unwrap_or(false) {
            return;
        }
        let loaded = self
            .with_db(app, |conn| crate::db::chat_load(conn, key))
            .unwrap_or_default();
        if let Ok(mut store) = self.store.lock() {
            store.entry(key.to_string()).or_insert(loaded);
        }
    }

    fn list(&self, key: &str) -> Vec<Value> {
        self.store
            .lock()
            .ok()
            .and_then(|store| store.get(key).cloned())
            .unwrap_or_default()
    }

    /// 落一条消息：内存 + 库各一份。
    ///
    /// 库写失败只影响「重启后还在不在」，当前这轮对话照常继续，
    /// 所以不让它冒泡成错误 —— 用户正在等回复，不该因为磁盘问题卡住。
    fn push(&self, app: &tauri::AppHandle, key: &str, message: Value) {
        if let Ok(mut store) = self.store.lock() {
            store.entry(key.to_string()).or_default().push(message.clone());
        }
        let _ = self.with_db(app, |conn| crate::db::chat_append(conn, key, &message));
    }

    fn patch(&self, app: &tauri::AppHandle, key: &str, id: &str, patch: &Value) {
        if let Ok(mut store) = self.store.lock() {
            if let Some(list) = store.get_mut(key) {
                for message in list.iter_mut() {
                    if message.get("id").and_then(Value::as_str) == Some(id) {
                        if let (Some(target), Some(source)) = (message.as_object_mut(), patch.as_object()) {
                            for (field, value) in source {
                                target.insert(field.clone(), value.clone());
                            }
                        }
                        break;
                    }
                }
            }
        }
        let _ = self.with_db(app, |conn| crate::db::chat_patch(conn, key, id, patch));
    }

    fn clear(&self, app: &tauri::AppHandle, key: &str) {
        if let Ok(mut store) = self.store.lock() {
            store.remove(key);
        }
        let _ = self.with_db(app, |conn| crate::db::chat_clear(conn, key));
    }

    /// 页排版 agent 复用同一份存储，key 带 `layout:` 前缀隔离。
    /// 不单开一张表：会话本来就该「随工作台一起走」，分两处反而容易漏备份。
    pub(crate) fn history_of(&self, app: &tauri::AppHandle, key: &str) -> Vec<Value> {
        self.ensure_loaded(app, key);
        self.list(key)
    }

    pub(crate) fn append(&self, app: &tauri::AppHandle, key: &str, message: Value) {
        self.ensure_loaded(app, key);
        self.push(app, key, message);
    }
}

/// 把 `sessions.json` 的内容搬进 chat 表。
///
/// 抽成独立函数是为了能测：它只认「连接 + 一段 JSON」，不碰 AppHandle。
/// 幂等：库里已经有消息的 scope 直接跳过，所以重复调用不会让删掉的对话复活。
fn import_legacy(conn: &rusqlite::Connection, raw: &str) -> Result<(), String> {
    let map: HashMap<String, Vec<Value>> =
        serde_json::from_str(raw).map_err(|error| format!("旧会话文件解析失败：{error}"))?;
    for (key, messages) in &map {
        if crate::db::chat_len(conn, key).unwrap_or(0) > 0 {
            continue;
        }
        for message in messages {
            crate::db::chat_append(conn, key, message)?;
        }
    }
    Ok(())
}

/* ---------- 上下文裁剪 ---------- */

/// 粗略 token 估算：CJK 按 1 字 ≈ 1 token，其余按 4 字符 ≈ 1 token。
/// 不引 tiktoken —— 这里只需要"别超窗口"，不需要精确计费。
pub(crate) fn estimate_tokens(text: &str) -> usize {
    let mut cjk = 0usize;
    let mut other = 0usize;
    for ch in text.chars() {
        if ('\u{4e00}'..='\u{9fff}').contains(&ch) || ('\u{3000}'..='\u{303f}').contains(&ch) {
            cjk += 1;
        } else {
            other += 1;
        }
    }
    cjk + other.div_ceil(4)
}

/// 从最新往回取，直到占满预算
pub(crate) fn trim_history(history: &[Value], budget: usize) -> Vec<Value> {
    let mut used = 0usize;
    let mut picked: Vec<Value> = Vec::new();
    for message in history.iter().rev() {
        let content = message.get("content").and_then(Value::as_str).unwrap_or("");
        let cost = estimate_tokens(content);
        // 第一条（最新的）无论多大都留，否则会答非所问
        if !picked.is_empty() && used + cost > budget {
            break;
        }
        used += cost;
        picked.push(message.clone());
    }
    picked.reverse();
    picked
}

/* ---------- 命令 ---------- */

/// 前端进来先拉一次当前会话（只给要显示的一段）
#[tauri::command]
pub fn chat_history(app: tauri::AppHandle, state: tauri::State<'_, Sessions>, key: String) -> Vec<Value> {
    state.ensure_loaded(&app, &key);
    let all = state.list(&key);
    let start = all.len().saturating_sub(DISPLAY_LIMIT);
    all[start..].to_vec()
}

/// 追加一条消息（前端负责构造完整结构：id/role/time/attachments…）
#[tauri::command]
pub fn chat_push(app: tauri::AppHandle, state: tauri::State<'_, Sessions>, key: String, message: Value) {
    state.ensure_loaded(&app, &key);
    state.push(&app, &key, message);
}

/// 合并字段（流式结束后补 content / tool / changes / actions）
#[tauri::command]
pub fn chat_patch(
    app: tauri::AppHandle,
    state: tauri::State<'_, Sessions>,
    key: String,
    id: String,
    patch: Value,
) {
    state.ensure_loaded(&app, &key);
    state.patch(&app, &key, &id, &patch);
}

/// 清空某个人的会话
#[tauri::command]
pub fn chat_clear(app: tauri::AppHandle, state: tauri::State<'_, Sessions>, key: String) {
    state.clear(&app, &key);
}

/// 中断正在跑的请求：置标记，流循环下一块就退出，连接随之关闭
#[tauri::command]
pub fn chat_cancel(state: tauri::State<'_, Sessions>, key: String) {
    if let Ok(running) = state.running.lock() {
        if let Some(flag) = running.get(&key) {
            flag.store(true, Ordering::SeqCst);
        }
    }
}

/// chat_send 的返回：消息 id + 本轮产出的界面指令。
///
/// 指令不在这里直接应用，是因为只有前端知道 reducer 怎么落。
/// 顺序由 Rust 保证（先切页、再改视图、最后调外观），前端照着跑就行。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendResult {
    pub id: String,
    pub actions: Vec<Value>,
    /// 本轮工具新产出的材料，前端要并进 state
    pub assets: Vec<crate::model::Asset>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendArgs {
    pub key: String,
    /// 材料部分（由前端按当前页组装），模板由 Rust 拼
    pub context: String,
    /// 技能库附加段（`prompts/skills/*.md`，构建时由前端 glob 进来）
    #[serde(default)]
    pub extra: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub temperature: Option<f64>,
    pub reasoning: Option<String>,
    pub context_window: Option<usize>,
    /// 当前简历 / 页面：每轮都会写进定位头，工具也靠它取默认页
    #[serde(default)]
    pub resume_id: String,
    #[serde(default)]
    pub page_id: String,
    #[serde(default)]
    pub view: Option<String>,
    /// 工作区快照：工具层的只读镜像。前端每次发送时推一份，不改变数据归属。
    #[serde(default)]
    pub workspace: Value,
}

/// 发一轮对话：Rust 组上下文 → 流式回推 → 把 assistant 消息写进历史
#[tauri::command]
pub async fn chat_send(
    app: tauri::AppHandle,
    state: tauri::State<'_, Sessions>,
    args: SendArgs,
    on_event: Channel<StreamChunk>,
) -> Result<SendResult, String> {
    state.ensure_loaded(&app, &args.key);

    let flag = Arc::new(AtomicBool::new(false));
    if let Ok(mut running) = state.running.lock() {
        running.insert(args.key.clone(), flag.clone());
    }

    // 组装：模板 + 材料 + 定位头，历史按预算裁剪
    let mut system = SYSTEM_TEMPLATE.replace("{{context}}", &args.context);
    system.push_str("\n\n");
    system.push_str(&crate::agent::context::locate(
        &args.workspace,
        &args.key,
        &args.resume_id,
        &args.page_id,
        args.view.as_deref().unwrap_or("edit"),
    ));
    if !args.extra.trim().is_empty() {
        system.push_str("\n\n");
        system.push_str(args.extra.trim());
    }
    let window = args.context_window.unwrap_or(64_000);
    let budget = window.saturating_sub(estimate_tokens(&system) + 2_000) * 6 / 10;
    let history = trim_history(&state.list(&args.key), budget);

    let mut messages = vec![ChatMessage::system(system)];
    for message in &history {
        let role = message.get("role").and_then(Value::as_str).unwrap_or("user");
        let content = message.get("content").and_then(Value::as_str).unwrap_or("");
        if content.trim().is_empty() {
            continue;
        }
        messages.push(if role == "assistant" {
            ChatMessage::assistant(content)
        } else {
            ChatMessage::user(content)
        });
    }

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

    // 工具出站：界面指令与新产出的材料，本轮结束后随返回值交给前端
    let out = crate::tools::Outbox::default();
    // 数据目录给图片转化这类要碰文件的工具用；拿不到就传 None，
    // 工具会明确告诉模型「当前环境不支持」，而不是静默失败
    let root = crate::data_dir(&app).ok();
    let ctx = crate::tools::ToolCtx {
        workspace: &args.workspace,
        person_id: &args.key,
        resume_id: &args.resume_id,
        page_id: &args.page_id,
        out: &out,
        root: root.as_deref(),
    };

    let reply_id = format!("m-{}", uuid_like());
    // 会话这边有前端在等：把闭包转成通道推送，前端才有打字机效果
    let result = crate::agent::turn::run(
        &template,
        messages,
        &ctx,
        &flag,
        |chunk| {
            let _ = on_event.send(chunk);
        },
        crate::tools::Scope::Session,
    )
    .await;

    if let Ok(mut running) = state.running.lock() {
        running.remove(&args.key);
    }

    match result {
        Ok(turn) if !flag.load(Ordering::SeqCst) => {
            // 落一条 assistant 消息；content 由前端随后 patch（它还要解析改写块）
            state.push(
                &app,
                &args.key,
                json!({
                    "id": reply_id,
                    "role": "assistant",
                    "content": turn.content,
                    "time": chrono_like(),
                    "tool": if turn.used.is_empty() {
                        format!("{} · 已完成", args.model)
                    } else {
                        format!("{} · 用了 {}", args.model, turn.used.join(" / "))
                    },
                }),
            );
            Ok(SendResult {
                id: reply_id,
                actions: turn.actions,
                assets: turn.assets,
            })
        }
        Ok(_) | Err(_) if flag.load(Ordering::SeqCst) => Err("已中断".into()),
        Err(error) => Err(error),
        Ok(_) => Err("没有拿到回复".into()),
    }
}

/// 极简 id：时间戳 + 计数，够用且不引 uuid 依赖
pub(crate) fn uuid_like() -> String {
    use std::sync::atomic::AtomicU64;
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{}-{}", chrono_like(), seq)
}

pub(crate) fn chrono_like() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn tokens_count_cjk_and_latin() {
        assert_eq!(estimate_tokens("你好"), 2);
        assert_eq!(estimate_tokens("abcd"), 1);
        // 中文按字、英文按 4 字符
        assert!(estimate_tokens("你好 world") >= 3);
    }

    #[test]
    fn trim_keeps_latest_and_respects_budget() {
        let history: Vec<Value> = (0..10)
            .map(|_| json!({ "role": "user", "content": "x".repeat(40) }))
            .collect();
        // 每条约 10 token，预算 25 → 只留最后两条
        let kept = trim_history(&history, 25);
        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn trim_always_keeps_one() {
        let history = vec![json!({ "role": "user", "content": "y".repeat(4000) })];
        assert_eq!(trim_history(&history, 10).len(), 1);
    }
    fn temp_db(tag: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir()
            .join("sharp-resume-session-test")
            .join(format!("{tag}-{stamp}"));
        std::fs::create_dir_all(&dir).expect("建临时目录");
        dir.join("app.db")
    }

    #[test]
    fn imports_legacy_json_once_and_is_idempotent() {
        let path = temp_db("import");
        let raw = r#"{
            "p1": [{ "id": "m1", "role": "user", "content": "你好" }],
            "layout:r1:pg1": [{ "id": "m2", "role": "user", "content": "排版这一页" }]
        }"#;

        {
            let conn = crate::db::open(&path).expect("建库");
            import_legacy(&conn, raw).expect("第一次导入");
            assert_eq!(crate::db::chat_len(&conn, "p1").unwrap(), 1);
            assert_eq!(crate::db::chat_len(&conn, "layout:r1:pg1").unwrap(), 1);
        }
        {
            let conn = crate::db::open(&path).expect("再开一次库");
            import_legacy(&conn, raw).expect("第二次导入");
            // 幂等：不能插重复，否则用户删掉的对话会复活
            assert_eq!(crate::db::chat_len(&conn, "p1").unwrap(), 1);
            let list = crate::db::chat_load(&conn, "p1").unwrap();
            assert_eq!(list[0].get("content").and_then(Value::as_str), Some("你好"));
        }

        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn broken_legacy_file_reports_error_instead_of_wiping() {
        let path = temp_db("broken");
        let conn = crate::db::open(&path).expect("建库");
        assert!(import_legacy(&conn, "{ not json").is_err(), "坏文件要报错");
        assert_eq!(crate::db::chat_len(&conn, "p1").unwrap(), 0);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }
}
