//! 知识库：预置（编译期嵌入）+ 用户自建（app.db），一个检索入口。
//!
//! 为什么两套来源合成一个索引：对模型来说「知道什么」只该有一个答案 ——
//! 预置条目和用户条目在检索时不该分两套工具、两套排序。界面上才分
//! 「预置（只读）」与「我的（可改）」，检索一视同仁。
//!
//! 预置条目来自仓库根目录的 `knowledge/manifest.json`（规范见 `knowledge/README.md`），
//! 构建期用 include_str! 嵌进二进制，安装目录里看不到、也改不了。
//! 用户条目落在 app.db 的 `knowledge` 表，跟着数据目录走。
//!
//! 检索是「词项 + 向量」两路融合：
//!   - 词项：中文二元组 + 英文/数字词，BM25；标题 ×3、标签 ×2、正文 ×1。
//!     零依赖、离线、可解释，是永远可用的那一半。
//!   - 向量：只在「manifest 带 embedding 段」且「设置里配了嵌入模型」时才算 query 向量。
//!     查询向量必须在运行时计算 —— 这是语义检索绕不过去的一次外部调用；
//!     没配就自动只走词项，索引不用重算。

use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::State;

use crate::db::Shared;

/* ---------- 预置数据 ---------- */

const PRESET_MANIFEST: &str = include_str!("../../knowledge/manifest.json");
const PRESET_VECTORS: &[u8] = include_bytes!("../../knowledge/vectors.bin");

