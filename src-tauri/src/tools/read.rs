//! 只读工具：候选人、简历页、经历库。
//!
//! 输出刻意用「人话 + 缩进」而不是 JSON：
//!   - 模型读这种格式比读 JSON 稳，也不会把 id 当内容抄进简历
//!   - token 更省（一个字段一行）
//! 但 id 必须留在里面 —— 后续写工具要靠它定位。

use serde_json::{json, Value};

use super::ToolCtx;

pub fn no_params() -> Value {
    json!({ "type": "object", "properties": {} })
}

pub fn page_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "pageId": {
                "type": "string",
                "description": "要读的页面 id。不传就是用户当前所在的那一页。"
            }
        }
    })
}

pub fn library_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "keyword": { "type": "string", "description": "只看字段或记录里包含这个词的条目，可省略" },
            "limit": { "type": "integer", "description": "最多返回几条，可省略（默认全部）" }
        }
    })
}

/* ---------- 取值辅助 ---------- */

fn person<'a>(ctx: &ToolCtx<'a>) -> Result<&'a Value, String> {
    ctx.workspace["people"]
        .as_array()
        .and_then(|list| list.iter().find(|item| item["id"].as_str() == Some(ctx.person_id)))
        .ok_or_else(|| "取不到当前候选人：工作区快照里没有这个 id".to_string())
}

fn resume<'a>(person: &'a Value, resume_id: &str) -> Result<&'a Value, String> {
    person["resumes"]
        .as_array()
        .and_then(|list| list.iter().find(|item| item["id"].as_str() == Some(resume_id)))
        .ok_or_else(|| "取不到当前简历：快照里这个候选人名下没有该 id".to_string())
}

fn items_of(value: &Value) -> Vec<Value> {
    value.as_array().cloned().unwrap_or_default()
}

fn text_of(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or("").trim().to_string()
}

