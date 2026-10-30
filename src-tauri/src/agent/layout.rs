//! 页排版 agent：一个页面一条会话，只看这一页的版式，不改内容。
//!
//! 为什么单独开一个 agent 而不是让会话 agent 顺手调样式：
//!   1. 排版是「局部最优」问题——它要盯着这一页的内容量和留白反复权衡，
//!      和「用户到底想表达什么」是两件事，混在一个上下文里会互相干扰。
//!   2. 会话按候选人隔离、跟着对话走；排版要跟着**页**走。生命周期不同，
//!      用同一份会话就会互相污染。
//!   3. 权限必须收窄：会话 agent 只有 `request_layout`，真正写样式的
//!      `set_page_style` 只在这一层可用。
//!
//! **触发方式**：不是「点预览跑一次」。这一页的内容一变、或者会话 agent
//! 要求重排，都作为一条输入投进这一页的信箱（`Layouts`），由它自己串行消化。
//! 界面上的「预览」只是换个看法，不该顺手烧一次模型调用。
//!
//! 会话 key：`layout:<resumeId>:<pageId>`，与人的会话共用 app.db。
//!
//! **它不改内容**，也**不能提问**：拿不准就 `note_on_page` 在纸面上留一句给用户看，
//! 或者按现有内容保守排版。批注只是普通 action，随 `layout:applied` 的
//! `turn.actions` 一起回传，`drain` 里不用另做处理 —— 多一条专用通道就多一处会漏掉的分支。

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{Emitter, Manager};

use crate::net::{AiRequest, ChatMessage};
use crate::session::{self, Sessions};
use crate::tools::{Scope, ToolCtx};

/// 与会话 agent 共用一套「每轮注入定位」的做法：页面的形状每轮都可能变。
///
/// 职责边界（只能改版面、不能改内容、不能提问、一轮最多两条批注）写在
/// `prompts/layout.md` 里 —— 提示词归提示词，这里只负责把 `{{page}}` 换成
/// 这一页的形状。
const SYSTEM: &str = include_str!("../../../prompts/layout.md");

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutArgs {
    pub person_id: String,
    pub resume_id: String,
    pub page_id: String,
    pub workspace: Value,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub temperature: Option<f64>,
    pub reasoning: Option<String>,
    pub context_window: Option<usize>,
    /// 前端量的实测值：随本次提交带上来，用完即弃（落库会变成下次启动的假数据）
    #[serde(default)]
    pub measure: Option<crate::agent::facts::PageMeasure>,
    /// 本轮原件的页数：会话侧 `chat_send` 回传、前端存内存再带回来；刷新后就是 None
    #[serde(default)]
    pub origin: Option<crate::agent::facts::Origin>,
}

/// 投进信箱的一条输入。
///
/// 两种来源共用一条队列：内容刚变了（change）、有人明确要求重排（request）。
/// 分开两条队列的话「先改再要求」和「先要求再改」会打架，还得处理谁先谁后；
/// 合在一起按到达顺序拼成一段话，模型自己会读。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    /// change = 这一页的内容变了；request = 用户或会话 agent 提的要求
    #[serde(default)]
    pub kind: String,
    pub text: String,
}

impl Note {
    fn is_request(&self) -> bool {
        self.kind == "request"
    }
}

#[derive(Default)]
struct Slot {
    /// 还没被消化的输入，按到达顺序
    notes: Vec<Note>,
    /// 已经有消费者在排队：新输入只追加，不再起第二个
    scheduled: bool,
    /// 最近一次提交的上下文快照。轮次开始时用它 —— 排队期间内容还会变，
    /// 用最早那份就等于拿旧内容排版
    latest: Option<LayoutArgs>,
}

/// 每页一个信箱，串行消化。
///
/// 为什么需要队列而不是「每来一次就调一次模型」：
///   1. 用户连着打十个字，前端按保存节拍会挤出好几次变更；每次都调就是
///      好几个并发请求，钱照付、样式互相覆盖、会话历史还被灌满。
///   2. 中途插入是常态而不是异常：一次排版要几秒到几十秒，这期间用户
///      几乎一定会继续改。丢掉这些输入，等于排完还是错的版。
///
/// 实现上只要三样东西：**累积**（notes）、**单飞**（scheduled）、
/// **跑完再看一眼**（drain 的循环）。第二样和第三样必须互相咬住，
/// 否则就是「要么重复跑、要么漏掉最后一条」。
#[derive(Default)]
pub struct Layouts {
    slots: Mutex<HashMap<String, Slot>>,
}

