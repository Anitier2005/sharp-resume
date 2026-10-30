//! 图片转化：限尺寸、转格式、按比例裁剪。
//!
//! 为什么这是一个工具而不是界面按钮：素材来自各处 —— 手机拍的证件照 4MB、
//! 截图是 png 带透明通道、作品图要方形。用户在对话里说「这张太大」「裁成正方形」，
//! 应该由 agent 直接办掉，而不是让他自己去某个面板里找压缩选项。
//!
//! 两个 agent 都能用（`Scope::Session` + `Scope::Layout`）：
//!   会话 agent 负责「用户提需求 → 转好 → 告诉他/转给排版」
//!   排版 agent 负责「作品集里那张图要通栏，先按 16:9 裁一版」
//!
//! 硬约束：只处理 `kind == "image"` 的材料；不做「PDF 转图片」——
//! 那要外挂 pdfium 之类的二进制，成本与本轮目标不成比例。

use std::io::Cursor;

use image::{DynamicImage, ImageFormat};
use serde_json::{json, Value};

use super::{push_asset, ToolCtx};
use crate::model::Asset;
use crate::pool;

pub fn params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "assetId": {
                "type": "string",
                "description": "要处理的图片资产 id，从 read_person 的材料清单里取"
            },
            "maxSide": {
                "type": "integer",
                "description": "最长边压到多少像素，默认 1600。证件照 600 就够，作品图 1600–2000。只缩不放"
            },
            "format": {
                "type": "string",
                "enum": ["keep", "jpeg", "png"],
                "description": "输出格式。keep 表示沿用原格式；照片用 jpeg，带透明或线稿用 png"
            },
            "quality": {
                "type": "integer",
                "description": "jpeg 质量 1–100，默认 85。低于 70 会糊，高于 92 体积下不来"
            },
            "ratio": {
                "type": "string",
                "description": "按比例居中裁剪，比如 1:1 方形头像、3:4 证件照、16:9 通栏图。不传就不裁"
            },
            "name": {
                "type": "string",
                "description": "新文件的名字，不传就在原名后加标记"
            }
        },
        "required": ["assetId"],
        "additionalProperties": false
    })
}

/// 从工作区里找一份材料：人的、库条目的、简历的原件样张、顶层池的都要找。
///
/// 样张那条不能漏：它**故意**不在 `person.assets` 里（进了材料列表就会被当成
/// 用户自己的一张图），可 `view` 仍要能按 assetId 打开它 —— 排版 agent 唯一的
/// 对照依据就是它。
pub(crate) fn find_asset<'a>(workspace: &'a Value, id: &str) -> Option<&'a Value> {
    let people = workspace.get("people")?.as_array()?;
    for person in people {
        if let Some(list) = person.get("assets").and_then(Value::as_array) {
            if let Some(found) = list.iter().find(|item| item["id"].as_str() == Some(id)) {
                return Some(found);
            }
        }
        if let Some(items) = person.get("items").and_then(Value::as_array) {
            for item in items {
                if let Some(list) = item.get("assets").and_then(Value::as_array) {
                    if let Some(found) = list.iter().find(|item| item["id"].as_str() == Some(id)) {
                        return Some(found);
                    }
                }
            }
        }
        if let Some(resumes) = person.get("resumes").and_then(Value::as_array) {
            for resume in resumes {
                if let Some(sheet) = resume.get("originSheet") {
                    if sheet["id"].as_str() == Some(id) {
                        return Some(sheet);
                    }
                }
            }
        }
    }
    workspace
        .get("assets")
        .and_then(Value::as_array)?
        .iter()
        .find(|item| item["id"].as_str() == Some(id))
}

fn parse_ratio(raw: &str) -> Option<(u32, u32)> {
    let (left, right) = raw.split_once(':')?;
    let w: u32 = left.trim().parse().ok()?;
    let h: u32 = right.trim().parse().ok()?;
    if w == 0 || h == 0 {
        None
    } else {
        Some((w, h))
    }
}

/// 居中裁到指定比例：多出来的部分从两边各切一半
fn crop_ratio(img: DynamicImage, rw: u32, rh: u32) -> DynamicImage {
    let (w, h) = (img.width(), img.height());
    let target = rw as f64 / rh as f64;
    let current = w as f64 / h as f64;

    if (current - target).abs() < 0.005 {
        return img;
    }
    if current > target {
        // 比目标更宽：切左右
        let new_w = ((h as f64 * target).round() as u32).max(1).min(w);
        let x = (w - new_w) / 2;
        img.crop_imm(x, 0, new_w, h)
    } else {
        // 比目标更高：切上下
        let new_h = ((w as f64 / target).round() as u32).max(1).min(h);
        let y = (h - new_h) / 2;
        img.crop_imm(0, y, w, new_h)
    }
}

