//! 会话与上下文：真相源在 Rust。
//!
//! 为什么把会话搬到 Rust：
//!   1. **内存**：长对话如果整份留在 WebView 里（React state + JSON），内存只涨不落。
//!      这里只在需要时把消息投影给前端，前端不持有全量副本。
//!   2. **一致性**：上下文窗口、裁剪策略、system 模板都在一处，换模型不用改前端。
//!   3. **可恢复**：消息落盘在 SharpResumeData/sessions.json，崩溃不丢。
//!
//! 两套窗口，别混在一起：
//!   - **模型侧**（`chat_send`）：按 token 预算从最新往回裁，用掉约 60% 的上下文窗口
//!     给回复留余量；被裁掉的那段压成摘要接在 system 后面。至少留最近一条 user 消息，
//!     否则模型会答非所问。
//!   - **界面侧**（`chat_history`）：全量返回。用户要能翻到自己说过的话，
//!     压缩只体现为一条「压缩点」分隔线（`digest`），提示此处之前已压成摘要。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::ipc::Channel;

use crate::net::{AiRequest, ChatMessage, StreamChunk};

/// system 模板随包硬编码：改 `prompts/system.md` 重新打包即可生效
const SYSTEM_TEMPLATE: &str = include_str!("../../prompts/system.md");

/// 用户没在模型配置里填「压缩预留」时的默认值：够放一轮摘要提示词 + 被摘要的那几段
pub(crate) const DEFAULT_RESERVE: usize = 2_000;
/// 历史只占窗口的一部分，剩下的留给模型输出（prompt + completion 共享同一个窗口）
const HISTORY_SHARE: usize = 60;

/// 被挤出上下文的历史至少这么多 token 才值得生成一次摘要 ——
/// 少了就生成，等于每轮都多一次模型调用，换来的信息却很有限。
const DIGEST_TRIGGER: usize = 1_200;

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

/// 历史预算：窗口减去预留给压缩提示词与 system 的那部分，再按 `HISTORY_SHARE` 折算。
///
/// 只给模型侧用（会话与页排版各一处）：界面显示全量历史，不再拿它裁渲染。
pub(crate) fn history_budget(window: usize, reserved: usize) -> usize {
    window.saturating_sub(reserved) * HISTORY_SHARE / 100
}

/// 把被挤出窗口的历史压成要点。
///
/// 为什么不是「直接丢掉」：聊到第 30 轮时，第 3 轮定下的口径（「QPS 只写 3000」）
/// 会被静默丢弃，模型随后就可能给出前后矛盾的稿子。摘要保住主线；
/// 精确细节由 `search_history` 工具按需回原文捞。
///
/// 失败一律静默（返回 None）：摘要只是优化，不能因为它挂了就让这一轮对话发不出去。
async fn digest_for_dropped(
    app: &tauri::AppHandle,
    state: &Sessions,
    args: &SendArgs,
    dropped: &[Value],
) -> Option<String> {
    let existing = state
        .with_db(app, |conn| crate::db::chat_digest_get(conn, &args.key))
        .ok()
        .flatten();

    if dropped.is_empty() {
        return existing.map(|(_, text)| text).filter(|text| !text.trim().is_empty());
    }

    let fresh_tokens: usize = dropped
        .iter()
        .map(|message| estimate_tokens(message.get("content").and_then(Value::as_str).unwrap_or("")))
        .sum();
    if existing.is_some() && fresh_tokens < 400 {
        // 只多挤出来一点点：沿用上一份摘要，别为了两句话再花一次调用
        return existing.map(|(_, text)| text);
    }
    if fresh_tokens < DIGEST_TRIGGER && existing.is_none() {
        // 第一次触发也要求够长，否则短会话会被无谓地摘要
        return None;
    }

    let mut transcript = String::new();
    if let Some((_, text)) = &existing {
        transcript.push_str("（以下是更早的摘要）\n");
        transcript.push_str(text.trim());
        transcript.push_str("\n\n（以下是新增的对话）\n");
    }
    for message in dropped {
        if message.get("internal").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        let role = message.get("role").and_then(Value::as_str).unwrap_or("user");
        let content = message.get("content").and_then(Value::as_str).unwrap_or("").trim();
        if content.is_empty() {
            continue;
        }
        let speaker = if role == "assistant" { "助手" } else { "用户" };
        // 单条截断：一轮回答可能上千字，全文塞进去等于没省
        transcript.push_str(&format!("{speaker}：{}\n", snippet_chars(content, 700)));
    }

    // 摘要用「压缩」用途的模型；没配就沿用本轮主模型
    let (base_url, api_key, model) = match &args.compact {
        Some(conf) if !conf.model.trim().is_empty() => {
            (conf.base_url.clone(), conf.api_key.clone(), conf.model.clone())
        }
        _ => (args.base_url.clone(), args.api_key.clone(), args.model.clone()),
    };
    let request = AiRequest {
        base_url,
        api_key,
        model,
        messages: vec![
            ChatMessage::system(
                "把下面的对话压成一份要点，供后续继续对话时当背景。\
                 必须保留：用户定下的口径与偏好、具体数字与时间、已确认的结论、还没做完的事。\
                 不要写客套话，不要复述过程，不要评价。用第三人称陈述，不超过 400 字。",
            ),
            ChatMessage::user(transcript),
        ],
        temperature: Some(0.1),
        reasoning: None,
        max_tokens: Some(700),
        tools: None,
    };
    let text = crate::net::chat_once(request).await.ok()?;
    let text = text.trim().to_string();
    if text.is_empty() {
        return None;
    }

    // upto = 最后一条被摘要的消息：同一条不重复摘要
    let upto = dropped
        .last()
        .and_then(|message| message.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if !upto.is_empty() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let _ = state.with_db(app, |conn| {
            crate::db::chat_digest_set(conn, &args.key, &upto, &text, stamp)
        });
    }
    Some(text)
}

