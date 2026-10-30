//! 知识库：三层（库 → 条目 → chunk）+ 可选向量化。
//!
//! 为什么是三层：库是主题边界（删库=整块不要），条目是人写人读的单位，
//! chunk 才是检索单位。混成一层时，「删掉这个主题」要一条条勾。
//!
//! 为什么向量要用户手动点：算向量要花钱、要等。导入时自动算，等于
//! 让「我只是想先存着」也付一次调用费。所以默认只分片，向量化是显式动作。
//!
//! 一致性：每个库只认一个嵌入模型（存在 `knowledge_library.embedding_model`）。
//! 运行时配置的模型与它不一致 → 向量通道整段失效、退回词频（hash）检索，
//! 而不是拿两套模型的向量算余弦（那只剩看似合理、实则无意义的分数）。
//!
//! 边界：库与条目用户可增删改；模型只有两个只读工具，没有写入口。

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::State;

use crate::db::Shared;

const PRESET_MANIFEST: &str = include_str!("../../knowledge/manifest.json");
/// 预置条目所在的虚拟库 id：表里没有这一行
const PRESET_LIBRARY: &str = "lib-preset";
const PRESET_LIBRARY_NAME: &str = "预置求职知识";

/* ---------- 预置清单 ---------- */

#[derive(Debug, Deserialize)]
struct Manifest {
    #[serde(default)]
    entries: Vec<ManifestEntry>,
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
    /// 这一片讲什么（≤60 字）。参与词项打分，也直接回给模型，省一次 read_knowledge
    #[serde(default)]
    summary: String,
    /// 别人可能怎么问。同样 ×2 加权，让口语化提问也能命中
    #[serde(default)]
    questions: Vec<String>,
}

fn preset_entries() -> Vec<ManifestEntry> {
    serde_json::from_str::<Manifest>(PRESET_MANIFEST)
        .map(|manifest| manifest.entries)
        .unwrap_or_default()
}