impl Layouts {
    /// 投一条输入。返回 true 表示调用方需要起一个消费者。
    fn enqueue(&self, key: &str, note: Note, args: LayoutArgs) -> bool {
        let Ok(mut slots) = self.slots.lock() else {
            return false;
        };
        let slot = slots.entry(key.to_string()).or_default();
        slot.notes.push(note);
        // 快照每次都覆盖：轮次开始时该看到最新的内容
        slot.latest = Some(args);
        if slot.scheduled {
            return false;
        }
        slot.scheduled = true;
        true
    }

    /// 取走这一轮要处理的输入。返回 None 表示信箱空了，消费者可以退出。
    fn take(&self, key: &str) -> Option<(Vec<Note>, LayoutArgs)> {
        let Ok(mut slots) = self.slots.lock() else {
            return None;
        };
        let slot = slots.get_mut(key)?;
        if slot.notes.is_empty() {
            // 关排班必须和「判断为空」在同一把锁里完成。
            // 分开做的话：这边刚判完空，那边插进一条并看到 scheduled 还是 true
            // 就不再起消费者，接着这边把 scheduled 置 false —— 那一条永远没人跑。
            slot.scheduled = false;
            slot.latest = None;
            return None;
        }
        let notes = std::mem::take(&mut slot.notes);
        let Some(args) = slot.latest.clone() else {
            // 正常不会发生（enqueue 必设快照）。真发生就清空并退出，
            // 否则这一页会卡在 scheduled=true 再也不会被处理
            slot.scheduled = false;
            return None;
        };
        Some((notes, args))
    }

    fn clear(&self, key: &str) {
        if let Ok(mut slots) = self.slots.lock() {
            slots.remove(key);
        }
    }

    /// 主动让出排班，但保留还没处理的输入。
    ///
    /// 用在「连续跑了太多轮」时：用户一直在改，信箱永远不空，消费者就永远不退出。
    /// 歇一次，下一次变更会把它重新叫醒，顺便把积压的输入一起带走。
    fn release(&self, key: &str) {
        if let Ok(mut slots) = self.slots.lock() {
            if let Some(slot) = slots.get_mut(key) {
                slot.scheduled = false;
                if slot.notes.is_empty() {
                    slot.latest = None;
                }
            }
        }
    }
}

/// 把这一轮攒下的输入拼成一段人话
fn compose(notes: &[Note]) -> String {
    let mut changes: Vec<&str> = Vec::new();
    let mut requests: Vec<&str> = Vec::new();
    for note in notes {
        let text = note.text.trim();
        if text.is_empty() {
            continue;
        }
        if note.is_request() {
            requests.push(text);
        } else {
            changes.push(text);
        }
    }

    let mut out = String::new();
    if !changes.is_empty() {
        out.push_str("【这一页刚发生的改动】\n");
        out.push_str(&changes.join("\n"));
    }
    if !requests.is_empty() {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str("【要求】\n");
        out.push_str(&requests.join("\n"));
    }
    if out.is_empty() {
        out.push_str("请检查这一页的版式。");
    }
    out
}

fn text_len(value: Option<&Value>) -> usize {
    value
        .and_then(Value::as_str)
        .map(|text| text.trim().chars().count())
        .unwrap_or(0)
}

fn fields_len(fields: &[Value]) -> usize {
    fields
        .iter()
        .map(|field| text_len(field.get("label")) + text_len(field.get("value")))
        .sum()
}

/// 块的 role 写成一个短标记。没有 role 的块什么都不加 ——
/// 缺失等同于 `plain`，说「role: plain」只是噪音。
fn role_mark(block: &Value) -> String {
    block
        .get("role")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|role| !role.is_empty())
        .map(|role| format!("〔role: {role}〕"))
        .unwrap_or_default()
}

