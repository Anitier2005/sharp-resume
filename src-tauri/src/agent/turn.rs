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

use serde_json::Value;

use crate::net::{self, AiRequest, ChatMessage, StreamChunk};
use crate::tools::ToolCtx;

/// **没有轮次上限。** 这里只记一件事：停下来的理由必须是「它绕圈」或「它不说话」，
/// 不是「它做了几次」。
///
/// 为什么去掉数字帽：一次「还原整份简历」要读全文、渲染页图、读人、读页、抽图、
/// 写字段 —— 实测 8 次工具操作、6 个来回就打满，最后一个来回刚拿到图就被掐断，
/// 用户看到的是「用满了 6 轮」。那是把「做得慢」当成「出问题了」。
/// 真正的死循环由两条盯着：同一工具同参数连调 3 次（`RepeatGuard`）、
/// 连续两轮白卷（`MAX_EMPTY_ROUNDS`）。用户随时可以按停止。
const MAX_EMPTY_ROUNDS: usize = 2;

pub struct TurnResult {
    /// 这一轮（可能跨多次请求）累计说出来的正文
    pub content: String,
    /// 调用过的工具名，按顺序
    pub used: Vec<String>,
    /// 页面控制工具产出的界面指令，按产出顺序。前端拿到后依次应用。
    pub actions: Vec<Value>,
    /// 工具产出的新材料（图片转化会产出）。前端要把它们并进 state 的资产列表，
    /// 否则模型下一轮引用的 `asset:` 是个不存在的 id。
    pub assets: Vec<crate::model::Asset>,
    /// 这一轮没有正常收尾时，为什么。`None` = 模型真的说完了。
    ///
    /// 它**不改 content**：正文原样保留（哪怕是半句旁白），
    /// 给用户的诊断单独挂在消息的 `note` 上 —— 后者不参与模型上下文。
    pub unfinished: Option<Unfinished>,
    /// 本轮（跨多次请求累加）思考链字数。
    ///
    /// 只进请求日志，不进消息：`reasoning=1200 字 / 正文 0 字` 一眼就能区分
    /// 「想了很久没输出」和「什么都没想就交了白卷」——这正是白卷最难诊断的地方。
    pub reasoning_chars: usize,
}

/// 一轮怎么收尾的。判据只有一个来源：服务端的 `finish_reason`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// 还要工具：接着跑，不是结束
    Continue,
    /// 正常说完
    Done,
    /// 没有工具调用，也没有正文
    EmptyReply,
    /// 输出被 max_tokens 截断
    Truncated,
    /// 被内容过滤掐掉
    Filtered,
}

/// 判据顺序刻意如此：**先看有没有工具调用，再看 finish_reason，最后才看正文**。
/// 反过来（正文非空就算说完）正是「说了半句话被当成完整答复」那个 bug 的成因。
fn classify(reason: Option<&str>, reply: &str, has_calls: bool) -> Stop {
    if has_calls {
        return Stop::Continue;
    }
    match reason {
        Some("length") | Some("max_tokens") => Stop::Truncated,
        Some("content_filter") => Stop::Filtered,
        _ if !reply.trim().is_empty() => Stop::Done,
        _ => Stop::EmptyReply,
    }
}

/// 没有正常收尾的四种情况。文案是给人看的诊断，不是给模型看的上下文。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unfinished {
    /// 提醒过一次，仍然只调工具、不给正文
    EmptyAfterNudge,
    /// 续写过一次，仍然被长度截断
    Truncated,
    /// 同一工具同参数连续调了 3 次 —— 这是**唯一**会被主动掐断的情况
    Loop,
    /// 被安全过滤拦截：不是模型抽风，也不是白卷，换个说法还能救
    Filtered,
}

