//! 抽图工具：把 PDF 里的内嵌位图登记成候选材料，交给模型去挑。
//!
//! 为什么要它：用户上传的简历 PDF 里常常带证件照，而模型看不见 PDF 里的图 ——
//! 它只拿到文字。以前只有「把 PDF 渲染成整页图片给视觉模型看」这一条路，
//! 看得到但拿不到：那张照片没法变成一个可用的 `assetId`。
//!
//! 三条边界：
//!   1. **系统不判断哪张是证件照**。只给尺寸与「占版心」的占比，选哪张由模型定，
//!      挑不出就问用户。猜错一次用户就得多说一轮。
//!   2. **幂等靠内容寻址**：asset id 就是池内存储名（`<sha16>.<ext>`），同一张图
//!      永远同一个 id。所以「再抽一次」不需要分页参数：已登记的按 id 跳过、
//!      不占名额，天然拿到下一批。
//!   3. 失败也要说清「照片没有放上去」—— 那句是模型必须转告用户的话。

use std::io::Cursor;

use image::{ImageFormat, RgbaImage};
use serde_json::{json, Value};

use super::{push_asset, ToolCtx};
use crate::pdfembed::{self, Embedded, Report, NEAR_PAGE_PCT};
use crate::pool;

/// 一次最多落几张进池。`push_asset` 的硬上限就是 8（`tools/mod.rs`），
/// 这里自己控住：依赖它静默截断的话，清单会列出池里并不存在的 id
const MAX_CANDIDATES: usize = 8;

pub fn extract_images_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "stored": {
                "type": "string",
                "description": "附件的存储名（会话材料段里那一行「附件标识」）。不是文件名"
            },
            "name": {
                "type": "string",
                "description": "用户看到的原文件名，只用于显示与新资产命名"
            }
        },
        "required": ["stored"],
        "additionalProperties": false
    })
}

pub fn extract_images(args: &Value, ctx: &ToolCtx) -> Result<String, String> {
    let stored = args
        .get("stored")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or("stored 不能为空：取材料段里那条「附件标识」，不是文件名")?;
    let name = args
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(stored);

    if ext_of(name).or_else(|| ext_of(stored)).as_deref() != Some("pdf") {
        return Err(format!(
            "这不是 PDF（`{name}`）。抽图只对 PDF 生效 —— 图片本身就是材料，直接当材料用就行"
        ));
    }

    // 没有数据目录就写不了池，候选图拿不到 assetId
    let Some(root) = ctx.root else {
        return Ok(register_failed(name));
    };
    let Ok(bytes) = crate::attach::read(root, stored) else {
        return Ok(open_failed(name));
    };

    let report = match pdfembed::extract(&bytes) {
        Ok(report) => report,
        Err(_) => return Ok(open_failed(name)),
    };
    if report.images.is_empty() {
        return Ok(if report.unsupported.is_empty() {
            no_bitmaps(name)
        } else {
            all_unreadable(name, &report.unsupported)
        });
    }

    let order = order_by_share(&report);
    let mut rows: Vec<String> = Vec::new();
    let mut fresh = 0usize;
    let mut pushed = 0usize;
    for index in order {
        let image = &report.images[index];
        let Ok((data, ext)) = encode_candidate(image) else {
            // 编不出文件的图既不算候选也不占名额，直接跳过
            continue;
        };
        let id = crate::snapshot::pool_name(&pool::sha256_hex(&data), Some(ext))?;
        if known(ctx, &id) {
            // 已登记过：跳过、不重复落池、不占名额 —— 这就是「再抽一次拿下一批」
            continue;
        }
        fresh += 1;
        if pushed >= MAX_CANDIDATES {
            continue;
        }
        let file = format!("{}-p{}-{}.{ext}", stem(name), image.page, pushed + 1);
        let mut asset = pool::put_at(root, &id, &file, &data)?;
        asset.extra.insert("role".into(), json!("candidate"));
        // source 写**文件名**而不是源 PDF 的 assetId：源附件不进 person.assets
        // （8 个名额全给候选图），写 id 就成了谁也查不到的悬空引用
        asset.extra.insert("source".into(), json!(name));
        asset.extra.insert("pageIndex".into(), json!(image.page));
        push_asset(ctx, asset)?;
        pushed += 1;
        rows.push(row_text(&id, image, report.pct_of(image), data.len()));
    }

    let mut out = format!(
        "【抽图结果】`{name}` 里共 {} 张内嵌位图，按来源页序排列。\
         这里只列尺寸，不判断哪张是证件照 —— 选哪张由你定。",
        report.images.len()
    );
    if !rows.is_empty() {
        out.push('\n');
        out.push_str(&rows.join("\n"));
    }
    if !report.unsupported.is_empty() {
        out.push_str(&format!(
            "\n还有 {} 张没抽出（{} 不支持）。",
            report.unsupported.len(),
            labels(&report.unsupported)
        ));
    }
    let hidden = fresh.saturating_sub(pushed);
    if hidden > 0 {
        out.push_str(&format!(
            "\n还有 {hidden} 张没列（一次最多列 8 张），需要的话说「把剩下的也抽出来」。"
        ));
    }
    out.push_str(
        "\n放照片只有一条路：把每行开头反引号里的 `assetId` 传给 `set_field` 的 `assetId` 参数，\
         指向页头那个图片字段。\n挑不出哪张是人像时，问用户一句「用第几张」，不要猜。",
    );
    Ok(out)
}