/// 按字符截断（不切坏汉字），用于摘要输入
fn snippet_chars(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        out.push('…');
    }
    out
}

/* ---------- 命令 ---------- */

/// 会话投影：该会话的全部消息 + 压缩点。
///
/// 折叠那套已经反转：用户要看完整历史，压缩只在列表里留一条标记，
/// 让人一眼看出「这条之前的对话，模型手里只剩摘要」。
#[derive(Serialize)]
pub struct HistorySlice {
    messages: Vec<Value>,
    /// 压缩点：`upto` 是最后一条被压进摘要的消息 id，前端据此在这条后面画分隔线
    digest: Option<DigestMark>,
}

#[derive(Serialize)]
struct DigestMark {
    upto: String,
    text: String,
}

/// 前端进来先拉一次当前会话，全量返回。
///
/// 不再接窗口参数：显示范围不该随模型配置变化 —— 用户翻不到自己说过的话，
/// 会觉得对话被吞了。窗口只约束模型侧（见 `chat_send`）。
#[tauri::command]
pub fn chat_history(app: tauri::AppHandle, state: tauri::State<'_, Sessions>, key: String) -> HistorySlice {
    state.ensure_loaded(&app, &key);
    // 摘要读失败当作没有：显示历史比显示摘要重要，不能因为读它的库出问题就白屏
    let digest = state
        .with_db(&app, |conn| crate::db::chat_digest_get(conn, &key))
        .ok()
        .flatten()
        .filter(|(_, text)| !text.trim().is_empty())
        .map(|(upto, text)| DigestMark { upto, text });
    HistorySlice {
        messages: state.list(&key),
        digest,
    }
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
    /// 本轮原件的页数。前端存进内存，随下一次 `layout_submit` 传回来 ——
    /// 排版侧要靠它决定「沿用原件页数」；刷新即失效，不落库
    pub origin: Option<crate::agent::facts::Origin>,
    /// 本轮原件的**样张**（第 1 页页图，已在材料池里）。前端并进 `resume.originSheet`：
    /// 它挂在简历上而不是页上，排版 agent 每轮都能重新 `view` 一次对照版式。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_sheet: Option<crate::model::Asset>,
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
    /// 「压缩」用途的模型（设置里单独配）。不给就沿用本轮主模型。
    #[serde(default)]
    pub compact: Option<CompactConfig>,
    /// 预留给压缩提示词的 token：从窗口里先扣掉它，再算能放多少历史。
    /// 用户可在模型配置里调，缺省 `DEFAULT_RESERVE`。
    #[serde(default)]
    pub reserve: Option<usize>,
    /// 本轮附件（inbox 里的 stored 名 + 原名）。
    /// 正文、OCR、页面渲染都在后端做，前端只负责说「哪几个附件」。
    #[serde(default)]
    pub attachments: Vec<crate::attach::AttachRef>,
    /// 设置里配的 OCR 接口。缺省表示只有内置 OCR（lite 构建则等于没有）
    #[serde(default)]
    pub ocr: Option<crate::attach::OcrConf>,
    /// 前端量的页面实测值。只在本次请求里有效 —— 落库的测量值下次启动会变成假数据。
    #[serde(default)]
    pub measure: Option<crate::agent::facts::PageMeasure>,
}