impl Unfinished {
    pub fn text(&self, used: &[String]) -> String {
        match self {
            Unfinished::EmptyAfterNudge => format!(
                "这一轮模型只调用了工具（{}），没有给出正文。已经写进工作区的改动仍然有效，但它没有把结论告诉你。",
                join(used)
            ),
            Unfinished::Truncated => "这一轮输出被长度截断，内容可能不完整。".to_string(),
            Unfinished::Loop => format!(
                "同一个工具同样参数连续调了 3 次，这一轮没有收尾。已执行 {} 次工具操作，改动已生效。",
                used.len()
            ),
            // 用户看到的是一片空白；不写这一句，他分不清是「被安全过滤」还是「模型抽风」
            Unfinished::Filtered => {
                "这一轮内容被安全过滤拦截了，没有正文。换个说法或删掉可能触发拦截的表述再试。"
                    .to_string()
            }
        }
    }
}

/// 工具名连成一句人话；一个都没调时给个能读的占位。
fn join(names: &[String]) -> String {
    if names.is_empty() { "无".to_string() } else { names.join(" / ") }
}

/// 护栏一：同工具同参连调的计数。
///
/// 模型卡在「读同一页 → 发现没变化 → 再读同一页」时，轮次上限要烧满 6 轮，
/// 每轮都是一次完整的请求。连到第 3 次就停 —— 它已经转不出来了。
#[derive(Default)]
struct RepeatGuard {
    last: Option<String>,
    count: usize,
}

impl RepeatGuard {
    /// 记一次调用，返回是否已经连到第 3 次（`name` + `arguments` 原文比）
    fn hit(&mut self, name: &str, args: &str) -> bool {
        let signature = format!("{name}\u{0}{args}");
        if self.last.as_deref() == Some(signature.as_str()) {
            self.count += 1;
        } else {
            self.last = Some(signature);
            self.count = 1;
        }
        self.count >= 3
    }
}

/// 护栏二：白卷连到第 `MAX_EMPTY_ROUNDS` 轮就停。
fn too_many_empty(rounds: usize) -> bool {
    rounds >= MAX_EMPTY_ROUNDS
}

/// 收尾：界面指令 + 一条内容信封。
///
/// 顺序不能反 —— 删掉用户正看着的那一页时，`openPage`（切到邻页）必须排在
/// `mutate`（真的删）前面，否则中间会闪一下空面板。信封在轮末才追加，
/// 天然满足「界面先行、内容落地在后」。
fn drain_actions(out: &crate::tools::Outbox) -> Vec<Value> {
    let mut actions = out
        .actions
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default();
    if let Some(envelope) = crate::tools::edit::flush_all(out) {
        actions.push(envelope);
    }
    actions
}

