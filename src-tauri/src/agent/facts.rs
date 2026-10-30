//! 页面事实：页数 / 实测 / 达标与否的**唯一来源**。
//!
//! 两个注入点（`context::locate` 的【页面事实】块、`set_page_layout` 的结论行）都从
//! 这里取字符串 —— 各写一遍就会出现「同一页两个页数」，那正是这套机制要防的错。
//!
//! 阈值也只有这一处：`FIT_MAX` / `TOLERANCE_MAX`。前端不复刻，渲染层不复刻。

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

/// 本轮原件。`pages: None` = 读不出（文件损坏或不是 PDF）—— 绝不写成 0。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Origin {
    pub name: String,
    #[serde(default)]
    pub pages: Option<u32>,
}

/// 目标页数从哪来。决定文案里那句括注（「这一轮明确要求」vs「之前要求过」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TargetSource {
    /// 用户这一轮明确要求
    UserThisTurn,
    /// 之前要求过，仍然有效
    UserBefore,
    /// 沿用原件页数（原件本身就不超过 3 页时才有意义）
    FromOrigin,
    /// 用户没说过要几页
    #[default]
    Unset,
}

/// 目标页数的三态 + 来源。`pages: None` 就是 Unset。
#[derive(Debug, Clone, Copy, Default)]
pub struct Target {
    pub pages: Option<u32>,
    pub source: TargetSource,
}

/// 上一次成功测量的残留数值。stale 时带它，文案里 `{c0}` 那个版本号才有真值。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Previous {
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub one_page: f64,
    #[serde(default)]
    pub content_px: f64,
}

/// 前端量的数。只有它知道 DOM，所以「过期」也在前端判，这里只接结论。
///
/// 全字段默认值：前端版本比后端新/旧时少给一两个键，不该让整轮请求解析失败。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageMeasure {
    #[serde(default)]
    pub page_id: String,
    /// ok | stale | unknown
    #[serde(default)]
    pub status: String,
    /// 测量版本号，每次成功测量 +1（前端模块级计数器）
    #[serde(default)]
    pub version: u32,
    /// 内容版本（8 位 hex，前端算）
    #[serde(default)]
    pub content: String,
    /// A4 一页在这个容器里的高度（px）= 纸宽 * 297/210
    #[serde(default)]
    pub one_page: Option<f64>,
    /// 纸面实际高度（px）
    #[serde(default)]
    pub content_px: Option<f64>,
    /// 正文行高（px），用来把溢出量折算成「多少行」
    #[serde(default)]
    pub line_height: Option<f64>,
    /// stale 时带上一次的数
    #[serde(default)]
    pub previous: Option<Previous>,
    /// unknown 的原因：no-preview | fonts | timeout | empty
    #[serde(default)]
    pub reason: Option<String>,
}

/// 一次请求里工具能看到的全部页面事实。组装点只有两个（`chat_send` / `run_turn`）。
#[derive(Debug, Clone, Default)]
pub struct PageFacts {
    pub origin: Option<Origin>,
    pub target: Target,
    pub measure: Option<PageMeasure>,
    /// 当前页名与 kind（file 页不做页数判定）
    pub page_label: String,
    pub page_kind: String,
}

impl PageFacts {
    /// 按方案 §6 的表格组装一份。`chat_send` 与测试都走这里 —— 各抄一遍页名/目标的
    /// 取法，迟早有一处取错。
    ///
    /// 每轮现组：不落库、不进程级缓存。上一次测量的数残留成本次，正是 F-014 要防的错。
    pub fn assemble(
        workspace: &Value,
        person_id: &str,
        resume_id: &str,
        page_id: &str,
        measure: Option<PageMeasure>,
        origin: Option<Origin>,
        turn: Option<u32>,
    ) -> Self {
        let resume = workspace
            .get("people")
            .and_then(Value::as_array)
            .and_then(|list| {
                list.iter()
                    .find(|person| person["id"].as_str() == Some(person_id))
            })
            .and_then(|person| person.get("resumes"))
            .and_then(Value::as_array)
            .and_then(|list| {
                list.iter()
                    .find(|resume| resume["id"].as_str() == Some(resume_id))
            });
        let page = resume
            .and_then(|resume| resume.get("pages"))
            .and_then(Value::as_array)
            .and_then(|list| list.iter().find(|page| page["id"].as_str() == Some(page_id)));

        // 量的不是这一页（用户切页那一瞬间的窗口期）：整份作废。
        // 把上一页的页数当成这一页的实测报出去，比老实说「量不到」错得更远。
        let measure = match measure {
            Some(measure)
                if !measure.page_id.is_empty() && !page_id.is_empty() && measure.page_id != page_id =>
            {
                None
            }
            other => other,
        };

        // 找不到简历时也要给出一份「未设置」的目标：报错交给工具层，
        // 事实块本身不该因为这个整块消失。
        // `turn` 只在这一步有用（判「这一轮 / 之前」），结论落在 `target.source` 上，
        // 所以不必留在结构体里给后续读者看。
        let target = match resume {
            Some(resume) => target_of(resume, origin.as_ref(), turn),
            None => target_of(&Value::Null, origin.as_ref(), turn),
        };

        Self {
            origin,
            target,
            measure,
            page_label: page
                .and_then(|page| page["label"].as_str())
                .unwrap_or_default()
                .to_string(),
            // 缺省空串 = native：`block` 只把 "file" 当整页原件
            page_kind: page
                .and_then(|page| page["kind"].as_str())
                .unwrap_or_default()
                .to_string(),
        }
    }
}

/// 量不到的原因。四句话写在这里，两处注入共用同一份措辞。
///
/// `NoPreview` 这个名字是历史包袱（当年只有预览 DOM 可量）。离屏宿主现在常驻，
/// 前端没给 measure、或这一页还没量到时都会落到这里 —— 真实含义只是「这一页还没量到」，
/// **不是**「预览不可用」。文案与注释都不许再把它说成预览的问题。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Reason {
    #[default]
    NoPreview,
    Fonts,
    Timeout,
    Empty,
}

impl Reason {
    pub fn text(&self) -> &'static str {
        match self {
            Self::NoPreview => "这一页还没量到，纸面可能还在排",
            Self::Fonts => "字体还没加载完",
            Self::Timeout => "测量超时",
            Self::Empty => "这一页是空的，没有可量的内容",
        }
    }

    /// 前端 `reason` 字符串 → 枚举；认不出的一律当「这一页还没量到」。
    pub fn parse(raw: Option<&str>) -> Self {
        match raw {
            Some("fonts") => Self::Fonts,
            Some("timeout") => Self::Timeout,
            Some("empty") => Self::Empty,
            _ => Self::NoPreview,
        }
    }

    /// `测量不可用：{…}` 括号里只要原因本身：前面已经写了「尚未实测」，
    /// 再说一遍「量不到」是同一句话说两次。
    fn brief(&self) -> &'static str {
        match self {
            Self::NoPreview => "这一页还没量到",
            Self::Fonts => "字体还没加载完",
            Self::Timeout => "测量超时",
            Self::Empty => "这一页是空的",
        }
    }
}

