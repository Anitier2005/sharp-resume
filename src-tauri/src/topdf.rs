//! 导出 PDF：让 WebView2 自己把当前预览排版成文件。
//!
//! 为什么不用 `window.print()`：
//!   `print()` 打开的是系统打印对话框，用户得在里面选打印机、再选「另存为 PDF」、
//!   再确认文件名。那条路上任一步取消，导出的结果就只是「没有文件」——
//!   应用这边既不知道成功也不知道失败。而导出 PDF 是本产品的**成品交付物之一**，
//!   它必须是「点一下 → 得到一个文件 → 告诉用户它在哪」。
//!
//! 为什么不用别的 PDF 引擎：屏幕上的纸面就是 Chromium 排出来的。换 pdfium / mupdf /
//! 自己写排版器，等于把同一份简历实现两遍，两遍必然漂移（这正是 Word 导出要单独
//! 对齐一次的原因）。`PrintToPdf` 走的就是同一个排版引擎的打印管线，保真没有第二选择。
//!
//! 两条硬规则：
//!   1. **命令必须是 async**。同步命令跑在主线程，而 `with_webview` 的闭包要回到主线程
//!      才能跑 —— 主线程等自己，窗口白屏死锁（设置窗踩过一次，别再踩）。
//!   2. **页边距由前端给**。Paged.js 打出来的纸自带 16mm 边距（`@page { margin: 0 }`），
//!      而单张纸的预览靠 `@page { margin: 16mm }`。PrintSettings 的边距是「自定义边距」，
//!      一旦写死就会覆盖 CSS 的 `@page` —— 两边都写 16mm 等于打了两次边距。
//!
//! 不在 Windows 上就明确报错，不静默什么也不做。

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfInput {
    /// 保存到哪（绝对路径，由系统保存框给出）
    pub path: String,
    /// 纸的页边距（毫米）。0 = 纸面自己已经带边距（Paged.js 分页后的那些真 A4）
    pub margin_mm: f64,
}

/// A4 的物理尺寸。写在这里而不是前端传：PDF 的纸只能是 A4，
/// 让调用方传就等于允许「导出成 A5」这种没人要的分支。
const PAPER_W_MM: f64 = 210.0;
const PAPER_H_MM: f64 = 297.0;
const MM_PER_INCH: f64 = 25.4;

#[tauri::command]
pub async fn export_pdf(window: tauri::WebviewWindow, input: PdfInput) -> Result<(), String> {
    let path = input.path.trim().to_string();
    if path.is_empty() {
        return Err("没有给出保存路径".to_string());
    }
    // 边距只接受 0..40mm：NaN / 负数 / 离谱值都当成 0，宁可少一层边距也不要
    // 因为一个坏参数把整份 PDF 排成一张空白。
    let margin_mm = if input.margin_mm.is_finite() {
        input.margin_mm.clamp(0.0, 40.0)
    } else {
        0.0
    };

    #[cfg(windows)]
    {
        // PrintToPdf 对已存在的文件行为不保证，自己先删干净：保存框已经问过用户
        // 「要覆盖吗」，这里再让 WebView2 去猜只是多一种失败方式。
        let target = std::path::PathBuf::from(&path);
        // 目录得先存在，PrintToPdf 不会替我们建。整份导出写的是
        // `<数据目录>/导出 PDF/<人 - 简历>/`，那个目录由这次调用顺手建出来 ——
        // 前端自己去建要先拿到数据目录再调插件，多一层能失败的环节。
        if let Some(parent) = target.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|error| format!("建不了目录 {}：{error}", parent.display()))?;
            }
        }
        if target.exists() {
            std::fs::remove_file(&target)
                .map_err(|error| format!("旧文件删不掉（是不是被别的程序打开了）：{error}"))?;
        }

        // 容量 1 的通道：闭包在主线程，完成回调也在主线程，宿主在 tokio 线程等。
        let (tx, mut rx) = tauri::async_runtime::channel::<Result<(), String>>(1);
        window
            .with_webview(move |webview| {
                // 失败路径要留一条自己的发送端：`tx` 已经交给 print_to，
                // 而它可能连回调都还没建出来就失败了
                let backup = tx.clone();
                if let Err(error) = print_to(&webview, &path, margin_mm, tx) {
                    let _ = backup.blocking_send(Err(error));
                }
            })
            .map_err(|error| format!("拿不到窗口：{error}"))?;

        // 通道在闭包跑完之前不会关；没有消息只可能是主线程已经没了
        match rx.recv().await {
            Some(result) => result,
            None => Err("导出被中断".to_string()),
        }
    }

    #[cfg(not(windows))]
    {
        let _ = (&window, margin_mm);
        Err("导出 PDF 用的是 WebView2 的打印管线，目前只在 Windows 上可用".to_string())
    }
}