/// 把这一页的「形状」写成人话：模型据此判断挤还是空。
///
/// 必须读 `blocks`。早先这里读的是 v4 的 `header` / `sections`，
/// 模型升到 v5 之后那两个字段恒为空 —— 排版 agent 每轮看到的都是
/// 「页头 0 个字段、0 个分区」，等于闭着眼睛排版。
///
/// 另一件必须交代的事：块的 `role`。结构化生成器按 role 选容器
/// （tags / kv / table / grid-2…），排版 agent 不知道这一块已被接管成什么形态，
/// 就会在 `parts` 里再手写一遍，两套排法叠在一起。
fn describe_page(args: &LayoutArgs, facts: &crate::agent::facts::PageFacts) -> String {
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

    // 整页原件：版式在图片/PDF 里，改了也是白改
    if page.get("kind").and_then(Value::as_str) == Some("file") {
        return format!(
            "第 {index} 页 / 共 {} 页，页名「{label}」\n\
             这一页是用户上传的整页原件，版式由文件本身决定，没有可调的空间。",
            pages.len()
        );
    }

    let blocks = page
        .get("blocks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut header_fields = 0usize;
    let mut header_chars = 0usize;
    let mut header_block = String::new();
    let mut header_role = String::new();
    let mut extra_groups = 0usize;
    let mut media = 0usize;
    let mut partition_lines: Vec<String> = Vec::new();
    let mut total_entries = 0usize;
    let mut total_bullets = 0usize;
    let mut chars = 0usize;
    let mut seen_header = false;

    for block in &blocks {
        match block.get("kind").and_then(Value::as_str).unwrap_or("") {
            "fields" => {
                let fields = block
                    .get("fields")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let len = fields_len(&fields);
                chars += len;
                if seen_header {
                    extra_groups += 1;
                } else {
                    // 第一个字段块就是页头
                    seen_header = true;
                    header_fields = fields.len();
                    header_chars = len;
                    header_block = block
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    header_role = role_mark(block);
                }
            }
            "section" => {
                let title = block
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let items = block
                    .get("items")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();

                let mut bullets = 0usize;
                let mut section_chars = title.chars().count();
                for entry in &items {
                    // 条目字段为空时是继承库里的那份，这里算不到，
                    // 字数只作量级参考，不追求精确
                    if let Some(fields) = entry.get("fields").and_then(Value::as_array) {
                        section_chars += fields_len(fields);
                    }
                    let marks = entry
                        .get("bullets")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    bullets += marks.iter().filter(|mark| text_len(Some(mark)) > 0).count();
                    section_chars += marks.iter().map(|mark| text_len(Some(mark))).sum::<usize>();
                }

                total_entries += items.len();
                total_bullets += bullets;
                chars += section_chars;
                partition_lines.push(format!(
                    "· 分区「{}」（块 {}）：{} 条，{} 句要点，约 {} 字{}",
                    if title.is_empty() { "未命名" } else { &title },
                    block.get("id").and_then(Value::as_str).unwrap_or("?"),
                    items.len(),
                    bullets,
                    section_chars,
                    role_mark(block)
                ));
            }
            "media" => media += 1,
            // 未知块类型跳过：数据要留着，但不能当成内容算进版面
            _ => {}
        }
    }

    let mut out = format!(
        "第 {index} 页 / 共 {} 页，页名「{label}」\n\
         页头 {header_fields} 个字段（约 {header_chars} 字）",
        pages.len()
    );
    if !header_block.is_empty() {
        out.push_str(&format!("｜块 {header_block}{header_role}"));
    }
    if extra_groups > 0 {
        out.push_str(&format!("，另有 {extra_groups} 组补充字段"));
    }
    if media > 0 {
        out.push_str(&format!("，{media} 张图片"));
    }
    out.push('\n');

    if partition_lines.is_empty() {
        out.push_str("这一页还没有分区。\n");
    } else {
        out.push_str(&format!(
            "共 {} 个分区、{total_entries} 条条目、{total_bullets} 句要点\n",
            partition_lines.len()
        ));
        for line in &partition_lines {
            out.push_str(line);
            out.push('\n');
        }
    }
    out.push_str(&format!("正文合计约 {chars} 字\n"));
    // 顺序有理由：先说这一页有多少内容，紧接着说实测多少页 ——
    // 「多少字 → 实测几页 → 现在怎么排的」，读者才是沿着排版决策的路线走
    out.push_str(&crate::agent::facts::measure_section(facts));

    // 现在怎么排的：不说清的话，agent 会重复改同一块，或者把已有覆盖冲掉
    let layout = page.get("layout");
    // 预设可能是从旧的三档疏密映射来的（`style.density`）：不回落的话，
    // 历史页面在快照里显示「classic」，实际渲染却是 roomy —— 说的和看到的不一致
    let preset = layout
        .and_then(|value| value.get("preset"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            page.get("style")
                .and_then(|style| style.get("density"))
                .and_then(Value::as_str)
        })
        .unwrap_or("");
    let html = layout
        .and_then(|value| value.get("html"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let parts = layout
        .and_then(|value| value.get("parts"))
        .and_then(Value::as_object)
        .map(|map| map.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();

    if !html.is_empty() {
        out.push_str(&format!(
            "当前版面：整页手写 HTML（{} 字），生成器不再介入\n",
            html.chars().count()
        ));
    } else {
        out.push_str(&format!(
            "当前版面：结构化生成 · 预设 {}\n",
            if preset.is_empty() { "classic" } else { preset }
        ));
        if !parts.is_empty() {
            out.push_str(&format!("已手写过的块：{}\n", parts.join(" / ")));
        }
    }

    // 可用图片：写 parts 或 html 时要引用 asset id，列出来才不用再问一次
    let images: Vec<String> = person
        .get("assets")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter(|asset| asset.get("kind").and_then(Value::as_str) == Some("image"))
                .map(|asset| {
                    format!(
                        "{} {}",
                        asset.get("id").and_then(Value::as_str).unwrap_or("?"),
                        asset.get("name").and_then(Value::as_str).unwrap_or("")
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    if !images.is_empty() {
        out.push_str(&format!("可用图片：{}", images.join("、")));
    }

    // 原件样张：这一页是从原件改编来的，版式唯一的依据是那张页图。
    //
    // 只给「有样张」的页加这一段 —— 没有原件时让模型自己判断，不要凭空诱导它
    // 去找一张不存在的图。样张挂在简历上（不是材料列表里），所以也不会出现在
    // 上面那行「可用图片」里：它不该被当图片块引用。
    if let Some(id) = resume
        .get("originSheet")
        .and_then(|sheet| sheet.get("id"))
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
    {
        out.push_str(&format!(
            "\n## 原件样张\n\
             这一页的内容来自一份原件，原件长什么样可以自己看：调 view，参数 assetId = {id}。\n\
             原件是「长什么样」的唯一依据；不要凭审美另起一套。看不清版式就照结构化生成，不要自己发明版式。\n\
             这张图只是参考：不要把它当页面上的图片块引用。\n"
        ));
    }
    out
}

/// 跑一轮：喂进这一轮攒下的输入，把模型给出的动作交出去。
///
/// 返回的是 actions 而不是文字：排版产出的是「改版式」这个意图，
/// 正文只进历史，不给界面看。
async fn run_turn(
    app: &tauri::AppHandle,
    args: &LayoutArgs,
    prompt: &str,
) -> Result<crate::agent::turn::TurnResult, String> {
    let key = format!("layout:{}:{}", args.resume_id, args.page_id);
    // 排版侧不传 turn（`None`）：对排版 agent 来说，那个页数确实是「用户之前要求过」
    let facts = crate::agent::facts::PageFacts::assemble(
        &args.workspace,
        &args.person_id,
        &args.resume_id,
        &args.page_id,
        args.measure.clone(),
        args.origin.clone(),
        None,
    );
    // 排版侧也要知道「这一轮到底能不能看图」：它会在 `view` 里渲染页面给自己看，
    // 能力是三态里唯一靠实测的那一栏，不许由提示词假定。
    let capability = crate::capability::resolve(&args.base_url, &args.model, &args.api_key);
    let mut system = SYSTEM.replace("{{page}}", &describe_page(args, &facts));
    system.push('\n');
    system.push_str(&crate::agent::facts::capability_block(&capability));

    let budget_window = args.context_window.unwrap_or(64_000);
    let budget = budget_window.saturating_sub(session::estimate_tokens(&system) + 2_000) * 6 / 10;
    let history = {
        let sessions = app.state::<Sessions>();
        session::trim_history(&sessions.history_of(app, &key), budget)
    };

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
    messages.push(ChatMessage::user(prompt));

    let template = AiRequest {
        base_url: args.base_url.clone(),
        api_key: args.api_key.clone(),
        model: args.model.clone(),
        messages: Vec::new(),
        temperature: args.temperature,
        reasoning: args.reasoning.clone(),
        max_tokens: None,
        tools: None,
    };

    let mut out = crate::tools::Outbox::default();
    out.vision = capability.can_see();
    out.model = args.model.clone();
    // 排版 agent 也能转图片：作品集里放一张压好的图，比让用户自己去压缩现实
    let root = crate::data_dir(app).ok();
    let ctx = ToolCtx {
        workspace: &args.workspace,
        person_id: &args.person_id,
        resume_id: &args.resume_id,
        page_id: &args.page_id,
        out: &out,
        root: root.as_deref(),
        facts: Some(&facts),
    };

    let cancel = std::sync::atomic::AtomicBool::new(false);
    let turn = crate::agent::turn::run(
        &template,
        messages,
        &ctx,
        &cancel,
        // 没有人在等打字机：正文增量丢掉，工具事件攒成进度推给界面，
        // 用户才知道「它真的在动」而不是卡死了
        |chunk| {
            if chunk.kind == "tool_start" || chunk.kind == "tool_end" {
                let _ = app.emit(
                    "layout:progress",
                    json!({
                        "pageId": args.page_id,
                        "kind": chunk.kind,
                        "tool": chunk.tool,
                        "detail": chunk.detail,
                    }),
                );
            }
        },
        Scope::Layout,
    )
    .await?;

    let reply_id = format!("l-{}", session::uuid_like());
    {
        let sessions = app.state::<Sessions>();
        let mut message = json!({
            "id": reply_id,
            "role": "assistant",
            "content": turn.content,
            "time": session::chrono_like(),
            "tool": if turn.used.is_empty() {
                format!("{} · 排版", args.model)
            } else {
                format!("{} · 用了 {}", args.model, turn.used.join(" / "))
            },
        });
        // 排版 agent 半路断了，用户更该知道：页面可能只改了一半
        if let Some(kind) = turn.unfinished {
            message["note"] = json!(kind.text(&turn.used));
        }
        sessions.append(app, &key, message);
    }

    Ok(turn)
}

/// 消费者：一直跑到信箱空为止。
///
/// **中途插入就是靠这个循环**。这一轮跑的时候新进来的输入不会被丢弃，
/// 它们躺在 notes 里；这一轮结束回到循环顶部再 take 一次就拿到了。
/// 所以「跑完一轮立刻再看一眼」不是优化，是正确性的一部分。
/// 一次唤醒最多连跑几轮。
///
/// 正常情况远远用不到：一轮跑完信箱就空了，消费者随即退出。
/// 只有用户持续编辑、每轮期间都有新输入时才会累积到这个数 ——
/// 那种情况连续跑十几轮既费钱，上下文也会被越填越满，不如歇一下等下一次变更。
const MAX_ROUNDS_PER_WAKE: usize = 12;

async fn drain(app: tauri::AppHandle, key: String, page_id: String) {
    let mut announced = false;
    let mut rounds = 0usize;
    loop {
        if rounds >= MAX_ROUNDS_PER_WAKE {
            // 先放开排班再看信箱：顺序反了的话，这期间插入的输入会没人消费
            app.state::<Layouts>().release(&key);
            break;
        }

        let next = { app.state::<Layouts>().take(&key) };
        let Some((notes, args)) = next else {
            break;
        };
        rounds += 1;
        if !announced {
            announced = true;
            // 先报一声「开始了」：模型可能直接就给结论、一个工具都不调，
            // 只靠工具事件的话界面会全程静止，看起来像没反应
            let _ = app.emit("layout:started", json!({ "pageId": page_id }));
        }

        let prompt = compose(&notes);
        match run_turn(&app, &args, &prompt).await {
            Ok(turn) => {
                // 每轮都报一声，哪怕一个动作都没有。
                // 界面靠它知道「这一轮问过了」——只在有改动时才报的话，
                // 「它判断不用动」和「它压根没跑」在界面上长得一模一样。
                let _ = app.emit(
                    "layout:applied",
                    json!({
                        "personId": args.person_id,
                        "resumeId": args.resume_id,
                        "pageId": args.page_id,
                        "actions": turn.actions,
                        // 新材料与动作一起回传：模型可能在同一轮里刚转完图就引用它，
                        // 分两次发会让前端出现「图还没到、HTML 已经指向它」的窗口
                        "assets": turn.assets,
                    }),
                );
            }
            Err(error) => {
                let _ = app.emit(
                    "layout:failed",
                    json!({ "pageId": args.page_id, "error": error }),
                );
                // 配置错、网络断：清空待办再退出。
                // 不清的话这一页会拿着同一个坏配置无限重试，每轮都失败一次
                app.state::<Layouts>().clear(&key);
                break;
            }
        }
    }

    // 走到这里说明信箱空了（或者刚失败清空）：这一页彻底闲下来了。
    // 界面等的就是这个信号 —— 没有它，最后一步「还在跑」会一直挂着
    let _ = app.emit("layout:idle", json!({ "pageId": page_id }));
}

/// 投一条输入（内容变更 / 要求）。立即返回，结果通过事件回传。
#[tauri::command]
pub fn layout_submit(
    app: tauri::AppHandle,
    args: LayoutArgs,
    note: Note,
) -> Result<(), String> {
    let key = format!("layout:{}:{}", args.resume_id, args.page_id);
    let page_id = args.page_id.clone();
    let spawn = { app.state::<Layouts>().enqueue(&key, note, args) };
    if spawn {
        tauri::async_runtime::spawn(async move { drain(app, key, page_id).await });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> LayoutArgs {
        LayoutArgs {
            person_id: "p1".into(),
            resume_id: "r1".into(),
            page_id: "pg2".into(),
            workspace: json!({
                "people": [{
                    "id": "p1",
                    "resumes": [{
                        "id": "r1",
                        "pages": [
                            { "id": "pg1", "kind": "native", "label": "第 1 页", "blocks": [] },
                            {
                                "id": "pg2",
                                "kind": "native",
                                "label": "English",
                                "blocks": [
                                    {
                                        "id": "b1",
                                        "kind": "fields",
                                        "fields": [{ "id": "f1", "label": "Name", "value": "Zhang" }]
                                    },
                                    {
                                        "id": "s1",
                                        "kind": "section",
                                        "title": "Experience",
                                        "items": [
                                            { "id": "e1", "bullets": ["a", "b"] },
                                            { "id": "e2", "bullets": ["c"] }
                                        ]
                                    }
                                ],
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
            measure: None,
            origin: None,
        }
    }

    /// 没有实测、没有原件：事实块落到「尚未实测」档
    fn no_facts() -> crate::agent::facts::PageFacts {
        crate::agent::facts::PageFacts::default()
    }

    /// 实测 1.00 页 + 用户之前要求过 1 页
    fn measured_facts() -> crate::agent::facts::PageFacts {
        crate::agent::facts::PageFacts {
            target: crate::agent::facts::Target {
                pages: Some(1),
                source: crate::agent::facts::TargetSource::UserBefore,
            },
            measure: Some(crate::agent::facts::PageMeasure {
                page_id: "pg2".into(),
                status: "ok".into(),
                version: 12,
                content: "v37".into(),
                one_page: Some(1000.0),
                content_px: Some(1000.0),
                line_height: Some(17.0),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn note(kind: &str, text: &str) -> Note {
        Note {
            kind: kind.into(),
            text: text.into(),
        }
    }

    #[test]
    fn page_brief_counts_blocks_and_keeps_style() {
        let text = describe_page(&sample(), &no_facts());
        assert!(text.contains("第 2 页 / 共 2 页"), "{text}");
        assert!(text.contains("「English」"), "{text}");
        assert!(text.contains("页头 1 个字段"), "{text}");
        assert!(text.contains("2 条条目"), "{text}");
        assert!(text.contains("3 句要点"), "{text}");
        assert!(text.contains("分区「Experience」"), "{text}");
        // 已有版式必须原样带上，否则模型每次都当第一次排
        assert!(text.contains("roomy"), "{text}");
    }

    /// 块 id 与图片 id 都要直接给出：排版 agent 写 parts 靠前者、引用图靠后者，
    /// 少一个就得再调一次工具去问
    #[test]
    fn page_brief_hands_over_block_ids_and_image_ids() {
        let mut args = sample();
        args.workspace["people"][0]["assets"] = json!([
            { "id": "a-photo", "kind": "image", "name": "证件照.jpg", "stored": "x.jpg" },
            { "id": "a-doc", "kind": "file", "name": "证书.pdf", "stored": "y.pdf" }
        ]);
        let text = describe_page(&args, &no_facts());
        assert!(text.contains("块 b1"), "页头块 id：{text}");
        assert!(text.contains("块 s1"), "分区块 id：{text}");
        assert!(text.contains("a-photo 证件照.jpg"), "可用图片：{text}");
        assert!(!text.contains("a-doc"), "非图片不该混进来：{text}");
    }

    /// 已经有手写覆盖时要报出来，否则 agent 会重复改同一块
    #[test]
    fn page_brief_reports_existing_overrides() {
        let mut args = sample();
        args.workspace["people"][0]["resumes"][0]["pages"][1]["layout"] = json!({
            "preset": "compact",
            "parts": { "s1": "<section>手写</section>" }
        });
        let text = describe_page(&args, &no_facts());
        assert!(text.contains("预设 compact"), "{text}");
        assert!(text.contains("已手写过的块：s1"), "{text}");

        args.workspace["people"][0]["resumes"][0]["pages"][1]["layout"] =
            json!({ "html": "<article>x</article>" });
        let text = describe_page(&args, &no_facts());
        assert!(text.contains("整页手写"), "{text}");
    }

    /// role 是排版 agent 判断「这一块已经被结构化生成接管成什么形态」的唯一线索。
    /// 不显示的话，它会在 parts 里把 tags 再手写一遍 —— 两套排法叠在一起，
    /// 用户看到的是错位，而模型以为自己只是「优化了一下」。
    #[test]
    fn page_brief_shows_block_roles() {
        let mut args = sample();
        args.workspace["people"][0]["resumes"][0]["pages"][1]["blocks"][0]["role"] = json!("kv");
        args.workspace["people"][0]["resumes"][0]["pages"][1]["blocks"][1]["role"] = json!("tags");
        let text = describe_page(&args, &no_facts());
        assert!(text.contains("〔role: kv〕"), "页头的 role：{text}");
        assert!(text.contains("〔role: tags〕"), "分区的 role：{text}");
    }

    /* ---------- 原件样张 ---------- */

    /// 有样张：必须给出 assetId 和「先去看」这句话，否则模型只会自己审美
    #[test]
    fn an_origin_sheet_hands_over_the_asset_id_and_says_it_is_the_reference() {
        let mut args = sample();
        args.workspace["people"][0]["resumes"][0]["originSheet"] = json!({
            "id": "a-sheet",
            "kind": "image",
            "name": "简历 样张.jpg",
            "stored": "abcd1234.jpg"
        });
        let text = describe_page(&args, &no_facts());
        assert!(text.contains("## 原件样张"), "{text}");
        assert!(text.contains("assetId = a-sheet"), "要给可调的 assetId：{text}");
        assert!(text.contains("view"), "{text}");
        assert!(text.contains("唯一依据"), "{text}");
        // 样张不是页面内容：它不该混进「可用图片」，否则模型会把它当图片块引用
        assert!(!text.contains("可用图片"), "{text}");
        assert!(!text.contains("a-sheet 简历 样张.jpg"), "{text}");
    }

    /// 没有样张：一个字都不许提，否则模型会去找一张不存在的图
    #[test]
    fn no_origin_sheet_no_section() {
        let text = describe_page(&sample(), &no_facts());
        assert!(!text.contains("原件样张"), "{text}");
        assert!(!text.contains("assetId ="), "{text}");
    }

    /// 整页原件页本来就不排版，也不必叫它去 view 自己对
    #[test]
    fn a_file_page_gets_no_origin_sheet_section() {
        let mut args = sample();
        args.page_id = "pg1".into();
        args.workspace["people"][0]["resumes"][0]["pages"][0]["kind"] = json!("file");
        args.workspace["people"][0]["resumes"][0]["originSheet"] = json!({
            "id": "a-sheet", "kind": "image", "name": "简历 样张.jpg", "stored": "x.jpg"
        });
        let text = describe_page(&args, &no_facts());
        assert!(text.contains("没有可调的空间"), "{text}");
        assert!(!text.contains("## 原件样张"), "{text}");
    }

    #[test]
    fn missing_page_is_reported_not_panicked() {
        let mut args = sample();
        args.page_id = "pg-nope".into();
        assert!(describe_page(&args, &no_facts()).contains("找不到这一页"));
    }

    #[test]
    fn file_page_is_told_there_is_nothing_to_arrange() {
        let mut args = sample();
        args.page_id = "pg1".into();
        args.workspace["people"][0]["resumes"][0]["pages"][0]["kind"] = json!("file");
        assert!(describe_page(&args, &no_facts()).contains("没有可调的空间"));
    }

    #[test]
    fn empty_page_says_so_instead_of_showing_zeros() {
        let mut args = sample();
        args.page_id = "pg1".into();
        let text = describe_page(&args, &no_facts());
        assert!(text.contains("还没有分区"), "{text}");
    }

    /* ---------- 实测段：位置、退化、file 页豁免 ---------- */

    /// 顺序是刻意的：多少内容 → 实测多少页 → 现在怎么排的。
    /// 插到开头或末尾都会让 agent 先看到版式再看到容量。
    #[test]
    fn measured_section_sits_between_content_and_layout() {
        let args = sample();
        let facts = measured_facts();
        let text = describe_page(&args, &facts);

        assert!(text.contains("## 这一页的实测"), "{text}");
        let section = text.find("## 这一页的实测").unwrap();
        assert!(text.find("正文合计约").unwrap() < section, "{text}");
        assert!(section < text.find("当前版面：").unwrap(), "{text}");
        // 文案来自 facts：切掉版本尾注后必须原样出现（实测段把版本号放到了段尾）
        let line = crate::agent::facts::measure_line(&facts);
        let shared = &line[..line.find("测量版本").expect("fit 文案带版本号")];
        assert!(text.contains(shared), "{text}");
        // 排版侧没有 turn：库里写的是「这一轮」，这里必须降级成「之前要求过」
        assert!(text.contains("用户之前要求过"), "{text}");
        assert!(!text.contains("用户这一轮明确要求"), "{text}");
    }

    #[test]
    fn measured_section_says_cannot_measure_when_nothing_arrived() {
        let text = describe_page(&sample(), &no_facts());
        assert!(text.contains("## 这一页的实测"), "{text}");
        assert!(text.contains("尚未实测（预览没打开，量不到）"), "{text}");
        assert!(!text.contains("没有溢出"), "{text}");
    }

    /// file 页的版式在文件里，量出来的页数也说明不了什么：早退，不追加实测段
    #[test]
    fn file_page_gets_no_measured_section() {
        let mut args = sample();
        args.page_id = "pg1".into();
        args.workspace["people"][0]["resumes"][0]["pages"][0]["kind"] = json!("file");
        let text = describe_page(&args, &measured_facts());
        assert!(text.contains("没有可调的空间"), "{text}");
        assert!(!text.contains("## 这一页的实测"), "{text}");
    }

    /* ---------- 信箱：并发与中途插入 ---------- */

    #[test]
    fn a_burst_of_inputs_starts_only_one_consumer() {
        let layouts = Layouts::default();
        // 用户连打几个字，前端挤出一串变更
        assert!(layouts.enqueue("k", note("change", "改 A"), sample()));
        assert!(!layouts.enqueue("k", note("change", "改 B"), sample()));
        assert!(!layouts.enqueue("k", note("change", "改 C"), sample()));

        // 但三条都还在，一条都不能丢
        let (notes, _) = layouts.take("k").expect("应当有输入");
        assert_eq!(notes.len(), 3);
    }

    #[test]
    fn inputs_arriving_mid_run_are_kept_for_the_next_round() {
        let layouts = Layouts::default();
        layouts.enqueue("k", note("change", "第一轮：改了页头"), sample());

        // 消费者取走这一轮，开始跑
        let (first, _) = layouts.take("k").expect("第一轮");
        assert_eq!(first.len(), 1);

        // 跑的过程中用户又改了两处
        assert!(!layouts.enqueue("k", note("change", "第二轮：加了分区"), sample()));
        assert!(!layouts.enqueue("k", note("request", "压到一页内"), sample()));

        // 跑完回到循环顶部：新输入原样在，且会自动组成第二轮
        let (second, _) = layouts.take("k").expect("中途插入的输入不能丢");
        assert_eq!(second.len(), 2);
        assert!(second[1].is_request());
    }

    #[test]
    fn empty_inbox_lets_the_consumer_quit_and_a_later_input_revives_it() {
        let layouts = Layouts::default();
        layouts.enqueue("k", note("change", "改了"), sample());
        layouts.take("k");

        // 这一轮跑完，信箱空了 → 消费者退出
        assert!(layouts.take("k").is_none());

        // 之后再有变更，必须能重新起一个消费者（scheduled 已经放开了）
        assert!(
            layouts.enqueue("k", note("change", "又改了"), sample()),
            "退出后必须能重新排上班，否则这一页此后再也不排版"
        );
    }

    #[test]
    fn latest_snapshot_wins_so_a_queued_round_sees_current_content() {
        let layouts = Layouts::default();
        let mut old = sample();
        old.workspace["people"][0]["resumes"][0]["pages"][1]["label"] = json!("旧名字");
        layouts.enqueue("k", note("change", "改 A"), old);
        layouts.take("k").expect("第一轮");

        // 排队期间页名改了：第二轮必须拿到新的那份快照
        let mut fresh = sample();
        fresh.workspace["people"][0]["resumes"][0]["pages"][1]["label"] = json!("新名字");
        layouts.enqueue("k", note("change", "改 B"), fresh);

        let (_, args) = layouts.take("k").expect("第二轮");
        assert!(describe_page(&args, &no_facts()).contains("新名字"), "排队期间的新内容要跟上");
    }

    #[test]
    fn different_pages_do_not_block_each_other() {
        let layouts = Layouts::default();
        let mut other = sample();
        other.page_id = "pg1".into();

        assert!(layouts.enqueue("k1", note("change", "改 A"), sample()));
        assert!(layouts.enqueue("k2", note("change", "改 B"), other));
    }

    #[test]
    fn clearing_after_a_failure_prevents_an_endless_retry_loop() {
        let layouts = Layouts::default();
        layouts.enqueue("k", note("change", "改 A"), sample());
        layouts.take("k");
        layouts.enqueue("k", note("change", "改 B"), sample());

        layouts.clear("k");
        assert!(layouts.take("k").is_none());
        // 清干净之后还能重新开始
        assert!(layouts.enqueue("k", note("change", "改 C"), sample()));
    }

    #[test]
    fn releasing_after_too_many_rounds_keeps_pending_inputs_and_can_be_woken_again() {
        let layouts = Layouts::default();
        layouts.enqueue("k", note("change", "改 A"), sample());
        layouts.take("k").expect("第一轮");

        // 跑的过程中又来了一条，但这一轮已经跑满上限、准备让出
        assert!(!layouts.enqueue("k", note("change", "改 B"), sample()));
        layouts.release("k");

        // 让出之后必须能被下一次变更重新叫醒，否则这一页就永久停摆了
        assert!(
            layouts.enqueue("k", note("change", "改 C"), sample()),
            "让出排班后必须能重新起来"
        );

        // 让出期间积压的那条不能被丢掉
        let (notes, _) = layouts.take("k").expect("应当有输入");
        assert_eq!(notes.len(), 2, "积压 + 新的都要在：{notes:?}");
        assert_eq!(notes[0].text, "改 B");
        assert_eq!(notes[1].text, "改 C");
    }

    /* ---------- 拼装 ---------- */

    #[test]
    fn changes_and_requests_are_labelled_separately() {
        let text = compose(&[
            note("change", "~ 页头 · 职位：A → B"),
            note("request", "把内容压到一页内"),
        ]);
        assert!(text.contains("【这一页刚发生的改动】"), "{text}");
        assert!(text.contains("~ 页头 · 职位：A → B"), "{text}");
        assert!(text.contains("【要求】"), "{text}");
        assert!(text.contains("把内容压到一页内"), "{text}");
        // 改动在前、要求在后：先让它知道发生了什么，再听要什么
        assert!(text.find("改动").unwrap() < text.find("要求").unwrap());
    }

    #[test]
    fn empty_notes_do_not_produce_an_empty_prompt() {
        assert!(!compose(&[]).trim().is_empty());
        assert!(!compose(&[note("change", "   ")]).trim().is_empty());
    }
}