/// 把测量折算成判定。这是全仓唯一的 pages 计算。
#[derive(Debug, Clone, Copy)]
pub enum Fit {
    /// 实测页数（≤ FIT_MAX）
    Fit(f64),
    /// 在容忍带内（FIT_MAX < 页数 ≤ TOLERANCE_MAX）
    Tolerance(f64),
    /// 溢出：页数、超出多少页、折成多少行正文
    Overflow { pages: f64, over: f64, lines: u32 },
    Unknown(Reason),
    /// 内容在上次测量之后改过：旧值作废，只留上一次的实测页数供文案引用。
    Stale { pages: f64 },
}

// 阈值只有这一处。前端不许复刻。
pub const FIT_MAX: f64 = 1.00;
pub const TOLERANCE_MAX: f64 = 1.02;

/// paper.css 默认正文行高（`--fs:10px × --lh:1.7`）。前端没上传行高时用它折算行数。
const DEFAULT_LINE_HEIGHT: f64 = 17.0;

/// 实测口径（逐字定稿）：测量发生在预览 DOM 上 —— 374 容器 − 两侧各 34px = 正文 306px。
/// 这里**不写 16mm**，那是「接近整页」面积换算用的打印口径，与实测不是同一把尺子。
pub const MEASURE_RULER: &str =
    "A4 210×297mm、正文宽 306px、页边距 34px、预览容器宽 374px";

/// 完整事实块。会话侧 system 注入用它。
pub fn block(f: &PageFacts) -> String {
    // file 页是用户上传的整页图片，没有可排的空间：只给一句 verdict 就够，
    // 给它编实测/目标槽位会诱导模型去「压」一张图片。
    if f.page_kind == "file" {
        return format!("结论：{}", FILE_VERDICT);
    }

    let mut out = String::from(
        "【页面事实】下面是程序实测的当前值。页数只能照抄这几个数：不要改写、不要四舍五入、不要按比例推算。\n\n",
    );
    out.push_str(&origin_line(f.origin.as_ref()));
    out.push('\n');
    out.push_str("目标页数：");
    out.push_str(&target_line(&f.target, f.origin.as_ref()));
    out.push('\n');
    out.push_str(&format!("这一页「{}」：{}\n", f.page_label, measure_line(f)));
    out.push_str(&format!("结论：{}", verdict_line(f)));
    out
}

/// fit 段（带 `## 这一页的实测` 标题）：比 `measure_line` 多一层小标题，
/// 留给需要「一整段实测」的调用方。
pub fn measure_section(f: &PageFacts) -> String {
    let mut out = String::from("## 这一页的实测\n");
    let fit = fit_of(f.measure.as_ref());

    match fit {
        Fit::Fit(pages) => out.push_str(&format!("{}。\n", measured_sentence(pages, false))),
        Fit::Tolerance(pages) => out.push_str(&format!("{}。\n", measured_sentence(pages, true))),
        Fit::Overflow { pages, over, lines } => {
            // 超出量单独成行：这里是「一行一件事」的版面描述，
            // 与 measure_line 里那句合在一行的写法共用同一个 over_sentence。
            out.push_str(&format!("实测 {pages:.2} 页（{MEASURE_RULER}）。\n"));
            out.push_str(&over_sentence(over, lines));
            out.push('\n');
        }
        Fit::Unknown(_) | Fit::Stale { .. } => {
            out.push_str(&unavailable_head(f, fit));
            out.push('\n');
            out.push_str("不知道有没有达标。这一轮不要说「已经压到一页」「放在一页内」。\n");
            return out;
        }
    }

    match (f.target.pages, f.target.source) {
        // 与 block 的 verdict 同一口径：原件页数默认值不盖「达标」章。
        // 两侧说法不一致就等于又造了第二份判断。
        (Some(t), TargetSource::FromOrigin) => {
            let conclusion = if matches!(fit, Fit::Overflow { .. }) {
                format!("超出原件页数 {t} 页。")
            } else {
                format!("{}，这一页在里面。", origin_target_ref(t))
            };
            out.push_str(&format!(
                "目标页数 {}。{conclusion}\n",
                target_short(&f.target)
            ));
            out.push_str(&format!(
                "{}。内容一改，这些数字作废，要重新量。\n",
                version_line(f)
            ));
        }
        (Some(_), _) => {
            let verdict = match fit {
                Fit::Overflow { .. } => "未达标。",
                _ => "达标。",
            };
            out.push_str(&format!(
                "目标页数 {}。{verdict}\n",
                target_short(&f.target)
            ));
            // 段尾要交代「这些数字会作废」，否则模型会拿旧实测值当永久结论
            out.push_str(&format!(
                "{}。内容一改，这些数字作废，要重新量。\n",
                version_line(f)
            ));
        }
        (None, _) => {
            out.push_str(&format!("目标页数 {}。\n", target_short(&f.target)));
            out.push_str("没有目标，不讲达标与否；实测值仍然要照报。\n");
        }
    }
    out
}

/// 只要 {measure} 那一句。两处共用，保证「实测 1.00 页，没有溢出（口径）」一字不差。
pub fn measure_line(f: &PageFacts) -> String {
    let fit = fit_of(f.measure.as_ref());
    match fit {
        Fit::Fit(pages) => format!("{}。{}", measured_sentence(pages, false), version_line(f)),
        Fit::Tolerance(pages) => format!("{}{}", measured_sentence(pages, true), version_line(f)),
        Fit::Overflow { pages, over, lines } => format!(
            "实测 {pages:.2} 页，{}（{MEASURE_RULER}）{}",
            over_sentence(over, lines),
            version_line(f)
        ),
        Fit::Unknown(_) => format!(
            "{}量不到就不能说这一页「在一页内」。测量版本 无",
            unavailable_head(f, fit)
        ),
        Fit::Stale { .. } => format!("{}测量版本 无", unavailable_head(f, fit)),
    }
}

/// set_page_layout 返回值末尾追加的那一行。三态。
pub fn layout_result_line(f: &PageFacts) -> String {
    let fit = fit_of(f.measure.as_ref());
    let target = f.target.pages;
    let from_origin = f.target.source == TargetSource::FromOrigin;
    match fit {
        Fit::Fit(pages) | Fit::Tolerance(pages) => match target {
            // 原件页数是系统猜的默认值，不是用户的要求：不给「达标」章
            Some(t) if from_origin => format!(
                "实测 {pages:.2} 页，没有溢出（{}，这一页在里面）。",
                origin_target_ref(t)
            ),
            Some(t) => format!("实测 {pages:.2} 页，没有溢出（目标 {t} 页，达标）。"),
            // 未设目标时不能编达标结论，也不编新措辞：两句都取自 §4.4 的定稿句子。
            None => NO_TARGET_JUDGEMENT.replace("{}", &format!("实测 {pages:.2} 页，没有溢出。")),
        },
        Fit::Overflow { pages, over, .. } => match target {
            Some(t) if from_origin => format!(
                "实测 {pages:.2} 页，超出 {over:.2} 页（{}）。继续压，或停在这一档并说清差多少。",
                origin_target_ref(t)
            ),
            Some(t) => format!(
                "实测 {pages:.2} 页，超出 {over:.2} 页（目标 {t} 页，仍未达标）。继续压，或停在这一档并说清差多少。"
            ),
            None => NO_TARGET_JUDGEMENT
                .replace("{}", &format!("实测 {pages:.2} 页，超出 {over:.2} 页。")),
        },
        Fit::Unknown(_) | Fit::Stale { .. } => format!(
            "尚未实测（测量不可用：{}）。这一轮不能确认是否在一页内 —— 不要写「已压到一页」。",
            why_unavailable(fit)
        ),
    }
}