#[derive(Debug, Deserialize)]
struct Manifest {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    embedding: Option<ManifestEmbedding>,
    #[serde(default)]
    entries: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestEmbedding {
    model: String,
    dim: usize,
    /// int8 量化系数：存的是 round(x * scale)，x 已 L2 归一化
    #[serde(default = "default_scale")]
    scale: f32,
}

fn default_scale() -> f32 {
    127.0
}

#[derive(Debug, Deserialize)]
struct ManifestEntry {
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    chunks: Vec<ManifestChunk>,
}

#[derive(Debug, Deserialize)]
struct ManifestChunk {
    #[serde(default)]
    heading: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    vector: Option<usize>,
}

/* ---------- 对外数据结构 ---------- */

/// 列表用：不带正文，避免一次把上千条正文全传过去
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeEntry {
    pub id: String,
    pub title: String,
    pub tags: Vec<String>,
    /// true = 预置条目（只读）；false = 用户自建
    pub preset: bool,
    /// 更新时刻（unix 秒）；预置条目为 0
    pub updated_at: i64,
}

/// 详情/编辑用：带正文
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeDoc {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub preset: bool,
    #[serde(default)]
    pub updated_at: i64,
}

/// 新建/更新的入参：id 为空表示新建
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeDraft {
    #[serde(default)]
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub body: String,
}

/// 检索命中：够模型判断要不要展开，又不至于把上下文塞满
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeHit {
    pub id: String,
    pub title: String,
    pub tags: Vec<String>,
    /// 命中的那一小块的小标题（预置条目才有）
    pub heading: String,
    /// ≤120 字的摘要
    pub snippet: String,
    pub score: f32,
    pub preset: bool,
}

/// 向量检索的运行时配置（由前端在启动/保存设置时推一次）
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

/* ---------- 分词 ---------- */

fn is_cjk(ch: char) -> bool {
    matches!(ch as u32, 0x3400..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
}

/// 中文按相邻两字成组（二元组），英文/数字按连续字母数字成词。
///
/// 为什么不引 jieba：中文分词器要么带词典（几百 KB~几 MB），要么带 C 依赖，
/// 而检索只需要「能把相关段落捞出来」。二元组召回率足够，且不存在未登录词问题。
fn tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cjk: Vec<char> = Vec::new();
    let mut word = String::new();

    fn flush_cjk(buf: &mut Vec<char>, out: &mut Vec<String>) {
        if buf.len() == 1 {
            out.push(buf[0].to_string());
        } else {
            for pair in buf.windows(2) {
                out.push(pair.iter().collect::<String>());
            }
        }
        buf.clear();
    }
    fn flush_word(buf: &mut String, out: &mut Vec<String>) {
        if !buf.is_empty() {
            out.push(buf.to_lowercase());
            buf.clear();
        }
    }

    for ch in text.chars() {
        if is_cjk(ch) {
            flush_word(&mut word, &mut out);
            cjk.push(ch);
        } else if ch.is_alphanumeric() {
            flush_cjk(&mut cjk, &mut out);
            word.push(ch);
        } else {
            flush_cjk(&mut cjk, &mut out);
            flush_word(&mut word, &mut out);
        }
    }
    flush_cjk(&mut cjk, &mut out);
    flush_word(&mut word, &mut out);
    out
}

/* ---------- 索引 ---------- */

#[derive(Debug, Clone)]
struct Chunk {
    entry_id: String,
    title: String,
    tags: Vec<String>,
    heading: String,
    text: String,
    preset: bool,
    priority: u8,
    /// 在 vectors.bin 里的行号
    vector: Option<usize>,
    /// 词频（标题/标签的权重直接体现在重复计数里）
    tf: HashMap<String, u32>,
    len: u32,
}

#[derive(Debug, Clone, Default)]
struct EmbMeta {
    model: String,
    dim: usize,
    scale: f32,
    rows: Vec<Vec<i8>>,
}

#[derive(Debug, Default)]
struct Index {
    chunks: Vec<Chunk>,
    df: HashMap<String, u32>,
    avg_len: f32,
    embedding: Option<EmbMeta>,
}

impl Index {
    fn from_parts(entries: Vec<(KnowledgeDoc, Vec<ManifestChunk>)>, emb: Option<EmbMeta>) -> Self {
        let mut chunks = Vec::new();
        for (doc, raw) in entries {
            let priority = 3;
            if raw.is_empty() {
                // 用户条目：按段落切块
                for piece in split_chunks(&doc.body) {
                    chunks.push(make_chunk(&doc, priority, "", piece, None));
                }
            } else {
                for chunk in raw {
                    if chunk.text.trim().is_empty() {
                        continue;
                    }
                    let heading = chunk.heading.trim().to_string();
                    chunks.push(make_chunk(&doc, priority, &heading, chunk.text.trim().to_string(), chunk.vector));
                }
            }
        }

        // 词频与文档频率
        let mut df: HashMap<String, u32> = HashMap::new();
        let mut total_len = 0u32;
        for chunk in &mut chunks {
            let mut tf: HashMap<String, u32> = HashMap::new();
            // 权重用重复计数表达：标题 ×3、标签 ×2、正文 ×1
            for _ in 0..3 {
                for token in tokens(&chunk.title) {
                    *tf.entry(token).or_insert(0) += 1;
                }
            }
            for tag in &chunk.tags {
                for _ in 0..2 {
                    for token in tokens(tag) {
                        *tf.entry(token).or_insert(0) += 1;
                    }
                }
            }
            for token in tokens(&format!("{} {}", chunk.heading, chunk.text)) {
                *tf.entry(token).or_insert(0) += 1;
            }
            for token in tf.keys() {
                *df.entry(token.clone()).or_insert(0) += 1;
            }
            chunk.len = tf.values().sum();
            total_len += chunk.len;
            chunk.tf = tf;
        }

        let avg_len = if chunks.is_empty() {
            0.0
        } else {
            total_len as f32 / chunks.len() as f32
        };

        Self { chunks, df, avg_len, embedding: emb }
    }

    fn build(db: Option<&Path>) -> Self {
        let mut entries: Vec<(KnowledgeDoc, Vec<ManifestChunk>)> = Vec::new();

        // 预置
        if let Ok(manifest) = serde_json::from_str::<Manifest>(PRESET_MANIFEST) {
            for entry in manifest.entries {
                let doc = KnowledgeDoc {
                    id: entry.id,
                    title: entry.title,
                    tags: entry.tags,
                    body: String::new(),
                    preset: true,
                    updated_at: 0,
                };
                entries.push((doc, entry.chunks));
            }
        }

        let emb = parse_embedding();

        // 用户条目
        if let Some(path) = db {
            if let Ok(conn) = crate::db::open(path) {
                if let Ok(mut stmt) = conn.prepare(
                    "SELECT id, title, tags, body, updated_at FROM knowledge ORDER BY updated_at DESC",
                ) {
                    let rows = stmt.query_map([], |row| {
                        let tags: String = row.get(2)?;
                        Ok(KnowledgeDoc {
                            id: row.get(0)?,
                            title: row.get(1)?,
                            tags: serde_json::from_str(&tags).unwrap_or_default(),
                            body: row.get(3)?,
                            preset: false,
                            updated_at: row.get(4)?,
                        })
                    });
                    if let Ok(rows) = rows {
                        for doc in rows.flatten() {
                            entries.push((doc, Vec::new()));
                        }
                    }
                }
            }
        }

        Self::from_parts(entries, emb)
    }

