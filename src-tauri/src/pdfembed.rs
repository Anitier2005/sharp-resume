//! PDF 内嵌位图抽取：自己遍历 XObject，不依赖页面渲染。
//!
//! 为什么不用 `Document::get_page_images`：页没有自身 `/Resources` 就整页 Err、
//! XObject 是内联字典也 Err、不递归 Form、content 未解码、没有 SMask。抽图这件事
//! 要的是「尽量多拿到一张照片」，对这种残缺文档只能自己走一遍。
//!
//! 两条硬边界：
//!   1. 只认 XObject 里的 `Image`。内联图（`BI … ID … EI`）不支持，记 `unsupported`。
//!   2. **任何路径都不 panic**：解不出的编码记一条 `unsupported` 原因继续下一张。
//!      一张怪图不该把整轮对话毁掉。

use std::collections::{HashMap, HashSet};

use lopdf::content::Content;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};

/// 打印 `@page` 的 16mm 边距。**只服务「接近整页」的面积换算** ——
/// 与预览实测的 34px 不是一回事（一个量原件印出来多大，一个量 WebView 里多高）。
const PAGE_PAD_MM: f64 = 16.0;
const MM_TO_PT: f64 = 72.0 / 25.4;
/// MediaBox 读不出时的兜底：A4（pt）
const A4: (f64, f64) = (595.28, 841.89);
/// 单张边长上限：再大就不要了，防一张 20000px 的图把内存打爆
const MAX_SIDE: u32 = 4000;
/// Form 嵌套上限：自引用的坏文档在这里停手
const MAX_DEPTH: u8 = 8;
/// MediaBox 沿 `/Parent` 上溯的上限
const MAX_PARENT: u8 = 32;
/// 「接近整页」的判定线：面积占版心 ≥ 80%
pub const NEAR_PAGE_PCT: u32 = 80;

pub struct Embedded {
    pub width: u32,
    pub height: u32,
    /// 已解码的位图（RGBA8，可直接写 PNG）。`codec == "jpg"` 时是空的
    pub rgba: Vec<u8>,
    /// JPEG 原字节。DCTDecode 的流本身就是完整文件，**不重编码**（省一次有损转换，
    /// 也省掉把 4MP 照片压成 1.6MB PNG 的那两秒）。`codec == "jpg"` 时非空
    pub jpeg: Option<Vec<u8>>,
    /// 来源页序，1 基
    pub page: u32,
    /// 页面上的显示尺寸（pt）。None = content stream 里对不上，不可测
    pub disp: Option<(f64, f64)>,
    pub alpha: bool,
    /// 原始编码，仅供文案（jpg / png / raw）
    pub codec: &'static str,
}

pub struct Report {
    /// 按来源页序、页内出现顺序
    pub images: Vec<Embedded>,
    /// 每张解不出的图一条原因
    pub unsupported: Vec<&'static str>,
    /// 这份 PDF 有几页。原件页数走 `page_count`（更省），这个字段留给排查
    /// 「几页 / 抽到几张」时对账用
    #[allow(dead_code)]
    pub pages: usize,
    /// 版心面积（pt²），供算占比
    pub inner_area: f64,
}

impl Report {
    /// 这张图占版心百分之几。`disp` 不可测时 None（文案写「占版心 不可测」）。
    pub fn pct_of(&self, image: &Embedded) -> Option<u32> {
        let (w, h) = image.disp?;
        if self.inner_area <= 0.0 {
            return None;
        }
        Some((100.0 * w * h / self.inner_area).round().max(0.0) as u32)
    }
}

/// 抽图。`Err("open")` = 文件打不开（损坏 / 加密 / 不是 PDF）；
/// `Ok(report)` 里 `images` 与 `unsupported` 都空 = 没有内嵌位图。
pub fn extract(bytes: &[u8]) -> Result<Report, String> {
    let mut doc = Document::load_mem(bytes).map_err(|_| "open".to_string())?;
    if doc.is_encrypted() && doc.decrypt("").is_err() {
        return Err("open".into());
    }

    let pages = doc.get_pages();
    let mut report = Report {
        images: Vec::new(),
        unsupported: Vec::new(),
        pages: pages.len(),
        inner_area: inner_area_of(A4),
    };
    let mut seen: HashSet<ObjectId> = HashSet::new();

    for (page_no, page_id) in &pages {
        let media = page_media_box(&doc, *page_id);
        report.inner_area = inner_area_of(media);
        let (contexts, inline) = display_sizes(&doc, *page_id);
        if inline {
            report.unsupported.push("inline");
        }
        // 必须走 get_page_resources：只有它沿 /Parent 上溯，页自己没有 /Resources 时
        // 图全在父节点上（生成的 PDF 里极常见）
        let Ok((own, chain)) = doc.get_page_resources(*page_id) else {
            continue;
        };
        let resources = pick_resources(&doc, own, &chain);
        walk(&doc, resources, &contexts, *page_no, &mut report, &mut seen, 0);
    }
    Ok(report)
}

/// 页数：读不出返回 `None`，**绝不返回 0**（0 会被当成「原件是空文档」）。
pub fn page_count(bytes: &[u8]) -> Option<u32> {
    let doc = Document::load_mem(bytes).ok()?;
    // 加密文档的页树不可信，按「读不出」处理，不猜
    if doc.is_encrypted() {
        return None;
    }
    match doc.get_pages().len() {
        0 => None,
        n => Some(n as u32),
    }
}

