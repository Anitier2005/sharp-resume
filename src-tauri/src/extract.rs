//! 附件内容的 Rust 侧提取：PDF 文本、图片 OCR。
//!
//! 为什么放在 Rust 而不是前端：
//! - PDF 没有像样的纯 JS 方案（pdf.js 是几百 KB 的 wasm，为了一个附件不值）；
//! - OCR 依赖系统 tesseract.exe，子进程只能在 Rust 里调。
//!
//! 前端把文件字节整个传过来（体积上限由前端挡），这里只管解析与兜底。

use std::path::PathBuf;
use std::process::Command;

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

/// 图片 OCR：写临时文件 → 调 tesseract.exe → 读回文本 → 清理。
///
/// 不链 C 库（tesseract-sys 要 CMake + leptonica，本机必死），
/// 子进程是最稳的接法：用户装了就能用，没装给一条能落地的安装指引，
/// 而不是把「系统缺依赖」这种事藏成一个静默失败。
#[tauri::command]
pub fn ocr_image(bytes: Vec<u8>, ext: String) -> Result<String, String> {
    if bytes.is_empty() {
        return Err("这张图是空的".into());
    }
    // 先探一眼在不在：直接跑一次 OCR 的报错会更难读
    if Command::new("tesseract")
        .arg("--version")
        .output()
        .is_err()
    {
        return Err(
            "没有找到 tesseract.exe（OCR 引擎），图片转不了文字。\
             安装：https://github.com/UB-Mannheim/tesseract/wiki \
             —— 安装时在「Additional language data」里勾选 Chinese (Simplified)，装完重启本应用。"
                .into(),
        );
    }

    let dir = std::env::temp_dir();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let ext = sanitize_ext(&ext);
    let input: PathBuf = dir.join(format!("sharp-resume-ocr-{stamp}.{ext}"));
    std::fs::write(&input, &bytes).map_err(|error| format!("临时文件写不进去：{error}"))?;

    // tesseract 的输出约定：给它 basename，它自己加 .txt
    let out_base: PathBuf = dir.join(format!("sharp-resume-ocr-{stamp}"));
    let result = match Command::new("tesseract")
        .arg(&input)
        .arg(&out_base)
        .arg("-l")
        .arg("chi_sim+eng") // 中文简历里几乎总有英文字段名，双语一起认
        .output()
    {
        Ok(out) if out.status.success() => std::fs::read_to_string(out_base.with_extension("txt"))
            .map(|text| text.trim().to_string())
            .map_err(|error| format!("OCR 跑完了但结果读不出来：{error}")),
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            // 语言包缺失是最常见的翻车点，单独给一条能照着做的提示
            if stderr.contains("Failed loading language") || stderr.contains("chi_sim") {
                Err("tesseract 缺中文语言包（chi_sim）。重跑安装器，在 Additional language data 里勾选 Chinese (Simplified)。".into())
            } else {
                Err(format!("OCR 失败：{}", stderr.trim()))
            }
        }
        Err(error) => Err(format!("调不起 tesseract.exe：{error}")),
    };

    // 临时文件清掉；清理失败不值得挡结果
    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(out_base.with_extension("txt"));
    result
}

/// 扩展名只留字母数字，防止拼进临时文件名时被塞奇怪的东西
fn sanitize_ext(raw: &str) -> String {
    let clean: String = raw
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    if clean.is_empty() {
        "png".into()
    } else {
        clean
    }
}

#[cfg(test)]
mod tests {
    use super::sanitize_ext;

    #[test]
    fn ext_keeps_only_alnum_and_defaults_to_png() {
        assert_eq!(sanitize_ext("PNG"), "png");
        assert_eq!(sanitize_ext("../../etc/passwd"), "etcpasswd");
        assert_eq!(sanitize_ext(""), "png");
    }
}