    fn search(&self, query: &str, tags: Option<&[String]>, limit: usize) -> Vec<KnowledgeHit> {
        let query = query.trim();
        if query.is_empty() || self.chunks.is_empty() {
            return Vec::new();
        }
        let wanted: HashSet<String> = tags.unwrap_or(&[]).iter().map(|t| t.to_lowercase()).collect();

        // 词项打分
        let q_tokens = tokens(query);
        let mut lexical: Vec<(usize, f32)> = Vec::new();
        let n = self.chunks.len() as f32;
        for (i, chunk) in self.chunks.iter().enumerate() {
            if !wanted.is_empty() {
                let hit = chunk.tags.iter().any(|t| wanted.contains(&t.to_lowercase()));
                if !hit {
                    continue;
                }
            }
            let mut score = 0.0f32;
            for token in &q_tokens {
                let Some(&tf) = chunk.tf.get(token) else { continue };
                let df = *self.df.get(token).unwrap_or(&0) as f32;
                // BM25 的 idf（+1 防负）
                let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
                let dl = chunk.len.max(1) as f32;
                let denom = tf as f32 + 1.2 * (1.0 - 0.75 + 0.75 * dl / self.avg_len.max(1.0));
                score += idf * (tf as f32 * 2.2) / denom;
            }
            // 整句命中给一点加成：术语、编号这类原样出现时最可信
            if score > 0.0 && chunk.text.contains(query) {
                score += 2.0;
            }
            if score > 0.0 {
                lexical.push((i, score));
            }
        }
        lexical.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        lexical.truncate(20);

        // 向量打分（可选）
        let mut vectorical: Vec<(usize, f32)> = Vec::new();
        if let Some(emb) = &self.embedding {
            if !emb.rows.is_empty() {
                if let Some(query_vec) = query_vector(query, emb) {
                    for (i, chunk) in self.chunks.iter().enumerate() {
                        let Some(row) = chunk.vector else { continue };
                        let Some(vec) = emb.rows.get(row) else { continue };
                        let mut dot = 0.0f32;
                        for (a, b) in vec.iter().zip(query_vec.iter()) {
                            dot += *a as f32 * *b as f32;
                        }
                        // 两侧都按 scale 量化且单位化，点积除以 scale² 就是余弦
                        let denom = emb.scale * emb.scale;
                        vectorical.push((i, dot / denom.max(1.0)));
                    }
                    vectorical.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                    vectorical.truncate(20);
                }
            }
        }

        // RRF 融合
        let k = 60.0f32;
        let mut fused: HashMap<usize, f32> = HashMap::new();
        for (rank, (idx, _)) in lexical.iter().enumerate() {
            *fused.entry(*idx).or_insert(0.0) += 1.0 / (k + rank as f32 + 1.0);
        }
        for (rank, (idx, _)) in vectorical.iter().enumerate() {
            *fused.entry(*idx).or_insert(0.0) += 1.0 / (k + rank as f32 + 1.0);
        }
        let mut ranked: Vec<(usize, f32)> = fused.into_iter().collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked.truncate(limit);

        ranked
            .into_iter()
            .map(|(idx, score)| {
                let chunk = &self.chunks[idx];
                KnowledgeHit {
                    id: chunk.entry_id.clone(),
                    title: chunk.title.clone(),
                    tags: chunk.tags.clone(),
                    heading: chunk.heading.clone(),
                    snippet: snippet(&chunk.text, 120),
                    score,
                    preset: chunk.preset,
                }
            })
            .collect()
    }
}

fn make_chunk(
    doc: &KnowledgeDoc,
    priority: u8,
    heading: &str,
    text: String,
    vector: Option<usize>,
) -> Chunk {
    Chunk {
        entry_id: doc.id.clone(),
        title: doc.title.clone(),
        tags: doc.tags.clone(),
        heading: heading.to_string(),
        text,
        preset: doc.preset,
        priority,
        vector,
        tf: HashMap::new(),
        len: 0,
    }
}

/// 摘要：按字符截断而不是按字节，避免把汉字切成半个
fn snippet(text: &str, max: usize) -> String {
    let flat = text.replace('\n', " ");
    let trimmed = flat.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    let mut out: String = trimmed.chars().take(max).collect();
    out.push('…');
    out
}

/// 用户条目切块：先按空行分段，太长的段再按句子切（相邻留 1 句重叠）。
/// 与 `knowledge/README.md` 的「≤400 字、1 句重叠」保持同一口径。
fn split_chunks(body: &str) -> Vec<String> {
    const LIMIT: usize = 400;
    let mut out = Vec::new();
    for para in body.split("\n\n") {
        let para = para.trim();
        if para.is_empty() {
            continue;
        }
        if para.chars().count() <= LIMIT {
            out.push(para.to_string());
            continue;
        }
        let sentences: Vec<&str> = para
            .split_inclusive(['。', '！', '？', '\n', '.', '!', '?'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        let mut buf = String::new();
        let mut prev_tail = String::new();
        for sentence in sentences {
            if !buf.is_empty() && buf.chars().count() + sentence.chars().count() > LIMIT {
                out.push(buf.clone());
                // 1 句重叠：下一块从上一块最后一句开始
                buf = prev_tail.clone();
            }
            buf.push_str(sentence);
            prev_tail = sentence.to_string();
        }
        if !buf.trim().is_empty() {
            out.push(buf);
        }
    }
    if out.is_empty() {
        let trimmed = body.trim();
        if !trimmed.is_empty() {
            out.push(trimmed.to_string());
        }
    }
    out
}

/* ---------- 向量 ---------- */

/// 解析编译期嵌入的向量表；没有 embedding 段或数据不完整就当作没有。
fn parse_embedding() -> Option<EmbMeta> {
    let manifest: Manifest = serde_json::from_str(PRESET_MANIFEST).ok()?;
    let emb = manifest.embedding?;
    if emb.dim == 0 || PRESET_VECTORS.is_empty() {
        return None;
    }
    let rows: Vec<Vec<i8>> = PRESET_VECTORS
        .chunks_exact(emb.dim)
        .map(|row| row.iter().map(|b| *b as i8).collect())
        .collect();
    if rows.is_empty() {
        return None;
    }
    Some(EmbMeta { model: emb.model, dim: emb.dim, scale: emb.scale.max(1.0), rows })
}

static EMBED_CONF: OnceLock<RwLock<Option<EmbeddingConfig>>> = OnceLock::new();

fn embed_conf() -> &'static RwLock<Option<EmbeddingConfig>> {
    EMBED_CONF.get_or_init(|| RwLock::new(None))
}

fn current_embedding() -> Option<EmbeddingConfig> {
    embed_conf().read().ok().and_then(|guard| guard.clone())
}

/// 算查询向量：模型名必须与索引时一致，否则量纲不同、分数没意义 —— 直接放弃这条腿。
fn query_vector(query: &str, emb: &EmbMeta) -> Option<Vec<i8>> {
    let conf = current_embedding()?;
    if conf.model.trim().is_empty() || conf.model != emb.model {
        return None;
    }
    // 查询太长既贵又没意义：取前 240 字符
    let text: String = query.chars().take(240).collect();
    let float = embed_blocking(&conf, &text, emb.dim).ok()?;
    Some(float.iter().map(|x| (x * emb.scale).round().clamp(-127.0, 127.0) as i8).collect())
}

/// OpenAI 兼容的 embeddings 调用（同步版）。
///
/// 工具表的 run 是同步 fn，工具里没法 await —— 所以这里单独用 blocking client，
/// 而不是把整个工具层改成 async（那要动 40 个工具的签名）。
pub fn embed_blocking(config: &EmbeddingConfig, text: &str, dim: usize) -> Result<Vec<f32>, String> {
    let base = config.base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err("没有配置嵌入接口地址".into());
    }
    let url = if base.ends_with("/embeddings") {
        base.to_string()
    } else {
        format!("{base}/embeddings")
    };
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| format!("创建请求失败：{error}"))?;
    let response = client
        .post(&url)
        .bearer_auth(config.api_key.trim())
        .json(&json!({ "model": config.model, "input": text }))
        .send()
        .map_err(|error| format!("嵌入请求失败：{error}"))?;
    let status = response.status().as_u16();
    let body = response.text().map_err(|error| format!("读取响应失败：{error}"))?;
    if !(200..300).contains(&status) {
        return Err(format!(
            "嵌入接口返回 {status}：{}",
            body.chars().take(200).collect::<String>()
        ));
    }
    let parsed: Value =
        serde_json::from_str(&body).map_err(|error| format!("响应不是 JSON：{error}"))?;
    let vector = parsed
        .get("data")
        .and_then(Value::as_array)
        .and_then(|list| list.first())
        .and_then(|item| item.get("embedding"))
        .and_then(Value::as_array)
        .ok_or_else(|| "响应里没有 embedding 字段".to_string())?;
    let mut out: Vec<f32> = vector.iter().filter_map(Value::as_f64).map(|x| x as f32).collect();
    if out.len() != dim {
        return Err(format!("向量维度不一致：接口 {} 维，索引 {dim} 维", out.len()));
    }
    // L2 归一化：与索引侧同一套口径
    let norm = out.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-6 {
        for value in &mut out {
            *value /= norm;
        }
    }
    Ok(out)
}