/* ---------- 对外结构 ---------- */

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub id: String,
    pub name: String,
    pub note: String,
    pub builtin: bool,
    pub entries: usize,
    pub updated_at: i64,
    /// 向量化时用的嵌入模型（空 = 没向量化过）
    pub embedding_model: String,
    pub embedding_dim: usize,
    /// 已带向量的块数
    pub vectorized: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntrySummary {
    pub id: String,
    pub library_id: String,
    pub title: String,
    pub tags: Vec<String>,
    pub chunks: usize,
    /// 每块都有摘要（图片块除外）才算 enriched
    pub enriched: bool,
    pub updated_at: i64,
    pub preset: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkDoc {
    pub id: String,
    pub ord: usize,
    #[serde(default)]
    pub heading: String,
    pub text: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub questions: Vec<String>,
    /// text / image
    #[serde(default = "kind_text")]
    pub kind: String,
    /// 图片块：材料池存储名（`asset:<stored>`）
    #[serde(default)]
    pub asset: String,
    #[serde(default)]
    pub asset_name: String,
    /// 图片摘要来源：model / ocr / 空
    #[serde(default)]
    pub annotation: String,
    /// 只给索引与向量化用，不进 JSON
    #[serde(skip)]
    pub vector: Option<Vec<i8>>,
    #[serde(skip)]
    pub vector_model: String,
}

fn kind_text() -> String {
    "text".into()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryDoc {
    pub id: String,
    pub library_id: String,
    pub title: String,
    pub tags: Vec<String>,
    pub body: String,
    pub preset: bool,
    pub updated_at: i64,
    pub chunks: Vec<ChunkDoc>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryDraft {
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryDraft {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub library_id: String,
    pub title: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub body: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportTextDraft {
    pub library_id: String,
    pub title: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportImageDraft {
    pub library_id: String,
    #[serde(default)]
    pub title: String,
    pub asset: String,
    #[serde(default)]
    pub asset_name: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub annotation: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeHit {
    pub chunk_id: String,
    pub entry_id: String,
    pub library_id: String,
    pub library: String,
    pub title: String,
    pub tags: Vec<String>,
    pub heading: String,
    /// 这一片讲了什么（模型写过就用它，否则截正文 120 字）
    pub summary: String,
    /// 别人可能怎么问：给模型提示「这一片能答哪些问题」
    pub questions: Vec<String>,
    pub snippet: String,
    /// 图片块：`asset:<stored>`
    pub image: Option<String>,
    pub score: f32,
    pub preset: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Annotated {
    pub summary: String,
    pub annotation: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ingested {
    pub title: String,
    pub text: String,
    pub images: Vec<IngestedImage>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestedImage {
    pub name: String,
    pub ext: String,
    pub bytes: Vec<u8>,
}

/* ---------- 分词 ---------- */

fn is_cjk(ch: char) -> bool {
    matches!(ch as u32, 0x3400..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
}

/// 中文按相邻两字成组（二元组），英文/数字按连续字母数字成词。
/// 不引 jieba：检索只要「能把相关段落捞出来」，二元组召回率够且没有未登录词。
pub(crate) fn tokens(text: &str) -> Vec<String> {
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

/* ---------- 切块 ---------- */

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

/// 机械切块：按空行分段，超长段按句子切（相邻留 1 句重叠）。
/// 手写条目走这条；导入文档先由模型切，模型不可用才退到这里。
pub(crate) fn split_chunks(body: &str) -> Vec<String> {
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

/// 首行是 Markdown 标题时抽出来当 heading
pub(crate) fn split_heading(piece: &str) -> (String, String) {
    let mut lines = piece.lines();
    if let Some(first) = lines.next() {
        let trimmed = first.trim();
        if trimmed.starts_with('#') {
            let heading = trimmed.trim_start_matches('#').trim().to_string();
            let rest = lines.collect::<Vec<_>>().join("\n").trim().to_string();
            if !rest.is_empty() {
                return (heading, rest);
            }
            return (heading, trimmed.to_string());
        }
    }
    (String::new(), piece.trim().to_string())
}

/// 把正文里指向这些图片的 `![名](asset:x)` 剔掉，其余字符原样保留。
///
/// 为什么不用正则、也不逐行重建：换行就是段落边界（`split_chunks` 按空行切），
/// 逐行拼回去会把段落结构压平；这里只做「整体删除匹配片段」，空出来的空行会被切块逻辑当空白丢掉。
fn strip_asset_refs(body: &str, assets: &HashSet<String>) -> String {
    if assets.is_empty() {
        return body.to_string();
    }
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    loop {
        let Some(bang) = rest.find("![") else {
            out.push_str(rest);
            return out;
        };
        let (head, tail) = rest.split_at(bang);
        out.push_str(head);
        // tail 必然以 "![" 开头，后面跟 alt 和 ](asset:...)
        let matched = tail.find("](asset:").and_then(|open| {
            let name_start = open + "](asset:".len();
            let name_end = tail[name_start..].find(')')? + name_start;
            if !assets.contains(&tail[name_start..name_end]) {
                return None;
            }
            Some(name_end + 1)
        });
        match matched {
            Some(next) => rest = &tail[next..],
            None => {
                // 不是我们认识的图片引用：原样写回 "![", 从下一个字符继续找
                out.push_str("![");
                rest = &tail[2..];
            }
        }
    }
}

/* ---------- 向量 ---------- */

static EMBED_CONF: OnceLock<RwLock<Option<EmbeddingConfig>>> = OnceLock::new();

fn embed_conf() -> &'static RwLock<Option<EmbeddingConfig>> {
    EMBED_CONF.get_or_init(|| RwLock::new(None))
}

fn current_embedding() -> Option<EmbeddingConfig> {
    embed_conf().read().ok().and_then(|guard| guard.clone())
}

/// L2 归一化 + int8 量化：存 int8，一千条也就几 MB
fn quantize(row: &[f32]) -> Vec<i8> {
    row.iter()
        .map(|value| (value * 127.0).round().clamp(-127.0, 127.0) as i8)
        .collect()
}

fn embedding_url(base: &str) -> Result<String, String> {
    let base = base.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err("没有配置嵌入接口地址".into());
    }
    Ok(if base.ends_with("/embeddings") {
        base.to_string()
    } else {
        format!("{base}/embeddings")
    })
}

/// OpenAI 兼容 embeddings（同步版）。
///
/// 工具表的 run 是同步 fn，工具里没法 await —— 所以单独用 blocking client，
/// 而不是把整个工具层改成 async（那要动 40 个工具的签名）。
pub fn embed_blocking(config: &EmbeddingConfig, text: &str, dim: usize) -> Result<Vec<f32>, String> {
    let url = embedding_url(&config.base_url)?;
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
        serde_json::from_str(&body).map_err(|error| format!("嵌入响应不是 JSON：{error}"))?;
    let row = parsed
        .get("data")
        .and_then(Value::as_array)
        .and_then(|list| list.first())
        .and_then(|first| first.get("embedding"))
        .and_then(Value::as_array)
        .ok_or_else(|| "嵌入响应里没有 data[0].embedding".to_string())?;
    let floats: Vec<f32> = row
        .iter()
        .filter_map(Value::as_f64)
        .map(|value| value as f32)
        .collect();
    if floats.len() != dim {
        return Err(format!("嵌入维度对不上：期望 {dim}，拿到 {}", floats.len()));
    }
    Ok(floats)
}

/// 批量嵌入：向量化一个库时按 16 条一批，比逐条快一个量级
fn embed_batch_blocking(config: &EmbeddingConfig, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
    let url = embedding_url(&config.base_url)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|error| format!("创建请求失败：{error}"))?;
    let response = client
        .post(&url)
        .bearer_auth(config.api_key.trim())
        .json(&json!({ "model": config.model, "input": texts }))
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
        serde_json::from_str(&body).map_err(|error| format!("嵌入响应不是 JSON：{error}"))?;
    let list = parsed
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| "嵌入响应里没有 data".to_string())?;
    let mut out: Vec<Vec<f32>> = vec![Vec::new(); texts.len()];
    for item in list {
        let index = item.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
        let row = item
            .get("embedding")
            .and_then(Value::as_array)
            .ok_or_else(|| "嵌入响应里没有 embedding".to_string())?;
        if let Some(slot) = out.get_mut(index) {
            *slot = row.iter().filter_map(Value::as_f64).map(|v| v as f32).collect();
        }
    }
    if out.iter().any(Vec::is_empty) {
        return Err("嵌入接口返回的条数比请求少".into());
    }
    Ok(out)
}

/// 查询向量：维度必须与库里存的完全一致
fn query_vector(conf: &EmbeddingConfig, query: &str, dim: usize) -> Option<Vec<i8>> {
    let text: String = query.chars().take(240).collect();
    let float = embed_blocking(conf, &text, dim).ok()?;
    Some(quantize(&float))
}

fn vectorize_all(conf: &EmbeddingConfig, texts: &[String]) -> Result<Vec<Vec<i8>>, String> {
    let mut out: Vec<Vec<i8>> = Vec::with_capacity(texts.len());
    let mut dim = 0usize;
    for batch in texts.chunks(16) {
        let rows = embed_batch_blocking(conf, batch)?;
        for mut row in rows {
            if dim == 0 {
                dim = row.len();
            } else if row.len() != dim {
                return Err(format!("同一个嵌入模型返回了两种维度：{dim} / {}", row.len()));
            }
            let norm = row.iter().map(|value| value * value).sum::<f32>().sqrt();
            if norm > 0.0 {
                for value in row.iter_mut() {
                    *value /= norm;
                }
            }
            out.push(quantize(&row));
        }
    }
    Ok(out)
}

/* ---------- 索引 ---------- */

#[derive(Debug, Clone)]
struct Chunk {
    chunk_id: String,
    entry_id: String,
    library_id: String,
    library: String,
    title: String,
    tags: Vec<String>,
    heading: String,
    text: String,
    summary: String,
    questions: Vec<String>,
    preset: bool,
    asset: String,
    vector: Option<Vec<i8>>,
    vector_model: String,
    tf: HashMap<String, u32>,
    len: u32,
}

#[derive(Debug, Default)]
struct Index {
    chunks: Vec<Chunk>,
    df: HashMap<String, u32>,
    avg_len: f32,
}

impl Index {
    fn finish(mut chunks: Vec<Chunk>) -> Self {
        let mut df: HashMap<String, u32> = HashMap::new();
        let mut total_len = 0u32;
        for chunk in &mut chunks {
            let mut tf: HashMap<String, u32> = HashMap::new();
            // 权重用重复计数表达：标题 ×3、标签 ×2、小标题/摘要 ×2、正文 ×1。
            // 摘要本身是「同一件事的另一种说法」，给足权重才能让换说法命中。
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
            for extra in [&chunk.heading, &chunk.summary] {
                for _ in 0..2 {
                    for token in tokens(extra) {
                        *tf.entry(token).or_insert(0) += 1;
                    }
                }
            }
            // 预设问题是「别人可能怎么问」，和摘要同权：用户口语化的问法靠它命中
            for question in &chunk.questions {
                for _ in 0..2 {
                    for token in tokens(question) {
                        *tf.entry(token).or_insert(0) += 1;
                    }
                }
            }
            for token in tokens(&chunk.text) {
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
        Self { chunks, df, avg_len }
    }

    fn from_docs(docs: Vec<(EntryDoc, String)>) -> Self {
        let mut chunks = Vec::new();
        for (doc, library) in docs {
            for chunk in doc.chunks {
                if chunk.text.trim().is_empty() {
                    continue;
                }
                chunks.push(Chunk {
                    chunk_id: chunk.id,
                    entry_id: doc.id.clone(),
                    library_id: doc.library_id.clone(),
                    library: library.clone(),
                    title: doc.title.clone(),
                    tags: doc.tags.clone(),
                    heading: chunk.heading,
                    text: chunk.text,
                    summary: chunk.summary,
                    questions: chunk.questions,
                    preset: doc.preset,
                    asset: chunk.asset,
                    vector: chunk.vector,
                    vector_model: chunk.vector_model,
                    tf: HashMap::new(),
                    len: 0,
                });
            }
        }
        Self::finish(chunks)
    }

    fn build(db: Option<&Path>) -> Self {
        let mut all: Vec<Chunk> = Vec::new();
        for entry in preset_entries() {
            for (ord, chunk) in entry.chunks.into_iter().enumerate() {
                if chunk.text.trim().is_empty() {
                    continue;
                }
                all.push(Chunk {
                    chunk_id: format!("{}#{}", entry.id, ord + 1),
                    entry_id: entry.id.clone(),
                    library_id: PRESET_LIBRARY.into(),
                    library: PRESET_LIBRARY_NAME.into(),
                    title: entry.title.clone(),
                    tags: entry.tags.clone(),
                    heading: chunk.heading.trim().to_string(),
                    text: chunk.text.trim().to_string(),
                    summary: chunk.summary.trim().to_string(),
                    questions: chunk.questions,
                    preset: true,
                    asset: String::new(),
                    vector: None,
                    vector_model: String::new(),
                    tf: HashMap::new(),
                    len: 0,
                });
            }
        }
        if let Some(path) = db {
            if let Ok(conn) = crate::db::open(path) {
                if let Ok(list) = read_docs(&conn, None) {
                    for (doc, library) in list {
                        for chunk in doc.chunks {
                            if chunk.text.trim().is_empty() {
                                continue;
                            }
                            all.push(Chunk {
                                chunk_id: chunk.id,
                                entry_id: doc.id.clone(),
                                library_id: doc.library_id.clone(),
                                library: library.clone(),
                                title: doc.title.clone(),
                                tags: doc.tags.clone(),
                                heading: chunk.heading,
                                text: chunk.text,
                                summary: chunk.summary,
                                questions: chunk.questions,
                                preset: false,
                                asset: chunk.asset,
                                vector: chunk.vector,
                                vector_model: chunk.vector_model,
                                tf: HashMap::new(),
                                len: 0,
                            });
                        }
                    }
                }
            }
        }
        Self::finish(all)
    }

    fn search(
        &self,
        query: &str,
        tags: Option<&[String]>,
        library: Option<&str>,
        limit: usize,
    ) -> Vec<KnowledgeHit> {
        let query = query.trim();
        if query.is_empty() || self.chunks.is_empty() {
            return Vec::new();
        }
        let wanted: HashSet<String> = tags.unwrap_or(&[]).iter().map(|t| t.to_lowercase()).collect();
        let in_scope = |chunk: &Chunk| -> bool {
            if let Some(lib) = library {
                if !lib.is_empty() && chunk.library_id != lib {
                    return false;
                }
            }
            wanted.is_empty() || chunk.tags.iter().any(|t| wanted.contains(&t.to_lowercase()))
        };

        let q_tokens = tokens(query);
        let mut lexical: Vec<(usize, f32)> = Vec::new();
        let n = self.chunks.len() as f32;
        for (i, chunk) in self.chunks.iter().enumerate() {
            if !in_scope(chunk) {
                continue;
            }
            let mut score = 0.0f32;
            for token in &q_tokens {
                let Some(&tf) = chunk.tf.get(token) else { continue };
                let df = *self.df.get(token).unwrap_or(&0) as f32;
                let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
                let dl = chunk.len.max(1) as f32;
                let denom = tf as f32 + 1.2 * (1.0 - 0.75 + 0.75 * dl / self.avg_len.max(1.0));
                score += idf * (tf as f32 * 2.2) / denom;
            }
            if score > 0.0 && chunk.text.contains(query) {
                score += 2.0;
            }
            if score > 0.0 {
                lexical.push((i, score));
            }
        }
        lexical.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        lexical.truncate(20);

        // 向量通道：只在「运行时配了嵌入模型」且「这块就是该模型算的」时启用。
        // 模型不一致 → 这条腿整段失效，只剩词项（用户说的 hash 搜索）。
        let mut vectorical: Vec<(usize, f32)> = Vec::new();
        let conf = current_embedding();
        let dim = conf.as_ref().and_then(|conf| {
            self.chunks
                .iter()
                .find(|chunk| chunk.vector_model == conf.model)
                .and_then(|chunk| chunk.vector.as_ref().map(Vec::len))
        });
        if let (Some(conf), Some(dim)) = (conf, dim) {
            if let Some(query_vec) = query_vector(&conf, query, dim) {
                for (i, chunk) in self.chunks.iter().enumerate() {
                    if !in_scope(chunk) || chunk.vector_model != conf.model {
                        continue;
                    }
                    let Some(vec) = chunk.vector.as_ref() else { continue };
                    if vec.len() != dim {
                        continue;
                    }
                    let dot: i32 = vec
                        .iter()
                        .zip(query_vec.iter())
                        .map(|(a, b)| *a as i32 * *b as i32)
                        .sum();
                    vectorical.push((i, dot as f32 / (127.0 * 127.0)));
                }
                vectorical.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                vectorical.truncate(20);
            }
        }

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
                // 摘要和 snippet 是同一句话：模型写过就用它，没写过就截正文
                let summary_line = if chunk.summary.trim().is_empty() {
                    snippet(&chunk.text, 120)
                } else {
                    chunk.summary.clone()
                };
                KnowledgeHit {
                    chunk_id: chunk.chunk_id.clone(),
                    entry_id: chunk.entry_id.clone(),
                    library_id: chunk.library_id.clone(),
                    library: chunk.library.clone(),
                    title: chunk.title.clone(),
                    tags: chunk.tags.clone(),
                    heading: chunk.heading.clone(),
                    summary: summary_line.clone(),
                    questions: chunk.questions.clone(),
                    snippet: summary_line,
                    image: if chunk.asset.is_empty() {
                        None
                    } else {
                        Some(format!("asset:{}", chunk.asset))
                    },
                    score,
                    preset: chunk.preset,
                }
            })
            .collect()
    }
}

/* ---------- 缓存 ---------- */

static CACHE: OnceLock<RwLock<Option<Arc<Index>>>> = OnceLock::new();

fn cache() -> &'static RwLock<Option<Arc<Index>>> {
    CACHE.get_or_init(|| RwLock::new(None))
}

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

/* ---------- 存储读写 ---------- */

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn tags_json(tags: &[String]) -> String {
    serde_json::to_string(tags).unwrap_or_else(|_| "[]".into())
}

fn read_libraries(conn: &rusqlite::Connection) -> Result<Vec<Library>, String> {
    let mut out: Vec<Library> = Vec::new();
    let preset = preset_entries();
    if !preset.is_empty() {
        out.push(Library {
            id: PRESET_LIBRARY.into(),
            name: PRESET_LIBRARY_NAME.into(),
            note: "随程序分发，只读".into(),
            builtin: true,
            entries: preset.len(),
            updated_at: 0,
            embedding_model: String::new(),
            embedding_dim: 0,
            vectorized: 0,
        });
    }
    let mut stmt = conn
        .prepare(
            "SELECT l.id, l.name, l.note, l.updated_at,
                    (SELECT COUNT(*) FROM knowledge_entry e WHERE e.library_id = l.id),
                    l.embedding_model, l.embedding_dim,
                    (SELECT COUNT(*) FROM knowledge_chunk c
                       JOIN knowledge_entry e2 ON e2.id = c.entry_id
                      WHERE e2.library_id = l.id AND c.vector IS NOT NULL)
             FROM knowledge_library l ORDER BY l.ord, l.name",
        )
        .map_err(|error| format!("读取知识库失败：{error}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(Library {
                id: row.get(0)?,
                name: row.get(1)?,
                note: row.get(2)?,
                builtin: false,
                entries: row.get::<_, i64>(4)? as usize,
                updated_at: row.get(3)?,
                embedding_model: row.get(5)?,
                embedding_dim: row.get::<_, i64>(6)? as usize,
                vectorized: row.get::<_, i64>(7)? as usize,
            })
        })
        .map_err(|error| format!("读取知识库失败：{error}"))?;
    for row in rows.flatten() {
        out.push(row);
    }
    Ok(out)
}

fn read_chunks(conn: &rusqlite::Connection, entry_id: &str) -> Result<Vec<ChunkDoc>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, ord, heading, text, summary, questions, kind, asset, asset_name,
                    annotation, vector, vector_model
             FROM knowledge_chunk WHERE entry_id = ?1 ORDER BY ord",
        )
        .map_err(|error| format!("读取切片失败：{error}"))?;
    let rows = stmt
        .query_map(params![entry_id], |row| {
            let questions: String = row.get(5)?;
            let blob: Option<Vec<u8>> = row.get(10)?;
            Ok(ChunkDoc {
                id: row.get(0)?,
                ord: row.get::<_, i64>(1)? as usize,
                heading: row.get(2)?,
                text: row.get(3)?,
                summary: row.get(4)?,
                questions: serde_json::from_str(&questions).unwrap_or_default(),
                kind: row.get(6)?,
                asset: row.get(7)?,
                asset_name: row.get(8)?,
                annotation: row.get(9)?,
                vector: blob.map(|bytes| bytes.iter().map(|b| *b as i8).collect()),
                vector_model: row.get(11)?,
            })
        })
        .map_err(|error| format!("读取切片失败：{error}"))?;
    Ok(rows.flatten().collect())
}

fn read_entry(conn: &rusqlite::Connection, id: &str) -> Result<Option<EntryDoc>, String> {
    let mut stmt = conn
        .prepare("SELECT id, library_id, title, tags, body, updated_at FROM knowledge_entry WHERE id = ?1")
        .map_err(|error| format!("读取知识条目失败：{error}"))?;
    let mut rows = stmt
        .query_map(params![id], |row| {
            let tags: String = row.get(3)?;
            Ok(EntryDoc {
                id: row.get(0)?,
                library_id: row.get(1)?,
                title: row.get(2)?,
                tags: serde_json::from_str(&tags).unwrap_or_default(),
                body: row.get(4)?,
                preset: false,
                updated_at: row.get(5)?,
                chunks: Vec::new(),
            })
        })
        .map_err(|error| format!("读取知识条目失败：{error}"))?;
    let Some(Ok(mut doc)) = rows.next() else {
        return Ok(None);
    };
    doc.chunks = read_chunks(conn, &doc.id)?;
    Ok(Some(doc))
}

fn read_docs(
    conn: &rusqlite::Connection,
    library_id: Option<&str>,
) -> Result<Vec<(EntryDoc, String)>, String> {
    let base = "SELECT e.id, e.library_id, e.title, e.tags, e.body, e.updated_at, l.name
                FROM knowledge_entry e LEFT JOIN knowledge_library l ON l.id = e.library_id";
    let mut docs: Vec<(EntryDoc, String)> = Vec::new();
    if let Some(lib) = library_id {
        let mut stmt = conn
            .prepare(&format!("{base} WHERE e.library_id = ?1 ORDER BY e.ord, e.updated_at DESC"))
            .map_err(|error| format!("读取知识条目失败：{error}"))?;
        let rows = stmt
            .query_map(params![lib], map_doc)
            .map_err(|error| format!("读取知识条目失败：{error}"))?;
        docs.extend(rows.flatten());
    } else {
        let mut stmt = conn
            .prepare(&format!("{base} ORDER BY e.ord, e.updated_at DESC"))
            .map_err(|error| format!("读取知识条目失败：{error}"))?;
        let rows = stmt
            .query_map([], map_doc)
            .map_err(|error| format!("读取知识条目失败：{error}"))?;
        docs.extend(rows.flatten());
    }
    for (doc, _) in docs.iter_mut() {
        doc.chunks = read_chunks(conn, &doc.id)?;
    }
    Ok(docs)
}

fn map_doc(row: &rusqlite::Row<'_>) -> rusqlite::Result<(EntryDoc, String)> {
    let tags: String = row.get(3)?;
    let library: Option<String> = row.get(6)?;
    Ok((
        EntryDoc {
            id: row.get(0)?,
            library_id: row.get(1)?,
            title: row.get(2)?,
            tags: serde_json::from_str(&tags).unwrap_or_default(),
            body: row.get(4)?,
            preset: false,
            updated_at: row.get(5)?,
            chunks: Vec::new(),
        },
        library.unwrap_or_else(|| "未分类".into()),
    ))
}

fn write_chunks(
    conn: &rusqlite::Connection,
    entry_id: &str,
    chunks: &[(String, String, String, Vec<String>)],
    stamp: i64,
) -> Result<(), String> {
    for (index, (heading, text, summary, questions)) in chunks.iter().enumerate() {
        conn.execute(
            "INSERT INTO knowledge_chunk
                 (id, entry_id, ord, heading, text, summary, questions, updated_at, kind)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'text')",
            params![
                format!("{entry_id}#{}", index + 1),
                entry_id,
                index as i64 + 1,
                heading,
                text,
                summary,
                serde_json::to_string(questions).unwrap_or_else(|_| "[]".into()),
                stamp
            ],
        )
        .map_err(|error| format!("写入切片失败：{error}"))?;
    }
    Ok(())
}

/* ---------- 命令：库 ---------- */

#[tauri::command]
pub fn knowledge_libraries(state: State<'_, Shared>) -> Result<Vec<Library>, String> {
    state.with(|conn| read_libraries(conn))
}

#[tauri::command]
pub fn knowledge_library_save(state: State<'_, Shared>, draft: LibraryDraft) -> Result<Library, String> {
    let name = draft.name.trim().to_string();
    if name.is_empty() {
        return Err("库名不能为空".into());
    }
    if draft.id == PRESET_LIBRARY {
        return Err("预置库是只读的".into());
    }
    let id = if draft.id.trim().is_empty() {
        format!("lib-{}", now_ms())
    } else {
        draft.id.trim().to_string()
    };
    let note = draft.note.trim().to_string();
    let stamp = now_secs();
    state.with(|conn| {
        conn.execute(
            "INSERT INTO knowledge_library (id, name, note, ord, updated_at)
             VALUES (?1, ?2, ?3,
                     (SELECT COALESCE(MAX(ord), -1) + 1 FROM knowledge_library), ?4)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, note = excluded.note,
                                           updated_at = excluded.updated_at",
            params![id, name, note, stamp],
        )
        .map_err(|error| format!("保存知识库失败：{error}"))?;
        Ok(())
    })?;
    invalidate();
    state.with(|conn| {
        read_libraries(conn)?
            .into_iter()
            .find(|item| item.id == id)
            .ok_or_else(|| "库没保存成功".to_string())
    })
}

#[tauri::command]
pub fn knowledge_library_delete(state: State<'_, Shared>, id: String) -> Result<(), String> {
    if id == PRESET_LIBRARY {
        return Err("预置库是只读的".into());
    }
    state.with(|conn| {
        conn.execute(
            "DELETE FROM knowledge_chunk WHERE entry_id IN
                 (SELECT id FROM knowledge_entry WHERE library_id = ?1)",
            params![id],
        )
        .map_err(|error| format!("删除切片失败：{error}"))?;
        conn.execute("DELETE FROM knowledge_entry WHERE library_id = ?1", params![id])
            .map_err(|error| format!("删除条目失败：{error}"))?;
        conn.execute("DELETE FROM knowledge_library WHERE id = ?1", params![id])
            .map_err(|error| format!("删除知识库失败：{error}"))?;
        Ok(())
    })?;
    invalidate();
    Ok(())
}

/* ---------- 命令：条目 ---------- */

#[tauri::command]
pub fn knowledge_entries(state: State<'_, Shared>, library_id: String) -> Result<Vec<EntrySummary>, String> {
    if library_id == PRESET_LIBRARY {
        return Ok(preset_entries()
            .into_iter()
            .map(|entry| EntrySummary {
                id: entry.id,
                library_id: PRESET_LIBRARY.into(),
                title: entry.title,
                tags: entry.tags,
                chunks: entry.chunks.len(),
                enriched: true,
                updated_at: 0,
                preset: true,
            })
            .collect());
    }
    state.with(|conn| {
        let docs = read_docs(conn, Some(&library_id))?;
        Ok(docs
            .into_iter()
            .map(|(doc, _)| {
                let chunks = doc.chunks.len();
                let enriched = !doc.chunks.is_empty()
                    && doc
                        .chunks
                        .iter()
                        .all(|chunk| chunk.kind == "image" || !chunk.summary.trim().is_empty());
                EntrySummary {
                    id: doc.id,
                    library_id: doc.library_id,
                    title: doc.title,
                    tags: doc.tags,
                    chunks,
                    enriched,
                    updated_at: doc.updated_at,
                    preset: false,
                }
            })
            .collect())
    })
}

/// 库下的块（平铺）。
///
/// 条目只是「一次投料」的批次容器：界面上不再把它当一层 —— 用户看到的就是
/// 一块块可以改、可以删的知识，跟最初粘进来的那段文本是不是同一条没有关系。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkItem {
    pub id: String,
    /// 所属条目。界面只做两层（库 → chunk），但删除要按归属校验，
    /// 所以这里带出来 —— 否则前端每次删块都得先反查条目。
    pub entry_id: String,
    pub ord: u32,
    pub heading: String,
    pub text: String,
    pub summary: String,
    pub questions: Vec<String>,
    pub kind: String,
    pub asset: String,
    pub asset_name: String,
    pub annotation: String,
}

impl ChunkItem {
    fn new(entry_id: &str, chunk: &ChunkDoc) -> Self {
        ChunkItem {
            id: chunk.id.clone(),
            entry_id: entry_id.to_string(),
            ord: chunk.ord as u32,
            heading: chunk.heading.clone(),
            text: chunk.text.clone(),
            summary: chunk.summary.clone(),
            questions: chunk.questions.clone(),
            kind: chunk.kind.clone(),
            asset: chunk.asset.clone(),
            asset_name: chunk.asset_name.clone(),
            annotation: chunk.annotation.clone(),
        }
    }
}

#[tauri::command]
pub fn knowledge_chunks(state: State<'_, Shared>, library_id: String) -> Result<Vec<ChunkItem>, String> {
    if library_id == PRESET_LIBRARY {
        let mut out = Vec::new();
        for entry in preset_entries() {
            for (index, chunk) in entry.chunks.iter().enumerate() {
                out.push(ChunkItem {
                    id: format!("{}#{}", entry.id, index + 1),
                    entry_id: entry.id.clone(),
                    ord: index as u32 + 1,
                    heading: chunk.heading.clone(),
                    text: chunk.text.clone(),
                    summary: chunk.summary.clone(),
                    questions: chunk.questions.clone(),
                    kind: "text".into(),
                    asset: String::new(),
                    asset_name: String::new(),
                    annotation: String::new(),
                });
            }
        }
        return Ok(out);
    }
    let docs = state.with(|conn| read_docs(conn, Some(&library_id)))?;
    Ok(docs
        .iter()
        .flat_map(|(doc, _)| doc.chunks.iter().map(|chunk| ChunkItem::new(&doc.id, chunk)))
        .collect())
}

/// 改一块的正文。
///
/// 摘要与向量是照着旧正文算的，正文一改它们就是错的 —— 一并清掉，
/// 让「生成摘要」与「向量化」变成用户显式的下一步，而不是拿旧结论骗检索。
#[tauri::command]
pub fn knowledge_chunk_save(
    state: State<'_, Shared>,
    chunk_id: String,
    text: String,
) -> Result<Vec<ChunkItem>, String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("这一块不能是空的 —— 空块既检索不到，也占地方".into());
    }
    let (entry_id, kind, _) = state
        .with(|conn| locate_chunk(conn, &chunk_id))?
        .ok_or_else(|| "这块已经不在了".to_string())?;
    if kind == "image" {
        return Err("图片块改的是说明，不是正文".into());
    }
    let (heading, body) = split_heading(&text);
    state.with(|conn| {
        conn.execute(
            "UPDATE knowledge_chunk
                SET heading = ?1, text = ?2, summary = '', questions = '[]',
                    updated_at = ?3, vector = NULL, vector_model = ''
              WHERE id = ?4",
            params![heading, body, now_secs(), chunk_id],
        )
        .map_err(|error| format!("写入切片失败：{error}"))?;
        Ok(())
    })?;
    invalidate();
    let library_id: String = state.with(|conn| {
        conn.query_row(
            "SELECT library_id FROM knowledge_entry WHERE id = ?1",
            params![entry_id],
            |row| row.get(0),
        )
        .map_err(|error| format!("读取知识条目失败：{error}"))
    })?;
    knowledge_chunks(state, library_id)
}

