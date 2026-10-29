//! 资产池：用户原件的唯一存放处。
//!
//! 三条规则（用户定的）：
//!   1. **原样存放**：不转码、不重压缩、不改名。导出的就是当初进来的那些字节
//!   2. **内容寻址**：文件名 = sha256 前 16 位 + 扩展名。同一份文件拖两次只落一份
//!   3. **引用归零才回收**：删页删掉最后一次引用时，文件移进 `pool/.trash/<日期>/`
//!      而不是直接抹掉 —— 原件不可重建，留一段时间的退路
//!
//! 谁在引用文件由 `sync` 从工作区里现算，不做第二份引用记录：多一份状态就多一处
//! 会对不上的地方。asset 表里的 `refs` 只记「上一轮有没有被引用」，
//! 用来识别「从有到无」这个瞬间。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::db::{self, AssetRow, Shared};
use crate::model::Workspace;

/* ---------- sha256 ---------- */

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// 纯 Rust 的 sha256。只为「内容寻址」服务，不需要流式、不需要 HMAC。
pub fn sha256_hex(data: &[u8]) -> String {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let mut message = data.to_vec();
    let bit_len = (data.len() as u64) * 8;
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in message.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let (mut a, mut b, mut c, mut d) = (h[0], h[1], h[2], h[3]);
        let (mut e, mut f, mut g, mut hh) = (h[4], h[5], h[6], h[7]);

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    h.iter().map(|word| format!("{word:08x}")).collect()
}

/* ---------- 目录与取放 ---------- */

pub fn pool_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = crate::data_dir(app)?.join("pool");
    std::fs::create_dir_all(&dir).map_err(|error| format!("建 pool 目录失败：{error}"))?;
    Ok(dir)
}

/// 旧版本把材料放在 `assets/`；`read_at` 读的时候两个目录都找，写只进 `pool/`
/// 只取文件名部分：调用方给的是「池内存储名」，绝不允许带路径
pub(crate) fn safe_stored(stored: &str) -> Result<String, String> {
    let name = Path::new(stored)
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .ok_or_else(|| "材料名不合法".to_string())?;
    if name.is_empty() || name == "." || name == ".." {
        return Err("材料名不合法".into());
    }
    Ok(name)
}

pub fn mime_of(name: &str) -> String {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// 放进池子：算哈希 → 已存在就直接复用（去重）→ 否则落盘。
///
/// 返回 `{ stored, sha256, size, mime }`，前端拿它组装 Asset。
pub fn put(
    app: &tauri::AppHandle,
    state: &Shared,
    id: &str,
    name: &str,
    data: &[u8],
) -> Result<Value, String> {
    let sha = sha256_hex(data);
    let stored = crate::snapshot::pool_name(&sha, name.rsplit('.').next())?;
    let dir = pool_dir(app)?;
    let path = dir.join(&stored);

    // 同名同哈希 = 同一份内容，不重复写；同名不同哈希说明哈希前缀撞了（几乎不可能），
    // 那时用完整哈希兜底，绝不覆盖已有文件。
    let mut final_name = stored.clone();
    if path.exists() {
        let existing = std::fs::read(&path).map_err(|error| format!("读取池文件失败：{error}"))?;
        if sha256_hex(&existing) != sha {
            final_name = format!("{sha}.{}", stored.rsplit('.').next().unwrap_or("bin"));
        }
    }
    let path = dir.join(&final_name);
    if !path.exists() {
        crate::storage::atomic_write(&path, data)?;
    }

    let mime = mime_of(name);
    let row = AssetRow {
        id: id.to_string(),
        sha256: sha.clone(),
        stored: final_name.clone(),
        name: name.to_string(),
        mime: mime.clone(),
        bytes: data.len() as i64,
        added_at: crate::snapshot::now_ms() as i64,
        // 引用数由 `sync` 每轮按工作区现算；刚放进来时还不知道谁会用
        refs: 0,
    };
    state.with(|conn| db::asset_upsert(conn, &row))?;

    Ok(json!({
        "stored": final_name,
        "sha256": sha,
        "size": data.len(),
        "mime": mime,
    }))
}

/// 从指定数据目录读池里的文件。
///
/// 导出与测试只需要一个目录，不需要整个 `AppHandle` —— 少一层依赖才能直接测。
/// 只依赖数据目录的写入：给工具用（工具拿不到 `AppHandle`）。
///
/// 与 `put` 的差别只有一处：不写 asset 索引表。索引由 `write_state` 之后的
/// `sync` 按工作区里的资产统一补 —— 比让每个工具各写一遍更不容易漏。
pub fn put_at(
    root: &Path,
    id: &str,
    name: &str,
    data: &[u8],
) -> Result<crate::model::Asset, String> {
    let sha = sha256_hex(data);
    let dir = root.join("pool");
    std::fs::create_dir_all(&dir).map_err(|error| format!("建资产池失败：{error}"))?;

    let ext = name.rsplit('.').next();
    let mut final_name = crate::snapshot::pool_name(&sha, ext)?;
    let mut path = dir.join(&final_name);

    // 同一份内容只落一份。哈希前缀撞了就退回完整哈希，绝不覆盖已有文件
    if path.exists() {
        let existing = std::fs::read(&path).map_err(|error| format!("读取池文件失败：{error}"))?;
        if sha256_hex(&existing) != sha {
            final_name = format!("{sha}.{}", ext.unwrap_or("bin"));
            path = dir.join(&final_name);
        }
    }
    if !path.exists() {
        crate::storage::atomic_write(&path, data)?;
    }

    Ok(crate::model::Asset {
        id: id.to_string(),
        kind: "image".into(),
        name: name.to_string(),
        stored: final_name,
        sha256: Some(sha),
        size: Some(data.len() as u64),
        mime: Some(mime_of(name)),
        note: None,
        extra: Default::default(),
    })
}

pub fn read_at(root: &Path, stored: &str) -> Result<Vec<u8>, String> {
    let name = safe_stored(stored)?;
    let pool = root.join("pool").join(&name);
    if let Ok(bytes) = std::fs::read(&pool) {
        return Ok(bytes);
    }
    // 旧版本把材料放在 `assets/`
    let legacy = root.join("assets").join(&name);
    if let Ok(bytes) = std::fs::read(&legacy) {
        return Ok(bytes);
    }
    Err(format!("材料文件不存在：{name}"))
}

pub fn read(app: &tauri::AppHandle, stored: &str) -> Result<Vec<u8>, String> {
    read_at(&crate::data_dir(app)?, stored)
}

/// 移进 `.trash/<年月日>/`，不直接抹掉。原件不可重建，留退路。
fn trash(app: &tauri::AppHandle, stored: &str) -> Result<(), String> {
    let name = safe_stored(stored)?;
    let dir = pool_dir(app)?;
    let source = dir.join(&name);
    if !source.exists() {
        return Ok(());
    }
    let day = {
        // 用系统时间做目录名：不需要时区正确，只需要可读
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_secs())
            .unwrap_or(0);
        format!("trash-{}", secs / 86_400)
    };
    let target_dir = dir.join(".trash").join(day);
    std::fs::create_dir_all(&target_dir).map_err(|error| format!("建回收目录失败：{error}"))?;
    std::fs::rename(&source, target_dir.join(&name))
        .map_err(|error| format!("移动文件到回收目录失败：{error}"))
}

