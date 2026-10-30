//! 附件正文的 **Fact 层**：只增不改地留一份原件正文。
//!
//! 为什么必须有这一层：附件是「按轮一次性」的 —— 用户在第一条消息里传了一份 PDF，
//! 正文被拼进那一轮的 materials 段，用完就没了。那一轮模型如果调了工具却没写出正文
//! （真机事故：2404 字的一份简历，最后只剩「姓名 + 照片」），原件其实还躺在
//! `inbox/` 里，但模型既不知道它的名字，也没有任何工具能读回来 —— 2404 字永久消失。
//!
//! 标准口径：**附件正文是事实（Fact），不是投影（Projection）**。事实 append-only 落库，
//! 投影只给「指针 + 摘要」，模型要细节再按 `stored` 读回。
//!
//! 与 `pool/` 的分工：pool 是二进制材料池（按 sha 命名，管图片与原件本身）；
//! 这里是文本事实层（按同一套 `stored` 命名，只管「这份文件里写了什么字」）。
//! 两者不合并：一份 PDF 在 pool 里是字节、在这里是正文，回收策略也完全不同。
//!
//! 表结构走 `db::ensure_additive`（`CREATE TABLE IF NOT EXISTS`），不升 schema 版本 ——
//! 纯新增的表对老库、新库、测试库都是同一句话，不值得让所有老库跑一遍完整迁移。

use rusqlite::{params, Connection};

/// 摘要长度：Projection 里模型只看得到这么多字
const DIGEST_CHARS: usize = 120;
/// 清单/读回时取的正文前缀。比摘要宽三倍多：空白压成空格后字符数会缩水，
/// 留余量才能稳定给出 120 字
const HEAD_CHARS: usize = 400;

/// 一份原件的元信息（**不含全文**）：清单、摘要、指针都用它。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceMeta {
    pub stored: String,
    pub name: String,
    pub ext: String,
    pub pages: Option<u32>,
    /// 正文总字数（字符数，不是字节）
    pub chars: usize,
    pub added_at: i64,
    /// 正文前缀（最多 `HEAD_CHARS` 字）。`list` 只取这一段，不搬全文
    pub head: String,
}

/// 一份原件的正文
pub struct SourceDoc {
    pub meta: SourceMeta,
    pub text: String,
}

impl SourceMeta {
    /// Projection 里那一小段摘要：正文前 120 字，换行压成空格。
    pub fn digest(&self) -> String {
        digest_text(&self.head, self.chars)
    }
}

/// 摘要算法只有一份：`attach` 拼 Projection 与 `read_source` 报清单都调它，
/// 两处各写一遍迟早出现「同一份原件两个摘要」。
///
/// 为什么不是「整段前 120 字」：PDF 抽出来的正文按行断，前 120 字里可能一半是换行，
/// 压成空格后才能真的把 120 个字交到模型眼前。末尾还有正文时加省略号 ——
/// 没有省略号，模型会把「首行标题」当成「全文就这么点」。
pub fn digest_text(head: &str, chars: usize) -> String {
    let flat = squash(&head.chars().take(HEAD_CHARS).collect::<String>());
    if chars == 0 || flat.is_empty() {
        return "（没有文字层）".to_string();
    }
    let mut out: String = flat.chars().take(DIGEST_CHARS).collect();
    if chars > out.chars().count() {
        out.push('…');
    }
    out
}

/// 空白压成单空格：PDF 的折行、制表符、连续空格在上下文里都是噪声
fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|span| span.as_secs() as i64)
        .unwrap_or(0)
}

/// 落一份正文。同一份文件（同一个 `stored`）重复上传不产生第二行。
///
/// `stored` 是 inbox 里的名字（sha 前 16 位 + 扩展名），内容寻址天然幂等；
/// 这里再叠一层 `INSERT OR REPLACE`，是为了「同一个 stored 先落了空正文、
/// 后来又解析出字」这种补写也能就地覆盖，而不是留两行让读的人自己挑。
pub fn save(
    conn: &Connection,
    stored: &str,
    name: &str,
    ext: &str,
    pages: Option<u32>,
    text: &str,
) -> Result<(), String> {
    conn.execute(
        "INSERT OR REPLACE INTO source (stored, name, ext, pages, chars, text, added_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            stored,
            name,
            ext,
            pages.map(i64::from),
            text.chars().count() as i64,
            text,
            now()
        ],
    )
    .map_err(|error| format!("写入原件正文失败：{error}"))?;
    Ok(())
}