/* ---------- 缓存 ---------- */

static CACHE: OnceLock<RwLock<Option<Arc<Index>>>> = OnceLock::new();

fn cache() -> &'static RwLock<Option<Arc<Index>>> {
    CACHE.get_or_init(|| RwLock::new(None))
}

/// 写操作之后必须调用：索引里有用户条目的副本
fn invalidate() {
    if let Ok(mut guard) = cache().write() {
        *guard = None;
    }
}

fn index_for(db: Option<&Path>) -> Arc<Index> {
    if let Ok(guard) = cache().read() {
        if let Some(hit) = guard.as_ref() {
            return hit.clone();
        }
    }
    let built = Arc::new(Index::build(db));
    if let Ok(mut guard) = cache().write() {
        *guard = Some(built.clone());
    }
    built
}

/* ---------- 命令 ---------- */

fn tags_json(tags: &[String]) -> String {
    serde_json::to_string(tags).unwrap_or_else(|_| "[]".into())
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn is_preset_id(id: &str) -> bool {
    serde_json::from_str::<Manifest>(PRESET_MANIFEST)
        .map(|manifest| manifest.entries.iter().any(|entry| entry.id == id))
        .unwrap_or(false)
}

/// 列表：预置 + 用户自建，预置在前
#[tauri::command]
pub fn knowledge_list(state: State<'_, Shared>) -> Result<Vec<KnowledgeEntry>, String> {
    let mut out: Vec<KnowledgeEntry> = Vec::new();
    if let Ok(manifest) = serde_json::from_str::<Manifest>(PRESET_MANIFEST) {
        for entry in manifest.entries {
            out.push(KnowledgeEntry {
                id: entry.id,
                title: entry.title,
                tags: entry.tags,
                preset: true,
                updated_at: 0,
            });
        }
    }
    state.with(|conn| {
        let mut stmt = conn
            .prepare("SELECT id, title, tags, updated_at FROM knowledge ORDER BY updated_at DESC")
            .map_err(|error| format!("读取知识库失败：{error}"))?;
        let rows = stmt
            .query_map([], |row| {
                let tags: String = row.get(2)?;
                Ok(KnowledgeEntry {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    tags: serde_json::from_str(&tags).unwrap_or_default(),
                    preset: false,
                    updated_at: row.get(3)?,
                })
            })
            .map_err(|error| format!("读取知识库失败：{error}"))?;
        for row in rows.flatten() {
            out.push(row);
        }
        Ok(())
    })?;
    Ok(out)
}

/// 读一条完整正文（预置条目由 chunk 拼回）
#[tauri::command]
pub fn knowledge_get(state: State<'_, Shared>, id: String) -> Result<Option<KnowledgeDoc>, String> {
    if let Ok(manifest) = serde_json::from_str::<Manifest>(PRESET_MANIFEST) {
        if let Some(entry) = manifest.entries.into_iter().find(|item| item.id == id) {
            let body = entry
                .chunks
                .iter()
                .map(|chunk| {
                    let head = if chunk.heading.trim().is_empty() {
                        String::new()
                    } else {
                        format!("## {}\n\n", chunk.heading.trim())
                    };
                    format!("{head}{}", chunk.text.trim())
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            return Ok(Some(KnowledgeDoc {
                id: entry.id,
                title: entry.title,
                tags: entry.tags,
                body,
                preset: true,
                updated_at: 0,
            }));
        }
    }

    state.with(|conn| {
        let mut stmt = conn
            .prepare("SELECT id, title, tags, body, updated_at FROM knowledge WHERE id = ?1")
            .map_err(|error| format!("读取知识库失败：{error}"))?;
        let mut rows = stmt
            .query_map(params![id], |row| {
                let tags: String = row.get(2)?;
                Ok(KnowledgeDoc {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    tags: serde_json::from_str(&tags).unwrap_or_default(),
                    body: row.get(3)?,
                    preset: false,
                    updated_at: row.get(4)?,
                })
            })
            .map_err(|error| format!("读取知识库失败：{error}"))?;
        Ok(rows.next().and_then(Result::ok))
    })
}

/// 新建或更新用户条目（预置条目一律拒绝）
#[tauri::command]
pub fn knowledge_upsert(
    state: State<'_, Shared>,
    draft: KnowledgeDraft,
) -> Result<KnowledgeDoc, String> {
    let title = draft.title.trim().to_string();
    if title.is_empty() {
        return Err("标题不能为空".into());
    }
    let body = draft.body.trim().to_string();
    if body.is_empty() {
        return Err("正文不能为空 —— 空的条目检索不到，也占地方".into());
    }
    if is_preset_id(draft.id.trim()) {
        return Err("预置条目是只读的 —— 想改就「复制为我的条目」".into());
    }

    let id = if draft.id.trim().is_empty() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        format!("k-user-{stamp}")
    } else {
        draft.id.trim().to_string()
    };

    let updated_at = now_secs();
    let tags = tags_json(&draft.tags);
    state.with(|conn| {
        conn.execute(
            "INSERT INTO knowledge (id, title, tags, body, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET title = excluded.title, tags = excluded.tags,
                                           body = excluded.body, updated_at = excluded.updated_at",
            params![id, title, tags, body, updated_at],
        )
        .map_err(|error| format!("保存知识条目失败：{error}"))?;
        Ok(())
    })?;
    invalidate();
    Ok(KnowledgeDoc { id, title, tags: draft.tags, body, preset: false, updated_at })
}

/// 删除用户条目（预置条目一律拒绝）
#[tauri::command]
pub fn knowledge_delete(state: State<'_, Shared>, id: String) -> Result<(), String> {
    if is_preset_id(&id) {
        return Err("预置条目是只读的，删不掉".into());
    }
    state.with(|conn| {
        conn.execute("DELETE FROM knowledge WHERE id = ?1", params![id])
            .map_err(|error| format!("删除知识条目失败：{error}"))?;
        Ok(())
    })?;
    invalidate();
    Ok(())
}

/// 面板里的预览检索：和模型用的同一套排序
#[tauri::command]
pub fn knowledge_search(
    state: State<'_, Shared>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<KnowledgeHit>, String> {
    let path = state.path().to_path_buf();
    let index = index_for(Some(&path));
    Ok(index.search(&query, None, limit.unwrap_or(8).clamp(1, 20)))
}

/// 前端把「嵌入模型」设置推进来（没有就传 null）：向量通道的启用开关
#[tauri::command]
pub fn knowledge_configure(embedding: Option<EmbeddingConfig>) {
    if let Ok(mut guard) = embed_conf().write() {
        *guard = embedding.filter(|conf| !conf.model.trim().is_empty());
    }
}

/* ---------- 给工具层用 ---------- */

fn db_path(ctx: &crate::tools::ToolCtx) -> Option<PathBuf> {
    ctx.root.map(|root| root.join("app.db"))
}

/// tools::search_knowledge 的实现
pub fn search_tool(args: &Value, ctx: &crate::tools::ToolCtx) -> Result<String, String> {
    let query = args.get("query").and_then(Value::as_str).unwrap_or("").trim();
    if query.is_empty() {
        return Err("query 不能为空".into());
    }
    let tags: Option<Vec<String>> = args.get("tags").and_then(Value::as_array).map(|list| {
        list.iter()
            .filter_map(Value::as_str)
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect()
    });
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(5)
        .clamp(1, 10);

    let path = db_path(ctx);
    let index = index_for(path.as_deref());
    let hits = index.search(query, tags.as_deref(), limit);
    if hits.is_empty() {
        return Ok(json!({
            "hits": [],
            "note": "知识库里没有命中。写事实前如果这里没有依据，就照实说不知道，不要编。"
        })
        .to_string());
    }
    Ok(json!({
        "hits": hits,
        "note": "需要细节就用 read_knowledge 读全文；引用时按 title 说明出处。"
    })
    .to_string())
}

/// tools::read_knowledge 的实现
pub fn read_tool(args: &Value, ctx: &crate::tools::ToolCtx) -> Result<String, String> {
    let id = args.get("id").and_then(Value::as_str).unwrap_or("").trim();
    if id.is_empty() {
        return Err("id 不能为空".into());
    }

    // 预置条目直接从编译期清单拼
    if let Ok(manifest) = serde_json::from_str::<Manifest>(PRESET_MANIFEST) {
        if let Some(entry) = manifest.entries.into_iter().find(|item| item.id == id) {
            let body = entry
                .chunks
                .iter()
                .map(|chunk| {
                    let head = if chunk.heading.trim().is_empty() {
                        String::new()
                    } else {
                        format!("## {}\n\n", chunk.heading.trim())
                    };
                    format!("{head}{}", chunk.text.trim())
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            return Ok(json!({
                "id": entry.id,
                "title": entry.title,
                "tags": entry.tags,
                "preset": true,
                "body": body
            })
            .to_string());
        }
    }

    let Some(path) = db_path(ctx) else {
        return Err("当前环境读不到数据目录".into());
    };
    let conn = crate::db::open(&path)?;
    let mut stmt = conn
        .prepare("SELECT id, title, tags, body, updated_at FROM knowledge WHERE id = ?1")
        .map_err(|error| format!("读取知识库失败：{error}"))?;
    let mut rows = stmt
        .query_map(params![id], |row| {
            let tags: String = row.get(2)?;
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "title": row.get::<_, String>(1)?,
                "tags": serde_json::from_str::<Vec<String>>(&tags).unwrap_or_default(),
                "preset": false,
                "updatedAt": row.get::<_, i64>(4)?,
                "body": row.get::<_, String>(3)?,
            }))
        })
        .map_err(|error| format!("读取知识库失败：{error}"))?;
    match rows.next() {
        Some(Ok(value)) => Ok(value.to_string()),
        _ => Err(format!("知识库里没有 id = {id} 的条目")),
    }
}

/* ---------- 测试 ---------- */

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(id: &str, title: &str, body: &str) -> KnowledgeDoc {
        KnowledgeDoc {
            id: id.into(),
            title: title.into(),
            tags: vec![],
            body: body.into(),
            preset: false,
            updated_at: 0,
        }
    }

    #[test]
    fn tokenizer_splits_cjk_into_bigrams_and_latin_words() {
        let list = tokens("QPS 从 800 提到 3000");
        assert!(list.contains(&"qps".to_string()));
        assert!(list.contains(&"800".to_string()));
        assert!(list.contains(&"提到".to_string()));
    }

    #[test]
    fn bm25_prefers_the_matching_entry() {
        let index = Index::from_parts(
            vec![
                (doc("a", "电商中台", "QPS 从 800 提升到 3000，用 Redis 顶住峰值"), vec![]),
                (doc("b", "排版规则", "行高与段间距由预设决定"), vec![]),
            ],
            None,
        );
        let hits = index.search("QPS", None, 5);
        assert_eq!(hits.first().map(|h| h.id.as_str()), Some("a"));
    }

    #[test]
    fn empty_index_is_harmless() {
        let index = Index::from_parts(vec![], None);
        assert!(index.search("任何词", None, 5).is_empty());
    }

    #[test]
    fn long_body_is_chunked_within_limit() {
        let body = "句子。".repeat(500);
        let chunks = split_chunks(&body);
        assert!(chunks.len() > 1);
        for chunk in chunks {
            assert!(chunk.chars().count() <= 420, "块太长：{}", chunk.chars().count());
        }
    }
}
