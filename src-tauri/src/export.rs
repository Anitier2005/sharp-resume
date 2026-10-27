//! 导出执行器：把一份简历写成一个文件夹。
//!
//! 三条硬规则：
//!   1. **只回收自己放的文件**。目录里的其他东西（用户自己拷进去的材料）永不触碰。
//!      判断依据只有 `manifest.json`，不靠「看起来像」。
//!   2. **先写后删**。新文件全部成功之后才删旧的；中途失败只会多几个 `.part`，
//!      不会出现「内容缺失但看起来正常」的目录。
//!   3. **单文件原子**。每个文件先写 `.part` 再 rename，任何时刻读到的都是完整文件。
//!
//! file 页是**逐字节拷贝**：原件的 sha256 进 manifest，用户随时能校验。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::db::{self, Shared};
use crate::model::{Asset, Resume, ResumePage, Workspace};
use crate::snapshot::{self, PageKind, PlanInput, SnapshotPage};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportInput {
    pub person_id: String,
    pub resume_id: String,
    /// 用户指定目录（绝对路径）；为空就用 `<数据目录>/导出/<人 - 简历>`
    #[serde(default)]
    pub dir: Option<String>,
}

struct Planned {
    /// 页在简历里的序号，1 起
    index: usize,
    page: ResumePage,
    file_name: String,
    kind: PageKind,
    /// file 页：池内存储名 + sha256 + 原始文件名
    source: Option<(String, String, String)>,
}

fn asset_map(workspace: &Workspace) -> HashMap<String, Asset> {
    let mut map = HashMap::new();
    for asset in &workspace.assets {
        map.insert(asset.id.clone(), asset.clone());
    }
    // v4 时代材料挂在人 / 条目上，导出时也要能解析
    for person in &workspace.people {
        for asset in &person.assets {
            map.entry(asset.id.clone()).or_insert_with(|| asset.clone());
        }
        for item in &person.items {
            for asset in &item.assets {
                map.entry(asset.id.clone()).or_insert_with(|| asset.clone());
            }
        }
        for resume in &person.resumes {
            for page in &resume.pages {
                for block in &page.blocks {
                    for entry in &block.items {
                        for asset in &entry.assets {
                            map.entry(asset.id.clone()).or_insert_with(|| asset.clone());
                        }
                    }
                }
            }
        }
    }
    map
}

fn ext_of(name: &str) -> String {
    name.rsplit('.').next().unwrap_or("").to_ascii_lowercase()
}

/// 同名但内容不是我们上次写的那份：说明用户改过。
/// 不覆盖，另起一个名字，最后在结果里报出来让用户自己决定。
fn free_name(dir: &Path, wanted: &str, reserved: &HashSet<String>) -> String {
    let path = Path::new(wanted);
    let stem = path.file_stem().map(|v| v.to_string_lossy().to_string()).unwrap_or_else(|| "文件".into());
    let ext = path.extension().map(|v| v.to_string_lossy().to_string()).unwrap_or_default();
    let mut attempt = 2;
    loop {
        let name = if ext.is_empty() {
            format!("{stem} ({attempt})")
        } else {
            format!("{stem} ({attempt}).{ext}")
        };
        if !reserved.contains(&name.to_lowercase()) && !dir.join(&name).exists() {
            return name;
        }
        attempt += 1;
        if attempt > 99 {
            return wanted.to_string();
        }
    }
}