/* ---------- Resources 与遍历 ---------- */

/// 页自己没挂 /Resources 时，从继承链里挑第一个真有 XObject 的字典
fn pick_resources<'a>(
    doc: &'a Document,
    own: Option<&'a Dictionary>,
    chain: &[ObjectId],
) -> Option<&'a Dictionary> {
    let mut candidates: Vec<&Dictionary> = Vec::new();
    if let Some(dict) = own {
        candidates.push(dict);
    }
    for id in chain {
        if let Ok(dict) = doc.get_dictionary(*id) {
            candidates.push(dict);
        }
    }
    candidates.into_iter().find(|dict| dict.has(b"XObject"))
}

fn walk<'a>(
    doc: &'a Document,
    resources: Option<&'a Dictionary>,
    contexts: &HashMap<String, (f64, f64)>,
    page: u32,
    report: &mut Report,
    seen: &mut HashSet<ObjectId>,
    depth: u8,
) {
    if depth > MAX_DEPTH {
        return;
    }
    let Some(xobjects) = resources
        .and_then(|dict| dict.get(b"XObject").ok())
        .and_then(|obj| as_dict(doc, obj))
    else {
        return;
    };

    for (name, value) in xobjects.iter() {
        let Ok(id) = value.as_reference() else {
            // 内联字典（直接把流写在 Resources 里）与内联图一样不处理
            report.unsupported.push("inline");
            continue;
        };
        // 同一个 XObject 被多处复用（水印）只算一次
        if !seen.insert(id) {
            continue;
        }
        let Ok(stream) = doc.get_object(id).and_then(Object::as_stream) else {
            continue;
        };
        let subtype = stream
            .dict
            .get(b"Subtype")
            .and_then(Object::as_name_str)
            .unwrap_or_default();

        match subtype {
            "Form" => {
                // Form 的 /Resources 可能为空 → 继承调用方的字典
                let sub = stream
                    .dict
                    .get(b"Resources")
                    .ok()
                    .and_then(|obj| as_dict(doc, obj))
                    .or(resources);
                walk(doc, sub, contexts, page, report, seen, depth + 1);
            }
            "Image" => match decode_image(doc, stream) {
                Ok(mut image) => {
                    image.page = page;
                    let key = String::from_utf8_lossy(name).to_string();
                    // 图像空间是单位正方形，所以显示尺寸就是 hypot(a,b)、hypot(c,d)：
                    // 不能再乘像素尺寸（那样本机真实样本里 34×51mm 的 2 寸照片会算成 1.1 公里宽）
                    image.disp = contexts.get(&key).copied();
                    report.images.push(image);
                }
                Err(reason) => report.unsupported.push(reason),
            },
            _ => {}
        }
    }
}

/* ---------- 页面几何：MediaBox 与 CTM ---------- */

fn page_media_box(doc: &Document, page_id: ObjectId) -> (f64, f64) {
    let mut id = page_id;
    for _ in 0..MAX_PARENT {
        let Ok(dict) = doc.get_dictionary(id) else {
            break;
        };
        if let Some(values) = dict.get(b"MediaBox").ok().and_then(|obj| numbers(doc, obj)) {
            if values.len() >= 4 {
                let (w, h) = ((values[2] - values[0]).abs(), (values[3] - values[1]).abs());
                if w > 1.0 && h > 1.0 {
                    return (w, h);
                }
            }
        }
        match dict.get(b"Parent").and_then(Object::as_reference) {
            Ok(parent) => id = parent,
            Err(_) => break,
        }
    }
    A4
}

fn inner_area_of(media: (f64, f64)) -> f64 {
    let pad = PAGE_PAD_MM * MM_TO_PT;
    let w = (media.0 - 2.0 * pad).max(1.0);
    let h = (media.1 - 2.0 * pad).max(1.0);
    w * h
}

/// 图像 XObject 名 → 它在页面上的单位缩放 `(hypot(a,b), hypot(c,d))`。
///
/// 做最小图形状态机：只有 q / Q / cm / Do 参与，其它操作符一律跳过。
/// 返回的第二项表示这一页有没有内联图（`BI`）。
fn display_sizes(doc: &Document, page_id: ObjectId) -> (HashMap<String, (f64, f64)>, bool) {
    let mut sizes: HashMap<String, (f64, f64)> = HashMap::new();
    let mut inline = false;
    let Ok(data) = doc.get_page_content(page_id) else {
        return (sizes, inline);
    };
    let Ok(content) = Content::decode(&data) else {
        // content 解不开 = 拿不到 CTM，占比按「不可测」处理，不 panic
        return (sizes, inline);
    };

    let mut ctm = IDENTITY;
    let mut stack: Vec<[f64; 6]> = Vec::new();
    for op in &content.operations {
        match op.operator.as_str() {
            "q" => stack.push(ctm),
            "Q" => {
                if let Some(prev) = stack.pop() {
                    ctm = prev;
                }
            }
            "cm" => {
                if let Some(matrix) = matrix_of(&op.operands) {
                    // 行向量约定：先应用 cm、再应用当前 CTM
                    ctm = concat(matrix, ctm);
                }
            }
            "Do" => {
                if let Some(Object::Name(name)) = op.operands.first() {
                    let key = String::from_utf8_lossy(name).to_string();
                    // 同名多次出现取第一次：Doc 里重复画同一张图不影响尺寸判断
                    sizes.entry(key).or_insert_with(|| disp_of(ctm));
                }
            }
            "BI" => inline = true,
            _ => {}
        }
    }
    (sizes, inline)
}

