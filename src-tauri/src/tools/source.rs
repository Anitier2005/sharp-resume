//! `read_source`：按 `stored` 读回原件正文。
//!
//! 为什么必须有它：附件是**按轮一次性**的投影，正文拼进某一轮的 materials 就没了。
//! 那一轮模型如果没把正文写完（真机事故：2404 字的简历只剩「姓名 + 照片」），
//! 下一轮它手里只有一个 `stored` —— 没有这个工具，那个名字就是废字符。
//!
//! 数据来源只有两处，顺序不能反：
//!   1. Fact 层（`source` 表）：原文一字不差，且是当时那份解析结果；
//!   2. 库里没有、`inbox/` 里文件还在 → 现场重解析一次并补写进去。
//!      这一条是为老库补的：Fact 层是这一版才有的，升级前传的附件没有行。
//!
//! 返回值第一行是「原件名 + 总字数 + 本次区间」：模型要据此决定还要不要往下读，
//! 没有这一行它就只能靠猜「读完了没有」。

use serde_json::{json, Value};

use super::ToolCtx;

/// 一次读多少字（缺省）：约一页 A4 的中文量，够模型接着往下写，又不至于挤掉别的材料
const DEFAULT_LIMIT: usize = 4000;
/// 一次最多读多少字：再长就该分段读，否则一条工具结果能把整个窗口吃掉
const MAX_LIMIT: usize = 20_000;

/// 清单默认列几份、最多列几份。一份简历原件的量级是 1~3，20 足够；
/// 上限压在 50 是因为清单只是「我手上有什么」，不是为了把全库倒进上下文
const DEFAULT_LIST: usize = 20;
const MAX_LIST: usize = 50;

/// 一条都没有时的回答。用户看到的不是「空」，而是「下一步该干什么」
const NO_SOURCES: &str =
    "这个工作区里还没有原件。用户可以拖一份 PDF / 图片进对话，我会把它存下来。";

/// 回落到全表时开头必须写清：这些不一定是这个候选人的。
/// 不写这一句，模型会拿别人的原件当自己的简历来核对
const GLOBAL_HEAD: &str = "下面这些是系统里的全部原件，不都是这个候选人的：";

/// 结尾。它的作用是让模型知道「想不起来就再列一次」是允许的 —— 否则它会
/// 凭记忆编一个 stored，然后撞在「没有这么一份原件」上
const LIST_TAIL: &str = "原件不会随轮次消失；不确定有什么就再列一次。";

pub fn params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "stored": {
                "type": "string",
                "description": "原件标识，从 list_sources 的清单、材料段里的「（附件标识：…）」或 read_source 的报错里取，不是文件名"
            },
            "offset": {
                "type": "integer",
                "description": "从第几个字开始读，默认 0。上一段读到哪，这里就从哪接着读"
            },
            "limit": {
                "type": "integer",
                "description": "这一次读多少字，默认 4000，最多 20000"
            }
        },
        "required": ["stored"],
        "additionalProperties": false
    })
}

pub fn run(args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let stored = args
        .get("stored")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    if stored.is_empty() {
        return Err("要给 stored：材料段里那行「（附件标识：…）」就是它".into());
    }
    let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(DEFAULT_LIMIT)
        .clamp(1, MAX_LIMIT);

    let root = ctx
        .root
        .ok_or_else(|| "当前环境拿不到数据目录（浏览器调试模式没有原件）".to_string())?;
    let conn = crate::db::open(&root.join("app.db"))?;

    let doc = match crate::source::get(&conn, &stored)? {
        Some(doc) => doc,
        // 库里没有但文件还在：现场重解析并补写。老库升级上来的附件全走这条
        None => backfill(&conn, root, &stored)?,
    };

    let chars = doc.text.chars().count();
    if chars == 0 {
        return Ok(format!(
            "【原件 {} · 共 0 字】\n（这份原件没有文字层，读不到正文。要内容只能看原件图片，或请用户把文字贴过来。）",
            doc.meta.name
        ));
    }
    if offset >= chars {
        return Err(format!(
            "offset 超过正文长度（共 {chars} 字），最多只能从第 {} 字开始读",
            chars.saturating_sub(1)
        ));
    }
    let end = (offset + limit).min(chars);
    let slice: String = doc.text.chars().skip(offset).take(end - offset).collect();
    let tail = if end < chars {
        format!("\n（还没读完，接着读请传 offset = {end}）")
    } else {
        String::new()
    };
    Ok(format!(
        "【原件 {} · 共 {chars} 字 · 本次第 {offset}-{end} 字】\n{slice}{tail}",
        doc.meta.name
    ))
}

