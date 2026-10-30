//! SQLite 存储层：结构化数据的权威。
//!
//! 设计取舍：**表骨架 + JSON 叶子**，不做全字段拆表。
//!
//!   1. `Field{id,label,value}` 是唯一固定的原子结构，再往下没有 schema 可言——
//!      用户给一段经历写几个字段、字段叫什么，全由他自己决定。拆成 EAV
//!      （entity-attribute-value）只会把读写都变复杂，查询也不会更快：
//!      单人工作区的数据量本来就小，永远整份加载。
//!   2. 但「谁属于谁、排第几」必须由表来保证：级联删除、顺序、事务，
//!      这些是 JSON 文件给不了的。
//!
//! 于是 person / library_item / resume / page / message 各一行：行里 `body` 存整份
//! JSON（未知字段一并保留），只把 id、父 id、顺序、label 这些需要排序与引用的列抽出来。
//!
//! 为什么 body 里不含子节点：子节点已经有自己的行。重复存两遍的话，
//! 一处改了另一处没改就会出现「读出来是旧的、查出来是新的」。
//!
//! v5 起 page 行用上了 `kind`（native/file）与 `asset_id`；
//! 会话也从 sessions.json 搬到了 `chat` 表（第 4 件：所有数据都过同一道闸）。

use rusqlite::{params, Connection};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

use crate::model::{self, Workspace};

/// 表结构版本。往后加列/加表时递增，`migrate` 里按版本号补。
///   1 → v4 表骨架
///   2 → asset 索引表 + chat 会话表
///   3 → knowledge 表（用户自建知识库；预置条目走编译期嵌入，不进库）
///   4 → 知识库改成「库 → 条目 → chunk」三层，另加会话滚动摘要表
///   5 → 知识库支持向量化（库记住嵌入模型、chunk 存向量）与图片块
const SCHEMA_VERSION: i64 = 5;


/// 打开（必要时创建）数据库。
///
/// WAL 是必须的：自动保存是 500ms 防抖触发，同时工具层可能正在读，
/// 默认的 rollback journal 会让读把写挡住。
/// `synchronous=NORMAL` 在 WAL 下已经能保证「进程崩溃不丢已提交事务」，
/// 又不像 FULL 那样每次 fsync。
pub fn open(path: &Path) -> Result<Connection, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("创建数据目录失败：{error}"))?;
    }
    let conn = Connection::open(path).map_err(|error| format!("打开数据库失败：{error}"))?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA foreign_keys=ON;
         PRAGMA busy_timeout=5000;",
    )
    .map_err(|error| format!("初始化数据库参数失败：{error}"))?;
    migrate(&conn)?;
    Ok(conn)
}

/// 只增不改的结构：不升 schema version 也能装到任何已有的库上。
fn ensure_additive(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS settings (
             key   TEXT PRIMARY KEY,
             value TEXT NOT NULL
         );
         -- 附件正文的 Fact 层（见 source.rs）：附件只在某一轮里出现，正文必须留一份。
         -- stored 与 inbox 里的文件名同一个（内容寻址），所以重复上传天然只落一行。
         CREATE TABLE IF NOT EXISTS source (
             stored   TEXT PRIMARY KEY,
             name     TEXT NOT NULL,
             ext      TEXT NOT NULL,
             pages    INTEGER,
             chars    INTEGER NOT NULL,
             text     TEXT NOT NULL,
             added_at INTEGER NOT NULL
         );",
    )
    .map_err(|error| format!("创建设置/原件表失败：{error}"))
}

/// 常驻数据库连接。所有命令共用一份，避免每次读写都重新 open + migrate。
///
/// `Mutex<Option<Connection>>` 而不是 `Connection`：Connection 不是 Sync，
/// 而且首次访问才打开能保证「启动时不因为库坏掉而直接崩」。
pub struct Shared {
    path: PathBuf,
    conn: std::sync::Mutex<Option<Connection>>,
}

