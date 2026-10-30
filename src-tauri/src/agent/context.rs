//! 每轮注入的「定位头」。
//!
//! 为什么单独一块、而且每轮都重算：会话是按候选人隔离的，用户可能在会话中途
//! 切到另一份简历、另一页、或者从编辑切到预览。模型必须知道「你现在看的是哪个
//! 候选人的哪份简历的第几页」，否则会说「你刚才那份简历」这种不可靠的话。
//!
//! 注意：定位头是「位置」，页内容由前端的 context 段给，两者不重复。

use serde_json::Value;

/// 从工作区快照里查名字。查不到就用占位符，不让整轮对话失败。
fn text(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or("").trim().to_string()
}

fn display_name(fields: &Value) -> String {
    // 与前端 displayName 一致：第一个有值的字段
    fields
        .as_array()
        .and_then(|list| {
            list.iter()
                .find(|field| field["value"].as_str().map(|v| !v.trim().is_empty()).unwrap_or(false))
                .map(|field| {
                    let value = text(field, "value");
                    let label = text(field, "label");
                    if label.is_empty() { value } else { format!("{label}：{value}") }
                })
        })
        .unwrap_or_else(|| "（未填写）".to_string())
}

/// 这一页现在的版面状态，一行说清。
///
/// 只陈述事实（预设 / 页头 / 手写了几块 / 有没有整页接管），不建议下一步做什么 ——
/// 判断留给模型。不说清的话它会重复改同一块，或者把已有覆盖整片冲掉。
fn layout_line(page: &Value) -> String {
    let layout = page.get("layout");
    let field = |key: &str| {
        layout
            .and_then(|value| value.get(key))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let preset = field("preset");
    let head = field("head");
    let html = field("html");
    let parts = layout
        .and_then(|value| value.get("parts"))
        .and_then(Value::as_object)
        .map(|map| map.len())
        .unwrap_or(0);

    // 整页接管时 preset / head / parts 都不生效，但 preset 仍是这条数据的现状，
    // 一并说出来，模型才不会以为「没写预设」。
    if !html.is_empty() {
        return if preset.is_empty() {
            "- 版面：整页接管（html）\n".to_string()
        } else {
            format!("- 版面：preset={preset} · 整页接管（html）\n")
        };
    }

    let mut bits: Vec<String> = Vec::new();
    if !preset.is_empty() {
        bits.push(format!("preset={preset}"));
    }
    if !head.is_empty() {
        bits.push(format!("页头={head}"));
    }
    if parts > 0 {
        bits.push(format!("{parts} 块手写覆盖"));
    }
    if bits.is_empty() {
        "- 版面：生成器默认（未接管）\n".to_string()
    } else {
        format!("- 版面：{}\n", bits.join(" · "))
    }
}

/// 这一页的原件指针（`stored`）。挂在**简历**上：页的 `layout` 每轮被整体覆盖，
/// 原件不会跟着变。空串/缺字段就是没有原件，不要凭这个字段的有无去催模型。
fn origin_stored(resume: &Value) -> Option<String> {
    resume
        .get("originDoc")
        .and_then(|doc| doc.get("stored"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub fn locate(
    workspace: &Value,
    person_id: &str,
    resume_id: &str,
    page_id: &str,
    view: &str,
    facts: &crate::agent::facts::PageFacts,
    capability: &crate::capability::Capability,
) -> String {
    let person = workspace["people"]
        .as_array()
        .and_then(|list| list.iter().find(|item| text(item, "id") == person_id));

    let mut out = String::from("当前定位（每轮都会刷新，以这里为准）：\n");

    match person {
        Some(person) => {
            out.push_str(&format!("- 候选人：{}（id: {}）\n", display_name(&person["fields"]), person_id));
            let resumes = person["resumes"].as_array().cloned().unwrap_or_default();
            let resume = resumes.iter().find(|item| text(item, "id") == resume_id);
            match resume {
                Some(resume) => {
                    let pages = resume["pages"].as_array().cloned().unwrap_or_default();
                    out.push_str(&format!(
                        "- 简历：《{}》（id: {}，共 {} 页）\n",
                        {
                            let title = text(resume, "title");
                            if title.is_empty() { "未命名".to_string() } else { title }
                        },
                        resume_id,
                        pages.len()
                    ));
                    let index = pages.iter().position(|page| text(page, "id") == page_id);
                    match index.and_then(|i| pages.get(i).map(|page| (i, page))) {
                        Some((i, page)) => {
                            let label = text(page, "label");
                            out.push_str(&format!(
                                "- 当前页：第 {} 页 · {}（id: {}）\n",
                                i + 1,
                                if label.is_empty() { "未命名".to_string() } else { label },
                                page_id
                            ));
                            // 这一页「现在是什么状态」与「它从哪来」：都是位置与事实，
                            // 页内容仍由前端的 context 段给，两处不重复。
                            out.push_str(&layout_line(page));
                            // 整页原件页本身就是原件，叫它去复刻自己没有意义
                            if text(page, "kind") != "file" {
                                if let Some(stored) = origin_stored(resume) {
                                    out.push_str(&format!(
                                        "- 这一页有原件：调 render_doc_page（stored = {stored}、page = 1）看版式，按它复刻\n"
                                    ));
                                }
                            }
                        }
                        None => {
                            out.push_str(&format!("- 当前页：id = {page_id}（在这份简历里找不到）\n"));
                        }
                    }
                }
                None => out.push_str(&format!("- 简历：id = {resume_id}（在候选人名下找不到）\n")),
            }
        }
        None => out.push_str(&format!("- 候选人：id = {person_id}（工作区快照里没有）\n")),
    }

    out.push_str(&format!("- 视图：{}\n", if view == "preview" { "预览" } else { "编辑" }));
    // 页数事实紧跟定位：模型读到「你在第几页」的下一句就是「这页实测多少、有没有目标」。
    // 文案全在 facts 模块里，这里只负责放位置 —— 两处各写一遍就会出现同一页两个页数。
    out.push('\n');
    out.push_str(&crate::agent::facts::block(facts));
    // 能力也是实测值，跟页面事实同一处注入、同一个原则（照抄，不要自己推）。
    // 它决定模型这一轮该不该说「我看得到图」—— 写进提示词就会随换 key / 换网关过期。
    out.push('\n');
    out.push_str(&crate::agent::facts::capability_block(capability));
    out.push_str("\n- 你可以用工具读取真实内容；不要凭记忆断言简历里写了什么。");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::facts::{measure_line, PageFacts, PageMeasure};

    /// 测试默认用「没测出来」：它不该在页面事实的断言之外多说任何话
    fn unknown() -> crate::capability::Capability {
        crate::capability::Capability::default()
    }

    fn workspace() -> Value {
        serde_json::json!({
            "people": [{
                "id": "p1",
                "fields": [{ "label": "姓名", "value": "张三" }],
                "resumes": [{
                    "id": "r1",
                    "title": "简历",
                    "pages": [{ "id": "pg1", "label": "第一页", "kind": "native" }]
                }]
            }]
        })
    }

    /// 实测 1.00 页；传 1010.0 就是容忍带
    fn measure(content_px: f64) -> PageMeasure {
        PageMeasure {
            page_id: "pg1".into(),
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
    fn facts_block_sits_between_the_view_line_and_the_closing_line() {
        let ws = workspace();
        let facts = PageFacts::assemble(&ws, "p1", "r1", "pg1", Some(measure(1000.0)), None, Some(1));
        let text = locate(&ws, "p1", "r1", "pg1", "edit", &facts, &unknown());

        // 只能出现一次：注入点加了两处的话模型会看到两个页数
        assert_eq!(text.match_indices("【页面事实】").count(), 1, "{text}");
        let at = text.find("【页面事实】").expect("要有事实块");
        assert!(text.find("- 视图：编辑").unwrap() < at, "{text}");
        assert!(at < text.find("- 你可以用工具读取真实内容").unwrap(), "{text}");
        // 文案本身来自 facts：测试只证明「同一份」，不重抄句子
        assert!(text.contains(&measure_line(&facts)), "{text}");
    }

    #[test]
    fn without_measure_it_says_cannot_measure_instead_of_disappearing() {
        let ws = workspace();
        let facts = PageFacts::assemble(&ws, "p1", "r1", "pg1", None, None, Some(1));
        let text = locate(&ws, "p1", "r1", "pg1", "preview", &facts, &unknown());

        assert!(text.contains("【页面事实】"), "没有测量也要有事实块：{text}");
        assert!(text.contains("尚未实测"), "{text}");
        // 量不到时不许说「没有溢出」—— 那是上一批卡的核心约束
        assert!(!text.contains("没有溢出"), "{text}");
        assert!(text.contains("原件：这一轮没有上传原件。"), "{text}");
    }

    /// 同一份快照连跑两次、第二次带新测量：输出必须换值。
    /// 进程级缓存（或把 measure 存进 workspace）会让这条红。
    #[test]
    fn every_request_carries_its_own_measure() {
        let ws = workspace();
        let before = PageFacts::assemble(&ws, "p1", "r1", "pg1", Some(measure(1000.0)), None, Some(1));
        let after = PageFacts::assemble(&ws, "p1", "r1", "pg1", Some(measure(1010.0)), None, Some(2));

        let first = locate(&ws, "p1", "r1", "pg1", "edit", &before, &unknown());
        let second = locate(&ws, "p1", "r1", "pg1", "edit", &after, &unknown());

        assert!(first.contains(&measure_line(&before)), "{first}");
        assert!(second.contains(&measure_line(&after)), "{second}");
        assert_ne!(first, second, "第二次必须用新测量值");
    }

    /* ---------- 版面现状：三种形态各钉一条 ---------- */

    fn located(ws: &Value) -> String {
        let facts = PageFacts::assemble(ws, "p1", "r1", "pg1", None, None, Some(1));
        locate(ws, "p1", "r1", "pg1", "edit", &facts, &unknown())
    }

    /// 结构化排版：预设 + 页头 + 几块手写，三段都要在，缺的段不许占位
    #[test]
    fn layout_line_names_preset_head_and_handwritten_blocks() {
        let mut ws = workspace();
        ws["people"][0]["resumes"][0]["pages"][0]["layout"] = serde_json::json!({
            "preset": "compact",
            "head": "aside",
            "parts": { "b1": "<p>x</p>", "b2": "<p>y</p>", "b3": "<p>z</p>" }
        });
        let text = located(&ws);
        assert!(
            text.contains("- 版面：preset=compact · 页头=aside · 3 块手写覆盖"),
            "{text}"
        );

        // 没设页头就不许凭空印一句「页头=」
        ws["people"][0]["resumes"][0]["pages"][0]["layout"] =
            serde_json::json!({ "preset": "compact", "parts": { "b1": "<p>x</p>" } });
        let text = located(&ws);
        assert!(text.contains("- 版面：preset=compact · 1 块手写覆盖"), "{text}");
        assert!(!text.contains("页头="), "{text}");
    }

    /// 整页接管：模型必须知道生成器已经不介入了，否则它会去改 parts
    #[test]
    fn layout_line_says_whole_page_takeover() {
        let mut ws = workspace();
        ws["people"][0]["resumes"][0]["pages"][0]["layout"] = serde_json::json!({
            "preset": "compact",
            "html": "<section>接管</section>"
        });
        let text = located(&ws);
        assert!(text.contains("- 版面：preset=compact · 整页接管（html）"), "{text}");
        assert!(!text.contains("块手写覆盖"), "{text}");
    }

    /// 老数据没有 layout 字段：说「生成器默认」，不许留空也不许报错
    #[test]
    fn layout_line_defaults_to_generator_for_old_pages() {
        let text = located(&workspace());
        assert!(text.contains("- 版面：生成器默认（未接管）"), "{text}");
    }

    /// 有原件就必须给指针：这是被删掉的排版 agent 每轮都能看到的那句话，
    /// 合并之后只能由定位头补上，否则模型永远不知道该去核对原件版式
    #[test]
    fn origin_pointer_is_given_when_the_resume_has_one() {
        let mut ws = workspace();
        ws["people"][0]["resumes"][0]["originDoc"] = serde_json::json!({
            "stored": "abcd1234.pdf",
            "name": "简历.pdf",
            "pages": 3
        });
        let text = located(&ws);
        assert!(
            text.contains(
                "- 这一页有原件：调 render_doc_page（stored = abcd1234.pdf、page = 1）看版式，按它复刻"
            ),
            "{text}"
        );

        // 整页原件页本身就是原件：叫它去复刻自己没意义
        ws["people"][0]["resumes"][0]["pages"][0]["kind"] = serde_json::json!("file");
        let text = located(&ws);
        assert!(!text.contains("这一页有原件"), "{text}");
    }

    /// 没有原件时一个字都不提，免得模型去找一张不存在的图
    #[test]
    fn origin_pointer_is_silent_without_one() {
        let text = located(&workspace());
        assert!(!text.contains("render_doc_page"), "{text}");
        assert!(!text.contains("这一页有原件"), "{text}");
    }
}