/// 一行一个字段，空值不占行
fn field_lines(fields: &Value) -> String {
    items_of(fields)
        .iter()
        .filter_map(|field| {
            let label = text_of(field, "label");
            let value = text_of(field, "value");
            match (label.is_empty(), value.is_empty()) {
                (true, true) => None,
                (true, false) => Some(format!("- {value}")),
                (false, true) => Some(format!("- {label}：")),
                (false, false) => Some(format!("- {label}：{value}")),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 一行内并列，「公司：X ｜ 职位：Y」
fn inline_fields(fields: &Value) -> String {
    items_of(fields)
        .iter()
        .filter_map(|field| {
            let label = text_of(field, "label");
            let value = text_of(field, "value");
            if value.is_empty() {
                return None;
            }
            Some(if label.is_empty() { value } else { format!("{label}：{value}") })
        })
        .collect::<Vec<_>>()
        .join(" ｜ ")
}

/// 条目字段为空时继承库条目那份 —— 与前端 `resolveEntries` 同一套规则
fn effective_fields<'a>(entry: &'a Value, person: &'a Value) -> (&'a Value, Option<String>) {
    let own = entry["fields"].as_array().map(Vec::len).unwrap_or(0);
    if own > 0 {
        return (&entry["fields"], None);
    }
    if let Some(rid) = entry["refId"].as_str() {
        if let Some(item) = person["items"]
            .as_array()
            .and_then(|list| list.iter().find(|item| item["id"].as_str() == Some(rid)))
        {
            return (&item["fields"], Some(rid.to_string()));
        }
        return (&entry["fields"], Some(rid.to_string()));
    }
    (&entry["fields"], None)
}

fn label_of(value: &Value) -> String {
    let label = text_of(value, "label");
    if label.is_empty() { "未命名".to_string() } else { label }
}

/// 简历这一级的名字字段叫 `title`（页和分区才叫 `label`），取错就会显示「未命名」
fn title_of(value: &Value) -> String {
    let title = text_of(value, "title");
    if title.is_empty() { "未命名".to_string() } else { title }
}

fn truncate(text: &str, limit: usize) -> String {
    let mut out = String::new();
    for (index, ch) in text.chars().enumerate() {
        if index >= limit {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

/* ---------- 工具 ---------- */

pub fn read_person(_args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let person = person(ctx)?;
    let mut out = String::new();
    out.push_str(&format!("候选人 id：{}\n", ctx.person_id));

    let fields = field_lines(&person["fields"]);
    if fields.is_empty() {
        out.push_str("字段：（一个都没有，用户还没填）\n");
    } else {
        out.push_str("字段：\n");
        out.push_str(&fields);
        out.push('\n');
    }

    let resumes = items_of(&person["resumes"]);
    out.push_str(&format!("\n简历 {} 份：\n", resumes.len()));
    for item in &resumes {
        let pages = item["pages"].as_array().map(Vec::len).unwrap_or(0);
        let mark = if item["id"].as_str() == Some(ctx.resume_id) { " ← 当前" } else { "" };
        out.push_str(&format!(
            "- 《{}》{} 页（id: {}）{}\n",
            title_of(item),
            pages,
            text_of(item, "id"),
            mark
        ));
    }

    out.push_str(&format!(
        "\n经历库 {} 条（用 read_library 看内容）",
        items_of(&person["items"]).len()
    ));
    Ok(out)
}

pub fn list_pages(_args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let person = person(ctx)?;
    let resume = resume(person, ctx.resume_id)?;
    let pages = items_of(&resume["pages"]);
    if pages.is_empty() {
        return Ok(format!("《{}》还没有任何页面。", title_of(resume)));
    }
    let mut out = format!("《{}》共 {} 页：\n", title_of(resume), pages.len());
    for (index, page) in pages.iter().enumerate() {
        let mark = if page["id"].as_str() == Some(ctx.page_id) { " ← 当前" } else { "" };
        out.push_str(&format!(
            "{}. {}{}（id: {}）\n",
            index + 1,
            label_of(page),
            mark,
            text_of(page, "id")
        ));
    }
    Ok(out.trim_end().to_string())
}

pub fn read_page(args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let person = person(ctx)?;
    let resume = resume(person, ctx.resume_id)?;
    let pages = items_of(&resume["pages"]);
    let wanted = args["pageId"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(ctx.page_id);

    let (index, page) = pages
        .iter()
        .enumerate()
        .find(|(_, page)| page["id"].as_str() == Some(wanted))
        .ok_or_else(|| format!("《{}》里没有页面 id = {wanted}，先用 list_pages 看清楚", title_of(resume)))?;

    let mut out = String::new();
    out.push_str(&format!(
        "《{}》第 {} 页 · {}（id: {}）",
        title_of(resume),
        index + 1,
        label_of(page),
        text_of(page, "id")
    ));
    if page["id"].as_str() == Some(ctx.page_id) {
        out.push_str(" ← 用户正在看这一页");
    }
    out.push('\n');

    let header = inline_fields(&page["header"]);
    out.push_str(if header.is_empty() {
        "页头：（空）\n"
    } else {
        "页头："
    });
    if !header.is_empty() {
        out.push_str(&header);
        out.push('\n');
    }

    let sections = items_of(&page["sections"]);
    if sections.is_empty() {
        out.push_str("\n（这一页还没有任何分区）");
        return Ok(out);
    }

    for (si, section) in sections.iter().enumerate() {
        out.push_str(&format!(
            "\n分区 {}「{}」（id: {}）\n",
            si + 1,
            label_of(section),
            text_of(section, "id")
        ));
        let entries = items_of(&section["items"]);
        if entries.is_empty() {
            out.push_str("  （空）\n");
            continue;
        }
        for (ei, entry) in entries.iter().enumerate() {
            let (fields, ref_id) = effective_fields(entry, person);
            let line = inline_fields(fields);
            let suffix = match &ref_id {
                Some(id) => format!("（引用库条目 {id}）"),
                None => "（本页临时写的）".to_string(),
            };
            if line.is_empty() {
                out.push_str(&format!("  {}. （没有字段）{suffix}\n", ei + 1));
            } else {
                out.push_str(&format!("  {}. {line}{suffix}\n", ei + 1));
            }
            let bullets = item_strings(&entry["bullets"]);
            if bullets.is_empty() {
                out.push_str("     （这一条还没有措辞）\n");
            } else {
                for bullet in bullets {
                    out.push_str(&format!("     · {bullet}\n"));
                }
            }
        }
    }
    Ok(out.trim_end().to_string())
}

pub fn read_library(args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let person = person(ctx)?;
    let all = items_of(&person["items"]);
    let keyword = text_of(args, "keyword").to_lowercase();
    let limit = args["limit"].as_u64().map(|value| value as usize).unwrap_or(usize::MAX);

    let mut picked: Vec<&Value> = Vec::new();
    for item in &all {
        if !keyword.is_empty() {
            let haystack = format!("{} {}", inline_fields(&item["fields"]), text_of(item, "notes")).to_lowercase();
            if !haystack.contains(&keyword) {
                continue;
            }
        }
        picked.push(item);
        if picked.len() >= limit {
            break;
        }
    }

    if picked.is_empty() {
        return Ok(format!("经历库里没有匹配「{keyword}」的条目（共 {} 条）。", all.len()));
    }

    let mut out = format!("经历库共 {} 条，本次给 {} 条：\n", all.len(), picked.len());
    for (index, item) in picked.iter().enumerate() {
        let head = inline_fields(&item["fields"]);
        out.push_str(&format!(
            "\n{}. id={}{}\n",
            index + 1,
            text_of(item, "id"),
            if head.is_empty() { String::new() } else { format!(" ｜ {head}") }
        ));
        let notes = text_of(item, "notes");
        if notes.is_empty() {
            out.push_str("   记录：（空）\n");
        } else {
            out.push_str(&format!("   记录：{}\n", truncate(&notes, 240)));
        }
    }
    Ok(out.trim_end().to_string())
}

fn item_strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|list| {
            list.iter()
                .map(|item| item.as_str().unwrap_or("").trim().to_string())
                .filter(|text| !text.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> Value {
        json!({
            "people": [{
                "id": "p1",
                "fields": [{ "id": "f1", "label": "姓名", "value": "张三" }],
                "items": [{
                    "id": "it1",
                    "fields": [{ "id": "f2", "label": "公司", "value": "中远" }],
                    "notes": "负责驳船统计"
                }],
                "resumes": [{
                    "id": "r1",
                    "title": "中文简历",
                    "pages": [{
                        "id": "pg1",
                        "label": "通用版",
                        "header": [{ "id": "f3", "label": "职位", "value": "单证" }],
                        "sections": [{
                            "id": "s1",
                            "title": "工作经历",
                            "items": [{
                                "id": "e1",
                                "refId": "it1",
                                "fields": [],
                                "bullets": ["日处理 240 票"]
                            }]
                        }]
                    }]
                }]
            }]
        })
    }

    fn with_ctx<T>(run: impl FnOnce(&ToolCtx) -> T) -> T {
        let ws = workspace();
        let sink = std::sync::Mutex::new(Vec::<serde_json::Value>::new());
        let ctx = ToolCtx {
            workspace: &ws,
            person_id: "p1",
            resume_id: "r1",
            page_id: "pg1",
            actions: &sink,
        };
        run(&ctx)
    }

    #[test]
    fn page_inherits_library_fields() {
        let text = with_ctx(|ctx| read_page(&json!({}), ctx).unwrap());
        // 条目本身没有字段，应该显示库里的「公司：中远」，而不是「没有字段」
        assert!(text.contains("公司：中远"), "{text}");
        assert!(text.contains("引用库条目 it1"), "{text}");
        assert!(text.contains("日处理 240 票"), "{text}");
        assert!(text.contains("用户正在看这一页"), "{text}");
    }

    #[test]
    fn list_pages_marks_current() {
        let text = with_ctx(|ctx| list_pages(&json!({}), ctx).unwrap());
        assert!(text.contains("共 1 页"), "{text}");
        assert!(text.contains("← 当前"), "{text}");
    }

    #[test]
    fn unknown_page_reports_helpfully() {
        let err = with_ctx(|ctx| read_page(&json!({ "pageId": "nope" }), ctx).unwrap_err());
        assert!(err.contains("list_pages"), "{err}");
    }

    #[test]
    fn library_filter_and_limit() {
        let text = with_ctx(|ctx| read_library(&json!({ "keyword": "驳船" }), ctx).unwrap());
        assert!(text.contains("中远"), "{text}");
        let none = with_ctx(|ctx| read_library(&json!({ "keyword": "不存在的词" }), ctx).unwrap());
        assert!(none.contains("没有匹配"), "{none}");
    }
}