#[tauri::command]
pub fn knowledge_entry_get(state: State<'_, Shared>, id: String) -> Result<Option<EntryDoc>, String> {
    if let Some(entry) = preset_entries().into_iter().find(|item| item.id == id) {
        let chunks = entry
            .chunks
            .iter()
            .enumerate()
            .map(|(index, chunk)| ChunkDoc {
                id: format!("{}#{}", entry.id, index + 1),
                ord: index + 1,
                heading: chunk.heading.trim().to_string(),
                text: chunk.text.trim().to_string(),
                summary: chunk.summary.trim().to_string(),
                questions: chunk.questions.clone(),
                kind: "text".into(),
                asset: String::new(),
                asset_name: String::new(),
                annotation: String::new(),
                vector: None,
                vector_model: String::new(),
            })
            .collect();
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
        return Ok(Some(EntryDoc {
            id: entry.id,
            library_id: PRESET_LIBRARY.into(),
            title: entry.title,
            tags: entry.tags,
            body,
            preset: true,
            updated_at: 0,
            chunks,
        }));
    }
    state.with(|conn| read_entry(conn, &id))
}

/// 保存条目：切块 + 落库；文本没变的块保留原摘要
#[tauri::command]
pub fn knowledge_entry_save(state: State<'_, Shared>, draft: EntryDraft) -> Result<EntryDoc, String> {
    let title = draft.title.trim().to_string();
    if title.is_empty() {
        return Err("标题不能为空".into());
    }
    let body = draft.body.trim().to_string();
    if body.is_empty() {
        return Err("正文不能为空 —— 空的条目检索不到，也占地方".into());
    }
    if draft.library_id.trim().is_empty() || draft.library_id == PRESET_LIBRARY {
        return Err("先选一个自己的库（预置库只读）".into());
    }
    let entry_id = if draft.id.trim().is_empty() {
        format!("k-{}", now_ms())
    } else {
        draft.id.trim().to_string()
    };
    if preset_entries().iter().any(|item| item.id == entry_id) {
        return Err("预置条目是只读的".into());
    }

    state.with(|conn| {
        // 老片怎么继承：文字块按正文字面继承摘要（改一个字就重算，这是有意的）。
        // 图片块不参与切分 —— 它没有「正文」可切，按正文重切只会把图连标注一起弄丢。
        let mut keep: HashMap<String, (String, Vec<String>)> = HashMap::new();
        let mut images: Vec<ChunkDoc> = Vec::new();
        for chunk in read_chunks(conn, &entry_id)? {
            if chunk.kind == "image" {
                images.push(chunk);
            } else {
                keep.insert(chunk.text.trim().to_string(), (chunk.summary, chunk.questions));
            }
        }
        // 正文里 `![名](asset:x)` 那一行没了，就等于用户把这张图删了
        let alive: HashSet<String> = images
            .iter()
            .map(|chunk| chunk.asset.clone())
            .filter(|asset| !asset.is_empty() && body.contains(&format!("asset:{asset}")))
            .collect();
        images.retain(|chunk| alive.contains(&chunk.asset));
        images.sort_by_key(|chunk| chunk.ord);
        // 存回去的仍是原文（图片那行要留着，下次编辑才看得见图）；
        // 只在切文字块时把它剔掉，否则图片语法会混进正文片段被当成文字检索。
        let pieces = split_chunks(&strip_asset_refs(&body, &alive));
        if pieces.is_empty() && images.is_empty() {
            return Err("正文没有可切块的内容".into());
        }
        let stamp = now_secs();
        conn.execute(
            "INSERT INTO knowledge_entry (id, library_id, title, tags, body, ord, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5,
                     (SELECT COALESCE(MAX(ord), -1) + 1 FROM knowledge_entry WHERE library_id = ?2),
                     ?6)
             ON CONFLICT(id) DO UPDATE SET library_id = excluded.library_id,
                                           title = excluded.title, tags = excluded.tags,
                                           body = excluded.body, updated_at = excluded.updated_at",
            params![entry_id, draft.library_id, title, tags_json(&draft.tags), body, stamp],
        )
        .map_err(|error| format!("保存知识条目失败：{error}"))?;
        conn.execute("DELETE FROM knowledge_chunk WHERE entry_id = ?1", params![entry_id])
            .map_err(|error| format!("重置切片失败：{error}"))?;
        let mut ord = 0i64;
        for piece in pieces.iter() {
            let (heading, text) = split_heading(piece);
            let (summary, questions) = keep
                .get(text.trim())
                .cloned()
                .unwrap_or_else(|| (String::new(), Vec::new()));
            ord += 1;
            conn.execute(
                "INSERT INTO knowledge_chunk
                     (id, entry_id, ord, heading, text, summary, questions, updated_at, kind)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'text')",
                params![
                    format!("{entry_id}#{ord}"),
                    entry_id,
                    ord,
                    heading,
                    text,
                    summary,
                    serde_json::to_string(&questions).unwrap_or_else(|_| "[]".into()),
                    stamp
                ],
            )
            .map_err(|error| format!("写入切片失败：{error}"))?;
        }
        // 图片块排在文字块后面：剔掉图片行之后正文里没有可靠的位置信息，
        // 与其猜，不如固定放末尾 —— 排错位置比排在末尾更让人困惑。
        for image in images.iter() {
            ord += 1;
            conn.execute(
                "INSERT INTO knowledge_chunk
                     (id, entry_id, ord, heading, text, summary, questions, updated_at,
                      kind, asset, asset_name, annotation, vector, vector_model)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'image', ?9, ?10, ?11, ?12, ?13)",
                params![
                    format!("{entry_id}#{ord}"),
                    entry_id,
                    ord,
                    image.heading,
                    image.text,
                    image.summary,
                    serde_json::to_string(&image.questions).unwrap_or_else(|_| "[]".into()),
                    stamp,
                    image.asset,
                    image.asset_name,
                    image.annotation,
                    image.vector.as_ref().map(|row| row.iter().map(|b| *b as u8).collect::<Vec<u8>>()),
                    image.vector_model
                ],
            )
            .map_err(|error| format!("写入图片块失败：{error}"))?;
        }
        Ok(())
    })?;
    invalidate();
    state.with(|conn| read_entry(conn, &entry_id))?
        .ok_or_else(|| "条目没保存成功".to_string())
}