pub fn run(
    root: &Path,
    workspace: &Workspace,
    resume: &Resume,
    input: &ExportInput,
) -> Result<Value, String> {
    let person = workspace
        .people
        .iter()
        .find(|item| item.id == input.person_id)
        .ok_or_else(|| format!("找不到候选人 {}", input.person_id))?;
    let person_name = {
        let name = person
            .fields
            .iter()
            .find(|field| !field.value.trim().is_empty())
            .map(|field| field.value.trim().to_string())
            .unwrap_or_default();
        if name.is_empty() { "未命名".to_string() } else { name }
    };
    let assets = asset_map(workspace);

    // 1. 先算计划：文件名、编号、manifest 骨架
    let snapshot_pages: Vec<SnapshotPage> = resume
        .pages
        .iter()
        .map(|page| {
            if page.kind == "file" {
                let asset = page.asset_id.as_deref().and_then(|id| assets.get(id));
                SnapshotPage {
                    id: page.id.clone(),
                    label: page.label.clone(),
                    kind: PageKind::File,
                    ext: asset.map(|item| ext_of(&item.name)),
                    sha256: asset.and_then(|item| item.sha256.clone()),
                }
            } else {
                SnapshotPage {
                    id: page.id.clone(),
                    label: page.label.clone(),
                    kind: PageKind::Native,
                    ext: None,
                    sha256: None,
                }
            }
        })
        .collect();

    let plan = snapshot::plan(&PlanInput {
        person_id: person.id.clone(),
        person_name: person_name.clone(),
        resume_id: resume.id.clone(),
        resume_title: resume.title.clone(),
        pages: snapshot_pages,
        exported_at: None,
    });

    // 2. 目标目录
    let dir = match input.dir.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        Some(raw) => PathBuf::from(raw),
        None => root.join("导出").join(&plan.dir_name),
    };

    if dir.exists() && !dir.is_dir() {
        return Err(format!("{} 是个文件，不是目录", dir.display()));
    }
    let old_manifest = read_manifest(&dir)?;
    if !dir.exists() {
        std::fs::create_dir_all(&dir).map_err(|error| format!("建导出目录失败：{error}"))?;
    } else if old_manifest.is_none() {
        // 非空又没有我们的 manifest：不猜，直接拒绝。整目录同步意味着要删文件，
        // 猜错的代价是删掉用户自己放进去的材料。
        let busy = std::fs::read_dir(&dir)
            .map_err(|error| format!("读取目录失败：{error}"))?
            .flatten()
            .any(|entry| entry.file_name() != "manifest.json");
        if busy {
            return Err(format!(
                "{} 里已经有别的东西，而且不是本应用生成的。换一个目录名，或先把它清空",
                dir.display()
            ));
        }
    }

    let old_files: HashSet<String> = old_manifest
        .as_ref()
        .map(|value| manifest_files(value))
        .unwrap_or_default();
    let mut reserved: HashSet<String> = HashSet::new();
    let mut written: Vec<String> = Vec::new();
    let mut conflicts: Vec<String> = Vec::new();
    let accent = workspace.ui.accent.clone();

    // 3. 逐页写。先写 .part 再 rename：任何时刻读到的都是完整文件
    for (offset, page) in resume.pages.iter().enumerate() {
        let index = offset + 1;
        let planned = plan.files.get(offset);
        let wanted = planned.map(|file| file.name.clone()).unwrap_or_default();
        let name = if old_files.contains(&wanted) || !dir.join(&wanted).exists() {
            wanted.clone()
        } else {
            let free = free_name(&dir, &wanted, &reserved);
            conflicts.push(wanted.clone());
            free
        };
        reserved.insert(name.to_lowercase());

        let target = dir.join(&name);
        if page.kind == "file" {
            let asset = page
                .asset_id
                .as_deref()
                .and_then(|id| assets.get(id))
                .ok_or_else(|| {
                    format!("第 {index} 页是文件页，但它指向的材料不在了（id = {}）", page.asset_id.clone().unwrap_or_default())
                })?;
            let bytes = crate::pool::read_at(root, &asset.stored)?;
            // 逐字节拷贝：不转码、不重压缩
            crate::storage::atomic_write(&target, &bytes)?;
        } else {
            let bytes = crate::docx::page_docx(
                &person_name,
                page,
                &person.items,
                &assets,
                &accent,
            );
            crate::storage::atomic_write(&target, &bytes)?;
        }
        written.push(name);

        // 页里的图片旁挂：正文留一行说明，文件本体放进来，HR 拿到的是完整资料
        for (slot, block) in page.blocks.iter().enumerate() {
            if block.kind != "media" {
                continue;
            }
            let Some(asset) = block.asset_id.as_deref().and_then(|id| assets.get(id)) else {
                continue;
            };
            let Some(bytes) = crate::pool::read_at(root, &asset.stored).ok() else {
                continue;
            };
            let ext = ext_of(&asset.name);
            let base = snapshot::clean_name(
                block.caption.as_deref().unwrap_or(&asset.name),
            );
            let base = if base.is_empty() { format!("附件 {}", slot + 1) } else { base };
            let attach = format!("{:02}-{:02} {}.{}", index, slot + 1, base, if ext.is_empty() { "bin".into() } else { ext });
            let attach = if old_files.contains(&attach) || !dir.join(&attach).exists() {
                attach
            } else {
                free_name(&dir, &attach, &reserved)
            };
            reserved.insert(attach.to_lowercase());
            crate::storage::atomic_write(&dir.join(&attach), &bytes)?;
            written.push(attach);
        }
    }

    // 4. 删旧文件。只删「上一次 manifest 里列过、这次不再生成」的那些
    let keep: HashSet<String> = written.iter().map(|name| name.to_lowercase()).collect();
    let mut removed = 0usize;
    for old in &old_files {
        if keep.contains(&old.to_lowercase()) {
            continue;
        }
        let path = dir.join(old);
        if path.is_file() {
            let _ = std::fs::remove_file(&path);
            removed += 1;
        }
    }

    // 5. manifest 最后写：它的存在代表「这一份快照完整」
    let manifest = json!({
        "version": 1,
        "app": "sharp-resume",
        "exportedAt": plan.manifest.exported_at,
        "personId": person.id,
        "personName": person_name,
        "resumeId": resume.id,
        "resumeTitle": resume.title,
        "pages": plan.files.iter().enumerate().map(|(offset, file)| {
            let name = written.get(offset).cloned().unwrap_or_else(|| file.name.clone());
            json!({
                "index": file.page_index,
                "id": file.page_id,
                "label": file.label,
                "file": name,
                "kind": file.kind,
                "sha256": file.sha256,
            })
        }).collect::<Vec<_>>(),
    });
    let text = serde_json::to_string_pretty(&manifest)
        .map_err(|error| format!("manifest 序列化失败：{error}"))?;
    crate::storage::atomic_write(&dir.join("manifest.json"), format!("{text}\n").as_bytes())?;

    Ok(json!({
        "dir": dir.to_string_lossy(),
        "dirName": plan.dir_name,
        "files": written,
        "removed": removed,
        "conflicts": conflicts,
    }))
}