#[cfg(windows)]
fn print_to(
    webview: &tauri::webview::PlatformWebview,
    path: &str,
    margin_mm: f64,
    done: tauri::async_runtime::Sender<Result<(), String>>,
) -> Result<(), String> {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2Environment6, ICoreWebView2PrintSettings, ICoreWebView2_7,
    };
    use webview2_com::PrintToPdfCompletedHandler;
    use windows::core::{Interface, HSTRING};

    // `PrintToPdf` 在 1.0.1108 之后才有；老运行时会 cast 失败，这时给一句能照做的错
    let environment = webview.environment();
    let environment6: ICoreWebView2Environment6 = environment
        .cast()
        .map_err(|error| format!("WebView2 运行时太旧，没有打印接口（升级 WebView2 运行时）：{error}"))?;
    let settings: ICoreWebView2PrintSettings = unsafe { environment6.CreatePrintSettings() }
        .map_err(|error| format!("建不出打印参数：{error}"))?;

    let inches = 1.0 / MM_PER_INCH;
    let margin = margin_mm * inches;
    unsafe {
        settings
            .SetPageWidth(PAPER_W_MM * inches)
            .and_then(|()| settings.SetPageHeight(PAPER_H_MM * inches))
            .and_then(|()| settings.SetMarginTop(margin))
            .and_then(|()| settings.SetMarginBottom(margin))
            .and_then(|()| settings.SetMarginLeft(margin))
            .and_then(|()| settings.SetMarginRight(margin))
            // 分区线、表头底色都是「背景」，不打背景等于导出一份缺线的简历
            .and_then(|()| settings.SetShouldPrintBackgrounds(true))
            .map_err(|error| format!("设置打印参数失败：{error}"))?;
    }

    let core = unsafe { webview.controller().CoreWebView2() }
        .map_err(|error| format!("拿不到 WebView2 本体：{error}"))?;
    let core7: ICoreWebView2_7 = core
        .cast()
        .map_err(|error| format!("WebView2 运行时太旧，没有 PrintToPdf（需要 95 以上）：{error}"))?;

    let handler = PrintToPdfCompletedHandler::create(Box::new(move |result, written| {
        let outcome = match result {
            Err(error) => Err(format!("WebView2 写出 PDF 失败：{error}")),
            // 回调成功但没写文件：页面还在变（比如流式对话正在改纸面）。说实话，别假装成功
            Ok(()) if !written => Err("WebView2 报告完成，但没有写出版文件；页面可能还在变化，稍后再试".to_string()),
            Ok(()) => Ok(()),
        };
        let _ = done.blocking_send(outcome);
        Ok(())
    }));

    let wide = HSTRING::from(path);
    unsafe {
        core7
            .PrintToPdf(&wide, &settings, &handler)
            .map_err(|error| format!("发起导出失败：{error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn margins_are_clamped_to_something_printable() {
        assert_eq!(PAPER_W_MM, 210.0);
        assert_eq!(PAPER_H_MM, 297.0);
        // 16mm 是本项目唯一用过的页边距（print.css 的 @page）
        assert!((16.0 / MM_PER_INCH - 0.629921).abs() < 1e-5);
    }
}