#[tauri::command]
pub fn knowledge_entry_delete(state: State<'_, Shared>, id: String) -> Result<(), String> {
    if preset_entries().iter().any(|item| item.id == id) {
        return Err("预置条目是只读的".into());
    }
    state.with(|conn| {
        conn.execute("DELETE FROM knowledge_chunk WHERE entry_id = ?1", params![id])
            .map_err(|error| format!("删除切片失败：{error}"))?;
        conn.execute("DELETE FROM knowledge_entry WHERE id = ?1", params![id])
            .map_err(|error| format!("删除条目失败：{error}"))?;
        Ok(())
    })?;
    invalidate();
    Ok(())
}

/* ---------- 命令：改块 / 删块 ---------- */

/// 按 id 找块，返回 (所属条目, kind, asset)：两个命令的「块不在」措辞必须一致
fn locate_chunk(
    conn: &rusqlite::Connection,
    chunk_id: &str,
) -> Result<Option<(String, String, String)>, String> {
    conn.query_row(
        "SELECT entry_id, kind, asset FROM knowledge_chunk WHERE id = ?1",
        params![chunk_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .optional()
    .map_err(|error| format!("读取切片失败：{error}"))
}

/// 重新给已有图片块写说明：模型/OCR 的结果由前端算好后传进来，这里只落库
#[tauri::command]
pub fn knowledge_image_update(
    state: State<'_, Shared>,
    chunk_id: String,
    summary: String,
    annotation: String,
) -> Result<EntryDoc, String> {
    let (entry_id, kind, _) = state
        .with(|conn| locate_chunk(conn, &chunk_id))?
        .ok_or_else(|| "这块已经不在了".to_string())?;
    if kind != "image" {
        return Err("只有图片块需要这样改说明".into());
    }
    state.with(|conn| {
        // 说明换了，旧向量就是错的：别让检索拿旧向量去比对新文字
        conn.execute(
            "UPDATE knowledge_chunk
                SET summary = ?1, annotation = ?2, updated_at = ?3,
                    vector = NULL, vector_model = ''
              WHERE id = ?4",
            params![summary.trim(), annotation.trim(), now_secs(), chunk_id],
        )
        .map_err(|error| format!("写入图片说明失败：{error}"))?;
        Ok(())
    })?;
    invalidate();
    state.with(|conn| read_entry(conn, &entry_id))?
        .ok_or_else(|| "条目读不回来了".to_string())
}

/// 删掉单块；图片块会顺手把条目正文里对应的 ![名](asset:x) 一起删掉。
///
/// 返回 `()`：界面是两层（库 → chunk），删完统一重读列表，
/// 把「删掉的块所属条目」再回传一遍只是噪声。块删光时空条目一起删。
#[tauri::command]
pub fn knowledge_chunk_delete(
    state: State<'_, Shared>,
    entry_id: String,
    chunk_id: String,
) -> Result<(), String> {
    state.with(|conn| {
        let (owner, kind, asset) = locate_chunk(conn, &chunk_id)?
            .ok_or_else(|| "这块已经不在了".to_string())?;
        // 归属不对就先拦下：只按 chunk_id 删会删进别的条目，正文也改到别人头上
        if owner != entry_id {
            return Err("这块不属于这条条目".into());
        }
        // 先改正文、后删块：万一后一步失败，剩下的是「正文那行没了、块还在」，
        // 下次编辑保存时 alive 判定会把它清掉；反过来则是「块没了、正文那行还在」，
        // 下次保存会把那行 markdown 当文字块捡回来，图就永久退化成一段文本。
        if kind == "image" && !asset.is_empty() {
            let body: String = conn
                .query_row(
                    "SELECT body FROM knowledge_entry WHERE id = ?1",
                    params![entry_id],
                    |row| row.get(0),
                )
                .map_err(|error| format!("读取知识条目失败：{error}"))?;
            let gone: HashSet<String> = [asset].into_iter().collect();
            conn.execute(
                "UPDATE knowledge_entry SET body = ?1, updated_at = ?2 WHERE id = ?3",
                params![strip_asset_refs(&body, &gone), now_secs(), entry_id],
            )
            .map_err(|error| format!("更新条目正文失败：{error}"))?;
        }
        let removed = conn
            .execute(
                "DELETE FROM knowledge_chunk WHERE id = ?1 AND entry_id = ?2",
                params![chunk_id, entry_id],
            )
            .map_err(|error| format!("删除切片失败：{error}"))?;
        // 0 行说明中途有人改过归属：别让「点了删除却提示成功」出现
        if removed == 0 {
            return Err("这块已经不在了".into());
        }
        Ok(())
    })?;
    invalidate();
    // 界面只有两层（库 → chunk），块删光之后那条条目就再也点不到了 ——
    // 留着只会让库上的「条目数」比看得见的多。所以这里级联删掉空条目。
    state.with(|conn| {
        let left: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM knowledge_chunk WHERE entry_id = ?1",
                params![entry_id],
                |row| row.get(0),
            )
            .map_err(|error| format!("读取切片失败：{error}"))?;
        if left == 0 {
            conn.execute("DELETE FROM knowledge_entry WHERE id = ?1", params![entry_id])
                .map_err(|error| format!("删除空条目失败：{error}"))?;
        }
        Ok(())
    })
}

/* ---------- 命令：摘要 ---------- */

/// 补摘要与关联问题的提示词（用户自建条目走这条）。
///
/// 与分片提示词同一套口径：summary 写结论不写过程，questions 写成用户的口语问法。
/// 这两样都参与词项打分（×2 权重），写得像标题就等于白写。
const ENRICH_PROMPT: &str = include_str!("../../prompts/enrich.md");

#[tauri::command]
pub async fn knowledge_entry_enrich(
    state: State<'_, Shared>,
    id: String,
    ai: AiConfig,
    force: Option<bool>,
) -> Result<EntryDoc, String> {
    if ai.model.trim().is_empty() || ai.base_url.trim().is_empty() {
        return Err("还没有配置对话模型：设置 → 模型，摘要要用它生成".into());
    }
    if preset_entries().iter().any(|item| item.id == id) {
        return Err("预置条目是只读的".into());
    }
    let doc = state
        .with(|conn| read_entry(conn, &id))?
        .ok_or_else(|| format!("找不到条目 {id}"))?;
    if doc.chunks.is_empty() {
        return Err("这一条还没切块".into());
    }
    let todo: Vec<&ChunkDoc> = doc
        .chunks
        .iter()
        .filter(|chunk| chunk.kind == "text")
        .filter(|chunk| force.unwrap_or(false) || chunk.summary.trim().is_empty())
        .collect();
    if todo.is_empty() {
        return Ok(doc);
    }

    let payload = todo
        .iter()
        .map(|chunk| {
            json!({
                "index": chunk.ord,
                "heading": chunk.heading,
                "text": chunk.text.chars().take(1200).collect::<String>(),
            })
        })
        .collect::<Vec<_>>();
    let request = crate::net::AiRequest {
        base_url: ai.base_url,
        api_key: ai.api_key,
        model: ai.model,
        messages: vec![
            crate::net::ChatMessage::system(ENRICH_PROMPT),
            crate::net::ChatMessage::user(format!(
                "文章标题：{}\n\n待处理的块：\n{}",
                doc.title,
                serde_json::to_string_pretty(&payload).unwrap_or_default()
            )),
        ],
        temperature: Some(0.2),
        reasoning: None,
        max_tokens: Some(2_048),
        tools: None,
    };
    let raw = crate::net::chat_once(request).await?;
    let parsed = parse_enrich(&raw)?;
    if parsed.is_empty() {
        return Err("模型没有返回可用的摘要".into());
    }

    let stamp = now_secs();
    state.with(|conn| {
        for (index, (summary, questions)) in &parsed {
            conn.execute(
                "UPDATE knowledge_chunk SET summary = ?1, questions = ?2, updated_at = ?3
                 WHERE entry_id = ?4 AND ord = ?5",
                params![
                    summary,
                    serde_json::to_string(questions).unwrap_or_else(|_| "[]".into()),
                    stamp,
                    id,
                    *index as i64
                ],
            )
            .map_err(|error| format!("写入摘要失败：{error}"))?;
        }
        Ok(())
    })?;
    invalidate();
    state.with(|conn| read_entry(conn, &id))?
        .ok_or_else(|| "条目读不回来了".to_string())
}

fn parse_enrich(raw: &str) -> Result<Vec<(usize, (String, Vec<String>))>, String> {
    let mut out = Vec::new();
    for item in parse_json_array(raw, "摘要")? {
        let Some(index) = item.get("index").and_then(Value::as_u64) else { continue };
        let summary = item
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if summary.is_empty() {
            continue;
        }
        let questions = item
            .get("questions")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(|text| text.trim().to_string())
                    .filter(|text| !text.is_empty())
                    .take(5)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        out.push((index as usize, (summary, questions)));
    }
    Ok(out)
}

/// 模型爱加解释或 ``` 围栏：先剥壳、直接解析、失败再「找最外层括号」
fn parse_json_array(raw: &str, label: &str) -> Result<Vec<Value>, String> {
    let trimmed = raw.trim();
    let body = trimmed
        .strip_prefix("```")
        .map(|rest| rest.split_once('\n').map(|(_, tail)| tail).unwrap_or(rest))
        .unwrap_or(trimmed)
        .trim_end()
        .strip_suffix("```")
        .unwrap_or(trimmed)
        .trim();
    let value = serde_json::from_str::<Value>(body).ok().or_else(|| {
        let start = body.find('[')?;
        let end = body.rfind(']')?;
        serde_json::from_str::<Value>(&body[start..=end]).ok()
    });
    value
        .and_then(|value| value.as_array().cloned())
        .ok_or_else(|| format!("看不懂模型返回的{label}：{}", snippet(raw, 120)))
}

/* ---------- 命令：向量化 ---------- */

/// 把一个库的文字块全部向量化。
///
/// 用户定的三条：只手动触发、每库单一模型、模型不一致就退回 hash 检索。
#[tauri::command]
pub async fn knowledge_vectorize(
    state: State<'_, Shared>,
    library_id: String,
    embedding: EmbeddingConfig,
) -> Result<Library, String> {
    if library_id == PRESET_LIBRARY {
        return Err("预置库是只读的，不能改写它的向量".into());
    }
    if embedding.model.trim().is_empty() || embedding.base_url.trim().is_empty() {
        return Err("先在 设置 → 模型 → 嵌入 里配一个嵌入模型".into());
    }
    let items: Vec<(String, String)> = state.with(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT c.id, c.text FROM knowledge_chunk c
                   JOIN knowledge_entry e ON e.id = c.entry_id
                  WHERE e.library_id = ?1 AND c.kind = 'text' ORDER BY c.ord",
            )
            .map_err(|error| format!("读取切片失败：{error}"))?;
        let rows = stmt
            .query_map(params![library_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| format!("读取切片失败：{error}"))?;
        Ok(rows.flatten().collect())
    })?;
    if items.is_empty() {
        return Err("这个库里还没有文字块 —— 先导入文档".into());
    }

    let conf = embedding.clone();
    let texts: Vec<String> = items.iter().map(|(_, text)| text.clone()).collect();
    // 嵌入是阻塞 IO：放到阻塞线程池，别占住 async 运行时
    let vectors = tauri::async_runtime::spawn_blocking(move || vectorize_all(&conf, &texts))
        .await
        .map_err(|error| format!("向量化中断：{error}"))??;
    let dim = vectors.first().map(Vec::len).unwrap_or(0);

    let stamp = now_secs();
    state.with(|conn| {
        for (index, (id, _)) in items.iter().enumerate() {
            let Some(row) = vectors.get(index) else { continue };
            let blob: Vec<u8> = row.iter().map(|value| *value as u8).collect();
            conn.execute(
                "UPDATE knowledge_chunk SET vector = ?1, vector_model = ?2, updated_at = ?3
                 WHERE id = ?4",
                params![blob, embedding.model, stamp, id],
            )
            .map_err(|error| format!("写入向量失败：{error}"))?;
        }
        conn.execute(
            "UPDATE knowledge_library SET embedding_model = ?1, embedding_dim = ?2, updated_at = ?3
             WHERE id = ?4",
            params![embedding.model, dim as i64, stamp, library_id],
        )
        .map_err(|error| format!("记录嵌入模型失败：{error}"))?;
        Ok(())
    })?;
    invalidate();
    state.with(|conn| {
        read_libraries(conn)?
            .into_iter()
            .find(|item| item.id == library_id)
            .ok_or_else(|| "库读不回来了".to_string())
    })
}