/// 候选排序：`pct` 升序（小的先留 —— 证件照总比整页扫描件小）；
/// `pct` 不可测的排最后，按像素面积升序兜底。
pub(crate) fn order_by_share(report: &Report) -> Vec<usize> {
    let mut order: Vec<usize> = (0..report.images.len()).collect();
    let area = |index: usize| {
        let image = &report.images[index];
        image.width as u64 * image.height as u64
    };
    order.sort_by(|left, right| {
        match (report.pct_of(&report.images[*left]), report.pct_of(&report.images[*right])) {
            (Some(a), Some(b)) => a.cmp(&b),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => area(*left).cmp(&area(*right)),
        }
    });
    order
}

fn row_text(id: &str, image: &Embedded, pct: Option<u32>, bytes: usize) -> String {
    let share = match pct {
        Some(value) if value >= NEAR_PAGE_PCT => format!("接近整页（占版心 {value}%）"),
        Some(value) => format!("不接近整页（占版心 {value}%）"),
        None => "占版心 不可测".to_string(),
    };
    let alpha = if image.alpha { "带透明" } else { "不透明" };
    format!(
        "- `{id}`｜第 {} 页｜{}×{} px｜宽高比 {:.2}｜{alpha}｜{share}｜{:.1} KB",
        image.page,
        image.width,
        image.height,
        image.width as f64 / image.height as f64,
        bytes as f64 / 1024.0
    )
}

/// 已登记 = 工作区里能找到，或本轮出站口里已经有
fn known(ctx: &ToolCtx, id: &str) -> bool {
    if super::image::find_asset(ctx.workspace, id).is_some() {
        return true;
    }
    ctx.out
        .assets
        .lock()
        .map(|list| list.iter().any(|asset| asset.id == id))
        .unwrap_or(false)
}

/// 候选图的文件字节 + 扩展名。
///
/// JPEG 原样落盘（PDF 里那张流本身就是完整文件，重编码一次只会更糊更大）；
/// 其余解出来的像素统一写 PNG。
pub(crate) fn encode_candidate(image: &Embedded) -> Result<(Vec<u8>, &'static str), String> {
    if image.codec == "jpg" {
        if let Some(jpeg) = &image.jpeg {
            return Ok((jpeg.clone(), "jpg"));
        }
        return Err("JPEG 流丢了原始字节".into());
    }
    let buffer = RgbaImage::from_raw(image.width, image.height, image.rgba.clone())
        .ok_or_else(|| "位图尺寸与像素数据对不上".to_string())?;
    let mut out = Vec::new();
    buffer
        .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .map_err(|error| format!("PNG 编码失败：{error}"))?;
    Ok((out, "png"))
}

