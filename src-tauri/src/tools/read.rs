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

/// 一个字段怎么说给模型听。
///
/// 图片字段要说清 assetId —— 模型后面要引用它（写进 HTML 的 `asset:` 或
/// 设成页头照片），只告诉它「这里有一张图」等于没说。
///
/// 每一行都带 `[字段 id]`：写工具按 id 定位（field 全局唯一），
/// 读的时候不给，模型就只能猜参数名或者先调一次 add_field —— 两种都很糟。
fn field_phrase(field: &Value) -> Option<String> {
    let id = text_of(field, "id");
    let label = text_of(field, "label");
    let value = text_of(field, "value");
    let image = text_of(field, "assetId");

    let body = if !image.is_empty() {
        let mark = format!("〔图片 {image}〕");
        match (label.is_empty(), value.is_empty()) {
            (true, true) => mark,
            (true, false) => format!("图注「{value}」 {mark}"),
            (false, true) => format!("{label}：{mark}"),
            (false, false) => format!("{label}：{mark} 图注「{value}」"),
        }
    } else if label.is_empty() && value.is_empty() {
        // 空字段照样要出现：它可能是刚建好还没来得及填的一张图片位，
        // 模型要往这里写就得先知道它的 id
        if id.is_empty() {
            return None;
        }
        "（空字段）".to_string()
    } else {
        match (label.is_empty(), value.is_empty()) {
            (true, false) => value,
            (false, true) => format!("{label}："),
            _ => format!("{label}：{value}"),
        }
    };

    Some(if id.is_empty() {
        body
    } else {
        format!("[{id}] {body}")
    })
}