/* ---------- 命令：导入 ---------- */

/// 解析一个文件：PDF / MD / TXT 的文本，或图片原字节。
#[tauri::command]
pub fn ingest_file(name: String, bytes: Vec<u8>) -> Result<Ingested, String> {
    let ext = name
        .rsplit_once('.')
        .map(|(_, tail)| tail.to_ascii_lowercase())
        .unwrap_or_default();
    let title = name
        .rsplit_once('.')
        .map(|(head, _)| head.to_string())
        .unwrap_or_else(|| name.clone());

    if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "avif") {
        return Ok(Ingested {
            title,
            text: String::new(),
            images: vec![IngestedImage { name: name.clone(), ext, bytes }],
        });
    }

    let text = match ext.as_str() {
        "pdf" => pdf_extract::extract_text_from_mem(&bytes)
            .map_err(|error| format!("这份 PDF 解析失败：{error}"))?,
        "md" | "markdown" | "txt" | "text" => String::from_utf8_lossy(&bytes).to_string(),
        "docx" | "doc" => {
            return Err("Word 还不支持直接解析 —— 请另存为 PDF 或纯文本再导入".into())
        }
        _ => return Err(format!("不认识的文件类型 .{ext} —— 支持 PDF / MD / TXT / 图片")),
    };
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("这个文件里没有可提取的文字（可能是扫描件）".into());
    }
    Ok(Ingested { title, text, images: Vec::new() })
}

