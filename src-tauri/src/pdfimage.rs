//! PDF 页面 → 图片。
//!
//! 为什么要这一步：PDF 里的文字能提出来（`extract.rs`），但版式信息全丢了。
//! 视觉模型要「看」简历原件的排布，只能给它图片。
//!
//! 渲染器是**按平台挑**的，不绑死 Windows：
//!   1. Windows：`Windows.Data.Pdf`（Win10 起系统自带，零体积代价、最快）；
//!   2. 非 Windows / 系统接口不可用：调系统里装着的 `pdftoppm` / `gs` / `mutool`；
//!   3. 全都不可用：明确报错，让调用方退回「只给文字」。
//!
//! 前端另有一条**跨平台主路径**（`src/lib/pdf-render.ts`，pdf.js 跑在 WebView 里）：
//! 附件那条线根本不经过这里。这里只服务工具（`view_asset`）—— 工具在 Rust 里跑，
//! 没法借 WebView 的画布。
//!
//! 为什么不打包 pdfium / mupdf：
//!   - pdfium 要额外带 10MB+ 的 dll（自己下二进制、按架构分发、跟着升级）；
//!   - mupdf 要 cmake 整包编译，这机器上连 NASM 都没有。
//! 系统接口 + 用户已装工具的组合，在桌面场景里覆盖率已经足够。

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
pub fn pdf_pages(bytes: &[u8], max_pages: usize, width: u32) -> Result<Vec<PageImage>, String> {
    cli::pdf_pages(bytes, max_pages, width)
}

/// 没有系统接口时的退路：调用户机器上装着的命令行工具。
///
/// 编译条件写成 `any(not(windows), test)`：非 Windows 才用得到，但让它在
/// Windows 的 `cargo test` 里也参与编译 —— 否则这段代码永远没人编译过，
/// 等移植那天才发现一堆类型错误。
#[cfg(any(not(windows), test))]
mod cli {
    use super::PageImage;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// 按覆盖率排序：poppler 在 Linux 发行版 / Homebrew 里最常见，ghostscript 次之
    const TOOLS: &[&str] = &["pdftoppm", "gs", "mutool"];

    pub fn pdf_pages(bytes: &[u8], max_pages: usize, width: u32) -> Result<Vec<PageImage>, String> {
        if bytes.is_empty() {
            return Err("这份 PDF 是空的".into());
        }
        let dir = std::env::temp_dir().join(format!("sharp-resume-pdf-{}", crate::session::uuid_like()));
        std::fs::create_dir_all(&dir).map_err(|error| format!("建临时目录失败：{error}"))?;
        let source = dir.join("in.pdf");
        std::fs::File::create(&source)
            .and_then(|mut file| file.write_all(bytes))
            .map_err(|error| format!("写临时 PDF 失败：{error}"))?;

        let result = convert(&source, &dir, max_pages.clamp(1, 8), width);
        // 临时文件必须清掉：PDF 是用户的原件，不该留在 temp 里
        let _ = std::fs::remove_dir_all(&dir);
        result
    }

    fn convert(source: &Path, dir: &Path, max_pages: usize, width: u32) -> Result<Vec<PageImage>, String> {
        let mut tried: Vec<String> = Vec::new();
        for &tool in TOOLS {
            let output = match tool {
                "pdftoppm" => run(
                    tool,
                    &[
                        "-jpeg".into(),
                        "-scale-to-x".into(),
                        width.to_string(),
                        "-scale-to-y".into(),
                        "-1".into(),
                        "-f".into(),
                        "1".into(),
                        "-l".into(),
                        max_pages.to_string(),
                        source.to_string_lossy().into_owned(),
                        dir.join("page").to_string_lossy().into_owned(),
                    ],
                ),
                "gs" => run(
                    tool,
                    &[
                        "-q".into(),
                        "-dNOPAUSE".into(),
                        "-dBATCH".into(),
                        "-sDEVICE=jpeg".into(),
                        "-dJPEGQ=75".into(),
                        "-dFirstPage=1".into(),
                        format!("-dLastPage={max_pages}"),
                        // 150dpi 对 A4 约 1240px 宽，和 WinRT 那条路对齐
                        "-r150".into(),
                        format!("-sOutputFile={}", dir.join("page-%d.jpg").to_string_lossy()),
                        source.to_string_lossy().into_owned(),
                    ],
                ),
                _ => run(
                    tool,
                    &[
                        "draw".into(),
                        "-q".into(),
                        "-w".into(),
                        width.to_string(),
                        "-o".into(),
                        dir.join("page-%d.jpg").to_string_lossy().into_owned(),
                        source.to_string_lossy().into_owned(),
                        format!("1-{max_pages}"),
                    ],
                ),
            };
            match output {
                Ok(()) => {
                    let pages = collect(dir, max_pages)?;
                    if !pages.is_empty() {
                        return Ok(pages);
                    }
                    tried.push(format!("{tool}（跑完了但没产出图片）"));
                }
                Err(error) => tried.push(format!("{tool}（{error}）")),
            }
        }
        Err(format!(
            "这台机器上没有可用的 PDF 渲染器：试过 {}。装 poppler-utils（pdftoppm）或 ghostscript 即可；\
             也可以直接把 PDF 拖进会话 —— 那条路由前端渲染，不依赖系统组件。",
            tried.join("、")
        ))
    }

    fn run(tool: &str, args: &[String]) -> Result<(), String> {
        let output = Command::new(tool)
            .args(args)
            .output()
            .map_err(|error| format!("启动失败：{error}"))?;
        if output.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("退出码 {}：{}", output.status, stderr.trim()))
    }

    /// 收产物。三种工具的命名规则不同（page-1.jpg / page-01.jpg / page-1.png），
    /// 一律按「page*」捡，按文件名排序就是页序（零填充与否都成立）
    fn collect(dir: &Path, max_pages: usize) -> Result<Vec<PageImage>, String> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
            .map_err(|error| format!("读临时目录失败：{error}"))?
            .filter_map(|entry| entry.ok().map(|item| item.path()))
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("page") && name != "page")
            })
            .collect();
        files.sort();
        files.truncate(max_pages);

        let mut pages = Vec::with_capacity(files.len());
        for path in files {
            let jpeg = std::fs::read(&path).map_err(|error| format!("读渲染结果失败：{error}"))?;
            let (width, height) =
                image::image_dimensions(&path).map_err(|error| format!("解析渲染结果失败：{error}"))?;
            pages.push(PageImage {
                jpeg,
                width,
                height,
            });
        }
        Ok(pages)
    }
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

    /// 命令行退路：一个工具都没装时要说人话（macOS / Linux 上最常见的失败）
    #[cfg(any(not(windows), test))]
    #[test]
    fn cli_backend_reports_missing_tools() {
        use std::process::Command;
        let any_installed = ["pdftoppm", "gs", "mutool"]
            .iter()
            .any(|tool| Command::new(tool).arg("-v").output().is_ok());
        if any_installed {
            return;
        }
        let error = super::cli::pdf_pages(&[1, 2, 3], 1, DEFAULT_WIDTH).unwrap_err();
        assert!(error.contains("没有可用的 PDF 渲染器"), "{error}");
        assert!(error.contains("拖进会话"), "要给出退路：{error}");
    }

    #[cfg(any(not(windows), test))]
    #[test]
    fn cli_backend_rejects_empty_input() {
        assert!(super::cli::pdf_pages(&[], 1, DEFAULT_WIDTH).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn empty_pdf_reports_a_business_error() {
        let error = pdf_pages(&[], 1, DEFAULT_WIDTH).unwrap_err();
        assert!(error.contains("空的"), "{error}");
    }
}
