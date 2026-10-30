//! 构建期：先验知识库清单，再交给 tauri。
//!
//! 为什么校验必须在这里：`knowledge.rs` 用的是 `include_str!` + `unwrap_or_default()` ——
//! 清单写坏（少个逗号、chunk 超长、id 撞车）时运行时只会得到一个**空知识库**，
//! 界面上「预置求职知识」照样在、条目数写着 0，看不出是坏了还是本来就没有。
//! 契约（`knowledge/README.md` §5.1）写的就是「构建期校验，dev 警告、release 失败」，
//! 之前没人实现。
//!
//! 空清单是合法的（还没开始收集资料）：缺文件或 entries 为空 → 只警告。

use std::path::PathBuf;

fn main() {
    // OCR 资源已经改为编译期 include_bytes! 嵌进 exe（运行时释放），
    // 这里不再往产物旁拷 DLL / tessdata —— 也就不需要 CARGO_FEATURE_OCR 分支。
    check_knowledge();
    tauri_build::build();
}

/// 正文上限（汉字）。英文按 2 字符 ≈ 1 汉字折算，与 `knowledge-check.mjs` 同口径。
const MAX_TEXT: usize = 400;
/// summary 上限（汉字）：写结论，不写「本文介绍了……」
const MAX_SUMMARY: usize = 60;

fn check_knowledge() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../knowledge/manifest.json");
    // 清单变了要重跑：否则改完清单直接 cargo build 会「没事可做」，用的还是旧库
    println!("cargo:rerun-if-changed={}", path.display());

    let release = std::env::var("PROFILE").as_deref() == Ok("release");
    let fail = |message: String| {
        if release {
            // 带着坏库发版比构建失败糟得多：用户装上一个搜不到任何东西的版本
            panic!("知识库清单不合法：{message}");
        }
        println!("cargo:warning=知识库清单不合法（dev 档按空库继续）：{message}");
    };

    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            println!("cargo:warning=没有 knowledge/manifest.json（{error}），这次按「知识库为空」构建");
            return;
        }
    };
    let manifest: serde_json::Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(error) => return fail(format!("不是合法 JSON：{error}")),
    };

    if manifest["version"].as_u64() != Some(1) {
        return fail(format!("version 必须是 1，实际是 {}", manifest["version"]));
    }
    let Some(entries) = manifest["entries"].as_array() else {
        return fail("缺少 entries 数组".into());
    };
    if entries.is_empty() {
        println!("cargo:warning=知识库清单里没有条目，这次按「知识库为空」构建");
        return;
    }

    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut chunks_total = 0usize;
    for (index, entry) in entries.iter().enumerate() {
        let Some(id) = entry["id"].as_str().filter(|id| !id.trim().is_empty()) else {
            return fail(format!("第 {} 条 entry 没有 id", index + 1));
        };
        if !seen.insert(id) {
            return fail(format!("entry id 重复：{id}"));
        }
        if entry["title"].as_str().unwrap_or("").trim().is_empty() {
            return fail(format!("{id} 没有 title（检索时权重 ×3，空着等于白给）"));
        }
        let Some(chunks) = entry["chunks"].as_array() else {
            return fail(format!("{id} 缺少 chunks"));
        };
        if chunks.is_empty() {
            return fail(format!("{id} 的 chunks 是空的"));
        }
        for (position, chunk) in chunks.iter().enumerate() {
            chunks_total += 1;
            let chunk_id = chunk["id"].as_str().unwrap_or("");
            let expected = format!("{id}#{}", position + 1);
            if chunk_id != expected {
                return fail(format!("chunk id 应为「{expected}」，实际是「{chunk_id}」"));
            }
            for field in ["heading", "summary", "text"] {
                if chunk[field].as_str().unwrap_or("").trim().is_empty() {
                    return fail(format!("{chunk_id} 缺少 {field}"));
                }
            }
            let questions = chunk["questions"].as_array().map(Vec::len).unwrap_or(0);
            if questions != 3 {
                return fail(format!("{chunk_id} 的 questions 必须是 3 条，实际 {questions} 条"));
            }
            let text = chunk["text"].as_str().unwrap_or("");
            let size = weighted_len(text);
            if size > MAX_TEXT as f64 {
                return fail(format!("{chunk_id} 正文折合 {size:.0} 字，超过 {MAX_TEXT}"));
            }
            let summary = chunk["summary"].as_str().unwrap_or("");
            let brief = weighted_len(summary);
            if brief > MAX_SUMMARY as f64 {
                return fail(format!("{chunk_id} 的 summary 折合 {brief:.0} 字，超过 {MAX_SUMMARY}"));
            }
        }
    }
    println!(
        "cargo:warning=知识库：{} 条 entry / {chunks_total} 个块，已编进二进制",
        seen.len()
    );
}

/// 与 `scripts/knowledge-check.mjs` **逐字同口径**：汉字（U+3400–U+9FFF、U+F900–U+FAFF）算 1，
/// 其余每个字符（英文、数字、标点、空格）算 0.5。
///
/// 为什么不自己发明一套：两处口径不一致时，`node scripts/knowledge-check.mjs` 通过、构建却失败，
/// 作者只会以为构建脚本坏了。谁改口径，两边一起改。
fn weighted_len(text: &str) -> f64 {
    let mut total = 0.0;
    for ch in text.chars() {
        let code = ch as u32;
        let cjk = (0x3400..=0x9fff).contains(&code) || (0xf900..=0xfaff).contains(&code);
        total += if cjk { 1.0 } else { 0.5 };
    }
    total
}