/// 压缩历史摘要用的接入参数
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
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

    // 这个模型能不能看图：唯一来源是 capability::resolve（设置里那份实测结论），
    // 不再由前端传一个「用户勾的开关」上来。三处用到它 —— 材料组装、工具出站、
    // 【模型能力】事实块 —— 都读这同一个值，不许各判各的。
    let mut capability = crate::capability::resolve(&args.base_url, &args.model, &args.api_key);
    // 收图前的一次性探针：结论是「没测出来」而这一轮又带了附件，就当场测一次再决定。
    //
    // 为什么必须自动测：三态里 `unknown` 不发图（宁可少发也不要换一个 400），
    // 于是「用户没去设置里点那个按钮」就等于「能看图的模型永远只收到元数据」——
    // 那正是这条链路坏了几个版本的症状。测一次的成本是这一轮多一两秒，
    // 收益是一座已经建好的桥真的通了。结论落库，同一组 baseUrl+模型+key 只测这一次。
    if crate::capability::should_auto_probe(&capability)
        && !args.attachments.is_empty()
        && !args.api_key.trim().is_empty()
    {
        capability = crate::capability::probe(&args.base_url, &args.model, &args.api_key).await;
    }
    let vision = capability.can_see();

    // 附件解析全在这里：取字、OCR、PDF 渲染都是 Rust 的活，
    // 前端只传「哪几个附件」，拿不到字节也拿不到正文。
    // 正文一解析出来就进 Fact 层（source 表）—— 附件按轮一次性，错过就没第二次
    let root = crate::data_dir(&app).ok();
    let db = tauri::Manager::state::<crate::db::Shared>(&app);
    let attached = match (&root, args.attachments.is_empty()) {
        (Some(dir), false) => {
            crate::attach::materials(
                dir,
                &args.attachments,
                vision,
                args.ocr.as_ref(),
                &args.model,
                db.inner(),
            )
            .await
        }
        _ => crate::attach::Materials {
            section: String::new(),
            images: Vec::new(),
            origin: None,
            origin_sheet: None,
        },
    };
    let attach_section = attached.section;
    let attach_images = attached.images;
    // 原件页数：随 SendResult 回传前端，下次排版提交时再带回来（不落库）
    let origin = attached.origin;
    // 原件样张：前端并进 resume.originSheet 落库，排版轮靠它对照版式
    let origin_sheet = attached.origin_sheet;
    let mut materials = args.context.clone();
    if !attach_section.is_empty() {
        materials.push_str("\n\n");
        materials.push_str(&attach_section);
    }

    // 历史在这里读一次：既用来数「第几条用户消息」（目标页数三态要用），
    // 也用来按预算裁剪。测量值只认本次请求带来的那份，历史里没有它的位置
    let all = state.list(&args.key);
    let turn = all
        .iter()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("user"))
        .count() as u32;
    let facts = crate::agent::facts::PageFacts::assemble(
        &args.workspace,
        &args.key,
        &args.resume_id,
        &args.page_id,
        args.measure.clone(),
        origin.clone(),
        Some(turn),
    );

    // 组装：模板 + 材料 + 定位头，历史按预算裁剪
    let mut system = SYSTEM_TEMPLATE.replace("{{context}}", &materials);
    system.push_str("\n\n");
    system.push_str(&crate::agent::context::locate(
        &args.workspace,
        &args.key,
        &args.resume_id,
        &args.page_id,
        args.view.as_deref().unwrap_or("edit"),
        &facts,
        &capability,
    ));
    if !args.extra.trim().is_empty() {
        system.push_str("\n\n");
        system.push_str(args.extra.trim());
    }
    let window = args.context_window.unwrap_or(64_000);
    // 窗口 = 配置里的上下文；先扣 system 与「压缩提示词预留」，剩下的才是历史的
    let reserved = estimate_tokens(&system) + args.reserve.unwrap_or(DEFAULT_RESERVE);
    let budget = history_budget(window, reserved);
    let history = trim_history(&all, budget);

    // 被挤出窗口的那一段：压成摘要接在 system 后面（失败就当作没有）
    let dropped = &all[..all.len().saturating_sub(history.len())];
    if let Some(digest) = digest_for_dropped(&app, &state, &args, dropped).await {
        system.push_str("\n\n【更早对话的摘要】\n");
        system.push_str(&digest);
        system.push_str("\n（需要原话或其中的具体数字时，用 search_history 工具检索旧消息）");
    }

    // 这一轮实际发出去的 system 落一份：终止判定出错时，第一件要查的就是
    // 「模型当时到底看见了什么」。路径留着，这一轮跑完还要在上面补一行思考字数。
    let context_log = root
        .as_ref()
        .and_then(|dir| write_context_log(dir, &args.key, &system));

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

    if !attach_images.is_empty() {
        if let Some(last) = messages.iter_mut().rev().find(|item| item.role == "user") {
            *last = ChatMessage::user_with_images(last.text(), &attach_images);
        }
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
    let mut out = crate::tools::Outbox::default();
    // 模型能不能看图：决定工具渲染出来的页面图片要不要真的发过去。
    // 派生自上面那一次 resolve —— 真相只有一个。
    out.vision = vision;
    out.model = args.model.clone();
    // 工具写目标页数时要落「第几条用户消息」：下一轮据此判「这一轮刚要求」还是「以前说过」
    out.turn = turn;
    // 数据目录给图片转化这类要碰文件的工具用；拿不到就传 None，
    // 工具会明确告诉模型「当前环境不支持」，而不是静默失败
    let ctx = crate::tools::ToolCtx {
        workspace: &args.workspace,
        person_id: &args.key,
        resume_id: &args.resume_id,
        page_id: &args.page_id,
        out: &out,
        root: root.as_deref(),
        facts: Some(&facts),
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

    // 思考链只落日志：它不进消息、不进上下文，但白卷现场没它就没法解释
    // ——「想了 1200 字却一个字没说」和「什么都没想」是两种完全不同的故障。
    // 写失败同样只是一个 `let _ =`：日志不能反过来把这一轮搞失败。
    //
    // **为什么是追加而不是写进同一行**：这个文件记的是「这一轮模型将看到什么」，
    // 必须在请求发出前就落地（请求失败也要留下证据）；而思考字数只有请求结束
    // 才知道。两者的时间点一个在前一个在后，合并成一行只能靠事后重写整个文件，
    // 中途崩溃就会留下半个假文件。下一个想「顺手合并」的人请先看这段。
    if let (Some(path), Ok(turn)) = (context_log.as_ref(), &result) {
        use std::io::Write;
        let _ = std::fs::OpenOptions::new()
            .append(true)
            .open(path)
            .and_then(|mut file| write!(file, "\n\nreasoning={} 字\n", turn.reasoning_chars));
    }

    match result {
        Ok(turn) if !flag.load(Ordering::SeqCst) => {
            // 落一条 assistant 消息；content 由前端随后 patch（它还要解析改写块）
            let mut message = json!({
                "id": reply_id,
                "role": "assistant",
                "content": turn.content,
                "time": chrono_like(),
                "tool": if turn.used.is_empty() {
                    format!("{} · 已完成", args.model)
                } else {
                    format!("{} · 用了 {}", args.model, turn.used.join(" / "))
                },
            });
            // 没正常收尾就把原因挂在消息上：content 保持原样（半句旁白也是它说的），
            // 诊断单独一条，且不参与模型上下文 —— 下一轮它不该读到自己的病历
            if let Some(kind) = turn.unfinished {
                message["note"] = json!(kind.text(&turn.used));
            }
            state.push(&app, &args.key, message);
            Ok(SendResult {
                id: reply_id,
                actions: turn.actions,
                assets: turn.assets,
                origin,
                origin_sheet,
            })
        }
        Ok(_) | Err(_) if flag.load(Ordering::SeqCst) => Err("已中断".into()),
        Err(error) => Err(error),
        Ok(_) => Err("没有拿到回复".into()),
    }
}

/// 把这一轮实际发给模型的 system 落到 `<数据目录>/logs/context/{unix秒}-{personId}.md`。
///
/// 放 `logs/` 而不是 `pool/`：后者是用户材料，混进去会被当成资产扫出来。
/// 任何一步失败都返回 `None` —— 观测不能反过来把这一轮搞失败。
fn write_context_log(
    root: &std::path::Path,
    person_id: &str,
    system: &str,
) -> Option<std::path::PathBuf> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // personId 是前端传进来的外部串。进文件名前把非 `[A-Za-z0-9_-]` 全换成 `_`，
    // 防的是**路径穿越**（`../`、绝对路径、Windows 保留名），不是审美：
    // 日志文件名由我们决定，不该有一格是用户数据说了算。
    let person: String = person_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let dir = root.join("logs").join("context");
    let path = dir.join(format!("{stamp}-{person}.md"));
    std::fs::create_dir_all(&dir).ok()?;
    std::fs::write(&path, system).ok()?;
    Some(path)
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

    /// 请求日志的落点必须真的存在：这个文件是白卷现场唯一的证据，
    /// 「以为写了其实没写」比不写更糟。用临时目录跑一遍真写盘。
    #[test]
    fn the_system_prompt_really_lands_in_logs_context() {
        let root = std::env::temp_dir().join(format!("sharp-ctx-{}", uuid_like()));
        // 外部传进来的 id 带路径分隔和 `..`，必须被拍平成一个文件名
        let path = write_context_log(&root, "p1/../x", "【定位】第 1 页\n正文到这里").expect("要真的写出来");

        let dir = root.join("logs").join("context");
        assert_eq!(path.parent(), Some(dir.as_path()), "{}", path.display());
        // 文件名是「unix 秒-personId.md」，`/` 与 `..` 都被拍成 `_`
        let name = path.file_name().and_then(|n| n.to_str()).unwrap();
        assert!(name.ends_with("-p1____x.md"), "{name}");
        assert!(
            name.trim_end_matches("-p1____x.md").chars().all(|c| c.is_ascii_digit()),
            "前缀必须是 unix 秒：{name}"
        );

        let text = std::fs::read_to_string(&path).expect("内容要读得回来");
        assert!(text.starts_with("【定位】第 1 页"), "{text}");
        assert!(text.contains("正文到这里"), "{text}");
        println!("context log = {}", path.display());

        // SHARP_KEEP_CTX_LOG=1 时留着现场，方便人肉看一眼真文件长什么样
        if std::env::var("SHARP_KEEP_CTX_LOG").is_err() {
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    #[test]
    fn send_args_carries_measure_and_the_result_hands_origin_back() {
        // 入参：前端把实测值随 chat_send 上送（不落库，只在本次请求里有效）
        let args: SendArgs = serde_json::from_value(json!({
            "key": "p1",
            "context": "",
            "baseUrl": "",
            "apiKey": "",
            "model": "",
            "measure": {
                "pageId": "pg1",
                "status": "ok",
                "version": 12,
                "content": "v37",
                "onePage": 1000.0,
                "contentPx": 1180.0,
                "lineHeight": 17.0
            }
        }))
        .expect("带 measure 的入参要能解析");
        let measure = args.measure.as_ref().expect("measure 要解析出来");
        assert_eq!(measure.status, "ok");
        assert_eq!(measure.version, 12);
        assert_eq!(measure.content_px, Some(1180.0));

        // 出参：原件页数回传前端，前端下次排版提交时再带回来
        let result = SendResult {
            id: "m-1".into(),
            actions: Vec::new(),
            assets: Vec::new(),
            origin: Some(crate::agent::facts::Origin {
                name: "简历.pdf".into(),
                pages: Some(3),
            }),
            origin_sheet: None,
        };
        let value = serde_json::to_value(&result).expect("SendResult 要能序列化");
        assert_eq!(value["origin"]["name"], "简历.pdf");
        assert_eq!(value["origin"]["pages"], 3);
        // 没有样张时不出这个键：前端 `if (sent.originSheet)` 才好判
        assert!(value.get("originSheet").is_none(), "{value}");
    }

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

    /// 窗口按配置算：64K 减 2K 预留、六成给历史；预留比窗口还大时归零而不是下溢
    #[test]
    fn budget_follows_the_configured_window() {
        assert_eq!(history_budget(64_000, 2_000), 62_000 * HISTORY_SHARE / 100);
        assert_eq!(history_budget(1_000, 2_000), 0);
        assert_eq!(history_budget(128_000, 8_000), 120_000 * HISTORY_SHARE / 100);
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