/// 目标页数三态的推导（方案 §2.7）。
pub fn target_of(resume: &Value, origin: Option<&Origin>, turn: Option<u32>) -> Target {
    let pages = resume
        .get("targetPages")
        .and_then(Value::as_u64)
        .map(|n| n as u32);

    if let Some(pages) = pages {
        let written = resume
            .get("targetPagesTurn")
            .and_then(Value::as_u64)
            .map(|n| n as u32);
        // 差 1 算「这一轮」：用户说「压到一页」→ 工具写入 turn=n → 下一轮 turn=n+1，
        // 那一轮正是用户看效果的时候。差 2 起降级为「之前要求过」。
        let source = match (written, turn) {
            (Some(written), Some(current)) if written + 1 >= current => TargetSource::UserThisTurn,
            _ => TargetSource::UserBefore,
        };
        return Target { pages: Some(pages), source };
    }

    // 读不出（None）与 0 不是一回事：`n` 只在 1..=3 时才算「原件就这么短」。
    match origin.and_then(|o| o.pages) {
        Some(n) if (1..=3).contains(&n) => Target {
            pages: Some(n),
            source: TargetSource::FromOrigin,
        },
        _ => Target::default(),
    }
}

/// 测量 → 判定。
pub fn fit_of(m: Option<&PageMeasure>) -> Fit {
    let Some(m) = m else {
        return Fit::Unknown(Reason::NoPreview);
    };

    if m.status == "stale" {
        return match m.previous.as_ref().and_then(previous_pages) {
            Some(pages) => Fit::Stale { pages },
            // 过期却没带上一次的数：两个版本号都报不出来，只能老实说量不到
            None => Fit::Unknown(Reason::NoPreview),
        };
    }

    if m.status != "ok" {
        return Fit::Unknown(Reason::parse(m.reason.as_deref()));
    }

    let Some(pages) = m.one_page.zip(m.content_px).and_then(|(base, px)| {
        if base > 0.0 && px > 0.0 {
            Some(px / base)
        } else {
            None
        }
    }) else {
        return Fit::Unknown(Reason::parse(m.reason.as_deref()));
    };

    if pages <= FIT_MAX {
        return Fit::Fit(pages);
    }
    if pages <= TOLERANCE_MAX {
        return Fit::Tolerance(pages);
    }

    let over_px = m.content_px.unwrap_or(0.0) - m.one_page.unwrap_or(0.0);
    let line_height = match m.line_height {
        Some(lh) if lh > 0.0 => lh,
        _ => DEFAULT_LINE_HEIGHT,
    };
    Fit::Overflow {
        pages,
        over: pages - FIT_MAX,
        lines: (over_px / line_height).round() as u32,
    }
}

const FILE_VERDICT: &str = "这一页是用户上传的整页原件，没有可排的空间，不做页数判定。";

/// 未设目标时 layout_result_line 的句式：两句都是 §4.4 的定稿句子，这里只是拼接。
const NO_TARGET_JUDGEMENT: &str = "{}没有目标页数，不讲达标与否。";

/// 「实测 X 页，没有溢出／在 2% 容忍内…（口径）」——fit 与容忍带共用。
/// `measure_line` 与 `measure_section` 都调它，两处措辞才不会漂。
fn measured_sentence(pages: f64, tolerance: bool) -> String {
    let qualifier = if tolerance {
        "在 2% 容忍内，仍算没有溢出。"
    } else {
        "没有溢出"
    };
    format!("实测 {pages:.2} 页，{qualifier}（{MEASURE_RULER}）")
}

fn over_sentence(over: f64, lines: u32) -> String {
    format!("超出 {over:.2} 页（{lines} 行正文）。")
}

/// 目标来自原件页数（`FromOrigin`）时的参照口径。
///
/// 「达标」只能盖在用户说过的目标上。原件页数是系统按快照给的默认值，不是要求：
/// 用户说了「压到一页」但没被记下来时，照原件 3 页说「达标」就是假绿灯 ——
/// 文案全部合规、只有目标是错的，是最难被发现的一种谎报。
fn origin_target_ref(pages: u32) -> String {
    format!("按原件页数 {pages} 页算")
}

/// `FromOrigin` 的结论句（不出现「达标」二字），verdict 用。
fn from_origin_verdict(pages: u32, overflow: bool) -> String {
    let head = if overflow {
        format!("超出原件页数 {pages} 页。")
    } else {
        format!("{}，这一页在里面。", origin_target_ref(pages))
    };
    // 只能陈述「系统记了什么」，不能替用户作证：本模块看不到用户那句话，
    // 一旦模型漏调 set_target_pages，替用户作证就成了伪证 —— 而这比「目标猜错」更危险，
    // 因为用户与模型都会信它。
    format!("{head}系统里没有记下目标页数。")
}

/// 量不到 / 过期的首句（不含「测量版本」尾注）。
fn unavailable_head(f: &PageFacts, fit: Fit) -> String {
    match fit {
        Fit::Stale { pages } => {
            let previous = f.measure.as_ref().and_then(|m| m.previous.as_ref());
            let old_content = previous.map(|p| p.content.as_str()).unwrap_or("");
            let now_content = f.measure.as_ref().map(|m| m.content.as_str()).unwrap_or("");
            format!(
                "尚未实测（内容在上次测量之后改过，旧测量作废：上次 {pages:.2} 页 / 内容版本 {old_content}，现在 {now_content}）。"
            )
        }
        Fit::Unknown(reason) => format!("尚未实测（{}）。", reason.text()),
        _ => String::new(),
    }
}

/// 量不到的那句「为什么」：unknown 四选一，过期说内容改过。
fn why_unavailable(fit: Fit) -> &'static str {
    match fit {
        Fit::Unknown(reason) => reason.brief(),
        _ => "内容在上次测量之后改过，旧测量作废",
    }
}

fn previous_pages(previous: &Previous) -> Option<f64> {
    if previous.one_page > 0.0 && previous.content_px > 0.0 {
        Some(previous.content_px / previous.one_page)
    } else {
        None
    }
}

fn version_line(f: &PageFacts) -> String {
    match f.measure.as_ref() {
        Some(m) => format!("测量版本 #{}，内容版本 {}", m.version, m.content),
        None => "测量版本 无".to_string(),
    }
}