fn read_manifest(dir: &Path) -> Result<Option<Value>, String> {
    let path = dir.join("manifest.json");
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).map_err(|error| format!("读取 manifest 失败：{error}"))?;
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return Ok(None);
    };
    if snapshot::is_our_manifest(&value) {
        Ok(Some(value))
    } else {
        Ok(None)
    }
}

fn manifest_files(manifest: &Value) -> HashSet<String> {
    let mut out = HashSet::new();
    if let Some(pages) = manifest.get("pages").and_then(Value::as_array) {
        for page in pages {
            if let Some(file) = page.get("file").and_then(Value::as_str) {
                out.insert(file.to_string());
            }
        }
    }
    out
}

/* ---------- 命令 ---------- */

#[tauri::command]
pub fn export_resume(
    app: tauri::AppHandle,
    state: tauri::State<'_, Shared>,
    input: ExportInput,
) -> Result<Value, String> {
    let workspace = state.with(|conn| db::load(conn))?
        .ok_or_else(|| "工作区里还没有数据".to_string())?;
    let resume = workspace
        .people
        .iter()
        .find(|person| person.id == input.person_id)
        .and_then(|person| person.resumes.iter().find(|item| item.id == input.resume_id))
        .ok_or_else(|| format!("找不到简历 {}", input.resume_id))?
        .clone();
    let root = crate::data_dir(&app)?;
    let result = run(&root, &workspace, &resume, &input)?;
    // 导出会读池里的文件，顺手做一次引用同步：删掉的页对应的文件进回收目录
    let _ = crate::pool::sync(&app, &state, &workspace);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// 每个用例一个独立目录：<临时>/sharp-resume-export-test/<tag>-<纳秒>
    fn temp_root(tag: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir()
            .join("sharp-resume-export-test")
            .join(format!("{tag}-{stamp}"));
        std::fs::create_dir_all(&dir).expect("建临时目录");
        dir
    }

    /// 一份最小的真实形状：一人一简历两页（正文页 + 上传原件页）
    fn workspace_with_two_pages() -> Workspace {
        serde_json::from_value(json!({
            "version": 5,
            "people": [{
                "id": "p1",
                "fields": [{ "id": "f1", "label": "姓名", "value": "张三" }],
                "assets": [{ "id": "a1", "kind": "image", "name": "原件.png", "stored": "scan.png" }],
                "items": [],
                "messages": [],
                "resumes": [{
                    "id": "r1",
                    "personId": "p1",
                    "title": "测试简历",
                    "updatedAt": 0,
                    "pages": [
                        {
                            "id": "pg1",
                            "kind": "native",
                            "label": "正文",
                            "blocks": [{
                                "id": "pg1-head",
                                "kind": "fields",
                                "fields": [
                                    { "id": "f2", "label": "姓名", "value": "张三" },
                                    { "id": "f3", "label": "职位", "value": "工程师" }
                                ]
                            }]
                        },
                        {
                            "id": "pg2",
                            "kind": "file",
                            "label": "原件",
                            "assetId": "a1",
                            "blocks": []
                        }
                    ]
                }]
            }],
            "ui": { "accent": "white" }
        }))
        .expect("测试数据要能解析")
    }

    fn names(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .expect("读目录")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .collect()
    }

    #[test]
    fn writes_every_page_and_copies_the_original_bytes() {
        let root = temp_root("roundtrip");
        let out = root.join("out");
        std::fs::create_dir_all(root.join("pool")).unwrap();
        std::fs::write(root.join("pool").join("scan.png"), b"PNGDATA").unwrap();

        let workspace = workspace_with_two_pages();
        let resume = workspace.people[0].resumes[0].clone();
        let input = ExportInput {
            person_id: "p1".into(),
            resume_id: "r1".into(),
            dir: Some(out.to_string_lossy().to_string()),
        };

        let result = run(&root, &workspace, &resume, &input).expect("导出应当成功");
        let files = result["files"].as_array().unwrap();
        // 两页 + manifest
        assert_eq!(files.len(), 2, "两页各一个文件：{files:?}");
        assert!(out.join("manifest.json").exists(), "manifest 必须写出来");

        // file 页是逐字节拷贝
        let copied = names(&out)
            .into_iter()
            .find(|name| name.ends_with(".png"))
            .expect("原件应当被拷进来");
        assert_eq!(std::fs::read(out.join(&copied)).unwrap(), b"PNGDATA");

        // 正文页是 docx（zip 头 PK）
        let docx = names(&out)
            .into_iter()
            .find(|name| name.ends_with(".docx"))
            .expect("正文页应当有 docx");
        let bytes = std::fs::read(out.join(&docx)).unwrap();
        assert_eq!(&bytes[..2], b"PK", "docx 是 zip，应以 PK 开头");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn rerun_replaces_manifest_and_drops_files_of_removed_pages() {
        let root = temp_root("rerun");
        let out = root.join("out");
        std::fs::create_dir_all(root.join("pool")).unwrap();
        std::fs::write(root.join("pool").join("scan.png"), b"PNGDATA").unwrap();

        let workspace = workspace_with_two_pages();
        let resume = workspace.people[0].resumes[0].clone();
        let input = ExportInput {
            person_id: "p1".into(),
            resume_id: "r1".into(),
            dir: Some(out.to_string_lossy().to_string()),
        };
        run(&root, &workspace, &resume, &input).expect("第一次导出");

        // 删掉第二页再导一次：它的文件应当被清掉，目录里只剩一页 + manifest
        let mut one_page = workspace.clone();
        one_page.people[0].resumes[0].pages.truncate(1);
        let resume = one_page.people[0].resumes[0].clone();
        let result = run(&root, &one_page, &resume, &input).expect("第二次导出");

        assert_eq!(result["removed"].as_u64().unwrap(), 1, "第二页的文件应被清掉");
        assert_eq!(result["files"].as_array().unwrap().len(), 1);
        assert!(
            !names(&out).iter().any(|name| name.ends_with(".png")),
            "原件不该还留着：{:?}",
            names(&out)
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn refuses_a_folder_that_is_not_ours() {
        let root = temp_root("foreign");
        let out = root.join("out");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("用户自己的东西.txt"), b"mine").unwrap();

        let workspace = workspace_with_two_pages();
        let resume = workspace.people[0].resumes[0].clone();
        let input = ExportInput {
            person_id: "p1".into(),
            resume_id: "r1".into(),
            dir: Some(out.to_string_lossy().to_string()),
        };

        let error = run(&root, &workspace, &resume, &input).expect_err("不该往别人的目录里写");
        assert!(error.contains("已经有别的东西"), "错误要说清原因：{error}");
        assert_eq!(std::fs::read(out.join("用户自己的东西.txt")).unwrap(), b"mine");

        std::fs::remove_dir_all(&root).ok();
    }
}