type Matrix = [f64; 6];
const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// PDF 的行向量约定：先 m 后 n 等于 m × n
fn concat(m: Matrix, n: Matrix) -> Matrix {
    [
        m[0] * n[0] + m[1] * n[2],
        m[0] * n[1] + m[1] * n[3],
        m[2] * n[0] + m[3] * n[2],
        m[2] * n[1] + m[3] * n[3],
        m[4] * n[0] + m[5] * n[2] + n[4],
        m[4] * n[1] + m[5] * n[3] + n[5],
    ]
}

/// 显示尺寸（pt），忽略斜切：hypot(a,b) 与 hypot(c,d)
fn disp_of(m: Matrix) -> (f64, f64) {
    (m[0].hypot(m[1]), m[2].hypot(m[3]))
}

fn matrix_of(operands: &[Object]) -> Option<Matrix> {
    if operands.len() < 6 {
        return None;
    }
    let mut out = [0.0f64; 6];
    for (slot, operand) in out.iter_mut().zip(operands.iter()) {
        *slot = number_of(operand)?;
    }
    Some(out)
}

/* ---------- 解码 ---------- */

fn decode_image(doc: &Document, stream: &Stream) -> Result<Embedded, &'static str> {
    let dict = &stream.dict;
    let width = dict.get(b"Width").ok().and_then(number_of).ok_or("filters")?;
    let height = dict.get(b"Height").ok().and_then(number_of).ok_or("filters")?;
    if width < 1.0 || height < 1.0 {
        return Err("filters");
    }
    let (width, height) = (width as u32, height as u32);
    if width > MAX_SIDE || height > MAX_SIDE {
        return Err("too-large");
    }

    let filters = stream.filters().unwrap_or_default();
    if filters.len() > 1 {
        // 多段 filter（如 [FlateDecode DCTDecode]）罕见，不猜
        return Err("filters");
    }

    match filters.first().map(String::as_str) {
        Some("DCTDecode") | Some("DCT") => Ok(Embedded {
            width,
            height,
            rgba: Vec::new(),
            // 流里就是完整 JPEG：原样交出去，不解码也不重编码
            jpeg: Some(stream.content.clone()),
            page: 0,
            disp: None,
            alpha: false,
            codec: "jpg",
        }),
        Some("JPXDecode") => Err("JPX"),
        Some("CCITTFaxDecode") | Some("CCF") => Err("CCITT"),
        Some("JBIG2Decode") => Err("JBIG2"),
        _ => {
            let raw = probe_decompress(stream)?;
            let space = parse_colorspace(doc, dict).ok_or("colorspace")?;
            let bpc = dict.get(b"BitsPerComponent").ok().and_then(number_of).unwrap_or(8.0) as u8;
            let invert = is_inverted(dict);
            let mut rgba = to_rgba(&raw, width, height, bpc, &space, invert)?;
            let pixels = (width as usize) * (height as usize);
            if rgba.len() != pixels * 4 {
                return Err("filters");
            }
            let mut alpha = false;
            if let Some(mask) = smask_alpha(doc, dict, pixels) {
                for index in 0..pixels {
                    rgba[index * 4 + 3] = mask[index];
                }
                alpha = true;
            }
            Ok(Embedded {
                width,
                height,
                rgba,
                jpeg: None,
                page: 0,
                disp: None,
                alpha,
                codec: if filters.is_empty() { "raw" } else { "png" },
            })
        }
    }
}

fn probe_decompress(stream: &Stream) -> Result<Vec<u8>, &'static str> {
    if stream.filters().unwrap_or_default().is_empty() {
        return Ok(stream.content.clone());
    }
    // 绕过 lopdf 对图像流的拒解（decompressed_content 见到 Subtype=="Image" 直接 Err）。
    // probe 是局部克隆，文档对象没被改。不这么写就得自己实现 5 种 PNG predictor，
    // 那是静默花屏的高发区。
    let mut probe = stream.clone();
    probe.dict.remove(b"Subtype");
    probe.decompressed_content().map_err(|_| "filters")
}

/// `/Decode [1 0]` 的灰度反相：只处理这一种，其它（CMYK 反相）忽略
fn is_inverted(dict: &Dictionary) -> bool {
    let Ok(Object::Array(items)) = dict.get(b"Decode") else {
        return false;
    };
    let values: Vec<f64> = items.iter().filter_map(number_of).collect();
    values.len() >= 2 && values[0] > values[1]
}

fn parse_colorspace(doc: &Document, dict: &Dictionary) -> Option<ColorSpace> {
    let obj = dict.get(b"ColorSpace").ok()?;
    space_of(doc, obj)
}