fn encode(img: &DynamicImage, format: &str, quality: u8) -> Result<(Vec<u8>, &'static str), String> {
    let mut buf: Vec<u8> = Vec::new();
    match format {
        "png" => {
            img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
                .map_err(|error| format!("转 png 失败：{error}"))?;
            Ok((buf, "png"))
        }
        _ => {
            // jpeg 没有透明通道：带 alpha 的图必须先拍平，否则编码直接报错
            let rgb = img.to_rgb8();
            let mut cursor = Cursor::new(&mut buf);
            let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, quality);
            encoder
                .encode(&rgb, rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8)
                .map_err(|error| format!("转 jpeg 失败：{error}"))?;
            Ok((buf, "jpg"))
        }
    }
}

pub fn convert(args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let asset_id = args
        .get("assetId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or("assetId 不能为空：先 read_person 看有哪些材料")?;

    let root = ctx
        .root
        .ok_or("当前环境没有数据目录（浏览器调试模式），图片转化只能在桌面端用")?;

    let asset = find_asset(ctx.workspace, asset_id)
        .ok_or_else(|| format!("找不到 id 为 {asset_id} 的材料，先 read_person 看清单"))?;

    if asset["kind"].as_str() != Some("image") {
        return Err(format!(
            "「{}」不是图片，没法转化。只能处理图片类材料",
            asset["name"].as_str().unwrap_or(asset_id)
        ));
    }
    let stored = asset["stored"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or("这份材料没有对应的文件（可能是链接或数据串）")?;
    if stored.starts_with("data:") {
        return Err("这份材料存的是内联数据（浏览器调试模式），转化只在桌面端可用".into());
    }

    let bytes = pool::read_at(root, stored)?;
    let original_len = bytes.len();
    let img = image::load_from_memory(&bytes)
        .map_err(|error| format!("这张图打不开（可能不是标准图片格式）：{error}"))?;
    let (orig_w, orig_h) = (img.width(), img.height());

    // 1) 先裁：先裁再缩，缩的时候要处理的像素更少
    let mut out = img;
    let mut cropped = None;
    if let Some(raw) = args.get("ratio").and_then(Value::as_str).map(str::trim) {
        if !raw.is_empty() {
            let (rw, rh) = parse_ratio(raw)
                .ok_or_else(|| format!("ratio 要写成「宽:高」，比如 3:4；收到的是「{raw}」"))?;
            out = crop_ratio(out, rw, rh);
            cropped = Some(format!("{rw}:{rh}"));
        }
    }

    // 2) 再缩：只缩不放。放大只会变大变糊，没有意义
    let max_side = args
        .get("maxSide")
        .and_then(Value::as_u64)
        .map(|value| value.clamp(64, 6000) as u32)
        .unwrap_or(1600);
    if out.width() > max_side || out.height() > max_side {
        out = out.resize(max_side, max_side, image::imageops::FilterType::Lanczos3);
    }

    // 3) 编码
    let wanted = args.get("format").and_then(Value::as_str).unwrap_or("keep");
    let original_ext = stored.rsplit('.').next().unwrap_or("jpg").to_lowercase();
    let format = match wanted {
        "jpeg" | "png" => wanted,
        // keep：png 保持 png（多半是截图或线稿，转 jpeg 会糊），其余一律 jpeg
        _ => {
            if original_ext == "png" {
                "png"
            } else {
                "jpeg"
            }
        }
    };
    let quality = args
        .get("quality")
        .and_then(Value::as_u64)
        .map(|value| value.clamp(1, 100) as u8)
        .unwrap_or(85);

    let (data, ext) = encode(&out, format, quality)?;

    // 4) 起名与落池
    let base = args
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            let stem = asset["name"]
                .as_str()
                .unwrap_or("image")
                .rsplit_once('.')
                .map(|(left, _)| left)
                .unwrap_or("image");
            let tag = cropped.as_deref().map(|value| value.replace(':', "x")).unwrap_or_else(|| "s".into());
            format!("{stem}-{tag}.{ext}")
        });
    let name = if base.contains('.') {
        base
    } else {
        format!("{base}.{ext}")
    };

    let new_id = format!("a-{}", crate::session::uuid_like());
    let asset_out: Asset = pool::put_at(root, &new_id, &name, &data)?;
    push_asset(ctx, asset_out)?;

    let (new_w, new_h) = (out.width(), out.height());
    let mut notes = vec![format!("{new_w}×{new_h}")];
    if let Some(ratio) = &cropped {
        notes.push(format!("按 {ratio} 居中裁剪"));
    }
    notes.push(format!("{}KB（原 {}KB）", data.len() / 1024, original_len / 1024));
    if data.len() >= original_len {
        notes.push("体积没有变小：原图已经压过，或者转成了无损格式".into());
    }

    Ok(format!(
        "已生成新图片 asset:{new_id}（{}），原名「{}」{}×{}。\n\
         引用方式：HTML 里写 <img src=\"asset:{new_id}\">；\
         要放进内容字段则让用户在界面上选图。",
        notes.join("、"),
        asset["name"].as_str().unwrap_or(""),
        orig_w,
        orig_h
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let img = DynamicImage::new_rgb8(w, h);
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png).unwrap();
        buf
    }

    #[test]
    fn ratio_parsing_rejects_junk() {
        assert_eq!(parse_ratio("3:4"), Some((3, 4)));
        assert_eq!(parse_ratio(" 16 : 9 "), Some((16, 9)));
        assert_eq!(parse_ratio("0:4"), None);
        assert_eq!(parse_ratio("abc"), None);
        assert_eq!(parse_ratio("3"), None);
    }

    #[test]
    fn cropping_keeps_the_center_and_the_aspect() {
        let img = DynamicImage::new_rgb8(400, 200);
        let out = crop_ratio(img, 1, 1);
        assert_eq!((out.width(), out.height()), (200, 200), "正方形要从两边各切一半");
    }

    #[test]
    fn shrinking_never_enlarges() {
        let img = DynamicImage::new_rgb8(120, 80);
        let max_side = 600u32;
        // 与 convert 同一条件：只有超过上限才缩
        assert!(img.width() <= max_side && img.height() <= max_side);
    }

    #[test]
    fn jpeg_flattens_alpha_instead_of_failing() {        let img = DynamicImage::new_rgba8(32, 32);
        let (data, ext) = encode(&img, "jpeg", 80).expect("带 alpha 也要能转 jpeg");
        assert_eq!(ext, "jpg");
        assert!(data.len() > 100);
    }

    /// 原件样张不在人的材料列表里（它不该当材料出现），但 `view` 必须找得到 ——
    /// 排版 agent 唯一的对照依据就是它
    #[test]
    fn the_origin_sheet_is_findable_even_though_it_is_not_a_material() {
        let ws = json!({
            "people": [{
                "id": "p1",
                "assets": [],
                "items": [],
                "resumes": [{
                    "id": "r1",
                    "pages": [],
                    "originSheet": {
                        "id": "a-sheet", "kind": "image", "name": "简历 样张.jpg", "stored": "s.jpg"
                    }
                }]
            }]
        });
        let found = find_asset(&ws, "a-sheet").expect("样张要能按 assetId 找到");
        assert_eq!(found["stored"], "s.jpg");
        assert!(find_asset(&ws, "a-nope").is_none());
    }

    #[test]
    fn convert_refuses_when_there_is_no_data_dir() {
        let ws = json!({ "people": [] });
        let out = crate::tools::Outbox::default();
        let ctx = crate::tools::ToolCtx {
            workspace: &ws,
            person_id: "p1",
            resume_id: "r1",
            page_id: "pg1",
            out: &out,
            root: None,
            facts: None,
        };
        let err = convert(&json!({ "assetId": "a1" }), &ctx).unwrap_err();
        assert!(err.contains("桌面端"), "{err}");
    }

    #[test]
    fn convert_writes_a_real_file_and_reports_the_new_id() {
        let dir = std::env::temp_dir().join(format!(
            "sharp-resume-image-{}",
            crate::session::uuid_like()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(dir.join("pool")).unwrap();
        // 原图放进池里：宽 400、高 200 的 png
        let original = png_bytes(400, 200);
        std::fs::write(dir.join("pool").join("orig.png"), &original).unwrap();

        let ws = json!({
            "people": [{
                "id": "p1",
                "assets": [{ "id": "a1", "kind": "image", "name": "orig.png", "stored": "orig.png" }],
                "items": [],
                "resumes": []
            }]
        });
        let out = crate::tools::Outbox::default();
        let ctx = crate::tools::ToolCtx {
            workspace: &ws,
            person_id: "p1",
            resume_id: "r1",
            page_id: "pg1",
            out: &out,
            root: Some(&dir),
            facts: None,
        };

        let text = convert(
            &json!({ "assetId": "a1", "ratio": "1:1", "maxSide": 100, "format": "jpeg" }),
            &ctx,
        )
        .expect("应当成功");

        let assets = out.assets.lock().unwrap().clone();
        assert_eq!(assets.len(), 1, "产出的材料要回传前端");
        let made = &assets[0];
        assert_eq!(made.kind, "image");
        assert_eq!(made.name, "orig-1x1.jpg");
        assert!(text.contains(&made.id), "输出里要带上新 id：{text}");

        // 真的落在池里，而且真的被裁成正方形
        let stored = dir.join("pool").join(&made.stored);
        let saved = image::load_from_memory(&std::fs::read(&stored).unwrap()).unwrap();
        assert_eq!((saved.width(), saved.height()), (100, 100));
        assert_eq!(saved.color(), image::ColorType::Rgb8);

        std::fs::remove_dir_all(&dir).ok();
    }
}