fn ext_of(name: &str) -> Option<String> {
    name.rsplit_once('.')
        .map(|(_, tail)| tail.to_ascii_lowercase())
        .filter(|tail| !tail.is_empty())
}

fn stem(name: &str) -> &str {
    name.rsplit_once('.').map(|(left, _)| left).unwrap_or(name)
}

fn labels(reasons: &[&'static str]) -> String {
    let mut out: Vec<&'static str> = Vec::new();
    for reason in reasons {
        let label = reason_label(reason);
        if !out.contains(&label) {
            out.push(label);
        }
    }
    out.join("、")
}

fn reason_label(reason: &str) -> &'static str {
    match reason {
        "JPX" => "编码 JPX",
        "CCITT" => "编码 CCITT",
        "JBIG2" => "编码 JBIG2",
        "bpc" => "位深",
        "colorspace" => "色彩空间",
        "too-large" => "尺寸过大",
        "inline" => "内联图",
        "jpeg" => "JPEG 数据",
        _ => "流的编码",
    }
}

/* ---------- 失败文案（逐字，见方案 §4.4；每条都要说清「照片没有放上去」） ---------- */

fn no_bitmaps(name: &str) -> String {
    format!(
        "【抽图结果】`{name}` 里没有内嵌位图 —— 文字是矢量的，没有可抽的图。\n\
         照片没有放上去：页头仍是空的。要放照片，让用户发一张图片，或者从其它材料里找现成的图片。"
    )
}