/// 读回一份正文。没有这一行就是 `None`（不认识 ≠ 读不出）
pub fn get(conn: &Connection, stored: &str) -> Result<Option<SourceDoc>, String> {
    let mut stmt = conn
        .prepare("SELECT stored, name, ext, pages, chars, added_at, text FROM source WHERE stored = ?1")
        .map_err(|error| format!("读取原件正文失败：{error}"))?;
    let mut rows = stmt
        .query(params![stored])
        .map_err(|error| format!("查询原件正文失败：{error}"))?;
    let row = rows
        .next()
        .map_err(|error| format!("读取原件正文行失败：{error}"))?;
    let Some(row) = row else { return Ok(None) };
    let text: String = row.get(6).map_err(|error| format!("读取原件正文失败：{error}"))?;
    let meta = SourceMeta {
        stored: row.get(0).map_err(|error| format!("读取原件名失败：{error}"))?,
        name: row.get(1).map_err(|error| format!("读取原件名失败：{error}"))?,
        ext: row.get(2).map_err(|error| format!("读取原件类型失败：{error}"))?,
        pages: row
            .get::<_, Option<i64>>(3)
            .map_err(|error| format!("读取原件页数失败：{error}"))?
            .map(|value| value as u32),
        chars: row
            .get::<_, i64>(4)
            .map_err(|error| format!("读取原件字数失败：{error}"))? as usize,
        added_at: row
            .get(5)
            .map_err(|error| format!("读取原件时间失败：{error}"))?,
        head: text.chars().take(HEAD_CHARS).collect(),
    };
    Ok(Some(SourceDoc { meta, text }))
}

/// 列出所有原件（**不带全文**，只带 400 字前缀）
pub fn list(conn: &Connection) -> Result<Vec<SourceMeta>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT stored, name, ext, pages, chars, added_at, substr(text, 1, ?1)
             FROM source ORDER BY added_at, stored",
        )
        .map_err(|error| format!("读取原件清单失败：{error}"))?;
    let rows = stmt
        .query_map(params![HEAD_CHARS as i64], |row| {
            Ok(SourceMeta {
                stored: row.get(0)?,
                name: row.get(1)?,
                ext: row.get(2)?,
                pages: row.get::<_, Option<i64>>(3)?.map(|value| value as u32),
                chars: row.get::<_, i64>(4)? as usize,
                added_at: row.get(5)?,
                head: row.get(6)?,
            })
        })
        .map_err(|error| format!("查询原件清单失败：{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("读取原件清单行失败：{error}"))?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        conn
    }

    /// 同一份文件拖两次：一行，不是两行（否则读回的人得自己挑哪行最新）
    #[test]
    fn saving_the_same_file_twice_keeps_one_row() {
        let conn = memory();
        save(&conn, "abc.pdf", "简历.pdf", "pdf", Some(1), "第一版正文").unwrap();
        save(&conn, "abc.pdf", "简历.pdf", "pdf", Some(1), "第二版正文").unwrap();

        let count: i64 = conn
            .query_row("SELECT count(*) FROM source", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
        // 后写的那份说了算
        assert_eq!(get(&conn, "abc.pdf").unwrap().unwrap().text, "第二版正文");
    }

    /// 中文一个都不能丢：字数按字符算，不是按字节
    #[test]
    fn chinese_text_survives_the_roundtrip() {
        let conn = memory();
        let body = "# 教育背景\n华北电力大学 本科\n2019 - 2023";
        save(&conn, "a.md", "简历.md", "md", None, body).unwrap();
        let doc = get(&conn, "a.md").unwrap().expect("应读得回来");
        assert_eq!(doc.text, body);
        assert_eq!(doc.meta.chars, body.chars().count());
        assert_eq!(doc.meta.name, "简历.md");
        assert_eq!(doc.meta.pages, None);
        assert!(doc.meta.added_at > 0);
        // 没有这一行就是 None，不是空文档
        assert!(get(&conn, "nope.md").unwrap().is_none());
    }

    /// 摘要：120 字截断 + 换行变空格 + 末尾省略号
    #[test]
    fn digest_truncates_to_120_chars_and_flattens_newlines() {
        let conn = memory();
        let mut body = String::new();
        for index in 0..40 {
            body.push_str(&format!("第{index}行：华北电力大学电气工程及其自动化\n"));
        }
        save(&conn, "b.pdf", "原件.pdf", "pdf", Some(2), &body).unwrap();
        let meta = get(&conn, "b.pdf").unwrap().unwrap().meta;

        let digest = meta.digest();
        assert!(!digest.contains('\n'), "换行必须压成空格：{digest}");
        assert!(digest.ends_with('…'), "后面还有正文，要有省略号：{digest}");
        assert_eq!(digest.chars().count(), DIGEST_CHARS + 1, "120 字 + 省略号：{digest}");
        assert!(digest.starts_with("第0行：华北电力大学"), "{digest}");

        // 短正文不截断、不加省略号
        save(&conn, "c.md", "短.md", "md", None, "只有一行").unwrap();
        assert_eq!(get(&conn, "c.md").unwrap().unwrap().meta.digest(), "只有一行");

        // 没有文字层：说清楚是「没有」，不是「读不出」
        save(&conn, "d.pdf", "扫描件.pdf", "pdf", Some(1), "  \n ").unwrap();
        assert_eq!(
            get(&conn, "d.pdf").unwrap().unwrap().meta.digest(),
            "（没有文字层）"
        );
    }

    /// 清单只带前缀，不搬全文
    #[test]
    fn list_carries_only_the_head() {
        let conn = memory();
        let body = "字".repeat(2000);
        save(&conn, "e.pdf", "长.pdf", "pdf", Some(3), &body).unwrap();
        let list = list(&conn).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].chars, 2000);
        assert_eq!(list[0].head.chars().count(), HEAD_CHARS);
        assert_eq!(list[0].digest().chars().count(), DIGEST_CHARS + 1);
    }
}
