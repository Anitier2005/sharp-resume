//! 页面事实：页数 / 实测 / 达标与否的**唯一来源**。
//!
//! 会话侧（`context::locate` 注入的【页面事实】块）与排版侧（`describe_page` 的
//! `## 这一页的实测` 段、`set_page_layout` 的结论行）都从这里取字符串 —— 两处各写一遍
//! 就会出现「同一页两个页数」，那正是这套机制要防的错。
//!
//! 阈值也只有这一处：`FIT_MAX` / `TOLERANCE_MAX`。前端不复刻，渲染层不复刻。

// Batch 0 只落骨架：文案与判定是 Batch 2 的活，在那之前这些类型没人调用。
// Batch 2 接线完成后请删掉这一行，让未使用的东西重新报警。
#![allow(dead_code)]

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
    /// 第几条用户消息（排版侧为 None）
    pub turn: Option<u32>,
}

/// 量不到的原因。四句话写在这里，两处注入共用同一份措辞。
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
            Self::NoPreview => "预览没打开，量不到",
            Self::Fonts => "字体还没加载完",
            Self::Timeout => "测量超时",
            Self::Empty => "这一页是空的，没有可量的内容",
        }
    }

    /// 前端 `reason` 字符串 → 枚举；认不出的一律当「预览没打开」。
    pub fn parse(raw: Option<&str>) -> Self {
        match raw {
            Some("fonts") => Self::Fonts,
            Some("timeout") => Self::Timeout,
            Some("empty") => Self::Empty,
            _ => Self::NoPreview,
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

/// 实测口径（逐字定稿）：测量发生在预览 DOM 上 —— 374 容器 − 两侧各 34px = 正文 306px。
/// 这里**不写 16mm**，那是「接近整页」面积换算用的打印口径，与实测不是同一把尺子。
pub const MEASURE_RULER: &str =
    "A4 210×297mm、正文宽 306px、页边距 34px、预览容器宽 374px";

/// 完整事实块。会话侧 system 注入用它。
pub fn block(_f: &PageFacts) -> String {
    String::new()
}

/// fit 段（带 `## 这一页的实测` 标题）。排版侧 describe_page 末尾追加用它。
pub fn measure_section(_f: &PageFacts) -> String {
    String::new()
}

/// 只要 {measure} 那一句。两处共用，保证「实测 1.00 页，没有溢出（口径）」一字不差。
pub fn measure_line(_f: &PageFacts) -> String {
    String::new()
}

/// set_page_layout 返回值末尾追加的那一行。三态。
pub fn layout_result_line(_f: &PageFacts) -> String {
    String::new()
}

/// 目标页数三态的推导（方案 §2.7）。
pub fn target_of(_resume: &Value, _origin: Option<&Origin>, _turn: Option<u32>) -> Target {
    Target::default()
}

/// 测量 → 判定。骨架阶段一律「量不到」。
pub fn fit_of(_m: Option<&PageMeasure>) -> Fit {
    Fit::Unknown(Reason::NoPreview)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn reason_code(reason: Reason) -> &'static str {
        match reason {
            Reason::NoPreview => "no-preview",
            Reason::Fonts => "fonts",
            Reason::Timeout => "timeout",
            Reason::Empty => "empty",
        }
    }
}