impl Shared {
    pub fn new(path: PathBuf) -> Self {
        Self { path, conn: std::sync::Mutex::new(None) }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 借连接执行一段同步操作。不要在 await 期间持有 —— 守卫不是 Send。
    pub fn with<T>(&self, f: impl FnOnce(&mut Connection) -> Result<T, String>) -> Result<T, String> {
        let mut guard = self
            .conn
            .lock()
            .map_err(|_| "数据库连接不可用".to_string())?;
        if guard.is_none() {
            *guard = Some(open(&self.path)?);
        }
        let conn = guard.as_mut().ok_or_else(|| "数据库连接不可用".to_string())?;
        f(conn)
    }
}

/// 建表 + 迁移。测试里要一个内存库时也走这条：口径必须和真实库一致，
/// 各写一份 CREATE TABLE 的测试库迟早和生产库不同。
pub(crate) fn migrate(conn: &Connection) -> Result<(), String> {
    // 纯新增的表放在版本门前面：版本门是为「改动既有结构」设的。
    // 把它当建表开关用，每加一张表就得升版本、让所有老库跑一遍完整迁移 —— 代价不对等；
    // 而 IF NOT EXISTS 对老库、新库、测试库都是同一句话。
    ensure_additive(conn)?;

    let current: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|error| format!("读取数据库版本失败：{error}"))?;
    if current >= SCHEMA_VERSION {
        return Ok(());
    }

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS person (
             id   TEXT PRIMARY KEY,
             ord  INTEGER NOT NULL,
             body TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS library_item (
             id        TEXT PRIMARY KEY,
             person_id TEXT NOT NULL REFERENCES person(id) ON DELETE CASCADE,
             ord       INTEGER NOT NULL,
             body      TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS resume (
             id        TEXT PRIMARY KEY,
             person_id TEXT NOT NULL REFERENCES person(id) ON DELETE CASCADE,
             ord       INTEGER NOT NULL,
             body      TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS page (
             id       TEXT PRIMARY KEY,
             resume_id TEXT NOT NULL REFERENCES resume(id) ON DELETE CASCADE,
             ord      INTEGER NOT NULL,
             label    TEXT NOT NULL DEFAULT '',
             -- native：我们构建的页；file：用户原件（v5 起使用）
             kind     TEXT NOT NULL DEFAULT 'native',
             asset_id TEXT,
             body     TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS message (
             id        TEXT PRIMARY KEY,
             person_id TEXT NOT NULL REFERENCES person(id) ON DELETE CASCADE,
             ord       INTEGER NOT NULL,
             body      TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS ui (
             key   TEXT PRIMARY KEY,
             value TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS meta (
             key   TEXT PRIMARY KEY,
             value TEXT NOT NULL
         );

         -- 知识库是三层：库 → 条目 → chunk。
         --   库：一个主题（「电商行业口径」「这家公司的岗位要求」），用户可增删改
         --   条目：库下面的一篇（标题 + 标签 + 正文），保存时按固定规则切块
         --   chunk：检索的最小单位，带摘要与关联问题（由模型生成，方便换说法也能命中）
         -- 预置知识走编译期嵌入的 manifest，不进这些表；两者在检索时合成一个索引。
         CREATE TABLE IF NOT EXISTS knowledge_library (
             id         TEXT PRIMARY KEY,
             name       TEXT NOT NULL,
             note       TEXT NOT NULL DEFAULT '',
             ord        INTEGER NOT NULL DEFAULT 0,
             updated_at INTEGER NOT NULL,
             -- 这个库做向量化时用的嵌入模型名：每个库只认一个模型，
             -- 运行时的嵌入模型与它不一致就退回词项检索（宁可降级，不要拿错向量算余弦）
             embedding_model TEXT NOT NULL DEFAULT '',
             embedding_dim   INTEGER NOT NULL DEFAULT 0
         );
         CREATE TABLE IF NOT EXISTS knowledge_entry (
             id         TEXT PRIMARY KEY,
             library_id TEXT NOT NULL,
             title      TEXT NOT NULL,
             tags       TEXT NOT NULL DEFAULT '[]',
             body       TEXT NOT NULL,
             ord        INTEGER NOT NULL DEFAULT 0,
             updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS knowledge_chunk (
             id         TEXT PRIMARY KEY,
             entry_id   TEXT NOT NULL,
             ord        INTEGER NOT NULL,
             heading    TEXT NOT NULL DEFAULT '',
             text       TEXT NOT NULL,
             summary    TEXT NOT NULL DEFAULT '',
             questions  TEXT NOT NULL DEFAULT '[]',
             updated_at INTEGER NOT NULL,
             -- int8 量化后的向量（L2 归一化 × 127），没有就是 NULL
             vector       BLOB,
             vector_model TEXT NOT NULL DEFAULT '',
             kind         TEXT NOT NULL DEFAULT 'text',
             asset        TEXT NOT NULL DEFAULT '',
             asset_name   TEXT NOT NULL DEFAULT '',
             annotation   TEXT NOT NULL DEFAULT ''
         );
         -- 会话滚动摘要：历史超预算时，被挤掉的那部分压成摘要，随窗口带下去。
         -- upto 记「已经摘要到哪条消息」，同一条不重复生成。
         CREATE TABLE IF NOT EXISTS chat_digest (
             scope      TEXT PRIMARY KEY,
             upto       TEXT NOT NULL,
             text       TEXT NOT NULL,
             updated_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_kentry_lib ON knowledge_entry(library_id, ord);
         CREATE INDEX IF NOT EXISTS idx_kchunk_entry ON knowledge_chunk(entry_id, ord);
         -- 资产索引：文件本体在 pool/ 下（哈希命名），这里只记「它是什么」
         CREATE TABLE IF NOT EXISTS asset (
             id       TEXT PRIMARY KEY,
             sha256   TEXT NOT NULL DEFAULT '',
             stored   TEXT NOT NULL,
             name     TEXT NOT NULL DEFAULT '',
             mime     TEXT NOT NULL DEFAULT '',
             bytes    INTEGER NOT NULL DEFAULT 0,
             added_at INTEGER NOT NULL DEFAULT 0,
             -- 上一轮有没有被引用。用来识别「从有到无」这个瞬间，
             -- 只有那一刻才回收文件；一直没被用过的新资产要留着
             refs     INTEGER NOT NULL DEFAULT 0
         );
         -- 会话：scope 是 personId（会话 agent）或 layout:<resumeId>:<pageId>（页排版 agent）
         CREATE TABLE IF NOT EXISTS chat (
             scope TEXT NOT NULL,
             ord   INTEGER NOT NULL,
             body  TEXT NOT NULL,
             PRIMARY KEY (scope, ord)
         );
         CREATE INDEX IF NOT EXISTS idx_item_person ON library_item(person_id, ord);
         CREATE INDEX IF NOT EXISTS idx_resume_person ON resume(person_id, ord);
         CREATE INDEX IF NOT EXISTS idx_page_resume ON page(resume_id, ord);
         CREATE INDEX IF NOT EXISTS idx_message_person ON message(person_id, ord);
         CREATE INDEX IF NOT EXISTS idx_asset_sha ON asset(sha256);",
    )
    .map_err(|error| format!("建表失败：{error}"))?;

    // v5：给已有的知识库表补列（新库在上面建表时就已经是完整结构）
    for statement in [
        "ALTER TABLE knowledge_library ADD COLUMN embedding_model TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE knowledge_library ADD COLUMN embedding_dim INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE knowledge_chunk ADD COLUMN vector BLOB",
        "ALTER TABLE knowledge_chunk ADD COLUMN vector_model TEXT NOT NULL DEFAULT ''",
        // text = 文字块；image = 图片块（一张图一块，保留原格式）
        "ALTER TABLE knowledge_chunk ADD COLUMN kind TEXT NOT NULL DEFAULT 'text'",
        // 图片块：材料池里的存储名 + 原始文件名
        "ALTER TABLE knowledge_chunk ADD COLUMN asset TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE knowledge_chunk ADD COLUMN asset_name TEXT NOT NULL DEFAULT ''",
        // 图片摘要来源：model（多模态）/ ocr / none（没有摘要）
        "ALTER TABLE knowledge_chunk ADD COLUMN annotation TEXT NOT NULL DEFAULT ''",
    ] {
        // 已经加过就报 duplicate column，忽略即可
        let _ = conn.execute(statement, []);
    }

    // v3 的平坦 knowledge 表 → v4 的「库 → 条目」。
    // 只在升级时搬一次：老表留着不动（不删用户数据），但之后的读写都走新表。
    if current < 4 {
        let legacy: i64 = conn
            .query_row("SELECT COUNT(*) FROM knowledge", [], |row| row.get(0))
            .unwrap_or(0);
        if legacy > 0 {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            conn.execute(
                "INSERT OR IGNORE INTO knowledge_library (id, name, note, ord, updated_at)
                 VALUES ('lib-default', '求职知识', '', 0, ?1)",
                params![now],
            )
            .map_err(|error| format!("建默认知识库失败：{error}"))?;
            conn.execute(
                "INSERT OR IGNORE INTO knowledge_entry
                     (id, library_id, title, tags, body, ord, updated_at)
                 SELECT id, 'lib-default', title, tags, body, 0, updated_at FROM knowledge",
                [],
            )
            .map_err(|error| format!("迁移旧知识条目失败：{error}"))?;

            // 旧条目只有正文，没有 chunk。不补的话迁移完就检索不到了 ——
            // 这里按同一套规则（knowledge::split_chunks）先切一遍，
            // 摘要留给用户在面板里点「生成摘要」。
            let mut stmt = conn
                .prepare("SELECT id, body FROM knowledge")
                .map_err(|error| format!("读取旧知识条目失败：{error}"))?;
            let rows: Vec<(String, String)> = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .map_err(|error| format!("读取旧知识条目失败：{error}"))?
                .flatten()
                .collect();
            drop(stmt);
            for (id, body) in rows {
                for (index, piece) in crate::knowledge::split_chunks(&body).iter().enumerate() {
                    let (heading, text) = crate::knowledge::split_heading(piece);
                    conn.execute(
                        "INSERT OR IGNORE INTO knowledge_chunk
                             (id, entry_id, ord, heading, text, summary, questions, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, '', '[]', ?6)",
                        params![
                            format!("{id}#{}", index + 1),
                            id,
                            index as i64 + 1,
                            heading,
                            text,
                            now
                        ],
                    )
                    .map_err(|error| format!("迁移旧知识切片失败：{error}"))?;
                }
            }
        }
    }

    conn.pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(|error| format!("写入数据库版本失败：{error}"))?;
    Ok(())
}

fn to_text(value: &Value) -> Result<String, String> {
    serde_json::to_string(value).map_err(|error| format!("序列化失败：{error}"))
}

fn from_text(text: &str, what: &str) -> Result<Value, String> {
    serde_json::from_str(text).map_err(|error| format!("{what} 的 JSON 解析失败：{error}"))
}

fn id_of(value: &Value) -> String {
    value.get("id").and_then(Value::as_str).unwrap_or("").to_string()
}

fn take_array(object: &mut Map<String, Value>, key: &str) -> Vec<Value> {
    object
        .remove(key)
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
}

/// 整份写入，一个事务。前端每 500ms 防抖调一次，数据量小，直接删了重插。
///
/// 为什么不做增量 diff：diff 要维护「表里有什么」的另一份状态，一旦对不上
/// 就会出现幽灵行或丢行；而这里的量级（几个人、几十页）重插只要毫秒。
/// 等真到了几千行再换成 upsert + 删除差集，接口不用变。
pub fn save(conn: &mut Connection, workspace: &Workspace) -> Result<(), String> {
    let root = serde_json::to_value(workspace).map_err(|error| format!("序列化失败：{error}"))?;
    let people = root
        .get("people")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let ui = root.get("ui").cloned().unwrap_or_else(|| json!({}));
    let version = root
        .get("version")
        .and_then(Value::as_u64)
        .unwrap_or(model::VERSION as u64);
    let mut extra = root.as_object().cloned().unwrap_or_default();
    extra.remove("version");
    extra.remove("people");
    extra.remove("ui");

    let tx = conn.transaction().map_err(|error| format!("开启事务失败：{error}"))?;
    // 级联删除会带走子表；ui/meta 没有外键，单独清
    tx.execute("DELETE FROM person", [])
        .map_err(|error| format!("清空候选人失败：{error}"))?;
    tx.execute("DELETE FROM ui", [])
        .map_err(|error| format!("清空 ui 失败：{error}"))?;

    for (index, person) in people.iter().enumerate() {
        let mut body = person
            .as_object()
            .cloned()
            .ok_or_else(|| "候选人不是对象".to_string())?;
        let items = take_array(&mut body, "items");
        let resumes = take_array(&mut body, "resumes");
        let messages = take_array(&mut body, "messages");
        let person_id = id_of(person);
        if person_id.is_empty() {
            return Err("候选人缺 id".into());
        }
        let body = Value::Object(body);
        tx.execute(
            "INSERT INTO person (id, ord, body) VALUES (?1, ?2, ?3)",
            params![person_id, index as i64, to_text(&body)?],
        )
        .map_err(|error| format!("写入候选人失败：{error}"))?;

        for (order, item) in items.iter().enumerate() {
            let item_id = id_of(item);
            if item_id.is_empty() {
                return Err(format!("候选人 {person_id} 名下有经历条目缺 id"));
            }
            tx.execute(
                "INSERT INTO library_item (id, person_id, ord, body) VALUES (?1, ?2, ?3, ?4)",
                params![item_id, person_id, order as i64, to_text(item)?],
            )
            .map_err(|error| format!("写入经历条目失败：{error}"))?;
        }

        for (order, resume) in resumes.iter().enumerate() {
            let mut body = resume
                .as_object()
                .cloned()
                .ok_or_else(|| "简历不是对象".to_string())?;
            let pages = take_array(&mut body, "pages");
            let resume_id = id_of(resume);
            if resume_id.is_empty() {
                return Err(format!("候选人 {person_id} 名下有简历缺 id"));
            }
            let body = Value::Object(body);
            tx.execute(
                "INSERT INTO resume (id, person_id, ord, body) VALUES (?1, ?2, ?3, ?4)",
                params![resume_id, person_id, order as i64, to_text(&body)?],
            )
            .map_err(|error| format!("写入简历失败：{error}"))?;

            for (order, page) in pages.iter().enumerate() {
                let page_id = id_of(page);
                if page_id.is_empty() {
                    return Err(format!("简历 {resume_id} 下有页面缺 id"));
                }
                let label = page.get("label").and_then(Value::as_str).unwrap_or("");
                // v4 没有 kind，一律 native；v5 起 page 会带自己的 kind
                let kind = page.get("kind").and_then(Value::as_str).unwrap_or("native");
                let asset_id = page.get("assetId").and_then(Value::as_str);
                tx.execute(
                    "INSERT INTO page (id, resume_id, ord, label, kind, asset_id, body)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![page_id, resume_id, order as i64, label, kind, asset_id, to_text(page)?],
                )
                .map_err(|error| format!("写入页面失败：{error}"))?;
            }
        }

        for (order, message) in messages.iter().enumerate() {
            // 消息理论上都有 id；缺了就用序号命名，至少不丢内容
            let mut message_id = id_of(message);
            if message_id.is_empty() {
                message_id = format!("{person_id}-msg-{order}");
            }
            tx.execute(
                "INSERT INTO message (id, person_id, ord, body) VALUES (?1, ?2, ?3, ?4)",
                params![message_id, person_id, order as i64, to_text(message)?],
            )
            .map_err(|error| format!("写入消息失败：{error}"))?;
        }
    }

    if let Some(object) = ui.as_object() {
        for (key, value) in object {
            tx.execute(
                "INSERT INTO ui (key, value) VALUES (?1, ?2)",
                params![key, to_text(value)?],
            )
            .map_err(|error| format!("写入 ui 失败：{error}"))?;
        }
    }

    tx.execute("DELETE FROM meta", [])
        .map_err(|error| format!("清空 meta 失败：{error}"))?;
    for (key, value) in [("version", json!(version)), ("root_extra", Value::Object(extra))] {
        tx.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)",
            params![key, to_text(&value)?],
        )
        .map_err(|error| format!("写入 meta 失败：{error}"))?;
    }

    tx.commit()
        .map_err(|error| format!("提交事务失败：{error}"))?;
    Ok(())
}

/// 读回整份工作区。全新库（一行都没有）返回 None，与旧版「文件不存在」对齐。
pub fn load(conn: &Connection) -> Result<Option<Workspace>, String> {
    let rows: i64 = conn
        .query_row("SELECT count(*) FROM person", [], |row| row.get(0))
        .map_err(|error| format!("统计候选人失败：{error}"))?;
    let ui_rows: i64 = conn
        .query_row("SELECT count(*) FROM ui", [], |row| row.get(0))
        .map_err(|error| format!("统计 ui 失败：{error}"))?;
    if rows == 0 && ui_rows == 0 {
        return Ok(None);
    }

    let mut people = Vec::new();
    let mut stmt = conn
        .prepare("SELECT id, body FROM person ORDER BY ord")
        .map_err(|error| format!("读取候选人失败：{error}"))?;
    let list: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|error| format!("查询候选人失败：{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("读取候选人行失败：{error}"))?;

    for (person_id, body) in list {
        let mut person = from_text(&body, "候选人")?;
        let object = person
            .as_object_mut()
            .ok_or_else(|| "候选人 JSON 不是对象".to_string())?;
        // 子节点各自成表，读的时候按顺序装回去；缺的补空数组，
        // 否则前端拿到 undefined 会在 `.map` 上炸
        object.insert("items".into(), load_children(conn, "library_item", "person_id", &person_id)?);
        object.insert("messages".into(), load_children(conn, "message", "person_id", &person_id)?);

        let mut resumes = Vec::new();
        let mut stmt = conn
            .prepare("SELECT id, body FROM resume WHERE person_id = ?1 ORDER BY ord")
            .map_err(|error| format!("读取简历失败：{error}"))?;
        let list: Vec<(String, String)> = stmt
            .query_map(params![person_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|error| format!("查询简历失败：{error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("读取简历行失败：{error}"))?;
        for (resume_id, body) in list {
            let mut resume = from_text(&body, "简历")?;
            let object = resume
                .as_object_mut()
                .ok_or_else(|| "简历 JSON 不是对象".to_string())?;
            object.insert("pages".into(), load_children(conn, "page", "resume_id", &resume_id)?);
            resumes.push(resume);
        }
        object.insert("resumes".into(), Value::Array(resumes));
        people.push(person);
    }

    let mut ui = Map::new();
    let mut stmt = conn
        .prepare("SELECT key, value FROM ui")
        .map_err(|error| format!("读取 ui 失败：{error}"))?;
    let list: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|error| format!("查询 ui 失败：{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("读取 ui 行失败：{error}"))?;
    for (key, value) in list {
        ui.insert(key, from_text(&value, "ui")?);
    }

    let version = meta(conn, "version")?
        .and_then(|value| value.as_u64())
        .unwrap_or(model::VERSION as u64) as u32;
    let extra = meta(conn, "root_extra")?
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();

    let mut root = Map::new();
    root.insert("version".into(), json!(version));
    root.insert("people".into(), Value::Array(people));
    root.insert("ui".into(), Value::Object(ui));
    root.extend(extra);

    let mut workspace: Workspace = serde_json::from_value(Value::Object(root))
        .map_err(|error| format!("数据库内容无法解析：{error}"))?;
    // 老库里存的是 v4 形状（header + sections），这里补成 v5 的 blocks。
    // 不在这里回写：读路径只读，回写交给 read_state 决定（它知道是不是真有变化）。
    workspace.normalize();
    model::validate(&workspace)?;
    Ok(Some(workspace))
}

/// 子表读取。表名列名只来自本文件里的常量，不存在注入问题。
fn load_children(
    conn: &Connection,
    table: &str,
    parent: &str,
    parent_id: &str,
) -> Result<Value, String> {
    let sql = format!("SELECT body FROM {table} WHERE {parent} = ?1 ORDER BY ord");
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|error| format!("读取 {table} 失败：{error}"))?;
    let rows: Vec<String> = stmt
        .query_map(params![parent_id], |row| row.get(0))
        .map_err(|error| format!("查询 {table} 失败：{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("读取 {table} 行失败：{error}"))?;
    let mut out = Vec::with_capacity(rows.len());
    for text in rows {
        out.push(from_text(&text, table)?);
    }
    Ok(Value::Array(out))
}

/// 库里记的模型版本。
///
/// 用来看「读出来已经是升级后的形状，但版本号还没跟上」这种情况：
/// 老库的 meta 可能写着 4，内容其实是 v5（`load` 时已经 normalize）。
pub fn version(conn: &Connection) -> Result<u32, String> {
    Ok(meta(conn, "version")?
        .and_then(|value| value.as_u64())
        .unwrap_or(model::VERSION as u64) as u32)
}

/* ---------- 应用设置（模型 / 密钥 / 槽位 / 自定义模型） ---------- */

/// 一次把全部设置读出来：前端启动时要同步用它们渲染，分五次 IPC 不如一次。
pub fn settings_all(conn: &Connection) -> Result<Vec<(String, String)>, String> {
    let mut stmt = conn
        .prepare("SELECT key, value FROM settings")
        .map_err(|error| format!("读取设置失败：{error}"))?;
    let rows = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|error| format!("读取设置失败：{error}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("读取设置失败：{error}"))
}

pub fn settings_put(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )
    .map_err(|error| format!("写入设置失败：{error}"))?;
    Ok(())
}

pub fn settings_drop(conn: &Connection, key: &str) -> Result<(), String> {
    conn.execute("DELETE FROM settings WHERE key = ?1", params![key])
        .map_err(|error| format!("删除设置失败：{error}"))?;
    Ok(())
}

fn meta(conn: &Connection, key: &str) -> Result<Option<Value>, String> {
    let mut stmt = conn
        .prepare("SELECT value FROM meta WHERE key = ?1")
        .map_err(|error| format!("读取 meta 失败：{error}"))?;
    let mut rows = stmt
        .query(params![key])
        .map_err(|error| format!("查询 meta 失败：{error}"))?;
    match rows
        .next()
        .map_err(|error| format!("读取 meta 行失败：{error}"))?
    {
        Some(row) => {
            let text: String = row.get(0).map_err(|error| format!("读取 meta 值失败：{error}"))?;
            Ok(Some(from_text(&text, "meta")?))
        }
        None => Ok(None),
    }
}

/// 清空工作区（前端「清空」走这条）。表结构保留，下次照常写。
pub fn clear(conn: &Connection) -> Result<(), String> {
    conn.execute_batch("DELETE FROM person; DELETE FROM ui; DELETE FROM meta; DELETE FROM chat;")
        .map_err(|error| format!("清空工作区失败：{error}"))
}

/* ---------- 会话 ---------- */

/// 读一个 scope 的全部消息（顺序即对话顺序）。
///
/// 为什么会话也进库：以前是 sessions.json 整份重写，每发一条消息就把所有人
/// 全部会话序列化一遍 —— 那是 O(n²) 的，而且写到一半崩溃会留下半截 JSON，
/// 下次启动整段历史静默消失。进表之后一次 append 就是一行 INSERT。
pub fn chat_load(conn: &Connection, scope: &str) -> Result<Vec<Value>, String> {
    let mut stmt = conn
        .prepare("SELECT body FROM chat WHERE scope = ?1 ORDER BY ord")
        .map_err(|error| format!("读取会话失败：{error}"))?;
    let rows: Vec<String> = stmt
        .query_map(params![scope], |row| row.get(0))
        .map_err(|error| format!("查询会话失败：{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("读取会话行失败：{error}"))?;
    let mut out = Vec::with_capacity(rows.len());
    for text in rows {
        out.push(from_text(&text, "会话消息")?);
    }
    Ok(out)
}

pub fn chat_append(conn: &Connection, scope: &str, message: &Value) -> Result<(), String> {
    conn.execute(
        "INSERT INTO chat (scope, ord, body)
         VALUES (?1, (SELECT COALESCE(MAX(ord), -1) + 1 FROM chat WHERE scope = ?1), ?2)",
        params![scope, to_text(message)?],
    )
    .map_err(|error| format!("写入会话失败：{error}"))?;
    Ok(())
}

/// 合并字段（流式结束后补 content / tool / changes）。
/// 读-改-写而不是 json_patch：一条 scope 最多几百条消息，省掉对 JSON1 扩展的依赖。
pub fn chat_patch(conn: &Connection, scope: &str, id: &str, patch: &Value) -> Result<(), String> {
    let list = chat_load(conn, scope)?;
    let mut hit = false;
    for message in list {
        if message.get("id").and_then(Value::as_str) != Some(id) {
            continue;
        }
        let mut merged = message.clone();
        if let (Some(target), Some(source)) = (merged.as_object_mut(), patch.as_object()) {
            for (key, value) in source {
                target.insert(key.clone(), value.clone());
            }
        }
        conn.execute(
            "UPDATE chat SET body = ?3 WHERE scope = ?1 AND ord = ?2",
            params![scope, ord_of(conn, scope, id)?, to_text(&merged)?],
        )
        .map_err(|error| format!("更新会话失败：{error}"))?;
        hit = true;
        break;
    }
    if hit {
        Ok(())
    } else {
        Err(format!("会话里没有 id = {id} 的消息"))
    }
}

fn ord_of(conn: &Connection, scope: &str, id: &str) -> Result<i64, String> {
    conn.query_row(
        "SELECT ord FROM chat WHERE scope = ?1 AND json_extract(body, '$.id') = ?2 LIMIT 1",
        params![scope, id],
        |row| row.get(0),
    )
    .map_err(|error| format!("找不到消息 {id}：{error}"))
}

pub fn chat_clear(conn: &Connection, scope: &str) -> Result<(), String> {
    conn.execute("DELETE FROM chat WHERE scope = ?1", params![scope])
        .map_err(|error| format!("清空会话失败：{error}"))?;
    // 摘要也要跟着清：留着它，新对话第一句就会被旧摘要污染
    conn.execute("DELETE FROM chat_digest WHERE scope = ?1", params![scope])
        .map_err(|error| format!("清空会话摘要失败：{error}"))?;
    Ok(())
}

/// 读滚动摘要：(已经摘要到哪条消息, 摘要正文)
pub fn chat_digest_get(conn: &Connection, scope: &str) -> Result<Option<(String, String)>, String> {
    conn.query_row(
        "SELECT upto, text FROM chat_digest WHERE scope = ?1",
        params![scope],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )
    .map(Some)
    .or_else(|error| match error {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        other => Err(format!("读取会话摘要失败：{other}")),
    })
}

pub fn chat_digest_set(
    conn: &Connection,
    scope: &str,
    upto: &str,
    text: &str,
    at: i64,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO chat_digest (scope, upto, text, updated_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(scope) DO UPDATE SET upto = excluded.upto, text = excluded.text,
                                          updated_at = excluded.updated_at",
        params![scope, upto, text, at],
    )
    .map_err(|error| format!("写入会话摘要失败：{error}"))?;
    Ok(())
}

pub fn chat_len(conn: &Connection, scope: &str) -> Result<usize, String> {
    let count: i64 = conn
        .query_row("SELECT count(*) FROM chat WHERE scope = ?1", params![scope], |row| {
            row.get(0)
        })
        .map_err(|error| format!("统计会话失败：{error}"))?;
    Ok(count as usize)
}

/// 把 v4 时代存进 `message` 表（person 的子节点）的会话搬进 `chat` 表。
///
/// 只在 chat 里还没有这个 scope 时搬：否则每次启动都会把旧消息再插一遍。
pub fn chat_import_legacy(conn: &mut Connection) -> Result<usize, String> {
    let people: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT DISTINCT person_id FROM message")
            .map_err(|error| format!("读取旧会话失败：{error}"))?;
        let rows = stmt
            .query_map([], |row| row.get(0))
            .map_err(|error| format!("查询旧会话失败：{error}"))?
            .collect::<Result<Vec<String>, _>>()
            .map_err(|error| format!("读取旧会话行失败：{error}"))?;
        rows
    };

    let mut moved = 0usize;
    for person_id in people {
        if chat_len(conn, &person_id)? > 0 {
            continue;
        }
        let messages = load_children(conn, "message", "person_id", &person_id)?;
        let Some(list) = messages.as_array() else { continue };
        let tx = conn.transaction().map_err(|error| format!("开启事务失败：{error}"))?;
        for (order, message) in list.iter().enumerate() {
            tx.execute(
                "INSERT INTO chat (scope, ord, body) VALUES (?1, ?2, ?3)",
                params![person_id, order as i64, to_text(message)?],
            )
            .map_err(|error| format!("导入旧会话失败：{error}"))?;
            moved += 1;
        }
        tx.commit().map_err(|error| format!("提交导入失败：{error}"))?;
    }
    Ok(moved)
}

/* ---------- 资产索引 ---------- */

#[derive(Debug, Clone)]
pub struct AssetRow {
    pub id: String,
    pub sha256: String,
    pub stored: String,
    pub name: String,
    pub mime: String,
    pub bytes: i64,
    pub added_at: i64,
    pub refs: i64,
}

const ASSET_COLS: &str = "id, sha256, stored, name, mime, bytes, added_at, refs";

fn asset_from(row: &rusqlite::Row<'_>) -> rusqlite::Result<AssetRow> {
    Ok(AssetRow {
        id: row.get(0)?,
        sha256: row.get(1)?,
        stored: row.get(2)?,
        name: row.get(3)?,
        mime: row.get(4)?,
        bytes: row.get(5)?,
        added_at: row.get(6)?,
        refs: row.get(7)?,
    })
}

pub fn asset_upsert(conn: &Connection, row: &AssetRow) -> Result<(), String> {
    conn.execute(
        "INSERT INTO asset (id, sha256, stored, name, mime, bytes, added_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET
           sha256=excluded.sha256, stored=excluded.stored, name=excluded.name,
           mime=excluded.mime, bytes=excluded.bytes",
        params![row.id, row.sha256, row.stored, row.name, row.mime, row.bytes, row.added_at],
    )
    .map_err(|error| format!("写入资产索引失败：{error}"))?;
    Ok(())
}

pub fn asset_set_refs(conn: &Connection, id: &str, refs: i64) -> Result<(), String> {
    conn.execute(
        "UPDATE asset SET refs = ?2 WHERE id = ?1",
        params![id, refs],
    )
    .map_err(|error| format!("更新资产引用失败：{error}"))?;
    Ok(())
}

pub fn asset_by_id(conn: &Connection, id: &str) -> Result<Option<AssetRow>, String> {
    let sql = format!("SELECT {ASSET_COLS} FROM asset WHERE id = ?1");
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|error| format!("读取资产失败：{error}"))?;
    match stmt.query_row(params![id], asset_from) {
        Ok(row) => Ok(Some(row)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(format!("查询资产失败：{error}")),
    }
}

pub fn asset_by_sha(conn: &Connection, sha256: &str) -> Result<Option<AssetRow>, String> {
    let sql = format!("SELECT {ASSET_COLS} FROM asset WHERE sha256 = ?1 LIMIT 1");
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|error| format!("读取资产失败：{error}"))?;
    match stmt.query_row(params![sha256], asset_from) {
        Ok(row) => Ok(Some(row)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(format!("查询资产失败：{error}")),
    }
}

pub fn asset_all(conn: &Connection) -> Result<Vec<AssetRow>, String> {
    let sql = format!("SELECT {ASSET_COLS} FROM asset ORDER BY added_at");
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|error| format!("读取资产失败：{error}"))?;
    let rows = stmt
        .query_map([], asset_from)
        .map_err(|error| format!("查询资产失败：{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("读取资产行失败：{error}"))?;
    Ok(rows)
}

pub fn asset_remove(conn: &Connection, id: &str) -> Result<(), String> {
    conn.execute("DELETE FROM asset WHERE id = ?1", params![id])
        .map_err(|error| format!("删除资产索引失败：{error}"))?;
    Ok(())
}

/// 知识库图片块引用到的池文件名。
///
/// 为什么回收前必须问一次：池是内容寻址的，同一份图片先进简历、再进知识库时
/// 存的是同一个文件。只按工作区算「还在不在用」，用户把简历里那份删掉，
/// 知识库里那张图会被一起挪进回收站 —— 图片块立刻变成一行读不出来的 markdown。
pub fn knowledge_asset_stored(conn: &Connection) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare("SELECT DISTINCT asset FROM knowledge_chunk WHERE kind = 'image' AND asset <> ''")
        .map_err(|error| format!("读取知识库图片引用失败：{error}"))?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| format!("查询知识库图片引用失败：{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("读取知识库图片引用行失败：{error}"))?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Workspace;

    fn memory() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        migrate(&conn).unwrap();
        conn
    }

    /// 设置存 settings 表，**不能**被工作区保存顺手清掉 ——
    /// 它和 meta（工作区快照的一部分）是两件事，混在一起会丢密钥。
    #[test]
    fn settings_survive_a_workspace_save() {
        let mut conn = memory();
        settings_put(&conn, "ai:v1", r#"{"model":"x"}"#).unwrap();
        save(&mut conn, &sample()).unwrap();
        assert_eq!(
            settings_all(&conn).unwrap(),
            vec![("ai:v1".to_string(), r#"{"model":"x"}"#.to_string())]
        );
        settings_drop(&conn, "ai:v1").unwrap();
        assert!(settings_all(&conn).unwrap().is_empty());
    }

    #[test]
    fn settings_put_overwrites_in_place() {
        let conn = memory();
        settings_put(&conn, "slots:v1", "{}").unwrap();
        settings_put(&conn, "slots:v1", "{\"layout\":null}").unwrap();
        assert_eq!(settings_all(&conn).unwrap().len(), 1);
        assert_eq!(settings_all(&conn).unwrap()[0].1, "{\"layout\":null}");
    }

    fn sample() -> Workspace {
        model::adopt(json!({
            "version": 4,
            "people": [{
                "id": "p1",
                "fields": [{ "id": "f1", "label": "姓名", "value": "张三", "pinned": true }],
                "items": [{
                    "id": "i1",
                    "fields": [{ "id": "f2", "label": "公司", "value": "中远" }],
                    "notes": "细节记录",
                    "assets": [{ "id": "a1", "kind": "image", "name": "证书.png", "stored": "x.png" }]
                }],
                "messages": [{ "id": "m1", "role": "user", "content": "你好" }],
                "resumes": [{
                    "id": "r1",
                    "personId": "p1",
                    "title": "外贸单证",
                    "updatedAt": 123,
                    "pages": [{
                        "id": "pg1",
                        "label": "第 1 页",
                        "header": [{ "id": "h1", "label": "姓名", "value": "张三" }],
                        "sections": [{
                            "id": "s1",
                            "title": "工作经历",
                            "items": [{ "id": "e1", "refId": "i1", "bullets": ["a", "b"] }]
                        }],
                        "futureLayout": { "columns": 2 }
                    }]
                }],
                "futureField": 42
            }],
            "ui": { "accent": "ink", "mode": "dark", "openPages": ["pg1"], "sideW": 260 },
            "futureTop": "keep me"
        }))
        .unwrap()
    }

    #[test]
    fn roundtrip_is_lossless_including_unknown_fields() {
        let mut conn = memory();
        let before = sample();
        save(&mut conn, &before).unwrap();

        let after = load(&conn).unwrap().expect("应有内容");
        let a = serde_json::to_value(&before).unwrap();
        let b = serde_json::to_value(&after).unwrap();
        assert_eq!(a, b, "往返后必须完全一致（含未知字段与顺序）");

        // 单独确认几处最容易丢的
        assert_eq!(b["futureTop"], json!("keep me"));
        assert_eq!(b["people"][0]["futureField"], json!(42));
        assert_eq!(
            b["people"][0]["resumes"][0]["pages"][0]["futureLayout"]["columns"],
            json!(2)
        );
        assert_eq!(b["people"][0]["items"][0]["assets"][0]["name"], json!("证书.png"));
        assert_eq!(b["people"][0]["messages"][0]["content"], json!("你好"));
    }

    #[test]
    fn empty_db_loads_as_none() {
        let conn = memory();
        assert!(load(&conn).unwrap().is_none());
    }

    /// 材料池回收要知道知识库图片在引用谁：只报图片块、去重、空 asset 不算
    #[test]
    fn knowledge_images_are_reported_as_pool_refs() {
        let conn = memory();
        conn.execute(
            "INSERT INTO knowledge_entry (id, library_id, title, body, updated_at)
             VALUES ('e1', 'lib-default', '门店', '![门店](asset:ab.png)', 0)",
            [],
        )
        .unwrap();
        for (id, kind, asset) in [
            ("c1", "image", "ab.png"),
            ("c2", "image", "ab.png"),
            ("c3", "text", "cd.png"),
            ("c4", "image", ""),
        ] {
            conn.execute(
                "INSERT INTO knowledge_chunk (id, entry_id, ord, text, updated_at, kind, asset)
                 VALUES (?1, 'e1', 0, '', 0, ?2, ?3)",
                params![id, kind, asset],
            )
            .unwrap();
        }
        assert_eq!(knowledge_asset_stored(&conn).unwrap(), vec!["ab.png".to_string()]);
    }

    #[test]
    fn overwrite_replaces_children_instead_of_accumulating() {
        let mut conn = memory();
        save(&mut conn, &sample()).unwrap();

        // 删掉那页再存一次：页表不能留下幽灵行
        let mut value = serde_json::to_value(sample()).unwrap();
        value["people"][0]["resumes"][0]["pages"] = json!([]);
        let next: Workspace = serde_json::from_value(value).unwrap();
        save(&mut conn, &next).unwrap();

        let pages: i64 = conn
            .query_row("SELECT count(*) FROM page", [], |row| row.get(0))
            .unwrap();
        assert_eq!(pages, 0, "旧页必须被删掉");
        let after = load(&conn).unwrap().unwrap();
        assert!(after.people[0].resumes[0].pages.is_empty());
    }

    #[test]
    fn deleting_person_cascades_to_children() {
        let mut conn = memory();
        save(&mut conn, &sample()).unwrap();
        conn.execute("DELETE FROM person WHERE id = 'p1'", []).unwrap();
        for table in ["library_item", "resume", "page", "message"] {
            let count: i64 = conn
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0))
                .unwrap();
            assert_eq!(count, 0, "{table} 应被级联删除");
        }
    }

    #[test]
    fn clear_empties_everything_but_keeps_schema() {
        let mut conn = memory();
        save(&mut conn, &sample()).unwrap();
        clear(&conn).unwrap();
        assert!(load(&conn).unwrap().is_none());
        // 清完还能继续写
        save(&mut conn, &sample()).unwrap();
        assert!(load(&conn).unwrap().is_some());
    }

    /// 拿本机真实数据跑一遍。手动执行：
    /// cargo test --lib real_workspace_survives_sqlite -- --ignored --nocapture
    #[test]
    #[ignore = "依赖本机 target/debug/SharpResumeData/workspace.json"]
    fn real_workspace_survives_sqlite() {
        let path = Path::new("target/debug/SharpResumeData/workspace.json.imported");
        let Ok(text) = std::fs::read_to_string(path) else {
            eprintln!("跳过：没有 {path:?}");
            return;
        };
        let before = model::adopt(serde_json::from_str(&text).expect("真实数据应是合法 JSON"))
            .expect("真实数据应能接管");
        let mut conn = memory();
        save(&mut conn, &before).unwrap();
        let after = load(&conn).unwrap().expect("应有内容");
        assert_eq!(
            serde_json::to_value(&before).unwrap(),
            serde_json::to_value(&after).unwrap(),
            "真实数据经 SQLite 往返必须完全一致"
        );
        eprintln!(
            "people={} pages={}",
            before.people.len(),
            before.people.iter().flat_map(|p| &p.resumes).flat_map(|r| &r.pages).count()
        );
    }

    /// 直接读真实落盘的 app.db，确认真实应用跑完迁移后库里有东西。
    /// 手动执行：cargo test --lib real_db_from_disk -- --ignored --nocapture
    #[test]
    #[ignore = "依赖本机 target/debug/SharpResumeData/app.db"]
    fn real_db_from_disk_loads() {
        let path = Path::new("target/debug/SharpResumeData/app.db");
        if !path.exists() {
            eprintln!("跳过：没有 {path:?}");
            return;
        }
        let conn = open(path).expect("应能打开真实库");
        let workspace = load(&conn).expect("应能读").expect("库里应有内容");
        let pages = workspace
            .people
            .iter()
            .flat_map(|person| &person.resumes)
            .flat_map(|resume| &resume.pages)
            .count();
        eprintln!(
            "people={} resumes={} pages={}",
            workspace.people.len(),
            workspace.people.iter().map(|p| p.resumes.len()).sum::<usize>(),
            pages
        );
        assert!(!workspace.people.is_empty(), "迁移后不该是空库");
    }
}