fn space_of(doc: &Document, obj: &Object) -> Option<ColorSpace> {
    match obj {
        Object::Reference(id) => space_of(doc, doc.get_object(*id).ok()?),
        Object::Name(name) => named_space(name),
        Object::Array(items) => {
            let head = items.first()?;
            let name = match head {
                Object::Name(raw) => String::from_utf8_lossy(raw).to_string(),
                Object::Reference(id) => doc.get_object(*id).ok()?.as_name_str().ok()?.to_string(),
                _ => return None,
            };
            match name.as_str() {
                "ICCBased" => {
                    let stream = match items.get(1)? {
                        Object::Reference(id) => doc.get_object(*id).ok()?.as_stream().ok()?,
                        Object::Stream(stream) => stream,
                        _ => return None,
                    };
                    match stream.dict.get(b"N").ok().and_then(number_of)? as i64 {
                        1 => Some(ColorSpace::Gray),
                        3 => Some(ColorSpace::Rgb),
                        4 => Some(ColorSpace::Cmyk),
                        _ => None,
                    }
                }
                "Indexed" | "I" => {
                    let base = space_of(doc, items.get(1)?)?;
                    let palette = match items.get(3)? {
                        Object::String(bytes, _) => bytes.clone(),
                        Object::Reference(id) => {
                            let stream = doc.get_object(*id).ok()?.as_stream().ok()?;
                            stream.decompressed_content().ok()?
                        }
                        _ => return None,
                    };
                    Some(ColorSpace::Indexed {
                        base: Box::new(base),
                        palette,
                    })
                }
                other => named_space(other.as_bytes()),
            }
        }
        _ => None,
    }
}

fn named_space(name: &[u8]) -> Option<ColorSpace> {
    match name {
        b"DeviceGray" | b"CalGray" | b"G" => Some(ColorSpace::Gray),
        b"DeviceRGB" | b"CalRGB" | b"RGB" => Some(ColorSpace::Rgb),
        b"DeviceCMYK" | b"CMYK" => Some(ColorSpace::Cmyk),
        // Separation / DeviceN / Lab 没有对应的朴素解，按不支持处理
        _ => None,
    }
}

enum ColorSpace {
    Gray,
    Rgb,
    Cmyk,
    Indexed { base: Box<ColorSpace>, palette: Vec<u8> },
}

impl ColorSpace {
    fn components(&self) -> Option<usize> {
        match self {
            ColorSpace::Gray => Some(1),
            ColorSpace::Rgb => Some(3),
            ColorSpace::Cmyk => Some(4),
            ColorSpace::Indexed { .. } => None,
        }
    }
}

fn to_rgba(
    raw: &[u8],
    width: u32,
    height: u32,
    bpc: u8,
    space: &ColorSpace,
    invert: bool,
) -> Result<Vec<u8>, &'static str> {
    let pixels = (width as usize) * (height as usize);
    let mut out = Vec::with_capacity(pixels * 4);
    match space {
        ColorSpace::Indexed { base, palette } => {
            let indexes = samples(raw, pixels, bpc)?;
            let components = base.components().ok_or("colorspace")?;
            for index in indexes {
                let at = index as usize * components;
                let chunk = palette.get(at..at + components).ok_or("colorspace")?;
                push_pixel(&mut out, chunk, base)?;
            }
        }
        ColorSpace::Gray => {
            for value in samples(raw, pixels, bpc)? {
                let value = if invert { 255 - value } else { value };
                out.extend_from_slice(&[value, value, value, 255]);
            }
        }
        ColorSpace::Rgb => {
            for pixel in samples(raw, pixels * 3, bpc)?.chunks_exact(3) {
                out.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
            }
        }
        ColorSpace::Cmyk => {
            for pixel in samples(raw, pixels * 4, bpc)?.chunks_exact(4) {
                push_pixel(&mut out, pixel, space)?;
            }
        }
    }
    Ok(out)
}

fn push_pixel(out: &mut Vec<u8>, parts: &[u8], space: &ColorSpace) -> Result<(), &'static str> {
    match space {
        ColorSpace::Gray => out.extend_from_slice(&[parts[0], parts[0], parts[0], 255]),
        ColorSpace::Rgb => out.extend_from_slice(&[parts[0], parts[1], parts[2], 255]),
        ColorSpace::Cmyk => {
            // 乘积式：比减法式更接近印刷观感
            let k = 255 - parts[3] as u32;
            let channel = |value: u8| ((255 - value as u32) * k / 255) as u8;
            out.extend_from_slice(&[channel(parts[0]), channel(parts[1]), channel(parts[2]), 255]);
        }
        ColorSpace::Indexed { .. } => return Err("colorspace"),
    }
    Ok(())
}