fn all_unreadable(name: &str, reasons: &[&'static str]) -> String {
    format!(
        "【抽图结果】`{name}` 里有 {count} 张内嵌位图，{count} 张都读不出来（{labels} 暂不支持）。\n\
         照片没有放上去：页头仍是空的。不要按文件名或页面样子断定哪张是照片。",
        count = reasons.len(),
        labels = labels(reasons)
    )
}

fn open_failed(name: &str) -> String {
    format!(
        "【抽图结果】`{name}` 打不开（文件损坏或加密），读不出里面的图。\n\
         照片没有放上去：页头仍是空的。让用户重新发一份，或者直接把照片发进来。"
    )
}

fn register_failed(name: &str) -> String {
    format!(
        "【材料登记】`{name}` 没能登记成材料，拿不到 assetId。\n\
         照片没有放上去：没有 assetId，`set_field` 写不进图片位。先把文件重新发一次。"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdfembed::fixtures::{pdf, pdf_with_pages, raw_image};
    use crate::tools::Outbox;
    use std::path::PathBuf;

    /// 一次抽图的工装：临时数据目录 + inbox 里那份 PDF
    struct Bench {
        root: PathBuf,
        stored: String,
        name: &'static str,
    }

    impl Bench {
        fn new(bytes: &[u8], name: &'static str) -> Self {
            let root = std::env::temp_dir().join(format!("sr-extract-{}", crate::session::uuid_like()));
            std::fs::create_dir_all(root.join("inbox")).unwrap();
            let stored =
                crate::snapshot::pool_name(&pool::sha256_hex(bytes), Some("pdf")).unwrap();
            std::fs::write(root.join("inbox").join(&stored), bytes).unwrap();
            Self {
                root,
                stored,
                name,
            }
        }

        fn args(&self) -> Value {
            json!({ "stored": self.stored, "name": self.name })
        }

        fn workspace(&self) -> Value {
            json!({ "people": [{ "id": "p1", "assets": [], "items": [], "resumes": [] }] })
        }
    }

    impl Drop for Bench {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// 走 `ToolCtx::of` 而不是字面量：以后 ToolCtx 加字段（比如 B2-4 的 facts），
    /// 这里不用跟着改
    fn ctx<'a>(bench: &'a Bench, workspace: &'a Value, out: &'a Outbox) -> ToolCtx<'a> {
        let mut ctx = ToolCtx::of(workspace, "p1", "r1", "pg1", out);
        ctx.root = Some(&bench.root);
        ctx
    }

    fn run(bench: &Bench, workspace: &Value, out: &Outbox) -> Result<String, String> {
        extract_images(&bench.args(), &ctx(bench, workspace, out))
    }

    fn rows(text: &str) -> Vec<&str> {
        text.lines().filter(|line| line.starts_with("- `")).collect()
    }

    #[test]
    fn no_bitmaps_says_the_photo_is_missing() {
        let bench = Bench::new(&pdf_with_pages(1), "纯文字.pdf");
        let ws = bench.workspace();
        let out = Outbox::default();
        let text = run(&bench, &ws, &out).unwrap();
        assert!(text.contains("没有内嵌位图"), "{text}");
        assert!(text.contains("照片没有放上去"), "{text}");
        assert!(out.assets.lock().unwrap().is_empty());
    }

    #[test]
    fn unreadable_images_say_how_many_and_why() {
        let jpx = |index: usize| {
            let mut stream = raw_image(8, 8, vec![index as u8; 8 * 8 * 3]);
            stream.dict.set("Filter", lopdf::Object::Name(b"JPXDecode".to_vec()));
            stream
        };
        let bench = Bench::new(
            &pdf(vec![("Im1", jpx(1)), ("Im2", jpx(2))], "/Im1 Do /Im2 Do"),
            "扫描件.pdf",
        );
        let ws = bench.workspace();
        let out = Outbox::default();
        let text = run(&bench, &ws, &out).unwrap();
        assert!(text.contains("2 张都读不出来"), "{text}");
        assert!(text.contains("编码 JPX 暂不支持"), "{text}");
        assert!(text.contains("照片没有放上去"), "{text}");

        let none = no_bitmaps("x.pdf");
        assert_ne!(text, none, "两类失败不能是同一句话");
        assert_ne!(text, open_failed("x.pdf"));
    }

    #[test]
    fn non_pdf_is_an_error_not_a_result() {
        let bench = Bench::new(b"whatever", "照片.png");
        let ws = bench.workspace();
        let out = Outbox::default();
        let ctx = ctx(&bench, &ws, &out);
        let err = extract_images(&json!({ "stored": "abc.png" }), &ctx).unwrap_err();
        assert!(err.contains("这不是 PDF"), "{err}");
    }

    #[test]
    fn broken_pdf_says_it_cannot_be_opened() {
        let bench = Bench::new(b"%PDF-1.4 garbage", "坏.pdf");
        let ws = bench.workspace();
        let out = Outbox::default();
        let text = run(&bench, &ws, &out).unwrap();
        assert!(text.contains("打不开（文件损坏或加密）"), "{text}");
        assert!(text.contains("照片没有放上去"), "{text}");
    }

    /// 没有数据目录 = 材料登记不了：必须说清「拿不到 assetId」
    #[test]
    fn no_data_dir_is_a_registration_failure() {
        let bench = Bench::new(&pdf_with_pages(1), "扫描件.pdf");
        let ws = bench.workspace();
        let out = Outbox::default();
        let ctx = ToolCtx::of(&ws, "p1", "r1", "pg1", &out);
        let text = extract_images(&bench.args(), &ctx).unwrap();
        assert!(text.contains("【材料登记】"), "{text}");
        assert!(text.contains("照片没有放上去"), "{text}");
    }

    /// F-001 / 8 张规则：第一次 8 张 + 说明，第二次把剩下的 3 张拿出来，id 不重复
    #[test]
    fn eleven_images_are_paged_without_any_offset_parameter() {
        let names = [
            "Im0", "Im1", "Im2", "Im3", "Im4", "Im5", "Im6", "Im7", "Im8", "Im9", "Im10",
        ];
        let mut images = Vec::new();
        let mut content = String::new();
        for (index, name) in names.iter().enumerate() {
            // 图像空间是单位正方形：cm 的缩放就是显示尺寸（pt）。
            // 190 → 90 递减，pct 从 10% 递减到 2%，升序排序后应从最后一张开始（小的先留）
            let mut pixels = vec![7u8; 300 * 300 * 3];
            pixels[0] = index as u8;
            images.push((*name, raw_image(300, 300, pixels)));
            let scale = 190.0 - 10.0 * index as f64;
            content.push_str(&format!("q {scale} 0 0 {scale} 0 0 cm /{name} Do Q "));
        }
        let bench = Bench::new(&pdf(images, &content), "扫描件.pdf");
        let ws = bench.workspace();

        let first = Outbox::default();
        let text = run(&bench, &ws, &first).unwrap();
        let first_assets = first.assets.lock().unwrap().clone();
        assert_eq!(first_assets.len(), 8, "一次最多 8 张");
        assert_eq!(rows(&text).len(), 8, "清单只列 8 行");
        assert!(text.contains("还有 3 张没列"), "{text}");
        assert!(text.contains("把剩下的也抽出来"), "{text}");

        // 清单按「占版心」升序排列
        let shares: Vec<u32> = rows(&text)
            .iter()
            .filter_map(|line| line.split("占版心 ").nth(1))
            .filter_map(|tail| tail.split('%').next())
            .filter_map(|value| value.parse().ok())
            .collect();
        assert_eq!(shares.len(), 8, "{text}");
        assert!(shares.windows(2).all(|pair| pair[0] <= pair[1]), "要升序：{shares:?}");

        // 把首批塞进工作区（前端 addAssets 落库后的样子），再调一次
        let mut ws2 = bench.workspace();
        let list: Vec<Value> = first_assets
            .iter()
            .map(|asset| serde_json::to_value(asset).unwrap())
            .collect();
        ws2["people"][0]["assets"] = Value::Array(list);

        let second = Outbox::default();
        let text = run(&bench, &ws2, &second).unwrap();
        let second_assets = second.assets.lock().unwrap().clone();
        assert_eq!(second_assets.len(), 3, "第二批只剩 3 张：{text}");
        assert_eq!(rows(&text).len(), 3, "{text}");
        let first_ids: Vec<&str> = first_assets.iter().map(|asset| asset.id.as_str()).collect();
        for asset in &second_assets {
            assert!(!first_ids.contains(&asset.id.as_str()), "id 不能重复：{}", asset.id);
        }
        assert!(second_assets.iter().all(|asset| {
            // source 是**原始文件名**（人类可读），不是源 PDF 的 assetId —— 源附件不进
            // person.assets，写 id 会是悬空引用
            asset.extra.get("role").and_then(Value::as_str) == Some("candidate")
                && asset.extra.get("source").and_then(Value::as_str) == Some("扫描件.pdf")
                && asset.extra.get("pageIndex").and_then(Value::as_u64) == Some(1)
        }));
    }

    /// 幂等：同一份内容连调两次，第二次一条都不重复落池
    #[test]
    fn the_same_file_twice_registers_nothing_new() {
        let mut pixels = vec![3u8; 60 * 60 * 3];
        pixels[1] = 1;
        let bench = Bench::new(&pdf(vec![("Im1", raw_image(60, 60, pixels))], "/Im1 Do"), "一份.pdf");
        let ws = bench.workspace();

        let first = Outbox::default();
        let text = run(&bench, &ws, &first).unwrap();
        assert_eq!(first.assets.lock().unwrap().len(), 1, "{text}");

        // 第二次：出站口与工作区都还是空的话，内容寻址也要认出来（这里把首批放进工作区）
        let mut ws2 = bench.workspace();
        ws2["people"][0]["assets"] = Value::Array(
            first
                .assets
                .lock()
                .unwrap()
                .iter()
                .map(|asset| serde_json::to_value(asset).unwrap())
                .collect(),
        );
        let second = Outbox::default();
        let text = run(&bench, &ws2, &second).unwrap();
        assert!(second.assets.lock().unwrap().is_empty(), "同一张图不该落两次：{text}");
        assert!(text.contains("张内嵌位图"), "{text}");
    }

    #[test]
    fn rows_carry_pct_or_say_it_is_unmeasurable() {
        let mut pixels = vec![5u8; 40 * 40 * 3];
        pixels[2] = 9;
        let mut other = vec![6u8; 40 * 40 * 3];
        other[2] = 8;
        let bench = Bench::new(
            &pdf(
                vec![("Im1", raw_image(40, 40, pixels)), ("Im2", raw_image(40, 40, other))],
                // 只画第一张：第二张的显示尺寸不可测
                // 87pt 见方 ≈ 占版心 2%
                "q 87 0 0 87 0 0 cm /Im1 Do Q",
            ),
            "两张.pdf",
        );
        let ws = bench.workspace();
        let out = Outbox::default();
        let text = run(&bench, &ws, &out).unwrap();
        assert_eq!(rows(&text).len(), 2, "{text}");
        assert!(text.contains("占版心 不可测"), "{text}");
        assert!(text.contains("占版心 2%"), "{text}");
        assert!(text.contains("｜不透明｜"), "{text}");

        // 落盘的文件必须是真的 PNG，而且 id 就是池内存储名
        for asset in out.assets.lock().unwrap().iter() {
            let path = bench.root.join("pool").join(&asset.stored);
            let bytes = std::fs::read(&path).expect("池里要有文件");
            let image = image::load_from_memory(&bytes).expect("要能解出图");
            assert_eq!(image.width(), 40);
            assert_eq!(asset.kind, "image");
        }
    }

    /// 本机真实样本：抽得出图、尺寸合理、占比在 1–30%、落盘文件能解码。
    /// 样本不存在就 skip（不把绝对路径写进断言，也不让 CI 红）
    #[test]
    fn real_sample_pdf_yields_usable_photos() {
        let Some(bytes) = real_sample() else {
            eprintln!("skipped: 本机没有可用的 PDF 样本（设 SHARP_RESUME_SAMPLE_PDF 指一份）");
            return;
        };
        let bench = Bench::new(&bytes, "华北电力大学 谭轶天.pdf");
        let ws = bench.workspace();
        let out = Outbox::default();
        let started = std::time::Instant::now();
        let text = run(&bench, &ws, &out).unwrap();
        let elapsed = started.elapsed();

        let assets = out.assets.lock().unwrap().clone();
        assert!(!assets.is_empty(), "真实简历里应当能抽到至少一张图：{text}");
        for asset in &assets {
            let path = bench.root.join("pool").join(&asset.stored);
            let bytes = std::fs::read(&path).expect("池里要有文件");
            let image = image::load_from_memory(&bytes).expect("落盘的文件要能解码");
            assert!(image.width() > 20 && image.height() > 20, "尺寸要合理");
            assert_eq!(asset.kind, "image");
        }
        let shares: Vec<u32> = rows(&text)
            .iter()
            .filter_map(|line| line.split("占版心 ").nth(1))
            .filter_map(|tail| tail.split('%').next())
            .filter_map(|value| value.parse().ok())
            .collect();
        println!(
            "真实样本：抽出 {} 张，占比 {shares:?}，耗时 {:?}\n{text}",
            assets.len(),
            elapsed
        );
        assert!(
            shares.iter().any(|value| (1..=30).contains(value)),
            "证件照的占比应落在 1–30%：{shares:?}"
        );
    }

    fn real_sample() -> Option<Vec<u8>> {
        if let Some(path) = option_env!("SHARP_RESUME_SAMPLE_PDF") {
            if let Ok(bytes) = std::fs::read(path) {
                return Some(bytes);
            }
        }
        for path in [
            "E:/文档/实习/华北电力大学 谭轶天.pdf",
            "E:/文档/实习/谭轶天简历.pdf",
        ] {
            if let Ok(bytes) = std::fs::read(path) {
                return Some(bytes);
            }
        }
        None
    }
}