/// `list_sources`：我手上到底有哪些原件。
///
/// 为什么要它：`stored` 只在「那一轮的消息」里出现过一次。轮次一过，模型手里
/// 只剩记忆 —— 记不住就等于没有，于是它会说「原件已经不在系统里了」，
/// 让用户重新贴一遍原文。事实层其实一直有：缺的只是「问一句有什么」的入口。
pub fn list_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "limit": {
                "type": "integer",
                "description": "最多列几份，默认 20"
            }
        },
        "additionalProperties": false
    })
}

/// 一行一份原件。**`stored = …` 必须在**：清单的全部意义就是交出下一步要用的那把钥匙
/// （`read_source` 的 stored、`render_doc_page` 的 stored 都是它）。
fn line_of(meta: &crate::source::SourceMeta) -> String {
    // 页数只有 PDF 有；没有就不写「0 页」——那是假话，也没人需要
    let pages = match meta.pages {
        Some(count) => format!("{count} 页，"),
        None => String::new(),
    };
    format!(
        "- {}（{}，{pages}{} 字，stored = {}）",
        meta.name, meta.ext, meta.chars, meta.stored
    )
}

pub fn list_sources(args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(DEFAULT_LIST)
        .clamp(1, MAX_LIST);

    let root = ctx
        .root
        .ok_or_else(|| "当前环境拿不到数据目录（浏览器调试模式没有原件）".to_string())?;
    let conn = crate::db::open(&root.join("app.db"))?;

    let (mut rows, global) = {
        let mine = crate::source::bound(&conn, ctx.person_id)?;
        if mine.is_empty() {
            // 老库（attach_ref 是这一版才有的）与「从别处带过来的原件」都走这条。
            // 宁可多说一句「这是全部」，也不能回答「你什么都没有」——
            // 后者会把模型逼回「让用户重新贴」那条路。
            let mut all = crate::source::list(&conn)?;
            all.reverse(); // source::list 按时间正序，清单要新在前
            (all, true)
        } else {
            (mine, false)
        }
    };

    if rows.is_empty() {
        return Ok(format!("{NO_SOURCES}\n{LIST_TAIL}"));
    }

    let total = rows.len();
    let shown = total.min(limit);
    rows.truncate(shown);

    let mut lines: Vec<String> = Vec::new();
    lines.push(if global {
        GLOBAL_HEAD.to_string()
    } else {
        format!("这个候选人用过的原件（共 {total} 份，新的在前）：")
    });
    for meta in &rows {
        lines.push(line_of(meta));
    }
    if shown < total {
        lines.push(format!(
            "（共 {total} 份，这里只列最新的 {shown} 份；要更多请把 limit 调大。）"
        ));
    }
    lines.push(LIST_TAIL.to_string());
    Ok(lines.join("\n"))
}