fn origin_line(origin: Option<&Origin>) -> String {
    match origin {
        None => "原件：这一轮没有上传原件。".to_string(),
        // 读不出与 0 页是两件事：写 0 会让模型以为「原件是空的」，那是它编出来的页数。
        // 所以这里给「读不出」并明确禁止它按 1 页算。
        Some(o) => match o.pages {
            Some(n) => format!("原件：{}，页数 {n} 页（来源：PDF 页树）", o.name),
            None => format!(
                "原件：{}，页数读不出（文件损坏或不是 PDF）。不要按 1 页算，也不要猜。",
                o.name
            ),
        },
    }
}

/// `{target}` 槽位：用户明说 > 原件页数（仅当 ≤3）> 未设置。
fn target_line(target: &Target, origin: Option<&Origin>) -> String {
    match (target.pages, target.source) {
        (Some(n), TargetSource::UserThisTurn) => {
            if n <= 1 {
                "1 页（用户这一轮明确要求：压到一页）".to_string()
            } else {
                format!("{n} 页（用户这一轮明确要求：压到 {n} 页）")
            }
        }
        (Some(n), TargetSource::UserBefore) => {
            if n <= 1 {
                "1 页（用户之前要求过，仍然有效）".to_string()
            } else {
                format!("{n} 页（用户之前要求过，仍然有效：压到 {n} 页）")
            }
        }
        // pages 有值但来源是 Unset 只可能来自不一致的存量数据：有目标就按「之前要求过」说，
        // 不能声称是「这一轮」说的（那是在编时间线）。
        (Some(n), TargetSource::Unset) => {
            if n <= 1 {
                "1 页（用户之前要求过，仍然有效）".to_string()
            } else {
                format!("{n} 页（用户之前要求过，仍然有效：压到 {n} 页）")
            }
        }
        (Some(n), TargetSource::FromOrigin) => {
            format!("{n} 页（沿用原件页数：原件只有 {n} 页；系统里没有记下目标页数）")
        }
        // 未设置不许落成 1 页：模型会把「1」当成用户定的目标，然后为了压到一页
        // 删掉用户没让它删的内容。宁可让它知道「没有目标」，也不给它一个假的默认值。
        //
        // 措辞只陈述「系统里没有记下」，不猜用户说过什么：事实层只能陈述「系统记了什么」，
        // 不能陈述「用户说过什么」—— 后者它无法核实，模型漏调 set_target_pages 时
        // 就会变成伪证，而读它的人（用户与模型）都会信。
        (None, _) => match origin {
            Some(o) if o.pages.is_none() => {
                "未设置（原件页数读不出，系统里也没有记下目标页数）".to_string()
            }
            _ => "未设置（系统里没有记下目标页数；任何页数都合规，不要自行假定一页）".to_string(),
        },
    }
}

/// `## 这一页的实测` 段里的目标行：比 `target_line` 短（不重复「压到 N 页」），
/// 因为段里紧跟着就是实测页数。
fn target_short(target: &Target) -> String {
    match (target.pages, target.source) {
        (Some(n), TargetSource::UserThisTurn) => format!("{n} 页（用户这一轮明确要求）"),
        (Some(n), TargetSource::UserBefore) => format!("{n} 页（用户之前要求过）"),
        (Some(n), TargetSource::Unset) => format!("{n} 页（用户之前要求过）"),
        (Some(n), TargetSource::FromOrigin) => format!("{n} 页（沿用原件页数）"),
        (None, _) => "未设置（系统里没有记下目标页数）".to_string(),
    }
}

/// `{verdict}` 槽位：按状态取一条，不拼接不省略。
fn verdict_line(f: &PageFacts) -> String {
    let fit = fit_of(f.measure.as_ref());
    let pages = match fit {
        Fit::Fit(p) | Fit::Tolerance(p) => p,
        Fit::Overflow { pages, .. } => pages,
        Fit::Unknown(_) | Fit::Stale { .. } => 0.0,
    };

    match fit {
        Fit::Unknown(_) | Fit::Stale { .. } => {
            "不知道：这一页量不到。不要说「一页」「单页」「已经放得下」。".to_string()
        }
        _ => match (f.target.pages, f.target.source) {
            // 原件页数是系统猜的默认值：可以说「这一页在里面」，但不能说「达标」。
            // 「达标」这两个字一旦出现，模型就会转述成「已经压到你要的页数了」。
            (Some(t), TargetSource::FromOrigin) => {
                from_origin_verdict(t, matches!(fit, Fit::Overflow { .. }))
            }
            (Some(t), _) => match fit {
                Fit::Overflow { .. } => format!(
                    "未达标：超出 {t} 页。不要说「已经压到一页」，也不要说「差不多」。"
                ),
                _ => format!("达标：这一页在 {t} 页内。"),
            },
            // 没有目标时也要把实测值说准：overflow 时那句「已经排到第二页上」是定稿原句，
            // fit 时不能沿用（那时并不成立），换成「仍然要照报」。
            (None, _) => match fit {
                Fit::Overflow { .. } => format!(
                    "没有目标页数，不讲达标与否。实测 {pages:.2} 页这件事仍然要说准：它已经排到第二页上。"
                ),
                _ => format!("没有目标页数，不讲达标与否。实测 {pages:.2} 页，仍然要照报。"),
            },
        },
    }
}

/* ==========================================================================
   【模型能力】：也是实测值
   ========================================================================== */