fn drain_assets(out: &crate::tools::Outbox) -> Vec<crate::model::Asset> {
    out.assets
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default()
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
        "extract_images" => "在原件里找内嵌图片".to_string(),
        "read_source" => "读原件正文".to_string(),
        "list_sources" => "列出这个人的原件".to_string(),
        // 宽松命名：同一个工具既能渲染 PDF 页，也能把图片附件直接给视觉看
        "render_doc_page" => "打开原件看一眼".to_string(),
        "set_target_pages" => match args["pages"].as_u64() {
            Some(pages) => format!("把目标定成 {pages} 页"),
            _ => "清掉目标页数".to_string(),
        },
        "set_open_pages" => "调整快捷标签".to_string(),
        "set_ui" => "调整界面".to_string(),
        "read_layout_guide" => "读组件手册".to_string(),
        "set_page_layout" => match args["preset"].as_str().filter(|value| !value.trim().is_empty()) {
            Some(preset) => format!("换成 {preset} 档"),
            None => match args["parts"].as_object().filter(|parts| !parts.is_empty()) {
                Some(parts) => format!("重写 {} 块的排法", parts.len()),
                None => match args["html"].as_str().filter(|html| !html.trim().is_empty()) {
                    Some(_) => "整页接管".to_string(),
                    None => "调整版面".to_string(),
                },
            },
        },
        "note_on_page" => "在纸面留批注".to_string(),
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
/// 命令层有前端在等，边跑边推；闭包让测试与离线路径也能直接收集事件，
/// 不必伪造一个 webview 端。
pub async fn run<E: FnMut(StreamChunk) + Send>(
    template: &AiRequest,
    messages: Vec<ChatMessage>,
    ctx: &ToolCtx<'_>,
    cancel: &AtomicBool,
    mut on_event: E,
) -> Result<TurnResult, String> {
    use std::sync::atomic::Ordering;

    let specs = crate::tools::specs();
    // 内容暂存区按轮计：先清一次，免得上一轮残留（取消 / 报错提前退出时没结算）
    // 悄悄漏进这一轮
    crate::tools::edit::begin(ctx.out, ctx.workspace);
    let mut messages = messages;
    let mut speech = String::new();
    let mut used: Vec<String> = Vec::new();
    let mut with_tools = true;
    // 每次请求的正文：终止判定与「说完了」的判据都是它，别用累计的 speech 去判
    let mut last_reply: String;
    let mut nudged = false;
    let mut continued = false;
    let mut empty_rounds = 0usize;
    let mut repeat = RepeatGuard::default();
    let mut unfinished: Option<Unfinished> = None;
    let mut reasoning_chars = 0usize;

    loop {
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
        last_reply = outcome.content.clone();
        speech.push_str(&outcome.content);
        reasoning_chars += outcome.reasoning_chars;

        match classify(outcome.finish_reason.as_deref(), &last_reply, !outcome.tool_calls.is_empty()) {
            Stop::Done => {
                return Ok(TurnResult {
                    content: speech,
                    used,
                    actions: drain_actions(ctx.out),
                    assets: drain_assets(ctx.out),
                    unfinished: None,
                    reasoning_chars,
                })
            }
            Stop::Continue => {}
            Stop::EmptyReply => {
                empty_rounds += 1;
                // 白卷：工具结果已经在上面了，它只是没说结论。提醒一次就好 ——
                // 提醒第二次一定还是白卷，那不如把「没说完」如实告诉用户。
                // 还要留得出一次请求：轮次已经烧完时提醒也发不出去
                if !nudged && !too_many_empty(empty_rounds) {
                    nudged = true;
                    messages.push(ChatMessage::user(format!(
                        "你刚才调用了 {}，结果已经在上面。用一两句话告诉我结论；还需要工具就继续调。",
                        join(&used)
                    )));
                    continue;
                }
                // 走到这里说明提醒已经发过（第一条白卷必定被提醒）
                unfinished = Some(Unfinished::EmptyAfterNudge);
                break;
            }
            Stop::Truncated => {
                // 截断不是模型的错，是预算的错：让它接着写比从头再来便宜
                if !continued {
                    continued = true;
                    messages.push(ChatMessage::user("继续，接着刚才没写完的部分说。"));
                    continue;
                }
                unfinished = Some(Unfinished::Truncated);
                break;
            }
            // 内容过滤：再说一遍会被同样掐掉，所以**不重试**；但状态要说清楚 ——
            // 用户面对的是一片空白，不告诉他是「被安全过滤」就等于让他以为模型抽风
            Stop::Filtered => {
                unfinished = Some(Unfinished::Filtered);
                break;
            }
        }
        if cancel.load(Ordering::SeqCst) {
            break;
        }

        messages.push(ChatMessage::assistant_calls(outcome.tool_calls.clone(), outcome.content.clone()));

        let mut tripped = false;
        for call in &outcome.tool_calls {
            let name = call.function.name.clone();
            let args: Value = serde_json::from_str(&call.function.arguments)
                .unwrap_or_else(|_| Value::Object(Default::default()));

            // 先判护栏再执行：连到第 3 次的那一次调用没有意义，不做
            if repeat.hit(&name, &call.function.arguments) {
                unfinished = Some(Unfinished::Loop);
                tripped = true;
                break;
            }

            on_event(StreamChunk::tool_event(
                "tool_start",
                name.clone(),
                describe(&name, &args),
            ));

            let result = match crate::tools::call(&name, &args, ctx) {
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
        // 这一批工具写进暂存区的东西立刻结算一次。攒到轮末才发一条信封，
        // 意味着「它一直在干活、页面一动不动」——用户没法判断它是不是卡死了。
        // flush 之后暂存区清空，后续写入继续攒，轮末 `drain_actions` 收尾剩余部分。
        if let Some(envelope) = crate::tools::edit::flush_all(ctx.out) {
            on_event(StreamChunk::envelope(envelope.to_string()));
        }
        if tripped {
            break;
        }

        // 工具渲染出来的图片（PDF 页面）只能靠一条新的用户消息带进去：
        // 工具结果在协议里是纯字符串，塞不进 image_url 段。
        // 模型看不了图时直接丢弃 —— 发过去只会换一个 400。
        let views = ctx
            .out
            .views
            .lock()
            .map(|mut list| std::mem::take(&mut *list))
            .unwrap_or_default();
        if ctx.out.vision && !views.is_empty() {
            messages.push(ChatMessage::user_with_images(
                "以下是刚刚渲染出来的页面图片，看图回答；不用再重复描述图片里已有的文字。",
                &views,
            ));
        }
    }

    // 走到这里只有两种原因：用户按了停止，或者被 `Loop` 护栏掐断。
    // 没有「轮次用尽」这一支了 —— 不设上限。

    Ok(TurnResult {
        content: speech,
        used,
        actions: drain_actions(ctx.out),
        assets: drain_assets(ctx.out),
        unfinished,
        reasoning_chars,
    })
}


#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn set_target_pages_is_described_in_words() {
        // 状态栏要显示「把目标定成 1 页」，不是「调用 set_target_pages」
        let text = describe("set_target_pages", &json!({ "pages": 1 }));
        assert!(text.contains("1 页"), "{text}");
        assert!(!text.contains("set_target_pages"), "{text}");

        let cleared = describe("set_target_pages", &json!({}));
        assert!(cleared.contains("清掉目标页数"), "{cleared}");
    }

    #[test]
    fn layout_tools_are_described_in_words() {
        // 状态栏要显示「读组件手册」「换成 compact 档」「整页接管」，
        // 不是「调用 set_page_layout」—— 这三个工具现在归会话 agent 自己调，
        // 退化成工具名会让用户看不懂它在干什么
        let guide = describe("read_layout_guide", &json!({}));
        assert!(guide.contains("组件手册"), "{guide}");

        let preset = describe("set_page_layout", &json!({ "preset": "compact" }));
        assert!(preset.contains("compact"), "{preset}");
        assert!(!preset.contains("set_page_layout"), "{preset}");

        let parts = describe(
            "set_page_layout",
            &json!({ "parts": { "b1": "<p>x</p>", "b2": "<p>y</p>" } }),
        );
        assert!(parts.contains("2 块"), "{parts}");

        let html = describe("set_page_layout", &json!({ "html": "<section></section>" }));
        assert!(html.contains("整页接管"), "{html}");

        // 什么都没给（只有 reset 之类）也要有一句人话，不能漏到兜底分支
        let plain = describe("set_page_layout", &json!({ "reset": true }));
        assert!(plain.contains("调整版面"), "{plain}");
        assert!(!plain.contains("set_page_layout"), "{plain}");

        let note = describe("note_on_page", &json!({ "text": "这一栏会挤出页" }));
        assert!(note.contains("批注"), "{note}");
        assert!(!note.contains("note_on_page"), "{note}");
    }

    /* ---------- 终止判定：五种输入各钉一条 ---------- */

    #[test]
    fn tool_calls_always_means_continue() {
        // 哪怕这一轮正文写了一大段、finish_reason 是 stop，只要有工具调用就还没完
        assert_eq!(
            classify(Some("stop"), "我看一下这一页", true),
            Stop::Continue
        );
        assert_eq!(classify(Some("tool_calls"), "", true), Stop::Continue);
        assert_eq!(classify(None, "", true), Stop::Continue);
    }

    #[test]
    fn a_reason_with_text_is_done() {
        assert_eq!(classify(Some("stop"), "改好了，两处都收紧了。", false), Stop::Done);
        // 没有 finish_reason（老网关）但真有正文：只能按说完处理
        assert_eq!(classify(None, "改好了。", false), Stop::Done);
    }

    #[test]
    fn length_or_max_tokens_is_truncated() {
        assert_eq!(classify(Some("length"), "第二段的改法是", false), Stop::Truncated);
        assert_eq!(classify(Some("max_tokens"), "", false), Stop::Truncated);
    }

    #[test]
    fn content_filter_is_filtered_not_empty() {
        // 被过滤和「白卷」要分开：前者再问一次还是被掐，后者值得提醒一次
        assert_eq!(classify(Some("content_filter"), "", false), Stop::Filtered);
        assert_eq!(classify(Some("content_filter"), "（已省略）", false), Stop::Filtered);
    }

    #[test]
    fn no_reason_no_text_is_an_empty_reply() {
        // 线上真出过：4 次工具调用后交回一条没有 content 的响应
        assert_eq!(classify(None, "", false), Stop::EmptyReply);
        assert_eq!(classify(Some("stop"), "   \n", false), Stop::EmptyReply);
    }

    /* ---------- 三态文案：{used} 与 {n} 必须真的填进去 ---------- */

    #[test]
    fn empty_after_nudge_names_the_tools_it_called() {
        let used = vec!["read_page".to_string(), "extract_images".to_string()];
        let text = Unfinished::EmptyAfterNudge.text(&used);
        assert!(text.contains("read_page / extract_images"), "{text}");
        assert!(text.contains("没有给出正文"), "{text}");
    }

    #[test]
    fn truncated_says_only_that_it_was_cut() {
        let text = Unfinished::Truncated.text(&[]);
        assert_eq!(text, "这一轮输出被长度截断，内容可能不完整。");
    }

    #[test]
    fn the_loop_guard_counts_the_tool_calls_it_did() {
        let used = vec!["read_page".to_string(); 4];
        let text = Unfinished::Loop.text(&used);
        assert!(text.contains("同样参数连续调了 3 次"), "{text}");
        assert!(text.contains("已执行 4 次工具操作"), "{text}");
        // 不设轮次上限之后，没有任何一句该提「轮次用尽」（「这一轮」是正常措辞）
        assert!(!text.contains("轮次"), "别再提轮次：{text}");
    }

    /* ---------- 两条护栏 ---------- */

    #[test]
    fn the_same_tool_with_the_same_args_trips_on_the_third_call() {
        let mut guard = RepeatGuard::default();
        assert!(!guard.hit("read_page", r#"{"pageId":"p1"}"#));
        assert!(!guard.hit("read_page", r#"{"pageId":"p1"}"#));
        assert!(guard.hit("read_page", r#"{"pageId":"p1"}"#), "第 3 次要停");

        // 参数不同就是另一件事，重新数
        let mut guard = RepeatGuard::default();
        assert!(!guard.hit("read_page", r#"{"pageId":"p1"}"#));
        assert!(!guard.hit("read_page", r#"{"pageId":"p1"}"#));
        assert!(!guard.hit("read_page", r#"{"pageId":"p2"}"#));
        assert!(!guard.hit("list_pages", "{}"));
        assert!(!guard.hit("list_pages", "{}"));
        assert!(guard.hit("list_pages", "{}"));
    }

    #[test]
    fn an_empty_reply_stops_rather_than_spinning_forever() {
        // 白卷只有一种收尾：提醒过一次还是空。不设上限之后不存在
        // 「名额用完所以没提醒」这种情况，别再留着那个分支的说法
        assert_eq!(classify(None, "", false), Stop::EmptyReply);
        let text = Unfinished::EmptyAfterNudge.text(&["read_page".to_string()]);
        assert!(text.contains("read_page"), "{text}");
        assert!(!text.contains("轮次"), "{text}");
    }

    #[test]
    fn filtered_says_it_was_blocked_instead_of_playing_dumb() {
        let text = Unfinished::Filtered.text(&[]);
        assert!(text.contains("安全过滤"), "{text}");
        assert!(text.contains("没有正文"), "{text}");
    }

    #[test]
    fn two_empty_rounds_are_enough() {
        assert!(!too_many_empty(1), "第一次白卷先提醒，不判死");
        assert!(too_many_empty(2), "第二次还白卷就停");
    }
}