/* ---------- 引用同步 ---------- */

/// 一次写入后调用：更新每个资产「有没有被引用」，并把从有到无的那些移进回收目录。
///
/// 只看 stored 不看 id：同一个文件可能被三处引用，换任何一处 id 都不该影响判定。
pub fn sync(app: &tauri::AppHandle, state: &Shared, workspace: &Workspace) -> Result<usize, String> {
    // 知识库图片也躺在同一个池里：只算工作区，会把「简历里删了、知识库里还留着」的那份回收掉
    let mut used = used_stored(workspace);
    used.extend(state.with(|conn| db::knowledge_asset_stored(conn))?);
    let rows = state.with(|conn| db::asset_all(conn))?;
    let mut reclaimed = 0usize;

    for row in rows {
        let now_used = used.contains(&row.stored);
        if row.refs > 0 && !now_used {
            // 从「有人用」变成「没人用」：删掉最后一次引用的就是这一刻
            trash(app, &row.stored)?;
            state.with(|conn| db::asset_remove(conn, &row.id))?;
            reclaimed += 1;
            continue;
        }
        let next = if now_used { 1 } else { 0 };
        if next != row.refs {
            state.with(|conn| db::asset_set_refs(conn, &row.id, next))?;
        }
    }
    Ok(reclaimed)
}

/// 工作区里所有出现过的池文件名。
///
/// 顶层池（workspace.assets）与 v4 时代嵌在人 / 条目上的材料都要算进去，
/// 否则老数据里的证书会在第一次保存时被当成孤儿。
pub fn used_stored(workspace: &Workspace) -> HashSet<String> {
    let mut used = HashSet::new();
    let mut by_id: HashMap<&str, &str> = HashMap::new();

    // 写成普通函数而不是闭包：闭包会一直可变借用 used，后面就没法再直接 insert
    fn note_asset(used: &mut HashSet<String>, asset: &crate::model::Asset) {
        // 链接不是文件：stored 里放的是网址，算进来会让「引用统计」多出一个不存在的文件
        if asset.kind == "link" {
            return;
        }
        if !asset.stored.trim().is_empty() && !asset.stored.starts_with("data:") {
            used.insert(asset.stored.clone());
        }
    }

    for asset in &workspace.assets {
        note_asset(&mut used, asset);
        by_id.insert(asset.id.as_str(), asset.stored.as_str());
    }
    for person in &workspace.people {
        for asset in &person.assets {
            note_asset(&mut used, asset);
            by_id.insert(asset.id.as_str(), asset.stored.as_str());
        }
        for item in &person.items {
            for asset in &item.assets {
                note_asset(&mut used, asset);
                by_id.insert(asset.id.as_str(), asset.stored.as_str());
            }
        }
        for resume in &person.resumes {
            for page in &resume.pages {
                if let Some(asset_id) = &page.asset_id {
                    if let Some(stored) = by_id.get(asset_id.as_str()) {
                        used.insert((*stored).to_string());
                    }
                }
                for block in &page.blocks {
                    if let Some(asset_id) = &block.asset_id {
                        if let Some(stored) = by_id.get(asset_id.as_str()) {
                            used.insert((*stored).to_string());
                        }
                    }
                    for entry in &block.items {
                        for asset in &entry.assets {
                            note_asset(&mut used, asset);
                        }
                    }
                }
            }
        }
    }
    used
}

