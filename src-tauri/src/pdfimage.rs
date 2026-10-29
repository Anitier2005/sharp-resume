//! PDF 页面 → 图片。
//!
//! 为什么要这一步：PDF 里的文字能提出来（`extract.rs`），但版式信息全丢了。
//! 视觉模型要「看」简历原件的排布，只能给它图片。
//!
//! 为什么用 Windows.Data.Pdf 而不是 pdfium / mupdf：
//!   - pdfium 要额外带一份 10MB+ 的 dll（还得自己下二进制、跟着架构走）；
//!   - mupdf 要 cmake 整包编译，这机器上连 NASM 都没有。
//!   Win10 起系统自带 PDF 渲染器，用它等于零体积代价 —— 这也是本轮才敢做
//!   「PDF 转图片」的原因（`tools/image.rs` 顶部那段「不做 PDF 转图片」的注释
//!   当时成立，是因为还没有这条零成本路径）。

use base64::Engine as _;

/// 一页渲染结果：JPEG 字节 + 实际像素尺寸。
///
/// 给前端走 data URL、给工具走资产池，两条路都要原始字节，
/// 所以这里不直接产出字符串 —— 拼 data URL 是使用方的事。
#[derive(Debug)]
pub struct PageImage {
    pub jpeg: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// 默认渲染宽度。1240px ≈ A4 150dpi：小四号字看得清，单页约 2000 token。
/// 再大 token 涨得比信息快，再小字段名会糊。
pub const DEFAULT_WIDTH: u32 = 1240;

/// 渲染上限：一份十几页的 PDF 全塞进上下文会直接爆窗口，
/// 默认只render 前几页，模型要更多可以再叫一次。
pub const MAX_PAGES: usize = 4;

#[cfg(windows)]
pub fn pdf_pages(bytes: &[u8], max_pages: usize, width: u32) -> Result<Vec<PageImage>, String> {
    use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
    use windows::Storage::Streams::{DataReader, DataWriter, InMemoryRandomAccessStream};

    if bytes.is_empty() {
        return Err("这份 PDF 是空的".into());
    }
    init_winrt();

    let stream = InMemoryRandomAccessStream::new()
        .map_err(|error| format!("创建内存流失败：{error}"))?;
    let writer = DataWriter::new().map_err(|error| format!("创建写入器失败：{error}"))?;
    writer
        .WriteBytes(bytes)
        .map_err(|error| format!("写入 PDF 字节失败：{error}"))?;
    let buffer = writer
        .DetachBuffer()
        .map_err(|error| format!("取缓冲区失败：{error}"))?;
    stream
        .WriteAsync(&buffer)
        .and_then(|action| action.join())
        .map_err(|error| format!("写入内存流失败：{error}"))?;
    stream
        .Seek(0)
        .map_err(|error| format!("回绕内存流失败：{error}"))?;

    let doc = PdfDocument::LoadFromStreamAsync(&stream)
        .and_then(|action| action.join())
        .map_err(|error| format!("这份 PDF 打不开：{error}"))?;
    let total = doc
        .PageCount()
        .map_err(|error| format!("读页数失败：{error}"))? as usize;
    if total == 0 {
        return Err("这份 PDF 没有页面".into());
    }

    let take = total.min(max_pages.max(1));
    let mut pages = Vec::with_capacity(take);
    for index in 0..take {
        let page = doc
            .GetPage(index as u32)
            .map_err(|error| format!("取第 {} 页失败：{error}", index + 1))?;
        let target = InMemoryRandomAccessStream::new()
            .map_err(|error| format!("创建渲染流失败：{error}"))?;
        let options = PdfPageRenderOptions::new()
            .map_err(|error| format!("创建渲染参数失败：{error}"))?;
        options
            .SetDestinationWidth(width)
            .map_err(|error| format!("设置渲染宽度失败：{error}"))?;
        page.RenderWithOptionsToStreamAsync(&target, &options)
            .and_then(|action| action.join())
            .map_err(|error| format!("渲染第 {} 页失败：{error}", index + 1))?;

        let size = target
            .Size()
            .map_err(|error| format!("读渲染结果大小失败：{error}"))? as usize;
        target
            .Seek(0)
            .map_err(|error| format!("回绕渲染流失败：{error}"))?;
        let reader = DataReader::CreateDataReader(&target)
            .map_err(|error| format!("创建读取器失败：{error}"))?;
        reader
            .LoadAsync(size as u32)
            .and_then(|action| action.join())
            .map_err(|error| format!("读取渲染结果失败：{error}"))?;
        let mut png = vec![0u8; size];
        reader
            .ReadBytes(&mut png)
            .map_err(|error| format!("拷出渲染结果失败：{error}"))?;

        pages.push(encode(&png, index + 1)?);
    }
    Ok(pages)
}

#[cfg(not(windows))]
pub fn pdf_pages(_bytes: &[u8], _max_pages: usize, _width: u32) -> Result<Vec<PageImage>, String> {
    Err("PDF 转图片目前只提供 Windows 构建".into())
}

/// 任意字节 → data URL。渲染出来的页面用 jpeg，材料里的原图各按自己的 mime。
pub fn data_url_of(mime: &str, bytes: &[u8]) -> String {
    format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

/// `PageImage` → data URL。前端直接把这一串塞进视觉消息。
pub fn data_url(jpeg: &[u8]) -> String {
    data_url_of("image/jpeg", jpeg)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageImageOut {
    pub data_url: String,
    pub width: u32,
    pub height: u32,
}

/// 前端把 PDF 字节传过来，拿回前几页的图片。
///
/// 为什么要给前端一条命令：视觉模型拿到的图片必须由前端塞进消息里
/// （Rust 组消息时只有字符串），所以渲染完要交回前端一份 data URL。
#[tauri::command]
pub fn pdf_page_images(
    bytes: Vec<u8>,
    max_pages: Option<usize>,
) -> Result<Vec<PageImageOut>, String> {
    let take = max_pages.unwrap_or(3).clamp(1, MAX_PAGES);
    Ok(pdf_pages(&bytes, take, DEFAULT_WIDTH)?
        .into_iter()
        .map(|page| PageImageOut {
            data_url: data_url(&page.jpeg),
            width: page.width,
            height: page.height,
        })
        .collect())
}

/// WinRT 要求当前线程初始化过 COM。聊天在 tokio 的工作线程上跑，
/// 那里没人替我们初始化；返回码里 `RPC_E_CHANGED_MODE`（已按别的模式初始化）
/// 不算失败，其余也一律放过 —— 真有问题会在下一步的 API 调用上暴露得更清楚。
#[cfg(windows)]
fn init_winrt() {
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
    // 失败也继续：最常见的失败是 RPC_E_CHANGED_MODE（这个线程已按别的模式初始化），
    // 那说明 COM 已经在，WinRT 调用照样能用
    unsafe {
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }
}

/// WinRT 吐出来的是 PNG，转成 JPEG 再发：同一页 PNG 300KB、JPEG 90KB，
/// 而视觉模型对 JPEG 的压缩痕迹并不敏感。
#[cfg(windows)]
fn encode(png: &[u8], index: usize) -> Result<PageImage, String> {
    let image = image::load_from_memory(png)
        .map_err(|error| format!("第 {index} 页解码失败：{error}"))?;
    let (width, height) = (image.width(), image.height());
    let rgb = image.to_rgb8();
    let mut jpeg = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 75);
    encoder
        .encode_image(&image::DynamicImage::ImageRgb8(rgb))
        .map_err(|error| format!("第 {index} 页编码失败：{error}"))?;
    Ok(PageImage {
        jpeg,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_width_fits_a_page() {
        // 1240 是「小四号字看得清、单页约 2000 token」的折中，改动要同步改注释
        assert_eq!(DEFAULT_WIDTH, 1240);
        assert!(MAX_PAGES >= 1);
    }

    #[cfg(windows)]
    #[test]
    fn empty_pdf_reports_a_business_error() {
        let error = pdf_pages(&[], 1, DEFAULT_WIDTH).unwrap_err();
        assert!(error.contains("空的"), "{error}");
    }
}