/// 导入文本：**用模型分片**，模型不可用时退回机械切块（导入不能因为模型挂了就失败）
#[tauri::command]
pub async fn knowledge_import_text(
    state: State<'_, Shared>,
    draft: ImportTextDraft,
    ai: AiConfig,
) -> Result<EntryDoc, String> {
    if draft.library_id.trim().is_empty() || draft.library_id == PRESET_LIBRARY {
        return Err("先选一个自己的库（预置库只读）".into());
    }
    let text = draft.text.trim().to_string();
    if text.is_empty() {
        return Err("没有可导入的正文".into());
    }
    // 标题不再是用户必须填的东西：没有就取正文第一行（批次容器的名字而已）
    let title = if draft.title.trim().is_empty() {
        text.lines()
            .next()
            .unwrap_or("未命名")
            .chars()
            .take(24)
            .collect::<String>()
            .trim()
            .to_string()
    } else {
        draft.title.trim().to_string()
    };

    let mut chunks: Vec<(String, String, String, Vec<String>)> = Vec::new();
    let mut model_error = String::new();
    for piece in split_for_model(&text) {
        match ask_chunks(&ai, &title, &piece).await {
            Ok(mut list) => chunks.append(&mut list),
            Err(error) => {
                if model_error.is_empty() {
                    model_error = error;
                }
                for part in split_chunks(&piece) {
                    let (heading, body) = split_heading(&part);
                    chunks.push((heading, body, String::new(), Vec::new()));
                }
            }
        }
    }
    if chunks.is_empty() {
        return Err(if model_error.is_empty() {
            "没有切出任何块".into()
        } else {
            model_error
        });
    }

    let entry_id = format!("k-{}", now_ms());
    let stamp = now_secs();
    state.with(|conn| {
        conn.execute(
            "INSERT INTO knowledge_entry (id, library_id, title, tags, body, ord, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5,
                     (SELECT COALESCE(MAX(ord), -1) + 1 FROM knowledge_entry WHERE library_id = ?2),
                     ?6)",
            params![entry_id, draft.library_id, title, tags_json(&draft.tags), text, stamp],
        )
        .map_err(|error| format!("保存知识条目失败：{error}"))?;
        write_chunks(conn, &entry_id, &chunks, stamp)?;
        Ok(())
    })?;
    invalidate();
    state.with(|conn| read_entry(conn, &entry_id))?
        .ok_or_else(|| "条目读不回来了".to_string())
}

