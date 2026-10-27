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

pub fn locate(
    workspace: &Value,
    person_id: &str,
    resume_id: &str,
    page_id: &str,
    view: &str,
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
    out.push_str("- 你可以用工具读取真实内容；不要凭记忆断言简历里写了什么。");
    out
}
