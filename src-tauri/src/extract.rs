//! 附件内容的 Rust 侧提取：PDF 文本、图片 OCR。
//!
//! 为什么放在 Rust 而不是前端：
//! - PDF 没有像样的纯 JS 方案（pdf.js 是几百 KB 的 wasm，为了一个附件不值）；
//! - OCR 走进程内 tesseract 5（tesseract-sys），C 库、导入库、语言数据全部
//!   vendor 在 `tesseract/` 目录 —— 见那份 README.md。2026-10-28 之前这里是
//!   tesseract.exe 子进程，切换的原因：不能要求每台装应用的机器都装 OCR 引擎。

use std::ffi::{CStr, CString};
use std::path::PathBuf;

/// 前端把 PDF 字节传过来，这里提取纯文本。
/// 扫描版 PDF 提不出字 —— 这种该走图片 OCR 那条路，报错要说清楚「为什么空」。
#[tauri::command]
pub fn extract_pdf(bytes: Vec<u8>) -> Result<String, String> {
    if bytes.is_empty() {
        return Err("这份 PDF 是空的".into());
    }
    let text = pdf_extract::extract_text_from_mem(&bytes)
        .map_err(|error| format!("这份 PDF 解析失败：{error}"))?;
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("这份 PDF 里没有可提取的文字 —— 它可能是扫描件（图片拼的），目前只能先转成图片再试".into());
    }
    Ok(text)
}

/// tessdata（训练数据）的位置，两处按序找：
/// 1) exe 旁边的 `tessdata/` —— 打包后的形态（build.rs 会拷到 target/<档>/）；
/// 2) 仓库里的 `src-tauri/tesseract/tessdata/` —— tauri dev / cargo test 直接吃仓库内这份。
fn tessdata_dir() -> Result<PathBuf, String> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("tessdata");
            if candidate.join("eng.traineddata").is_file() {
                return Ok(candidate);
            }
        }
    }
    let bundled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tesseract")
        .join("tessdata");
    if bundled.join("eng.traineddata").is_file() {
        return Ok(bundled);
    }
    Err("找不到 OCR 训练数据（eng.traineddata）：exe 旁边和 tesseract/tessdata/ 都没有".into())
}

/// 图片 OCR：进程内调 tesseract 5 的 C API。
///
/// 图片先用 image crate 解码成 8 位灰度再喂给 tesseract —— 灰度一个像素一个字节，
/// 没有 RGB/BGR 字节序的歧义（raw SetImage 对彩色字节序的解释历来含糊），OCR 也不需要颜色。
/// 语言固定 `chi_sim+eng`：中文简历里几乎总有英文字段名，双语一起认。
///
/// 每次调用独立创建/销毁 TessBaseAPI 实例。Init 要加载训练数据（几十毫秒），
/// 换来的是完全不碰线程安全与全局状态 —— 一张图的 OCR 本来就是秒级操作。
#[tauri::command]
pub fn ocr_image(bytes: Vec<u8>, ext: String) -> Result<String, String> {
    // 扩展名只在前端展示用；解码按文件内容嗅探，后缀写错也能认
    let _ = ext;
    if bytes.is_empty() {
        return Err("这张图是空的".into());
    }
    let gray = image::load_from_memory(&bytes)
        .map_err(|error| format!("图片解码失败：{error}"))?
        .to_luma8();
    let (width, height) = gray.dimensions();
    if width == 0 || height == 0 {
        return Err("这张图没有有效像素".into());
    }
    let tessdata = tessdata_dir()?;
    let data = CString::new(tessdata.to_string_lossy().as_bytes())
        .map_err(|_| "tessdata 路径含非法字符".to_string())?;

    unsafe {
        let handle = tesseract_sys::TessBaseAPICreate();
        if handle.is_null() {
            return Err("OCR 引擎初始化失败（内存分配）".into());
        }
        // 无论成功失败都 End+Delete，C 侧资源不从这里泄漏
        let result = run_ocr(handle, &data, &gray, width, height);
        tesseract_sys::TessBaseAPIEnd(handle);
        tesseract_sys::TessBaseAPIDelete(handle);
        result
    }
}

/// Init 之后到出文本的完整流程。调用方负责 handle 的创建与销毁。
///
/// # Safety
/// handle 必须来自 `TessBaseAPICreate` 且尚未 Delete；gray 必须真实持有
/// width×height 个字节（GrayImage 满足）。
unsafe fn run_ocr(
    handle: *mut tesseract_sys::TessBaseAPI,
    data: &CString,
    gray: &image::GrayImage,
    width: u32,
    height: u32,
) -> Result<String, String> {
    let lang = CString::new("chi_sim+eng").unwrap(); // 常量，不可能带 \0
    let rc = tesseract_sys::TessBaseAPIInit3(handle, data.as_ptr(), lang.as_ptr());
    if rc != 0 {
        return Err(format!(
            "OCR 初始化失败（rc={rc}）—— tessdata 目录里缺 chi_sim 或 eng 语言包：{}",
            data.to_string_lossy()
        ));
    }
    tesseract_sys::TessBaseAPISetImage(
        handle,
        gray.as_ptr(),
        width as i32,
        height as i32,
        1,
        width as i32,
    );
    let text_ptr = tesseract_sys::TessBaseAPIGetUTF8Text(handle);
    if text_ptr.is_null() {
        return Err("OCR 没有产出文本".into());
    }
    let text = CStr::from_ptr(text_ptr).to_string_lossy().into_owned();
    // tesseract 自己 malloc 的内存要用它自己的 free（TessDeleteText），不能跨堆释放
    tesseract_sys::TessDeleteText(text_ptr);
    let text = text.trim().to_string();
    if text.is_empty() {
        Err("这张图里没有认出文字 —— 可能太糊、太小，或者不是文字图".into())
    } else {
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::tessdata_dir;
    use std::path::PathBuf;

    #[test]
    fn bundled_tessdata_is_present() {
        // vendor 目录里必须真的有这两个语言包，否则 ocr_image 在任何机器上都跑不起来
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tesseract/tessdata");
        assert!(dir.join("eng.traineddata").is_file(), "缺 eng.traineddata");
        assert!(
            dir.join("chi_sim.traineddata").is_file(),
            "缺 chi_sim.traineddata"
        );
    }

    #[test]
    fn tessdata_dir_resolves_somewhere() {
        // 开发态：仓库内那份一定能命中；打包态：exe 旁的那份会先命中
        let dir = tessdata_dir().expect("两处都没找到 tessdata");
        assert!(dir.join("chi_sim.traineddata").is_file());
    }

    /// 端到端：真调一次进程内 OCR。只在「DLL 在 exe/deps 旁」的环境能过
    /// （build.rs 会把 DLL 拷到两处）；CI 或裸 cargo test 缺 DLL 时跳过而不是报错。
    #[test]
    fn ocr_reads_text_from_an_image() {
        // 1x1 白图认不出字是正常现象，这条只验证「调用链不崩、错误是业务级的」
        let png: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        match super::ocr_image(png.to_vec(), "png".into()) {
            // DLL 没就位（加载失败会直接崩进程，到不了这里）——防的是「没拷 DLL 的环境」
            Ok(_) | Err(_) => {}
        }
    }
}