/// 按 `/BitsPerComponent` 把样本展成 8 位
fn samples(raw: &[u8], count: usize, bpc: u8) -> Result<Vec<u8>, &'static str> {
    match bpc {
        8 => {
            if raw.len() < count {
                return Err("bpc");
            }
            Ok(raw[..count].to_vec())
        }
        // PDF 里 16 位图像的实际有效位数通常就是 8，取高字节
        16 => {
            if raw.len() < count * 2 {
                return Err("bpc");
            }
            Ok(raw.chunks_exact(2).take(count).map(|pair| pair[0]).collect())
        }
        1 | 2 | 4 => {
            let per_byte = (8 / bpc) as usize;
            let needed = count.div_ceil(per_byte);
            if raw.len() < needed {
                return Err("bpc");
            }
            let max = ((1u32 << bpc) - 1) as u32;
            let mask = max as u8;
            let mut out = Vec::with_capacity(count);
            for byte in raw.iter().take(needed) {
                for slot in 0..per_byte {
                    if out.len() == count {
                        break;
                    }
                    let shift = 8 - bpc as usize * (slot + 1);
                    let value = ((byte >> shift) & mask) as u32;
                    out.push(((value * 255 + max / 2) / max) as u8);
                }
            }
            Ok(out)
        }
        _ => Err("bpc"),
    }
}

/// `/SMask` 只支持 8 位灰度；解不出就丢掉 alpha，图照样产出
fn smask_alpha(doc: &Document, dict: &Dictionary, pixels: usize) -> Option<Vec<u8>> {
    let obj = dict.get(b"SMask").ok()?;
    let stream = match obj {
        Object::Reference(id) => doc.get_object(*id).ok()?.as_stream().ok()?,
        Object::Stream(stream) => stream,
        _ => return None,
    };
    let bpc = stream.dict.get(b"BitsPerComponent").ok().and_then(number_of).unwrap_or(8.0);
    if bpc as u8 != 8 {
        return None;
    }
    let raw = probe_decompress(stream).ok()?;
    (raw.len() >= pixels).then(|| raw[..pixels].to_vec())
}

/* ---------- 小工具 ---------- */

fn as_dict<'a>(doc: &'a Document, obj: &'a Object) -> Option<&'a Dictionary> {
    match obj {
        Object::Dictionary(dict) => Some(dict),
        Object::Reference(id) => doc.get_object(*id).ok()?.as_dict().ok(),
        _ => None,
    }
}

fn numbers(doc: &Document, obj: &Object) -> Option<Vec<f64>> {
    match obj {
        Object::Array(items) => Some(items.iter().filter_map(number_of).collect()),
        Object::Reference(id) => numbers(doc, doc.get_object(*id).ok()?),
        _ => None,
    }
}