/// 导入图片：一条目一块，保留原格式（材料池存的就是原字节）
#[tauri::command]
pub fn knowledge_import_image(
    state: State<'_, Shared>,
    draft: ImportImageDraft,
) -> Result<EntryDoc, String> {
    if draft.library_id.trim().is_empty() || draft.library_id == PRESET_LIBRARY {
        return Err("先选一个自己的库（预置库只读）".into());
    }
    if draft.asset.trim().is_empty() {
        return Err("图片还没存进材料池".into());
    }
    let name = if draft.asset_name.trim().is_empty() {
        "图片".to_string()
    } else {
        draft.asset_name.trim().to_string()
    };
    let title = if draft.title.trim().is_empty() {
        name.clone()
    } else {
        draft.title.trim().to_string()
    };
    let entry_id = format!("k-{}", now_ms());
    let stamp = now_secs();
    // 图片块的正文就是一句 Markdown：模型原样写进回答时，会话里能直接显示成图
    let markdown = format!("![{}](asset:{})", name, draft.asset);

    state.with(|conn| {
        conn.execute(
            "INSERT INTO knowledge_entry (id, library_id, title, tags, body, ord, updated_at)
             VALUES (?1, ?2, ?3, '[]', ?4,
                     (SELECT COALESCE(MAX(ord), -1) + 1 FROM knowledge_entry WHERE library_id = ?2),
                     ?5)",
            params![entry_id, draft.library_id, title, markdown, stamp],
        )
        .map_err(|error| format!("保存知识条目失败：{error}"))?;
        conn.execute(
            "INSERT INTO knowledge_chunk
                 (id, entry_id, ord, heading, text, summary, questions, updated_at,
                  kind, asset, asset_name, annotation)
             VALUES (?1, ?2, 1, '图片', ?3, ?4, '[]', ?5, 'image', ?6, ?7, ?8)",
            params![
                format!("{entry_id}#1"),
                entry_id,
                markdown,
                draft.summary.trim(),
                stamp,
                draft.asset,
                name,
                draft.annotation.trim(),
            ],
        )
        .map_err(|error| format!("保存图片块失败：{error}"))?;
        Ok(())
    })?;
    invalidate();
    state.with(|conn| read_entry(conn, &entry_id))?
        .ok_or_else(|| "条目读不回来了".to_string())
}

/// 给图片写一句话说明：多模态 → OCR → 都没有就留空
#[tauri::command]
pub async fn knowledge_image_annotate(
    bytes: Vec<u8>,
    ext: String,
    mode: String,
    ai: Option<AiConfig>,
    ocr: Option<AiConfig>,
) -> Result<Annotated, String> {
    if bytes.is_empty() {
        return Err("图片是空的".into());
    }
    match mode.as_str() {
        "model" => {
            let Some(ai) = ai else { return Err("没有配置多模态模型".into()) };
            let summary = describe_image(&bytes, &ext, &ai).await?;
            Ok(Annotated { summary, annotation: "model".into() })
        }
        "ocr" => {
            let Some(ocr) = ocr else { return Err("没有配置 OCR 模型".into()) };
            let text =
                crate::extract::ocr_api(bytes, ext, ocr.base_url, ocr.api_key, ocr.model).await?;
            Ok(Annotated { summary: snippet(&text, 120), annotation: "ocr".into() })
        }
        _ => Ok(Annotated { summary: String::new(), annotation: String::new() }),
    }
}

async fn describe_image(bytes: &[u8], ext: &str, ai: &AiConfig) -> Result<String, String> {
    let mime = match ext.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        _ => "image/jpeg",
    };
    use base64::Engine as _;
    let data_url = format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    );
    let content = json!([
        {
            "type": "text",
            "text": "用一句话说明这张图里有什么（不超过 60 字），只描述可见内容。"
        },
        { "type": "image_url", "image_url": { "url": data_url } }
    ]);
    let request = crate::net::AiRequest {
        base_url: ai.base_url.clone(),
        api_key: ai.api_key.clone(),
        model: ai.model.clone(),
        messages: vec![crate::net::ChatMessage {
            role: "user".into(),
            content: Some(content),
            tool_calls: None,
            tool_call_id: None,
        }],
        temperature: Some(0.1),
        reasoning: None,
        max_tokens: Some(300),
        tools: None,
    };
    let text = crate::net::chat_once(request).await?;
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("模型没有返回内容".into());
    }
    Ok(snippet(&text, 120))
}

/// 分片输入上限：一次两千字左右，既看得清结构又不撑爆上下文
fn split_for_model(text: &str) -> Vec<String> {
    const LIMIT: usize = 2_000;
    let mut out: Vec<String> = Vec::new();
    let mut buf = String::new();
    for para in text.split("\n\n") {
        let para = para.trim();
        if para.is_empty() {
            continue;
        }
        if para.chars().count() > LIMIT {
            for piece in split_chunks(para) {
                if !buf.is_empty() {
                    out.push(std::mem::take(&mut buf));
                }
                out.push(piece);
            }
            continue;
        }
        if !buf.is_empty() && buf.chars().count() + para.chars().count() > LIMIT {
            out.push(std::mem::take(&mut buf));
        }
        if !buf.is_empty() {
            buf.push_str("\n\n");
        }
        buf.push_str(para);
    }
    if !buf.trim().is_empty() {
        out.push(buf);
    }
    if out.is_empty() {
        out.push(text.to_string());
    }
    out
}

/// 分片模型的系统提示词。
///
/// 口径与 `knowledge/README.md` §3 是同一份：那边是给人看的工作流规范，这边是给模型看的执行说明。
/// **改一边就要改另一边** —— 两处不一致时，人按契约写、模型按提示词切，最后卡在字数与条数上。
const CHUNK_PROMPT: &str = include_str!("../../prompts/chunk.md");

async fn ask_chunks(
    ai: &AiConfig,
    title: &str,
    text: &str,
) -> Result<Vec<(String, String, String, Vec<String>)>, String> {
    if ai.model.trim().is_empty() || ai.base_url.trim().is_empty() {
        return Err("没有配置分片模型".into());
    }
    let request = crate::net::AiRequest {
        base_url: ai.base_url.clone(),
        api_key: ai.api_key.clone(),
        model: ai.model.clone(),
        messages: vec![
            crate::net::ChatMessage::system(CHUNK_PROMPT),
            crate::net::ChatMessage::user(format!("文档标题：{title}\n\n内容：\n{text}")),
        ],
        temperature: Some(0.2),
        reasoning: None,
        max_tokens: Some(4_096),
        tools: None,
    };
    let raw = crate::net::chat_once(request).await?;
    let mut out = Vec::new();
    for item in parse_json_array(&raw, "分片")? {
        let text = item.get("text").and_then(Value::as_str).unwrap_or("").trim().to_string();
        if text.is_empty() {
            continue;
        }
        let heading = item.get("heading").and_then(Value::as_str).unwrap_or("").trim().to_string();
        let summary = item.get("summary").and_then(Value::as_str).unwrap_or("").trim().to_string();
        let questions = item
            .get("questions")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(|text| text.trim().to_string())
                    .filter(|text| !text.is_empty())
                    .take(5)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        out.push((heading, text, summary, questions));
    }
    if out.is_empty() {
        return Err("模型没有切出可用的块".into());
    }
    Ok(out)
}

/* ---------- 命令：检索 ---------- */