/* ---------- 命令 ---------- */

#[tauri::command]
pub fn pool_put(
    app: tauri::AppHandle,
    state: tauri::State<'_, Shared>,
    id: String,
    name: String,
    data: Vec<u8>,
) -> Result<Value, String> {
    put(&app, &state, &id, &name, &data)
}

#[tauri::command]
pub fn pool_get(app: tauri::AppHandle, stored: String) -> Result<Vec<u8>, String> {
    read(&app, &stored)
}

/// 资产池总览：谁在用、谁没用。界面上的「资产」面板读这个。
#[tauri::command]
pub fn pool_list(app: tauri::AppHandle, state: tauri::State<'_, Shared>) -> Result<Value, String> {
    let dir = pool_dir(&app)?;
    let rows = state.with(|conn| db::asset_all(conn))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(json!({
            "id": row.id,
            "stored": row.stored,
            "name": row.name,
            "sha256": row.sha256,
            "size": row.bytes,
            "mime": row.mime,
            "used": row.refs > 0,
            "onDisk": dir.join(&row.stored).exists(),
        }));
    }
    Ok(json!({
        "dir": dir.to_string_lossy(),
        "files": out,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // 超过一个块（55 / 56 / 64 字节是 padding 的经典边界）
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        assert_eq!(sha256_hex(&[b'a'; 64]).len(), 64);
    }

    #[test]
    fn stored_name_rejects_paths() {
        assert!(safe_stored("a/b.png").is_ok_and(|name| name == "b.png"));
        assert!(safe_stored("..").is_err());
        assert!(safe_stored("").is_err());
    }

    #[test]
    fn mime_guesses_from_extension() {
        assert_eq!(mime_of("证书.PNG"), "image/png");
        assert_eq!(mime_of("a.mp4"), "video/mp4");
        assert_eq!(mime_of("a.unknown"), "application/octet-stream");
    }
    /// 引用统计：一份文件可能藏在四个地方，漏掉任何一处都会让它在保存后被误回收
    #[test]
    fn used_stored_counts_every_place_a_file_can_hide() {
        let ws: Workspace = serde_json::from_value(serde_json::json!({
            "version": 5,
            "assets": [{ "id": "a0", "kind": "image", "name": "池.png", "stored": "pool.png" }],
            "people": [{
                "id": "p1",
                "fields": [],
                "assets": [
                    { "id": "a1", "kind": "image", "name": "证件照.png", "stored": "photo.png" },
                    { "id": "a2", "kind": "link", "name": "作品链接", "stored": "https://example.com" }
                ],
                "items": [{
                    "id": "i1",
                    "fields": [],
                    "notes": "",
                    "assets": [{ "id": "a3", "kind": "file", "name": "证书.pdf", "stored": "cert.pdf" }]
                }],
                "messages": [],
                "resumes": [{
                    "id": "r1",
                    "personId": "p1",
                    "title": "简历",
                    "updatedAt": 0,
                    "pages": [
                        { "id": "pg1", "kind": "file", "label": "扫描件", "assetId": "a3", "blocks": [] },
                        { "id": "pg2", "kind": "native", "label": "正文", "blocks": [
                            { "id": "b1", "kind": "media", "assetId": "a1" }
                        ]}
                    ]
                }]
            }]
        }))
        .expect("测试数据");

        let used = used_stored(&ws);
        assert!(used.contains("pool.png"), "顶层池里的文件要算");
        assert!(used.contains("photo.png"), "人身上的证件照要算");
        assert!(used.contains("cert.pdf"), "库条目上的证书要算");
        assert_eq!(used.len(), 3, "链接不是文件，不该算进来：{used:?}");
    }

    #[test]
    fn data_url_assets_are_not_pool_files() {
        // 浏览器调试模式存的是 data URL，把它当成池文件会让回收逻辑找不到东西
        let ws: Workspace = serde_json::from_value(serde_json::json!({
            "version": 5,
            "people": [{
                "id": "p1",
                "fields": [],
                "assets": [{ "id": "a1", "kind": "image", "name": "x.png", "stored": "data:image/png;base64,AAA" }],
                "items": [],
                "messages": [],
                "resumes": []
            }]
        }))
        .expect("测试数据");
        assert!(used_stored(&ws).is_empty());
    }
}