fn number_of(obj: &Object) -> Option<f64> {
    match obj {
        Object::Integer(value) => Some(*value as f64),
        Object::Real(value) => Some(*value as f64),
        _ => None,
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    use lopdf::{dictionary, Document, Object, Stream};

    /// 造一份最小 PDF：第 1 页的 Resources 挂 `images`，content 是 `content`。
    /// 其余页是空页（只用来看页数）。
    pub fn pdf(images: Vec<(&str, Stream)>, content: &str) -> Vec<u8> {
        build(images, content, 1, None)
    }

    /// 未压缩的 RGB 8bpc 图像流（测试里最省事的一种）
    pub fn raw_image(width: i64, height: i64, pixels: Vec<u8>) -> Stream {
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => Object::Integer(width),
                "Height" => Object::Integer(height),
                "ColorSpace" => Object::Name(b"DeviceRGB".to_vec()),
                "BitsPerComponent" => Object::Integer(8),
            },
            pixels,
        )
    }

    pub fn pdf_with_pages(pages: u32) -> Vec<u8> {
        build(Vec::new(), "", pages, None)
    }

    /// 带**文字层**的最小 PDF：每行一句，Helvetica 12pt。
    ///
    /// 存在的理由：`pdf_with_pages` 造的页没有文字，验不了「正文投影」那条链路
    /// （摘要、指针、Fact 层落库都需要真能抽出字来）。
    pub fn pdf_with_text(lines: &[&str]) -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
            "Encoding" => "WinAnsiEncoding",
        });
        let mut fonts = lopdf::Dictionary::new();
        fonts.set("F1", Object::Reference(font));
        let resources = doc.add_object(dictionary! {
            "Font" => Object::Dictionary(fonts),
        });
        let mut content = String::from("BT /F1 12 Tf 40 800 Td 16 TL\n");
        for line in lines {
            // 括号与反斜杠在 PDF 字符串里是语法字符，测试文案里不出现就等于无事
            content.push_str(&format!("({}) Tj T*\n", line.replace('\\', " ").replace(['(', ')'], " ")));
        }
        content.push_str("ET");
        let contents = doc.add_object(Stream::new(
            lopdf::Dictionary::new(),
            content.into_bytes(),
        ));
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => Object::Reference(pages_id),
            "MediaBox" => vec![
                Object::Integer(0),
                Object::Integer(0),
                Object::Real(595.28),
                Object::Real(841.89),
            ],
            "Resources" => Object::Reference(resources),
            "Contents" => Object::Reference(contents),
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => Object::Array(vec![Object::Reference(page)]),
                "Count" => Object::Integer(1),
            }),
        );
        let catalog = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => Object::Reference(pages_id),
        });
        doc.trailer.set("Root", Object::Reference(catalog));
        let mut out = Vec::new();
        doc.save_to(&mut out).expect("测试用 PDF 要能写出");
        out
    }

    /// 挂着 /Encrypt 但空口令解不开的文档（lopdf 没有加密 API，手工塞一个坏字典）
    pub fn encrypted_stub() -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let resources = doc.add_object(dictionary! {});
        let contents = doc.add_object(Stream::new(lopdf::Dictionary::new(), Vec::new()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => Object::Reference(pages_id),
            "MediaBox" => vec![
                Object::Integer(0),
                Object::Integer(0),
                Object::Real(595.28),
                Object::Real(841.89),
            ],
            "Resources" => Object::Reference(resources),
            "Contents" => Object::Reference(contents),
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => Object::Array(vec![Object::Reference(page)]),
                "Count" => Object::Integer(1),
            }),
        );
        let catalog = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => Object::Reference(pages_id),
        });
        doc.trailer.set("Root", Object::Reference(catalog));
        // /Encrypt 必须是引用（is_encrypted 只认引用），否则 lopdf 会当没加密
        let encrypt = doc.add_object(dictionary! {
            "Filter" => Object::Name(b"Standard".to_vec()),
            "V" => Object::Integer(1),
            "R" => Object::Integer(2),
            "O" => Object::String(vec![0x11; 32], lopdf::StringFormat::Literal),
            "U" => Object::String(vec![0x22; 32], lopdf::StringFormat::Literal),
            "P" => Object::Integer(-1),
        });
        doc.trailer.set("Encrypt", Object::Reference(encrypt));
        let mut out = Vec::new();
        doc.save_to(&mut out).expect("测试用 PDF 要能写出");
        out
    }

    /// 只有一个图像、且挂了 /SMask（走引用 —— 内联流是非法 PDF）
    pub fn pdf_masked(image: Stream, mask: Stream, content: &str) -> Vec<u8> {
        build(vec![("Im1", image)], content, 1, Some(mask))
    }

    fn build(
        images: Vec<(&str, Stream)>,
        content: &str,
        page_count: u32,
        mask: Option<Stream>,
    ) -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let mask_id = mask.map(|stream| doc.add_object(Object::Stream(stream)));
        let mut kids = Vec::new();

        for index in 0..page_count.max(1) {
            let mut xobjects = lopdf::Dictionary::new();
            if index == 0 {
                for (name, stream) in &images {
                    let mut stream = stream.clone();
                    if let Some(mask_id) = mask_id {
                        stream.dict.set("SMask", Object::Reference(mask_id));
                    }
                    let id = doc.add_object(Object::Stream(stream));
                    xobjects.set(name.as_bytes(), Object::Reference(id));
                }
            }
            let resources = doc.add_object(dictionary! {
                "XObject" => Object::Dictionary(xobjects),
            });
            let body = if index == 0 { content } else { "" };
            let contents = doc.add_object(Stream::new(lopdf::Dictionary::new(), body.as_bytes().to_vec()));
            let page = doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => Object::Reference(pages_id),
                "MediaBox" => vec![
                    Object::Integer(0),
                    Object::Integer(0),
                    Object::Real(595.28),
                    Object::Real(841.89),
                ],
                "Resources" => Object::Reference(resources),
                "Contents" => Object::Reference(contents),
            });
            kids.push(Object::Reference(page));
        }

        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => Object::Array(kids),
                "Count" => Object::Integer(page_count.max(1) as i64),
            }),
        );
        let catalog = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => Object::Reference(pages_id),
        });
        doc.trailer.set("Root", Object::Reference(catalog));

        let mut out = Vec::new();
        doc.save_to(&mut out).expect("测试用 PDF 要能写出");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;
    use lopdf::{dictionary, Object};

    /// `dictionary!` 的 value 是 `Into<Object>`，裸字符串的 `into()` 会歧义，
    /// 统一走这个显式构造
    fn name(value: &str) -> Object {
        Object::Name(value.as_bytes().to_vec())
    }

    /// 未压缩的原始像素流（PDF 允许不带 Filter）
    fn raw(width: i64, height: i64, space: Object, bpc: i64, pixels: Vec<u8>) -> Stream {
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => Object::Integer(width),
                "Height" => Object::Integer(height),
                "ColorSpace" => space,
                "BitsPerComponent" => Object::Integer(bpc),
            },
            pixels,
        )
    }

    /// 带 FlateDecode 的流：靠 lopdf 自己的压缩，数据要够大它才肯压
    fn flate(dict: lopdf::Dictionary, pixels: Vec<u8>) -> Stream {
        let mut stream = Stream::new(dict, pixels);
        stream.compress().expect("压缩");
        stream
    }

    fn jpeg(width: u32, height: u32) -> Vec<u8> {
        let image = image::DynamicImage::new_rgb8(width, height);
        let mut out = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Jpeg)
            .expect("造 jpeg");
        out
    }

    /// F-003：证件照的占比来自 CTM，不是「1px = 1pt」
    #[test]
    fn display_size_comes_from_the_ctm() {
        let image = raw(413, 531, name("DeviceRGB"), 8, vec![200; 413 * 531 * 3]);
        // 5.5×6.9cm 的证件照 = 154.83×196.20pt。图像空间是单位正方形，
        // 所以 cm 直接就是显示尺寸
        let bytes = pdf(
            vec![("Im1", image)],
            "q 154.83 0 0 196.20 10 20 cm /Im1 Do Q",
        );
        let report = extract(&bytes).expect("能抽出来");
        assert_eq!(report.images.len(), 1);
        let (w, h) = report.images[0].disp.expect("有 Do 就有尺寸");
        assert!((w - 154.83).abs() < 0.1, "宽 {w}");
        assert!((h - 196.20).abs() < 0.1, "高 {h}");
        assert_eq!(report.pct_of(&report.images[0]), Some(8));
        assert!((report.inner_area - 379_055.0).abs() < 100.0, "版心 {}", report.inner_area);
    }

    /// cm 铺满版心 → 接近整页
    #[test]
    fn a_full_bleed_image_is_near_the_page() {
        let image = raw(100, 100, name("DeviceRGB"), 8, vec![10; 100 * 100 * 3]);
        let bytes = pdf(vec![("Im1", image)], "504.57 0 0 751.18 0 0 cm /Im1 Do");
        let report = extract(&bytes).expect("能抽出来");
        let pct = report.pct_of(&report.images[0]).expect("可测");
        assert!((pct as i64 - 100).abs() <= 1, "pct {pct}");
        assert!(pct >= NEAR_PAGE_PCT);
    }

    /// 只写在 Resources 里没画出来：尺寸不可测，但不能 panic
    #[test]
    fn an_unpainted_xobject_has_no_display_size() {
        let image = raw(40, 40, name("DeviceGray"), 8, vec![7; 40 * 40]);
        let bytes = pdf(vec![("Im1", image)], "q Q");
        let report = extract(&bytes).expect("能抽出来");
        assert_eq!(report.images.len(), 1);
        assert!(report.images[0].disp.is_none());
        assert_eq!(report.pct_of(&report.images[0]), None);
    }

    /// 加密又解不开 = 打不开（不要拿一个空报告冒充「没有内嵌位图」）
    #[test]
    fn an_encrypted_document_that_cannot_be_decrypted_is_open_failure() {
        let bytes = encrypted_stub();
        assert_eq!(extract(&bytes).err().as_deref(), Some("open"));
        assert_eq!(page_count(&bytes), None);
    }

    #[test]
    fn garbage_is_not_a_pdf_and_truncated_bytes_do_not_panic() {
        assert_eq!(extract(b"not a pdf").err().as_deref(), Some("open"));
        let bytes = pdf_with_pages(1);
        assert_eq!(extract(&bytes[..bytes.len() / 2]).err().as_deref(), Some("open"));
    }

    /// F-002：三类编码里 DCT / Flate 出图，JPX 记 unsupported
    #[test]
    fn dct_and_flate_decode_while_jpx_is_reported() {
        let dct = Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => Object::Integer(8),
                "Height" => Object::Integer(8),
                "ColorSpace" => name("DeviceRGB"),
                "BitsPerComponent" => Object::Integer(8),
                "Filter" => name("DCTDecode"),
            },
            jpeg(8, 8),
        );
        let mut pixels = vec![0u8; 64 * 64 * 3];
        pixels[0] = 100;
        pixels[1] = 150;
        pixels[2] = 200;
        let rgb = flate(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => Object::Integer(64),
                "Height" => Object::Integer(64),
                "ColorSpace" => name("DeviceRGB"),
                "BitsPerComponent" => Object::Integer(8),
            },
            pixels,
        );
        let jpx = Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => Object::Integer(8),
                "Height" => Object::Integer(8),
                "Filter" => name("JPXDecode"),
            },
            vec![0; 32],
        );
        let bytes = pdf(
            vec![("Im1", dct), ("Im2", rgb), ("Im3", jpx)],
            "/Im1 Do /Im2 Do /Im3 Do",
        );
        let report = extract(&bytes).expect("能抽出来");
        assert_eq!(report.images.len(), 2, "JPX 那张不该进 images");
        assert_eq!(report.unsupported, vec!["JPX"]);
        assert_eq!(report.images.iter().map(|i| i.page).collect::<Vec<_>>(), vec![1, 1]);
        assert_eq!(report.images[1].codec, "png");
        assert_eq!(&report.images[1].rgba[0..3], &[100, 150, 200]);
    }

    /// predictor 走错路是静默花屏：用「首行原样 + 其余行 Up」的真实数据断言像素
    #[test]
    fn png_predictor_streams_decode_to_the_right_pixels() {
        let (width, height) = (64usize, 64usize);
        let row = width * 3;
        let first = {
            let mut row = Vec::new();
            for _ in 0..width {
                row.extend_from_slice(&[100, 150, 200]);
            }
            row
        };
        let mut encoded = Vec::new();
        encoded.push(0u8);
        encoded.extend_from_slice(&first);
        for _ in 1..height {
            encoded.push(2u8); // Up：与上一行相同 → 差分全 0
            encoded.extend(std::iter::repeat_n(0u8, row));
        }

        let stream = flate(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => Object::Integer(width as i64),
                "Height" => Object::Integer(height as i64),
                "ColorSpace" => name("DeviceRGB"),
                "BitsPerComponent" => Object::Integer(8),
                "DecodeParms" => dictionary! {
                    "Predictor" => Object::Integer(12),
                    "Colors" => Object::Integer(3),
                    "BitsPerComponent" => Object::Integer(8),
                    // lopdf 的 decode_frame 会用 Columns × colors 算行宽，
                    // 所以这里填像素宽 64（PNG 预测器的语义），不是样本数 192
                    "Columns" => Object::Integer(width as i64),
                },
            },
            encoded,
        );
        assert!(
            stream.dict.has(b"Filter"),
            "测试数据没被 zlib 压起来，这条用例就没验到 predictor"
        );

        let bytes = pdf(vec![("Im1", stream)], "/Im1 Do");
        let report = extract(&bytes).expect("能抽出来");
        assert!(report.unsupported.is_empty(), "unsupported: {:?}", report.unsupported);
        let image = &report.images[0];
        assert_eq!((image.width, image.height), (64, 64));
        assert_eq!(&image.rgba[0..4], &[100, 150, 200, 255], "首行首像素");
        let second_row = width * 4;
        assert_eq!(&image.rgba[second_row..second_row + 4], &[100, 150, 200, 255], "第二行");
    }

    #[test]
    fn sixteen_bits_per_component_keeps_the_high_byte() {
        // 3×2 的 RGB，每个样本两字节：高字节才是有效值
        let mut pixels = Vec::new();
        for value in [10u8, 20, 30, 40, 50, 60] {
            pixels.push(value);
            pixels.push(0xEE);
        }
        // 每个样本 2 字节，缺一个样本都得报 bpc，所以补够 3×2×3 个
        while pixels.len() < 3 * 2 * 3 * 2 {
            pixels.push(0xEE);
        }
        let bytes = pdf(vec![("Im1", raw(3, 2, name("DeviceRGB"), 16, pixels))], "/Im1 Do");
        let report = extract(&bytes).expect("能抽出来");
        let image = &report.images[0];
        assert_eq!(image.width, 3);
        assert_eq!(image.height, 2);
        assert_eq!(&image.rgba[0..4], &[10, 20, 30, 255]);
        assert_eq!(&image.rgba[4..8], &[40, 50, 60, 255]);
    }

    #[test]
    fn cmyk_uses_the_product_formula() {
        // 纯青：C=255 → R=0，M/Y=0 → G=B=255
        let bytes = pdf(
            vec![("Im1", raw(1, 1, name("DeviceCMYK"), 8, vec![255, 0, 0, 0]))],
            "/Im1 Do",
        );
        let report = extract(&bytes).expect("能抽出来");
        assert_eq!(&report.images[0].rgba[0..4], &[0, 255, 255, 255]);
    }

    #[test]
    fn indexed_palette_maps_every_index() {
        let space: Object = Object::Array(vec![
            "Indexed".into(),
            name("DeviceRGB"),
            Object::Integer(1),
            Object::String(vec![255, 0, 0, 0, 0, 255], lopdf::StringFormat::Literal),
        ]);
        let bytes = pdf(vec![("Im1", raw(2, 1, space, 8, vec![0, 1]))], "/Im1 Do");
        let report = extract(&bytes).expect("能抽出来");
        let rgba = &report.images[0].rgba;
        assert_eq!(&rgba[0..4], &[255, 0, 0, 255]);
        assert_eq!(&rgba[4..8], &[0, 0, 255, 255]);
    }

    #[test]
    fn oversized_images_are_refused_instead_of_exploding() {
        let bytes = pdf(
            vec![("Im1", raw(5000, 100, name("DeviceRGB"), 8, vec![0; 16]))],
            "/Im1 Do",
        );
        let report = extract(&bytes).expect("抽图本身要成功");
        assert!(report.images.is_empty());
        assert_eq!(report.unsupported, vec!["too-large"]);
    }

    /// SMask 合成到 alpha；SMask 坏掉也照样出图（丢 alpha 不丢图）
    #[test]
    fn smask_becomes_alpha_and_a_broken_one_is_dropped() {
        let mask = raw(2, 2, name("DeviceGray"), 8, vec![0, 64, 128, 255]);
        let image = raw(2, 2, name("DeviceRGB"), 8, vec![9; 2 * 2 * 3]);

        let bytes = pdf_masked(image, mask, "/Im1 Do");
        let report = extract(&bytes).expect("能抽出来");
        assert!(report.unsupported.is_empty(), "unsupported: {:?}", report.unsupported);
        let embedded = &report.images[0];
        assert!(embedded.alpha);
        let alphas: Vec<u8> = embedded.rgba.chunks_exact(4).map(|px| px[3]).collect();
        assert_eq!(alphas, vec![0, 64, 128, 255]);

        let broken = Stream::new(
            dictionary! { "Filter" => name("FlateDecode") },
            b"not zlib".to_vec(),
        );
        let image = raw(2, 2, name("DeviceRGB"), 8, vec![9; 2 * 2 * 3]);
        let bytes = pdf_masked(image, broken, "/Im1 Do");
        let report = extract(&bytes).expect("SMask 坏掉不影响出图");
        assert!(report.unsupported.is_empty(), "unsupported: {:?}", report.unsupported);
        assert_eq!(report.images.len(), 1);
        assert!(!report.images[0].alpha);
        assert_eq!(&report.images[0].rgba[0..4], &[9, 9, 9, 255]);
    }

    #[test]
    fn page_count_is_none_when_it_cannot_be_read() {
        assert_eq!(page_count(&pdf_with_pages(3)), Some(3));
        assert_eq!(page_count(b"not a pdf"), None, "读不出就是 None，不是 0");
    }
}