/// 「这个模型能不能看图」的事实块。
///
/// 为什么它必须是一块**事实**而不是提示词里的一句话：能力会变 —— 换 key、换网关、
/// 换模型版本，都可能让同一句话从真变假。写进 `prompts/system.md` 就是写死一个
/// 会过期的断言，而模型会照着那个断言说话（然后用户发现"它说自己能看图，其实看不到"）。
///
/// 三段文案对应三态，**没有第四种**：
///   - 支持：允许它看图说话；
///   - 不支持：禁止它声称能看图，并给出仅有的两种原因说法；
///   - 没测出来：既不承认也不否认，让用户去测一次 —— 这是唯一诚实的第三条路。
pub fn capability_block(cap: &crate::capability::Capability) -> String {
    use crate::capability::State;

    match cap.state {
        State::Supported => "【模型能力】下面这一行是程序实测的，不是猜的。关于「能不能看图」，只能照抄这一行。\n\n支持图像输入。你可以直接看图；用户提到图里的内容时按图作答。".to_string(),
        State::Unsupported => format!(
            "【模型能力】支持状态：这个模型这次实测**不支持**图像输入（{}）。不要声称能看图；需要图片内容时，请用户把内容写成文字，或者换一个能看图的模型。原因只有两种说法：「这个模型不接受图像输入」或「没测出来」——不要自己编第三种。",
            cap.reason.text()
        ),
        State::Unknown => "【模型能力】支持状态：**没测出来**。既不要声称能看图，也不要断言看不到 —— 直接说「这一项还没测出来」，并建议用户去 设置 → 模型 点一次「测一次多模态」。".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn measure(
        status: &str,
        version: u32,
        content: &str,
        one_page: Option<f64>,
        content_px: Option<f64>,
        line_height: Option<f64>,
    ) -> PageMeasure {
        PageMeasure {
            page_id: "p1".into(),
            status: status.into(),
            version,
            content: content.into(),
            one_page,
            content_px,
            line_height,
            previous: None,
            reason: None,
        }
    }

    /// 实测 1.00 页（1000 / 1000）
    fn ok_fit() -> PageMeasure {
        measure("ok", 12, "v37", Some(1000.0), Some(1000.0), Some(17.0))
    }

    /// 实测 1.18 页、超出 0.18 页 → 180px / 17px ≈ 11 行正文
    fn ok_overflow() -> PageMeasure {
        measure("ok", 12, "v37", Some(1000.0), Some(1180.0), Some(17.0))
    }

    fn facts(measure: Option<PageMeasure>, target: Target, origin: Option<Origin>) -> PageFacts {
        PageFacts {
            origin,
            target,
            measure,
            page_label: "第一页".into(),
            page_kind: "native".into(),
        }
    }

    fn origin_pdf(pages: Option<u32>) -> Origin {
        Origin {
            name: "简历.pdf".into(),
            pages,
        }
    }

    fn target(pages: Option<u32>, source: TargetSource) -> Target {
        Target { pages, source }
    }

    #[test]
    fn ruler_is_frozen() {
        // 断言拆成「标签 + 数字」两段：口径串全仓只许出现在 MEASURE_RULER 那一行，
        // 测试里再抄一遍整句就变成第二个真相源了
        assert!(MEASURE_RULER.contains("预览容器宽"));
        assert!(MEASURE_RULER.contains("374px"));
        assert!(MEASURE_RULER.contains("正文宽 306px"));
        assert!(MEASURE_RULER.contains("页边距 34px"));
        // 防回退：860 是早期算错的容器宽，16mm 是打印口径 —— 都不许出现在实测文案里
        assert!(!MEASURE_RULER.contains("860"));
        assert!(!MEASURE_RULER.contains("16mm"));
    }

    #[test]
    fn thresholds_are_frozen() {
        assert_eq!(FIT_MAX, 1.00);
        assert_eq!(TOLERANCE_MAX, 1.02);
    }

    #[test]
    fn measure_parses_partial_payload() {
        // 前端只保证 pageId / status：其余键缺了也不能让整轮请求失败
        let m: PageMeasure =
            serde_json::from_value(serde_json::json!({ "pageId": "p1", "status": "ok" }))
                .expect("部分载荷应能反序列化");
        assert_eq!(m.page_id, "p1");
        assert_eq!(m.status, "ok");
        assert_eq!(m.version, 0);
        assert!(m.one_page.is_none());
        assert!(m.previous.is_none());
    }

    #[test]
    fn origin_reads_camel_case_and_missing_pages() {
        let origin: Origin =
            serde_json::from_value(serde_json::json!({ "name": "简历.pdf" })).unwrap();
        assert_eq!(origin.pages, None);
    }

    #[test]
    fn reason_texts_are_distinct() {
        let all = [Reason::NoPreview, Reason::Fonts, Reason::Timeout, Reason::Empty];
        for (index, reason) in all.iter().enumerate() {
            assert!(!reason.text().is_empty());
            assert_eq!(Reason::parse(Some(reason_code(*reason))), *reason);
            assert!(all[index + 1..].iter().all(|other| other.text() != reason.text()));
        }
    }

    #[test]
    fn reason_texts_never_blame_the_preview() {
        // 离屏宿主常驻之后，「量不到」只剩「这一页还没量到」。
        // 这句文案曾经把模型带进「预览不可用」的坑，别再回来。
        let all = [Reason::NoPreview, Reason::Fonts, Reason::Timeout, Reason::Empty];
        for reason in all {
            assert!(!reason.text().contains("预览"), "{:?}: {}", reason, reason.text());
            assert!(!reason.brief().contains("预览"), "{:?}: {}", reason, reason.brief());
        }
        assert!(Reason::NoPreview.text().contains("还没量到"));
    }

    fn reason_code(reason: Reason) -> &'static str {
        match reason {
            Reason::NoPreview => "no-preview",
            Reason::Fonts => "fonts",
            Reason::Timeout => "timeout",
            Reason::Empty => "empty",
        }
    }

    // ---------------------------------------------------------------- B2-1

    #[test]
    fn fit_boundaries_follow_the_two_thresholds() {
        // 1.00 → fit；1.005 / 1.02 → 容忍带；1.021 → overflow
        assert!(matches!(fit_of(Some(&ok_fit())), Fit::Fit(_)));
        assert!(matches!(
            fit_of(Some(&measure(
                "ok",
                12,
                "v37",
                Some(1000.0),
                Some(1005.0),
                Some(17.0)
            ))),
            Fit::Tolerance(_)
        ));
        assert!(matches!(
            fit_of(Some(&measure(
                "ok",
                12,
                "v37",
                Some(1000.0),
                Some(1020.0),
                Some(17.0)
            ))),
            Fit::Tolerance(_)
        ));
        assert!(matches!(
            fit_of(Some(&measure(
                "ok",
                12,
                "v37",
                Some(1000.0),
                Some(1021.0),
                Some(17.0)
            ))),
            Fit::Overflow { .. }
        ));
    }

    #[test]
    fn overflow_carries_two_digit_over_and_whole_lines() {
        match fit_of(Some(&ok_overflow())) {
            Fit::Overflow { pages, over, lines } => {
                assert_eq!(format!("{pages:.2}"), "1.18");
                assert_eq!(format!("{over:.2}"), "0.18");
                assert_eq!(lines, 11);
            }
            other => panic!("应判为溢出，实际 {other:?}"),
        }
    }

    #[test]
    fn overflow_falls_back_to_default_line_height() {
        // 没有行高就用 paper.css 默认 17px 折算，不许 panic、不许算出 0 行
        let m = measure("ok", 12, "v37", Some(1000.0), Some(1187.0), None);
        match fit_of(Some(&m)) {
            Fit::Overflow { lines, .. } => assert_eq!(lines, 11),
            other => panic!("应判为溢出，实际 {other:?}"),
        }
    }

    /// 期望串里用 `MEASURE_RULER` 变量拼，不抄整句：口径串全仓只有一处定义，
    /// 测试里再写一遍就成了第二个真相源（改了常量测试还会绿）。
    fn ruler() -> &'static str {
        MEASURE_RULER
    }

    #[test]
    fn measure_line_fit_is_verbatim() {
        let f = facts(
            Some(ok_fit()),
            target(Some(1), TargetSource::UserThisTurn),
            Some(origin_pdf(Some(3))),
        );
        assert_eq!(
            measure_line(&f),
            format!("实测 1.00 页，没有溢出（{}）。测量版本 #12，内容版本 v37", ruler())
        );
        assert!(measure_line(&f).contains(MEASURE_RULER));
        assert!(!measure_line(&f).contains("860"));
        assert!(!measure_line(&f).contains("16mm"));
    }

    #[test]
    fn measure_line_tolerance_is_verbatim() {
        let f = facts(
            Some(measure("ok", 12, "v37", Some(1000.0), Some(1010.0), Some(17.0))),
            target(Some(1), TargetSource::UserThisTurn),
            None,
        );
        assert_eq!(
            measure_line(&f),
            format!(
                "实测 1.01 页，在 2% 容忍内，仍算没有溢出。（{}）测量版本 #12，内容版本 v37",
                ruler()
            )
        );
    }

    #[test]
    fn measure_line_overflow_is_verbatim() {
        let f = facts(Some(ok_overflow()), target(Some(1), TargetSource::UserBefore), None);
        assert_eq!(
            measure_line(&f),
            format!(
                "实测 1.18 页，超出 0.18 页（11 行正文）。（{}）测量版本 #12，内容版本 v37",
                ruler()
            )
        );
    }

    #[test]
    fn measure_line_stale_is_verbatim() {
        let mut m = measure("stale", 0, "v37", None, None, Some(17.0));
        m.previous = Some(Previous {
            content: "v36".into(),
            one_page: 1000.0,
            content_px: 1000.0,
        });
        let f = facts(Some(m), target(Some(1), TargetSource::UserThisTurn), None);
        assert_eq!(
            measure_line(&f),
            "尚未实测（内容在上次测量之后改过，旧测量作废：上次 1.00 页 / 内容版本 v36，现在 v37）。测量版本 无"
        );
        assert!(!measure_line(&f).contains("没有溢出"));
    }

    #[test]
    fn measure_line_unknown_never_says_no_overflow() {
        let cases = [
            (None, "这一页还没量到，纸面可能还在排"),
            (
                Some(measure("unknown", 0, "v37", None, None, None)),
                "这一页还没量到，纸面可能还在排",
            ),
        ];
        for (raw, why) in cases {
            let f = facts(raw, target(Some(1), TargetSource::UserThisTurn), None);
            let line = measure_line(&f);
            assert_eq!(
                line,
                format!("尚未实测（{why}）。量不到就不能说这一页「在一页内」。测量版本 无")
            );
            // F-007 的核心：量不到时绝不能出现「没有溢出」
            assert!(!line.contains("没有溢出"));
        }

        let mut m = measure("unknown", 0, "v37", None, None, None);
        m.reason = Some("empty".into());
        let f = facts(Some(m), target(None, TargetSource::Unset), None);
        assert_eq!(
            measure_line(&f),
            "尚未实测（这一页是空的，没有可量的内容）。量不到就不能说这一页「在一页内」。测量版本 无"
        );
    }

    // ---------------------------------------------------------------- B2-2

    /// `Target` 没派生 `PartialEq`（B0-3 定死的形状），测试里比字段。
    fn parts(target: Target) -> (Option<u32>, TargetSource) {
        (target.pages, target.source)
    }

    #[test]
    fn target_of_five_sources() {
        let resume = json!({ "targetPages": 1, "targetPagesTurn": 11 });
        // 用户这一轮明说：turn 差 1
        assert_eq!(
            parts(target_of(&resume, None, Some(12))),
            (Some(1), TargetSource::UserThisTurn)
        );
        // 之前说过：turn 差 2
        assert_eq!(
            parts(target_of(&resume, None, Some(13))),
            (Some(1), TargetSource::UserBefore)
        );
        // 调用点没传 turn → 一律「之前要求过」
        assert_eq!(
            parts(target_of(&resume, None, None)),
            (Some(1), TargetSource::UserBefore)
        );
        // 原件 3 页 → 沿用
        assert_eq!(
            parts(target_of(&json!({}), Some(&origin_pdf(Some(3))), Some(12))),
            (Some(3), TargetSource::FromOrigin)
        );
        // 原件 4 页 → 不沿用，落成未设置（不是 4 页）
        assert_eq!(
            parts(target_of(&json!({}), Some(&origin_pdf(Some(4))), Some(12))),
            (None, TargetSource::Unset)
        );
        // 原件读不出 + 用户没说 → 未设置
        assert_eq!(
            parts(target_of(&json!({}), Some(&origin_pdf(None)), Some(12))),
            (None, TargetSource::Unset)
        );
    }

    #[test]
    fn target_slots_are_verbatim() {
        let origin_readable = origin_pdf(Some(3));
        let origin_unreadable = origin_pdf(None);
        let cases: Vec<(Target, Option<&Origin>, &str)> = vec![
            (
                target(Some(1), TargetSource::UserThisTurn),
                None,
                "1 页（用户这一轮明确要求：压到一页）",
            ),
            (
                target(Some(1), TargetSource::UserBefore),
                None,
                "1 页（用户之前要求过，仍然有效）",
            ),
            (
                target(Some(3), TargetSource::FromOrigin),
                Some(&origin_readable),
                "3 页（沿用原件页数：原件只有 3 页；系统里没有记下目标页数）",
            ),
            (
                target(None, TargetSource::Unset),
                Some(&origin_readable),
                "未设置（系统里没有记下目标页数；任何页数都合规，不要自行假定一页）",
            ),
            (
                target(None, TargetSource::Unset),
                Some(&origin_unreadable),
                "未设置（原件页数读不出，系统里也没有记下目标页数）",
            ),
        ];
        for (t, origin, expected) in cases {
            assert_eq!(target_line(&t, origin), expected);
        }
    }

    #[test]
    fn target_slot_spells_out_n_above_one() {
        assert_eq!(
            target_line(&target(Some(2), TargetSource::UserThisTurn), None),
            "2 页（用户这一轮明确要求：压到 2 页）"
        );
        assert!(target_line(&target(Some(2), TargetSource::UserThisTurn), None)
            .contains("压到 2 页"));
        assert_eq!(
            target_line(&target(Some(2), TargetSource::UserBefore), None),
            "2 页（用户之前要求过，仍然有效：压到 2 页）"
        );
    }

    #[test]
    fn origin_slot_never_writes_zero_pages() {
        let unreadable = facts(
            Some(ok_fit()),
            target(Some(1), TargetSource::UserThisTurn),
            Some(origin_pdf(None)),
        );
        let text = block(&unreadable);
        assert!(text.contains(
            "原件：简历.pdf，页数读不出（文件损坏或不是 PDF）。不要按 1 页算，也不要猜。"
        ));
        assert!(!text.contains("页数 0"));

        let no_attachment = facts(Some(ok_fit()), target(Some(1), TargetSource::UserThisTurn), None);
        assert!(block(&no_attachment).contains("原件：这一轮没有上传原件。"));
    }

    #[test]
    fn block_fit_is_verbatim() {
        let f = facts(
            Some(ok_fit()),
            target(Some(1), TargetSource::UserThisTurn),
            Some(origin_pdf(Some(3))),
        );
        assert_eq!(
            block(&f),
            format!(
                "【页面事实】下面是程序实测的当前值。页数只能照抄这几个数：不要改写、不要四舍五入、不要按比例推算。\n\
                 \n\
                 原件：简历.pdf，页数 3 页（来源：PDF 页树）\n\
                 目标页数：1 页（用户这一轮明确要求：压到一页）\n\
                 这一页「第一页」：实测 1.00 页，没有溢出（{}）。测量版本 #12，内容版本 v37\n\
                 结论：达标：这一页在 1 页内。",
                ruler()
            )
        );
        // 防回退：事实块里不许出现打印口径与早期错的容器宽
        let text = block(&f);
        assert!(!text.contains("16mm"));
        assert!(!text.contains("860"));
    }

    #[test]
    fn block_verdict_covers_the_five_states() {
        let cases = [
            (
                "有目标 + fit",
                facts(Some(ok_fit()), target(Some(1), TargetSource::UserBefore), None),
                "结论：达标：这一页在 1 页内。",
            ),
            (
                "有目标 + overflow",
                facts(
                    Some(ok_overflow()),
                    target(Some(1), TargetSource::UserBefore),
                    None,
                ),
                "结论：未达标：超出 1 页。不要说「已经压到一页」，也不要说「差不多」。",
            ),
            (
                "未设目标 + overflow",
                facts(Some(ok_overflow()), Target::default(), Some(origin_pdf(Some(3)))),
                "结论：没有目标页数，不讲达标与否。实测 1.18 页这件事仍然要说准：它已经排到第二页上。",
            ),
            (
                "未设目标 + fit",
                facts(Some(ok_fit()), Target::default(), None),
                "结论：没有目标页数，不讲达标与否。实测 1.00 页，仍然要照报。",
            ),
            (
                "量不到",
                facts(
                    None,
                    target(Some(1), TargetSource::UserThisTurn),
                    Some(origin_pdf(Some(3))),
                ),
                "结论：不知道：这一页量不到。不要说「一页」「单页」「已经放得下」。",
            ),
        ];
        for (name, f, expected) in cases {
            let text = block(&f);
            assert!(text.contains(expected), "{name} 的结论不对：{text}");
        }
    }

    /// 原件页数是默认值，不是用户的要求：三处文案都不许盖「达标」章。
    /// 三处必须同批（block / 实测段 / set_page_layout 结论），否则同一页两种说法。
    #[test]
    fn a_target_from_the_origin_never_claims_the_goal_was_met() {
        let fit_case = facts(
            Some(ok_fit()),
            target(Some(3), TargetSource::FromOrigin),
            Some(origin_pdf(Some(3))),
        );
        for (name, text) in [
            ("block", block(&fit_case)),
            ("measure_section", measure_section(&fit_case)),
            ("layout_result_line", layout_result_line(&fit_case)),
        ] {
            assert!(!text.contains("达标"), "{name} 不该出现「达标」：{text}");
            assert!(
                text.contains("按原件页数 3 页算"),
                "{name} 要用「按原件页数 3 页算」：{text}"
            );
        }
        assert!(
            block(&fit_case).contains("系统里没有记下目标页数。"),
            "{}",
            block(&fit_case)
        );

        let overflow_case = facts(
            Some(ok_overflow()),
            target(Some(3), TargetSource::FromOrigin),
            Some(origin_pdf(Some(3))),
        );
        // 结论句换成「超出原件页数」：说清超出的是原件页数，不是用户要求的页数
        for (name, text) in [
            ("block", block(&overflow_case)),
            ("measure_section", measure_section(&overflow_case)),
        ] {
            assert!(!text.contains("达标"), "{name} 不该出现「达标」：{text}");
            assert!(
                text.contains("超出原件页数 3 页。"),
                "{name} 要说清超出的是原件页数：{text}"
            );
        }
        let layout = layout_result_line(&overflow_case);
        assert!(!layout.contains("达标"), "{layout}");
        assert!(layout.contains("按原件页数 3 页算"), "{layout}");
    }

    /// 与上一条成对：用户说过的目标才配「达标」，别把两档合并成一句
    #[test]
    fn a_target_the_user_stated_still_gets_the_pass_mark() {
        let user_case = facts(
            Some(ok_fit()),
            target(Some(1), TargetSource::UserThisTurn),
            Some(origin_pdf(Some(3))),
        );
        assert!(block(&user_case).contains("达标：这一页在 1 页内。"));
        assert!(measure_section(&user_case).contains("达标。"));
        assert!(layout_result_line(&user_case).contains("（目标 1 页，达标）"));

        let before_case = facts(
            Some(ok_fit()),
            target(Some(1), TargetSource::UserBefore),
            None,
        );
        assert!(block(&before_case).contains("达标：这一页在 1 页内。"));
        assert!(measure_section(&before_case).contains("达标。"));
        assert!(layout_result_line(&before_case).contains("（目标 1 页，达标）"));
    }

    #[test]
    fn block_unknown_does_not_claim_no_overflow() {        let f = facts(None, target(Some(1), TargetSource::UserThisTurn), None);
        let text = block(&f);
        assert!(text.contains("尚未实测"));
        assert!(!text.contains("没有溢出"));
    }

    // ---------------------------------------------------------------- B2-4

    /// 指定页 id 的实测：1.00 页（传 1180.0 就是溢出）
    fn measure_on(page_id: &str, content_px: f64) -> PageMeasure {
        PageMeasure {
            page_id: page_id.into(),
            status: "ok".into(),
            version: 12,
            content: "v37".into(),
            one_page: Some(1000.0),
            content_px: Some(content_px),
            line_height: Some(17.0),
            ..Default::default()
        }
    }

    #[test]
    fn assemble_reads_label_kind_and_target_from_the_snapshot() {
        let ws = json!({ "people": [{ "id": "p1", "resumes": [{
            "id": "r1",
            "targetPages": 1,
            "targetPagesTurn": 11,
            "pages": [
                { "id": "pg1", "label": "第一页", "kind": "native" },
                { "id": "pg2", "label": "附件页", "kind": "file" }
            ]
        }]}] });

        let facts = PageFacts::assemble(
            &ws,
            "p1",
            "r1",
            "pg1",
            Some(measure_on("pg1", 1000.0)),
            None,
            Some(12),
        );
        assert_eq!(facts.page_label, "第一页");
        assert_eq!(facts.page_kind, "native");
        // turn（第几条用户消息）不进结构体，它的结论就是下面这个 source
        assert_eq!(
            parts(facts.target),
            (Some(1), TargetSource::UserThisTurn)
        );
        assert!(block(&facts).contains("实测 1.00 页"), "{}", block(&facts));

        let file_page = PageFacts::assemble(&ws, "p1", "r1", "pg2", None, None, None);
        assert_eq!(file_page.page_kind, "file");
        // 调用点没传 turn：即使库里写着「这一轮」，也只能说「之前要求过」
        assert_eq!(parts(file_page.target), (Some(1), TargetSource::UserBefore));
    }

    /// 用户切页那一瞬间，测量值还是上一页的：宁可说量不到，也不能把别的页的页数报成这一页
    #[test]
    fn a_measure_from_another_page_is_dropped() {
        let ws = json!({ "people": [{ "id": "p1", "resumes": [{
            "id": "r1",
            "pages": [{ "id": "pg1" }, { "id": "pg2" }]
        }]}] });
        let facts = PageFacts::assemble(
            &ws,
            "p1",
            "r1",
            "pg2",
            Some(measure_on("pg1", 1000.0)),
            None,
            Some(1),
        );
        assert!(facts.measure.is_none());
        let text = block(&facts);
        assert!(text.contains("尚未实测"), "{text}");
        assert!(!text.contains("实测 1.00 页"), "{text}");
    }

    #[test]
    fn assemble_survives_a_missing_resume() {
        // 快照里找不到简历时不许 panic，也不许把目标捏成 1 页
        let facts = PageFacts::assemble(&json!({ "people": [] }), "p1", "r1", "pg1", None, None, None);
        assert_eq!(parts(facts.target), (None, TargetSource::Unset));
        assert_eq!(facts.page_label, "");
        assert_eq!(facts.page_kind, "");
        assert!(block(&facts).contains("【页面事实】"));
    }

    #[test]
    fn assemble_keeps_the_origin_snapshot() {
        let ws = json!({ "people": [{ "id": "p1", "resumes": [{ "id": "r1", "pages": [] }] }] });
        let facts = PageFacts::assemble(
            &ws,
            "p1",
            "r1",
            "pg1",
            None,
            Some(origin_pdf(Some(3))),
            Some(1),
        );
        // 原件只有 3 页 → 沿用（用户没说页数时）
        assert_eq!(parts(facts.target), (Some(3), TargetSource::FromOrigin));
        assert!(block(&facts).contains("页数 3 页（来源：PDF 页树）"));
    }

    #[test]
    fn block_file_page_injects_only_the_verdict() {
        let mut f = facts(
            Some(ok_fit()),
            target(Some(1), TargetSource::UserThisTurn),
            Some(origin_pdf(Some(3))),
        );
        f.page_kind = "file".into();
        assert_eq!(
            block(&f),
            "结论：这一页是用户上传的整页原件，没有可排的空间，不做页数判定。"
        );
        let text = block(&f);
        // 整块短路：不实测、不判目标，也不新造槽位
        assert!(!text.contains("实测"));
        assert!(!text.contains("目标页数"));
        assert!(!text.contains("【页面事实】"));
    }

    // ---------------------------------------------------------------- B2-3

    #[test]
    fn measure_section_fit_is_verbatim() {
        let f = facts(
            Some(ok_fit()),
            target(Some(1), TargetSource::UserThisTurn),
            None,
        );
        assert_eq!(
            measure_section(&f),
            format!(
                "## 这一页的实测\n\
                 实测 1.00 页，没有溢出（{}）。\n\
                 目标页数 1 页（用户这一轮明确要求）。达标。\n\
                 测量版本 #12，内容版本 v37。内容一改，这些数字作废，要重新量。\n",
                ruler()
            )
        );
    }

    #[test]
    fn measure_section_overflow_inserts_the_over_line() {
        let f = facts(
            Some(ok_overflow()),
            target(Some(1), TargetSource::UserThisTurn),
            None,
        );
        assert_eq!(
            measure_section(&f),
            format!(
                "## 这一页的实测\n\
                 实测 1.18 页（{}）。\n\
                 超出 0.18 页（11 行正文）。\n\
                 目标页数 1 页（用户这一轮明确要求）。未达标。\n\
                 测量版本 #12，内容版本 v37。内容一改，这些数字作废，要重新量。\n",
                ruler()
            )
        );
    }

    #[test]
    fn measure_section_unknown_is_not_fit() {
        let f = facts(None, target(Some(1), TargetSource::UserThisTurn), None);
        let text = measure_section(&f);
        assert!(text.starts_with("## 这一页的实测"));
        assert_eq!(
            text,
            "## 这一页的实测\n\
             尚未实测（这一页还没量到，纸面可能还在排）。\n\
             不知道有没有达标。这一轮不要说「已经压到一页」「放在一页内」。\n"
        );
        assert!(!text.contains("没有溢出"));
        assert_ne!(text, measure_section(&facts(Some(ok_fit()), Target::default(), None)));
    }

    #[test]
    fn measure_section_without_target_replaces_the_last_two_lines() {
        let f = facts(Some(ok_fit()), Target::default(), Some(origin_pdf(Some(3))));
        let text = measure_section(&f);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "## 这一页的实测");
        assert!(lines[1].contains("实测 1.00 页，没有溢出"));
        assert!(lines[2].contains("未设置（系统里没有记下目标页数）"));
        assert!(lines[3].contains("没有目标，不讲达标与否；实测值仍然要照报。"));
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn measure_section_shares_the_measured_sentence_with_measure_line() {
        // 同一份 PageFacts：把 measure_line 的版本尾注切掉，剩下的必须原样出现在实测段里
        let f = facts(Some(ok_fit()), target(Some(1), TargetSource::UserThisTurn), None);
        let line = measure_line(&f);
        let shared = &line[..line.find("测量版本").expect("fit 文案带版本号")];
        assert_eq!(
            shared,
            format!("实测 1.00 页，没有溢出（{}）。", ruler())
        );
        assert!(measure_section(&f).contains(shared));
    }

    #[test]
    fn layout_result_line_three_states_verbatim() {
        let fit = facts(Some(ok_fit()), target(Some(1), TargetSource::UserBefore), None);
        assert_eq!(
            layout_result_line(&fit),
            "实测 1.00 页，没有溢出（目标 1 页，达标）。"
        );

        let overflow = facts(
            Some(measure("ok", 12, "v37", Some(1000.0), Some(1120.0), Some(17.0))),
            target(Some(1), TargetSource::UserBefore),
            None,
        );
        assert_eq!(
            layout_result_line(&overflow),
            "实测 1.12 页，超出 0.12 页（目标 1 页，仍未达标）。继续压，或停在这一档并说清差多少。"
        );

        let unknown = facts(None, target(Some(1), TargetSource::UserBefore), None);
        assert_eq!(
            layout_result_line(&unknown),
            "尚未实测（测量不可用：这一页还没量到）。这一轮不能确认是否在一页内 —— 不要写「已压到一页」。"
        );
        // 量不到的措辞必须与达标明显不同，且不许出现「没有溢出」
        assert_ne!(layout_result_line(&unknown), layout_result_line(&fit));
        assert!(!layout_result_line(&unknown).contains("没有溢出"));
    }

    #[test]
    fn layout_result_line_without_target_does_not_judge() {
        let f = facts(Some(ok_fit()), Target::default(), None);
        assert_eq!(
            layout_result_line(&f),
            "实测 1.00 页，没有溢出。没有目标页数，不讲达标与否。"
        );
        assert!(!layout_result_line(&f).contains("达标）"));
    }
}