/// 一行一个字段，空值不占行
fn field_lines(fields: &Value) -> String {
    items_of(fields)
        .iter()
        .filter_map(|field| field_phrase(field).map(|line| format!("- {line}")))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 一行内并列，「公司：X ｜ 职位：Y」
fn inline_fields(fields: &Value) -> String {
    items_of(fields)
        .iter()
        .filter_map(|field| {
            let value = text_of(field, "value");
            let image = text_of(field, "assetId");
            if value.is_empty() && image.is_empty() {
                return None;
            }
            field_phrase(field)
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

    // 材料清单：图片字段、页头照片、作品配图都要靠 assetId 引用，
    // 不列出来模型就只能瞎编一个 id
    let assets = items_of(&person["assets"]);
    if assets.is_empty() {
        out.push_str("\n\n材料：还没有（用户可以拖图进来，也能用 convert_image 从已有文件生成）");
    } else {
        out.push_str(&format!("\n\n材料 {} 份：\n", assets.len()));
        for asset in &assets {
            let kind = text_of(asset, "kind");
            let name = text_of(asset, "name");
            let mut line = format!("- {}（{kind}，id: {}）", name, text_of(asset, "id"));
            if let Some(size) = asset["size"].as_u64() {
                line.push_str(&format!(" {}KB", size / 1024));
            }
            if let Some(mime) = asset["mime"].as_str().filter(|value| !value.is_empty()) {
                line.push_str(&format!(" {mime}"));
            }
            out.push_str(&line);
            out.push('\n');
        }
    }
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
    // 现在的排版状态：不说清的话，agent 会重复改同一块，或者把已有的手写覆盖掉
    out.push_str(&layout_state(page));
    out.push('\n');

    if text_of(page, "kind") == "file" {
        out.push_str(&format!(
            "\n这是一页**原件**：内容在文件里（asset:{}），不在字段中。\n             能做的只有调整它在成品里的呈现方式，改不了里面的字。",
            text_of(page, "assetId")
        ));
        out.push_str(&notes_state(page));
        return Ok(out.trim_end().to_string());
    }

    let blocks = items_of(&page["blocks"]);
    if blocks.is_empty() {
        out.push_str("\n（这一页还没有任何内容块）");
        out.push_str(&notes_state(page));
        return Ok(out.trim_end().to_string());
    }

    for (bi, block) in blocks.iter().enumerate() {
        let id = text_of(block, "id");
        let kind = text_of(block, "kind");
        // role 是「这一块要不要换容器」，改它只影响排版不影响内容，
        // 但模型必须看得见现状，否则每轮都会重复设一次
        let role = text_of(block, "role");
        let role_note = if role.is_empty() {
            String::new()
        } else {
            format!("· role {role}")
        };
        match kind.as_str() {
            "fields" => {
                let role_tag = if bi == 0 { "｜页头" } else { "" };
                out.push_str(&format!("\n块 {id}（字段组{role_tag}）{role_note}\n"));
                let lines = field_lines(&block["fields"]);
                if lines.is_empty() {
                    out.push_str("  （空）\n");
                } else {
                    // 缩进两格：块的层级要在文本里看得出来，模型才不会把
                    // 「这一块的内容」和「下一块的内容」搅在一起
                    for line in lines.lines() {
                        out.push_str(&format!("  {line}\n"));
                    }
                }
            }
            "section" => {
                let title = text_of(block, "title");
                out.push_str(&format!(
                    "\n块 {id}（分区）{}{role_note}\n",
                    if title.is_empty() {
                        "（无标题）".to_string()
                    } else {
                        format!("「{title}」")
                    }
                ));
                let entries = items_of(&block["items"]);
                if entries.is_empty() {
                    out.push_str("  （空）\n");
                    continue;
                }
                for (ei, entry) in entries.iter().enumerate() {
                    let (fields, ref_id) = effective_fields(entry, person);
                    let line = inline_fields(fields);
                    // 条目 id 是 update_entry / remove_entry / move_entry 的唯一凭据
                    let entry_id = text_of(entry, "id");
                    let suffix = match &ref_id {
                        Some(id) => format!("（库条目 {id}）"),
                        None => String::new(),
                    };
                    if line.is_empty() {
                        out.push_str(&format!(
                            "  {}. [{entry_id}] （没有字段）{suffix}\n",
                            ei + 1
                        ));
                    } else {
                        out.push_str(&format!("  {}. [{entry_id}] {line}{suffix}\n", ei + 1));
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
            "media" => {
                let caption = text_of(block, "caption");
                out.push_str(&format!(
                    "\n块 {id}（图片）asset:{} {}{role_note}\n",
                    text_of(block, "assetId"),
                    if caption.is_empty() {
                        String::new()
                    } else {
                        format!("图注「{caption}」")
                    }
                ));
            }
            other => out.push_str(&format!("\n块 {id}（不认识的类型 {other}）{role_note}\n")),
        }
    }

    out.push_str(&notes_state(page));
    out.push_str(
        "\n（块 id 是改内容与版面的凭据：加条目用 add_entry 传所在分区块的 id，\
         改版面用 set_page_layout 的 parts）",
    );
    Ok(out.trim_end().to_string())
}

/// 纸面上的批注：排版 agent 留的话就靠这一段传给会话 agent。
/// 编号从 1 起 —— 模型引用「第 2 条批注」时两边指数要一致。
fn notes_state(page: &Value) -> String {
    let notes = items_of(&page["notes"]);
    if notes.is_empty() {
        return String::new();
    }
    let mut out = format!("\n\n排版批注 {} 条：\n", notes.len());
    for (index, note) in notes.iter().enumerate() {
        let block = text_of(note, "blockId");
        let place = if block.is_empty() {
            String::new()
        } else {
            format!(" · 块 {block}")
        };
        let text = text_of(note, "text");
        out.push_str(&format!("  [批注 {}]{place} {text}\n", index + 1));
    }
    out
}

/// 这一页现在是怎么排的：结构化、哪些块手写过、有没有整页接管
fn layout_state(page: &Value) -> String {
    let layout = page.get("layout");
    let preset = layout
        .and_then(|value| value.get("preset"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let html = layout
        .and_then(|value| value.get("html"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let parts = layout
        .and_then(|value| value.get("parts"))
        .and_then(Value::as_object)
        .map(|map| map.len())
        .unwrap_or(0);

    if !html.is_empty() {
        return format!(
            "排版：整页手写 HTML（{} 字），生成器不再介入",
            html.chars().count()
        );
    }
    let mut out = format!(
        "排版：结构化生成 · 预设 {}",
        if preset.is_empty() { "classic" } else { preset }
    );
    if parts > 0 {
        out.push_str(&format!(" · {parts} 个块手写过"));
    }
    out
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
                "assets": [
                    { "id": "a1", "kind": "image", "name": "证件照.jpg", "stored": "abc.jpg" },
                    { "id": "a2", "kind": "file", "name": "证书.pdf", "stored": "def.pdf" }
                ],
                "resumes": [{
                    "id": "r1",
                    "title": "中文简历",
                    "pages": [{
                        "id": "pg1",
                        "kind": "native",
                        "label": "通用版",
                        "layout": { "preset": "compact", "parts": { "s1": "<section>手写</section>" } },
                        "blocks": [
                            {
                                "id": "pg1-head",
                                "kind": "fields",
                                "fields": [
                                    { "id": "f3", "label": "职位", "value": "单证" },
                                    { "id": "f4", "label": "照片", "assetId": "a1", "value": "蓝底一寸" }
                                ]
                            },
                            {
                                "id": "s1",
                                "kind": "section",
                                "title": "工作经历",
                                "items": [{
                                    "id": "e1",
                                    "refId": "it1",
                                    "fields": [],
                                    "bullets": ["日处理 240 票"]
                                }]
                            }
                        ]
                    }]
                }]
            }]
        })
    }

    fn with_ctx<T>(run: impl FnOnce(&ToolCtx) -> T) -> T {
        let ws = workspace();
        let out = crate::tools::Outbox::default();
        let ctx = ToolCtx {
            workspace: &ws,
            person_id: "p1",
            resume_id: "r1",
            page_id: "pg1",
            out: &out,
            root: None,
        };
        run(&ctx)
    }

    #[test]
    fn page_inherits_library_fields() {
        let text = with_ctx(|ctx| read_page(&json!({}), ctx).unwrap());
        // 条目本身没有字段，应该显示库里的「公司：中远」，而不是「没有字段」
        assert!(text.contains("公司：中远"), "{text}");
        assert!(text.contains("库条目 it1"), "{text}");
        assert!(text.contains("日处理 240 票"), "{text}");
        assert!(text.contains("用户正在看这一页"), "{text}");
    }

    /// 读页必须读 blocks。曾经读 v4 的 header / sections，v5 之后恒为空，
    /// 两个 agent 都只能看到「这一页什么都没有」—— 排版就是在闭眼干活
    #[test]
    fn page_text_comes_from_blocks_not_legacy_fields() {
        let text = with_ctx(|ctx| read_page(&json!({}), ctx).unwrap());
        // 块 id 是改版面的凭据，必须出现
        assert!(text.contains("pg1-head"), "{text}");
        assert!(text.contains("块 s1（分区）「工作经历」"), "{text}");
        assert!(text.contains("职位：单证"), "{text}");
    }

    #[test]
    fn image_fields_show_their_asset_id() {
        let text = with_ctx(|ctx| read_page(&json!({}), ctx).unwrap());
        // 只说「这里有一张图」等于没说：后面要引用就得有 id
        assert!(text.contains("〔图片 a1〕"), "{text}");
        assert!(text.contains("图注「蓝底一寸」"), "{text}");
    }

    #[test]
    fn layout_state_tells_what_is_already_handwritten() {
        let text = with_ctx(|ctx| read_page(&json!({}), ctx).unwrap());
        // 不说清现状的话，agent 会重复改同一块，或者把已有覆盖冲掉
        assert!(text.contains("预设 compact"), "{text}");
        assert!(text.contains("1 个块手写过"), "{text}");
    }

    #[test]
    fn person_lists_assets_so_ids_can_be_referenced() {
        let text = with_ctx(|ctx| read_person(&json!({}), ctx).unwrap());
        assert!(text.contains("材料 2 份"), "{text}");
        assert!(text.contains("id: a1"), "{text}");
        assert!(text.contains("id: a2"), "{text}");
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

    /// 写工具全靠 id 定位：读的时候不把 id 摆出来，模型只能靠猜
    #[test]
    fn read_outputs_every_id_the_write_tools_need() {
        let text = with_ctx(|ctx| read_page(&json!({}), ctx).unwrap());
        // 字段 id（set_field / remove_field）
        assert!(text.contains("[f3] 职位：单证"), "{text}");
        assert!(text.contains("[f4]"), "{text}");
        // 块 id（add_entry / set_block_role / add_section 的 afterBlockId）
        assert!(text.contains("块 s1（分区）"), "{text}");
        // 条目 id（update_entry / remove_entry / move_entry）
        assert!(text.contains("[e1]"), "{text}");
        // 库条目 id
        assert!(text.contains("库条目 it1"), "{text}");
    }

    #[test]
    fn read_shows_block_role_when_set() {
        let ws = json!({
            "people": [{
                "id": "p1",
                "fields": [],
                "items": [],
                "assets": [],
                "resumes": [{
                    "id": "r1",
                    "title": "简历",
                    "pages": [{
                        "id": "pg1",
                        "kind": "native",
                        "label": "一",
                        "blocks": [
                            { "id": "b1", "kind": "section", "title": "技能", "role": "tags" },
                            { "id": "b2", "kind": "fields", "role": "kv",
                              "fields": [{ "id": "f9", "label": "语言", "value": "英语" }] }
                        ]
                    }]
                }]
            }]
        });
        let out = crate::tools::Outbox::default();
        let ctx = ToolCtx {
            workspace: &ws,
            person_id: "p1",
            resume_id: "r1",
            page_id: "pg1",
            out: &out,
            root: None,
        };
        let text = read_page(&json!({}), &ctx).unwrap();
        assert!(text.contains("· role tags"), "{text}");
        assert!(text.contains("· role kv"), "{text}");
    }

    /// 排版 agent 的批注要能被会话 agent 读到，否则 ask_session 之外没有通道
    #[test]
    fn read_page_appends_layout_notes() {
        let ws = json!({
            "people": [{
                "id": "p1",
                "fields": [],
                "items": [],
                "assets": [],
                "resumes": [{
                    "id": "r1",
                    "title": "简历",
                    "pages": [{
                        "id": "pg1",
                        "kind": "native",
                        "label": "一",
                        "blocks": [{ "id": "b1", "kind": "section", "title": "工作" }],
                        "notes": [
                            { "id": "n1", "text": "这块太挤，删一条", "blockId": "b1", "at": 1 },
                            { "id": "n2", "text": "整页偏左", "at": 2 }
                        ]
                    }]
                }]
            }]
        });
        let out = crate::tools::Outbox::default();
        let ctx = ToolCtx {
            workspace: &ws,
            person_id: "p1",
            resume_id: "r1",
            page_id: "pg1",
            out: &out,
            root: None,
        };
        let text = read_page(&json!({}), &ctx).unwrap();
        assert!(text.contains("排版批注 2 条"), "{text}");
        assert!(text.contains("[批注 1] · 块 b1 这块太挤，删一条"), "{text}");
        assert!(text.contains("[批注 2] 整页偏左"), "{text}");
    }
}