/// Fact 层没有这一行：从 `inbox/` 读回原件、按当时的扩展名重解析、补写进库。
///
/// 补写失败也照样把正文交给模型 —— 这一轮的回答不该因为落库失败而作废。
fn backfill(
    conn: &rusqlite::Connection,
    root: &std::path::Path,
    stored: &str,
) -> Result<crate::source::SourceDoc, String> {
    let bytes = crate::attach::read(root, stored).map_err(|error| {
        format!(
            "没有 stored = {stored} 的原件：{error}。先调 list_sources 看一眼手上到底有哪些原件，stored 从那里抄。"
        )
    })?;
    let ext = stored
        .rsplit_once('.')
        .map(|(_, tail)| tail.to_ascii_lowercase())
        .unwrap_or_default();
    let parsed = crate::attach::parse(&ext, &bytes);
    if let Err(error) =
        crate::source::save(conn, stored, stored, &ext, parsed.pages, &parsed.text)
    {
        eprintln!("[source] 补写原件正文失败（{stored}）：{error}");
    }
    Ok(crate::source::SourceDoc {
        meta: crate::source::SourceMeta {
            stored: stored.to_string(),
            name: stored.to_string(),
            ext,
            pages: parsed.pages,
            chars: parsed.text.chars().count(),
            added_at: 0,
            head: parsed.text.chars().take(400).collect(),
        },
        text: parsed.text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 临时数据目录 + inbox 里一份原件，返回 (root, stored)
    fn inbox_with(bytes: &[u8], ext: &str) -> (std::path::PathBuf, String) {
        let root = std::env::temp_dir().join(format!("sr-source-{}", crate::session::uuid_like()));
        std::fs::create_dir_all(root.join("inbox")).unwrap();
        let stored = crate::snapshot::pool_name(&crate::pool::sha256_hex(bytes), Some(ext)).unwrap();
        std::fs::write(root.join("inbox").join(&stored), bytes).unwrap();
        (root, stored)
    }

    fn ctx_with<'a>(root: &'a std::path::Path, ws: &'a Value, out: &'a super::super::Outbox) -> ToolCtx<'a> {
        ToolCtx {
            workspace: ws,
            person_id: "p",
            resume_id: "r",
            page_id: "g",
            out,
            root: Some(root),
            facts: None,
        }
    }

    /// 库里已有正文：按 offset/limit 切片，头一行交代总字数与本次区间
    #[test]
    fn reads_the_body_from_the_fact_layer_in_slices() {
        let (root, stored) = inbox_with(b"placeholder", "md");
        let conn = crate::db::open(&root.join("app.db")).unwrap();
        let body: String = (0..20).map(|index| format!("第{index}行正文\n")).collect();
        crate::source::save(&conn, &stored, "简历.md", "md", None, &body).unwrap();
        drop(conn);

        let ws = json!({ "people": [] });
        let out = super::super::Outbox::default();
        let ctx = ctx_with(&root, &ws, &out);

        let text = run(&json!({ "stored": stored, "limit": 30 }), &ctx).unwrap();
        let mut lines = text.lines();
        let header = lines.next().unwrap();
        assert!(header.contains("简历.md"), "{header}");
        assert!(header.contains(&format!("共 {} 字", body.chars().count())), "{header}");
        assert!(header.contains("本次第 0-30 字"), "{header}");
        assert!(text.contains("第0行正文"), "{text}");
        assert!(text.contains("接着读请传 offset = 30"), "{text}");

        // 第二段接着读，头一行要跟着变
        let next = run(&json!({ "stored": stored, "offset": 30, "limit": 30 }), &ctx).unwrap();
        assert!(next.lines().next().unwrap().contains("本次第 30-60 字"), "{next}");
        assert!(!next.contains("第0行正文"), "{next}");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 越界要说清上限，别让模型以为「再往后面还有字」
    #[test]
    fn offset_beyond_the_end_says_so() {
        let (root, stored) = inbox_with(b"short", "md");
        let conn = crate::db::open(&root.join("app.db")).unwrap();
        crate::source::save(&conn, &stored, "短.md", "md", None, "六个字的正文").unwrap();
        drop(conn);

        let ws = json!({ "people": [] });
        let out = super::super::Outbox::default();
        let ctx = ctx_with(&root, &ws, &out);

        let err = run(&json!({ "stored": stored, "offset": 99 }), &ctx).unwrap_err();
        assert!(err.contains("offset 超过正文长度"), "{err}");
        assert!(err.contains("共 6 字"), "{err}");

        // 缺参数也要给下一步，而不是只有一句「错了」
        let err = run(&json!({}), &ctx).unwrap_err();
        assert!(err.contains("附件标识"), "{err}");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 老库（Fact 层还没有这份附件）：现场重解析 + 补写，第二次就走库了
    #[test]
    fn a_missing_row_is_backfilled_from_the_inbox_once() {
        let (root, stored) = inbox_with("# 教育背景\n华北电力大学".as_bytes(), "md");
        let ws = json!({ "people": [] });
        let out = super::super::Outbox::default();
        let ctx = ctx_with(&root, &ws, &out);

        let text = run(&json!({ "stored": stored.clone() }), &ctx).unwrap();
        assert!(text.contains("华北电力大学"), "{text}");

        let conn = crate::db::open(&root.join("app.db")).unwrap();
        let doc = crate::source::get(&conn, &stored).unwrap().expect("补写后应有一行");
        assert_eq!(doc.text, "# 教育背景\n华北电力大学");

        // 文件删了也不影响：正文已经在库里
        std::fs::remove_file(root.join("inbox").join(&stored)).unwrap();
        let again = run(&json!({ "stored": stored }), &ctx).unwrap();
        assert!(again.contains("华北电力大学"), "{again}");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 库里查不到、文件也没了：要说清是哪一步断了
    #[test]
    fn an_unknown_stored_says_where_to_look() {
        let root = std::env::temp_dir().join(format!("sr-source-{}", crate::session::uuid_like()));
        std::fs::create_dir_all(root.join("inbox")).unwrap();
        let ws = json!({ "people": [] });
        let out = super::super::Outbox::default();
        let ctx = ctx_with(&root, &ws, &out);
        let err = run(&json!({ "stored": "deadbeefdeadbeef.pdf" }), &ctx).unwrap_err();
        assert!(err.contains("没有 stored"), "{err}");
        assert!(err.contains("list_sources"), "要给下一把抓手：{err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 清单里的每一行都必须带 `stored = …`：那是模型下一步唯一能用的钥匙。
    /// 只给文件名，它就只能说「我这儿有一份叫 XX 的文件」，然后卡住。
    #[test]
    fn list_sources_names_the_stored_it_wants() {
        let root = std::env::temp_dir().join(format!("sr-source-{}", crate::session::uuid_like()));
        std::fs::create_dir_all(root.join("inbox")).unwrap();
        let conn = crate::db::open(&root.join("app.db")).unwrap();
        let body = "字".repeat(2402);
        crate::source::save(
            &conn,
            "bcf9d0ec3fd6b381.pdf",
            "华北电力大学 谭轶天.pdf",
            "pdf",
            Some(1),
            &body,
        )
        .unwrap();
        crate::source::bind(&conn, "p", "bcf9d0ec3fd6b381.pdf").unwrap();
        drop(conn);

        let ws = json!({ "people": [] });
        let out = super::super::Outbox::default();
        let ctx = ctx_with(&root, &ws, &out);

        let text = list_sources(&json!({}), &ctx).unwrap();
        assert!(text.contains("华北电力大学 谭轶天.pdf"), "{text}");
        assert!(text.contains("stored = bcf9d0ec3fd6b381.pdf"), "{text}");
        assert!(text.contains("2402 字"), "{text}");
        assert!(text.contains("1 页"), "{text}");
        assert!(text.contains("pdf"), "{text}");
        assert!(
            !text.contains("不都是这个候选人"),
            "自己名下有的，不该说成全局清单：{text}"
        );
        assert!(text.contains("原件不会随轮次消失"), "结尾要有这句：{text}");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 没有 attach_ref（老库 / 从别处带过来的原件）：回落到全表，
    /// 并在开头说清「这不都是这个候选人的」—— 不说，模型会拿别人的原件当自己的简历核对。
    #[test]
    fn list_sources_falls_back_to_everything_when_unbound() {
        let root = std::env::temp_dir().join(format!("sr-source-{}", crate::session::uuid_like()));
        std::fs::create_dir_all(root.join("inbox")).unwrap();
        let conn = crate::db::open(&root.join("app.db")).unwrap();
        crate::source::save(&conn, "aaaaaaaaaaaaaaa1.pdf", "旧的.pdf", "pdf", Some(2), "旧简历").unwrap();
        crate::source::save(&conn, "bbbbbbbbbbbbbbb2.md", "笔记.md", "md", None, "一段笔记").unwrap();
        drop(conn);

        let ws = json!({ "people": [] });
        let out = super::super::Outbox::default();
        let ctx = ctx_with(&root, &ws, &out);

        let text = list_sources(&json!({}), &ctx).unwrap();
        assert!(
            text.starts_with("下面这些是系统里的全部原件，不都是这个候选人的"),
            "开头要先说清这是全局的：{text}"
        );
        assert!(text.contains("旧的.pdf") && text.contains("笔记.md"), "{text}");
        assert!(text.contains("stored = bbbbbbbbbbbbbbb2.md"), "{text}");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 一份都没有：给下一步（拖文件进来），而不是一句「没有」
    #[test]
    fn list_sources_with_an_empty_workspace_says_what_to_do() {
        let root = std::env::temp_dir().join(format!("sr-source-{}", crate::session::uuid_like()));
        std::fs::create_dir_all(root.join("inbox")).unwrap();
        let ws = json!({ "people": [] });
        let out = super::super::Outbox::default();
        let ctx = ctx_with(&root, &ws, &out);

        let text = list_sources(&json!({}), &ctx).unwrap();
        assert!(text.contains("这个工作区里还没有原件"), "{text}");
        assert!(text.contains("拖一份"), "{text}");

        let _ = std::fs::remove_dir_all(&root);
    }
}