#[tauri::command]
pub fn knowledge_search(
    state: State<'_, Shared>,
    query: String,
    library_id: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<KnowledgeHit>, String> {
    let path = state.path().to_path_buf();
    let index = index_for(Some(&path));
    Ok(index.search(
        &query,
        None,
        library_id.as_deref().filter(|id| !id.is_empty()),
        limit.unwrap_or(8).clamp(1, 20),
    ))
}

/// 前端把「嵌入模型」设置推进来（没有就传 null）：向量通道的开关
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
    let hits = index.search(query, tags.as_deref(), None, limit);
    if hits.is_empty() {
        return Ok(json!({
            "hits": [],
            "note": "知识库里没有命中。写事实前如果这里没有依据，就照实说不知道，不要编。"
        })
        .to_string());
    }
    Ok(json!({
        "hits": hits,
        "note": "需要细节就用 read_knowledge（传 chunkId 读一块、传 entryId 读整篇）；图片块可以把它给的 asset:… 原样写进回答里，界面会显示成图。"
    })
    .to_string())
}

pub fn read_tool(args: &Value, ctx: &crate::tools::ToolCtx) -> Result<String, String> {
    let id = args.get("id").and_then(Value::as_str).unwrap_or("").trim();
    if id.is_empty() {
        return Err("id 不能为空".into());
    }
    let (entry_id, want_ord) = match id.split_once('#') {
        Some((head, tail)) => (head, tail.parse::<usize>().ok()),
        None => (id, None),
    };

    if let Some(entry) = preset_entries().into_iter().find(|item| item.id == entry_id) {
        let chunks: Vec<Value> = entry
            .chunks
            .iter()
            .enumerate()
            .filter(|(index, _)| want_ord.map(|ord| ord == index + 1).unwrap_or(true))
            .map(|(index, chunk)| {
                json!({
                    "chunkId": format!("{}#{}", entry.id, index + 1),
                    "heading": chunk.heading.trim(),
                    "summary": chunk.summary.trim(),
                    "questions": chunk.questions,
                    "text": chunk.text.trim(),
                })
            })
            .collect();
        if chunks.is_empty() {
            return Err(format!("预置条目里没有 {id} 这一块"));
        }
        return Ok(json!({
            "entryId": entry.id,
            "title": entry.title,
            "tags": entry.tags,
            "library": PRESET_LIBRARY_NAME,
            "preset": true,
            "chunks": chunks,
        })
        .to_string());
    }

    let Some(path) = db_path(ctx) else {
        return Err("当前环境读不到数据目录".into());
    };
    let conn = crate::db::open(&path)?;
    let doc = read_entry(&conn, entry_id)?
        .ok_or_else(|| format!("知识库里没有 id = {entry_id} 的条目"))?;
    let library = conn
        .query_row(
            "SELECT name FROM knowledge_library WHERE id = ?1",
            params![doc.library_id],
            |row| row.get::<_, String>(0),
        )
        .unwrap_or_default();
    let chunks: Vec<Value> = doc
        .chunks
        .iter()
        .filter(|chunk| want_ord.map(|ord| ord == chunk.ord).unwrap_or(true))
        .map(|chunk| {
            json!({
                "chunkId": chunk.id,
                "heading": chunk.heading,
                "text": chunk.text,
                "summary": chunk.summary,
                "image": if chunk.asset.is_empty() { None } else { Some(format!("asset:{}", chunk.asset)) },
            })
        })
        .collect();
    if chunks.is_empty() {
        return Err(format!("条目 {entry_id} 里没有 {id} 这一块"));
    }
    Ok(json!({
        "entryId": doc.id,
        "title": doc.title,
        "tags": doc.tags,
        "library": library,
        "preset": false,
        "chunks": chunks,
    })
    .to_string())
}

/* ---------- 测试 ---------- */

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(id: &str, title: &str, body: &str) -> EntryDoc {
        let chunks = split_chunks(body)
            .into_iter()
            .enumerate()
            .map(|(index, piece)| {
                let (heading, text) = split_heading(&piece);
                ChunkDoc {
                    id: format!("{id}#{}", index + 1),
                    ord: index + 1,
                    heading,
                    text,
                    summary: String::new(),
                    questions: Vec::new(),
                    kind: "text".into(),
                    asset: String::new(),
                    asset_name: String::new(),
                    annotation: String::new(),
                    vector: None,
                    vector_model: String::new(),
                }
            })
            .collect();
        EntryDoc {
            id: id.into(),
            library_id: "lib-test".into(),
            title: title.into(),
            tags: vec![],
            body: body.into(),
            preset: false,
            updated_at: 0,
            chunks,
        }
    }

    #[test]
    fn tokenizer_splits_cjk_into_bigrams_and_latin_words() {
        let list = tokens("QPS 从 800 提到 3000");
        assert!(list.contains(&"qps".to_string()));
        assert!(list.contains(&"800".to_string()));
    }

    #[test]
    fn bm25_prefers_the_matching_entry() {
        let index = Index::from_docs(vec![
            (doc("a", "电商中台", "QPS 从 800 提升到 3000，用 Redis 顶住峰值"), "行业".into()),
            (doc("b", "排版规则", "行高与段间距由预设决定"), "行业".into()),
        ]);
        let hits = index.search("QPS", None, None, 5);
        assert_eq!(hits.first().map(|h| h.entry_id.as_str()), Some("a"));
    }

    /// 降级路径：**没配嵌入模型时退回词频（hash）检索**，不报错、照样命中。
    ///
    /// 这条是产品口径（`knowledge/README.md` §5.6 / §8）：向量是加分项，不是前提。
    /// 谁把 embedding 写成检索的必需依赖，这里会红。
    #[test]
    fn search_without_an_embedding_model_falls_back_to_hash() {
        assert!(current_embedding().is_none(), "测试环境不该配着嵌入模型");
        let index = Index::from_docs(vec![
            (doc("a", "面试潜台词", "面试官问项目难点，其实是在看你会不会拆解问题"), "面试".into()),
            (doc("b", "排版规则", "行高与段间距由预设决定"), "行业".into()),
        ]);
        let hits = index.search("项目难点怎么答", None, None, 5);
        assert_eq!(hits.first().map(|h| h.entry_id.as_str()), Some("a"), "词项通道应当命中");
    }

    #[test]
    fn empty_index_is_harmless() {
        let index = Index::from_docs(vec![]);
        assert!(index.search("任何词", None, None, 5).is_empty());
    }

    #[test]
    fn long_body_is_chunked_within_limit() {
        for chunk in split_chunks(&"句子。".repeat(500)) {
            assert!(chunk.chars().count() <= 420, "块太长：{}", chunk.chars().count());
        }
    }

    /// 图片块不该被当成文字切进正文：编辑条目时保存不能把图弄丢
    #[test]
    fn image_markdown_is_stripped_before_chunking() {
        let body = "门店照片\n\n![门店](asset:ab12.png)\n\n位于二楼";
        let assets: HashSet<String> = ["ab12.png".to_string()].into_iter().collect();
        let stripped = strip_asset_refs(body, &assets);
        assert!(!stripped.contains("asset:"));
        assert!(stripped.contains("门店照片") && stripped.contains("位于二楼"));
        // 段落边界不能压平：空行还在，切出来仍是两块
        assert_eq!(split_chunks(&stripped).len(), 2);
        // 不认识的图片（外链、别的库）原样留着
        let other = "![外链](https://a.com/x.png)";
        assert_eq!(strip_asset_refs(other, &assets), other);
    }

    /// 删单块只会给一个 asset：别的图片块必须原样留着
    #[test]
    fn deleting_one_chunk_keeps_other_image_refs() {
        let body = "![甲](asset:a.png)\n\n![乙](asset:b.png)";
        let gone: HashSet<String> = ["a.png".to_string()].into_iter().collect();
        let stripped = strip_asset_refs(body, &gone);
        assert!(!stripped.contains("asset:a.png"));
        assert!(stripped.contains("![乙](asset:b.png)"));
    }

    #[test]
    fn summary_is_searchable() {
        let mut entry = doc("c", "电商中台", "峰值时每秒三千次请求，缓存挡在前面");
        entry.chunks[0].summary = "并发能力 3000 QPS".into();
        let index = Index::from_docs(vec![(entry, "行业".into())]);
        // 正文里没有「并发」，只有摘要里有 —— 换说法也要命中
        let hits = index.search("并发", None, None, 5);
        assert_eq!(hits.first().map(|h| h.entry_id.as_str()), Some("c"));
    }

    #[test]
    fn library_filter_keeps_other_libraries_out() {
        let index = Index::from_docs(vec![(doc("a", "电商中台", "QPS 3000"), "行业".into())]);
        assert_eq!(index.search("QPS", None, Some("lib-test"), 5).len(), 1);
        assert!(index.search("QPS", None, Some("lib-other"), 5).is_empty());
    }

    #[test]
    fn enrich_reply_survives_fences_and_chatter() {
        let raw = "好的：\n```json\n[{\"index\":1,\"summary\":\"并发三千\",\"questions\":[\"能扛多少并发\"]}]\n```";
        let parsed = parse_enrich(raw).expect("应当能解析");
        assert_eq!(parsed[0].0, 1);
        assert_eq!(parsed[0].1 .0, "并发三千");
    }

    #[test]
    fn heading_is_extracted_from_markdown_line() {
        let (heading, text) = split_heading("## 项目背景\n我负责支付链路");
        assert_eq!(heading, "项目背景");
        assert_eq!(text, "我负责支付链路");
    }

    #[test]
    fn type_filter_wants_string_field() {
        assert_eq!(kind_text(), "text");
    }
}
