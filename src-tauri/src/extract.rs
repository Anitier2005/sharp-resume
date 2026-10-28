//! 附件内容的 Rust 侧提取：PDF 文本、图片 OCR。
//!
//! 为什么放在 Rust 而不是前端：
//! - PDF 没有像样的纯 JS 方案（pdf.js 是几百 KB 的 wasm，为了一个附件不值）；
//! - OCR 走进程内 tesseract 5，DLL 与语言数据全部 vendor 在 `tesseract/` 目录
//!   （见那份 README.md）。2026-10-28 之前这里是 tesseract.exe 子进程，
//!   切换的原因：不能要求每台装应用的机器都装 OCR 引擎。
//!
//! **OCR 版是单文件**：两个 tesseract DLL、三个 VC 运行库、两份语言数据全部
//! `include_bytes!` 进 exe，第一次 OCR 时释放到系统临时目录再动态加载。
//! 这样就不再需要导入库 / bindgen / libclang —— 构建只要一个 MSVC 链接器，
//! 交付只有一个 exe。代价是首次识别多花一次写盘（约 13MB，几十毫秒）。

use base64::Engine as _;

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

/// 这一版构建有没有内置 OCR。
///
/// 默认构建不带 tesseract（单文件里要加 13MB，绝大多数人用不上）；
/// 前端据此决定图片走「本地 OCR」还是「设置里配的 OCR 接口」，也用于界面提示。
pub fn ocr_available() -> bool {
    cfg!(feature = "ocr")
}

/* ==========================================================================
   内置 OCR：资源嵌入 + 运行时释放 + 动态加载
   ========================================================================== */

#[cfg(feature = "ocr")]
mod engine {
    // c_char/c_int/c_void 在 win 子模块里用，这一层只需要 CString
    use std::ffi::CString;
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;

    // 编译期嵌进二进制。路径相对本文件：src-tauri/src/../tesseract/...
    const TESS_DLL: &[u8] = include_bytes!("../tesseract/bin/x64/tesseract50.dll");
    const LEPT_DLL: &[u8] = include_bytes!("../tesseract/bin/x64/leptonica-1.82.0.dll");
    // tesseract 是动态 CRT 编的，干净机器上不一定装了 VC++ 可再发行组件 ——
    // 这三份一起带上，OCR 版在任何 Windows 上都能起来（都是微软允许随应用分发的运行时）
    const MSVCP_DLL: &[u8] = include_bytes!("../tesseract/bin/x64/vc/msvcp140.dll");
    const VCRT_DLL: &[u8] = include_bytes!("../tesseract/bin/x64/vc/vcruntime140.dll");
    const VCRT1_DLL: &[u8] = include_bytes!("../tesseract/bin/x64/vc/vcruntime140_1.dll");
    const ENG_DATA: &[u8] = include_bytes!("../tesseract/tessdata/eng.traineddata");
    const CHI_DATA: &[u8] = include_bytes!("../tesseract/tessdata/chi_sim.traineddata");

    /* ---------- 释放 ---------- */

    /// 释放目录：临时目录下按版本分目录，升级后旧目录不会挡住新文件。
    fn cache_dir() -> PathBuf {
        std::env::temp_dir().join(format!("sharp-resume-ocr-{}", env!("CARGO_PKG_VERSION")))
    }

    /// 只在「文件不存在或大小不对」时写，重复启动不会反复覆盖（也避开正在被占用）
    fn write_if_needed(path: &Path, bytes: &[u8]) -> Result<(), String> {
        if let Ok(meta) = std::fs::metadata(path) {
            if meta.len() == bytes.len() as u64 {
                return Ok(());
            }
        }
        std::fs::write(path, bytes)
            .map_err(|error| format!("释放 OCR 组件失败（{}）：{error}", path.display()))
    }

    fn ensure_files() -> Result<PathBuf, String> {
        let dir = cache_dir();
        std::fs::create_dir_all(dir.join("tessdata"))
            .map_err(|error| format!("创建 OCR 缓存目录失败：{error}"))?;
        write_if_needed(&dir.join("tesseract50.dll"), TESS_DLL)?;
        write_if_needed(&dir.join("leptonica-1.82.0.dll"), LEPT_DLL)?;
        write_if_needed(&dir.join("msvcp140.dll"), MSVCP_DLL)?;
        write_if_needed(&dir.join("vcruntime140.dll"), VCRT_DLL)?;
        write_if_needed(&dir.join("vcruntime140_1.dll"), VCRT1_DLL)?;
        write_if_needed(&dir.join("tessdata").join("eng.traineddata"), ENG_DATA)?;
        write_if_needed(&dir.join("tessdata").join("chi_sim.traineddata"), CHI_DATA)?;
        Ok(dir)
    }

    /* ---------- 动态加载 ---------- */

    #[cfg(target_os = "windows")]
    mod win {
        use std::ffi::{c_char, c_int, c_void};
        use std::path::Path;

        #[link(name = "kernel32")]
        extern "system" {
            fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
            fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
        }

        /// 让它按「DLL 自己所在目录」找依赖（leptonica 与 VC 运行库都在同一目录）
        const LOAD_WITH_ALTERED_SEARCH_PATH: u32 = 0x0000_0008;

        pub fn load(path: &Path) -> Result<*mut c_void, String> {
            use std::os::windows::ffi::OsStrExt;
            let wide: Vec<u16> = path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let module = unsafe { LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), LOAD_WITH_ALTERED_SEARCH_PATH) };
            if module.is_null() {
                return Err(format!(
                    "加载 {} 失败：{}",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    std::io::Error::last_os_error()
                ));
            }
            Ok(module)
        }

        /// 按名字取符号并转成函数指针。名字必须是带 \0 的字节串。
        pub unsafe fn sym<T: Copy>(module: *mut c_void, name: &[u8]) -> Result<T, String> {
            let ptr = GetProcAddress(module, name.as_ptr());
            if ptr.is_null() {
                return Err(format!(
                    "tesseract 缺少导出 {}（版本不对？）",
                    String::from_utf8_lossy(&name[..name.len().saturating_sub(1)])
                ));
            }
            Ok(std::mem::transmute_copy::<*mut c_void, T>(&ptr))
        }

        pub type Create = unsafe extern "system" fn() -> *mut c_void;
        pub type Init = unsafe extern "system" fn(*mut c_void, *const c_char, *const c_char) -> c_int;
        pub type SetImage =
            unsafe extern "system" fn(*mut c_void, *const u8, c_int, c_int, c_int, c_int);
        pub type GetText = unsafe extern "system" fn(*mut c_void) -> *mut c_char;
        pub type DeleteText = unsafe extern "system" fn(*mut c_char);
        pub type End = unsafe extern "system" fn(*mut c_void);
        pub type Delete = unsafe extern "system" fn(*mut c_void);
    }

    #[cfg(target_os = "windows")]
    pub struct Engine {
        pub create: win::Create,
        pub init: win::Init,
        pub set_image: win::SetImage,
        pub get_text: win::GetText,
        pub delete_text: win::DeleteText,
        pub end: win::End,
        pub delete: win::Delete,
        pub tessdata: CString,
    }

    /// 只加载一次：DLL 在进程生命周期里常驻，重复 LoadLibrary 只是加计数，白费
    static ENGINE: OnceLock<Result<EngineState, String>> = OnceLock::new();

    #[cfg(target_os = "windows")]
    pub type EngineState = Engine;

    #[cfg(not(target_os = "windows"))]
    pub type EngineState = ();

    #[cfg(target_os = "windows")]
    fn load_engine() -> Result<EngineState, String> {
        let dir = ensure_files()?;
        // 先按依赖顺序把运行库与 leptonica 拉起来，再加载 tesseract ——
        // 顺序反了时错误信息只会说「找不到依赖」，很难定位是谁缺
        for name in ["vcruntime140.dll", "vcruntime140_1.dll", "msvcp140.dll", "leptonica-1.82.0.dll"] {
            win::load(&dir.join(name))?;
        }
        let module = win::load(&dir.join("tesseract50.dll"))?;
        let api = unsafe {
            Engine {
                create: win::sym(module, b"TessBaseAPICreate\0")?,
                init: win::sym(module, b"TessBaseAPIInit3\0")?,
                set_image: win::sym(module, b"TessBaseAPISetImage\0")?,
                get_text: win::sym(module, b"TessBaseAPIGetUTF8Text\0")?,
                delete_text: win::sym(module, b"TessDeleteText\0")?,
                end: win::sym(module, b"TessBaseAPIEnd\0")?,
                delete: win::sym(module, b"TessBaseAPIDelete\0")?,
                tessdata: CString::new(dir.join("tessdata").to_string_lossy().as_bytes())
                    .map_err(|_| "缓存目录含非法字符".to_string())?,
            }
        };
        Ok(api)
    }

    #[cfg(not(target_os = "windows"))]
    fn load_engine() -> Result<EngineState, String> {
        Err("内置 OCR 目前只提供 Windows 构建；其他平台请在设置里配置 OCR 接口".into())
    }

    pub fn engine() -> Result<&'static EngineState, String> {
        match ENGINE.get_or_init(load_engine) {
            Ok(state) => Ok(state),
            Err(message) => Err(message.clone()),
        }
    }

    #[cfg(target_os = "windows")]
    pub fn recognize(bytes: &[u8]) -> Result<String, String> {
        use std::ffi::CStr;

        if bytes.is_empty() {
            return Err("这张图是空的".into());
        }
        // 灰度一个像素一字节，没有 RGB/BGR 字节序的歧义；OCR 也不需要颜色
        let gray = image::load_from_memory(bytes)
            .map_err(|error| format!("图片解码失败：{error}"))?
            .to_luma8();
        let (width, height) = gray.dimensions();
        if width == 0 || height == 0 {
            return Err("这张图没有有效像素".into());
        }

        let engine = engine()?;
        // 语言固定 chi_sim+eng：中文简历里几乎总有英文字段名，双语一起认
        let lang = CString::new("chi_sim+eng").map_err(|_| "语言参数非法".to_string())?;

        unsafe {
            let handle = (engine.create)();
            if handle.is_null() {
                return Err("OCR 引擎初始化失败（内存分配）".into());
            }
            // 无论成功失败都 End+Delete，C 侧资源不从这里泄漏
            let result = (|| {
                let rc = (engine.init)(handle, engine.tessdata.as_ptr(), lang.as_ptr());
                if rc != 0 {
                    return Err(format!(
                        "OCR 初始化失败（rc={rc}）—— 语言数据可能没释放出来：{}",
                        engine.tessdata.to_string_lossy()
                    ));
                }
                (engine.set_image)(handle, gray.as_ptr(), width as i32, height as i32, 1, width as i32);
                let text_ptr = (engine.get_text)(handle);
                if text_ptr.is_null() {
                    return Err("OCR 没有产出文本".into());
                }
                let text = CStr::from_ptr(text_ptr).to_string_lossy().into_owned();
                // tesseract 自己 malloc 的内存要用它自己的 free（TessDeleteText）
                (engine.delete_text)(text_ptr);
                let text = text.trim().to_string();
                if text.is_empty() {
                    Err("这张图里没有认出文字 —— 可能太糊、太小，或者不是文字图".into())
                } else {
                    Ok(text)
                }
            })();
            (engine.end)(handle);
            (engine.delete)(handle);
            result
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn recognize(_bytes: &[u8]) -> Result<String, String> {
        Err("内置 OCR 目前只提供 Windows 构建".into())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn embedded_assets_are_present() {
            assert!(TESS_DLL.len() > 1_000_000, "tesseract DLL 没打进来");
            assert!(LEPT_DLL.len() > 1_000_000, "leptonica DLL 没打进来");
            assert!(ENG_DATA.len() > 1_000_000, "eng 语言包没打进来");
            assert!(CHI_DATA.len() > 1_000_000, "chi_sim 语言包没打进来");
        }

        #[test]
        fn assets_are_extracted_to_cache() {
            let dir = ensure_files().expect("释放失败");
            for name in ["tesseract50.dll", "leptonica-1.82.0.dll", "msvcp140.dll"] {
                let path = dir.join(name);
                assert!(path.is_file(), "{} 没释放出来", path.display());
            }
            assert!(dir.join("tessdata").join("chi_sim.traineddata").is_file());
        }
    }
}

/// 图片 OCR：带 ocr feature 时走内置引擎，否则明确告诉前端「这一版没有」。
#[cfg(feature = "ocr")]
#[tauri::command]
pub fn ocr_image(bytes: Vec<u8>, ext: String) -> Result<String, String> {
    // 扩展名只在前端展示用；解码按文件内容嗅探，后缀写错也能认
    let _ = ext;
    engine::recognize(&bytes)
}

/// 默认构建（没有 tesseract）的同名命令。
///
/// 它必须存在且**可被前端识别**：前端拿到的这句提示就是「切到 OCR 接口」的信号，
/// 不能改成一句笼统的失败，否则用户只看到「OCR 失败」却不知道下一步做什么。
#[cfg(not(feature = "ocr"))]
#[tauri::command]
pub fn ocr_image(_bytes: Vec<u8>, _ext: String) -> Result<String, String> {
    Err("这一版没有内置 OCR。可以在 设置 → 模型 → OCR 配置一个能看图的接口模型，或改用带 OCR 的构建（.tools\\build-ocr.bat）".into())
}

/// 走接口的 OCR：把图片发给多模态模型，让它把可见文字逐行抄出来。
///
/// 为什么需要它：默认构建不再内置 tesseract，而「模型本身不能看图」时
/// 又必须把图片变成文字才能塞进上下文 —— 这条通道就是那个兜底。
/// 接口按 OpenAI 兼容的 chat/completions 拼（image_url + data URL），
/// 提示词只要求「原样抄写」，不要它解释或总结。
#[tauri::command]
pub async fn ocr_api(
    bytes: Vec<u8>,
    ext: String,
    base_url: String,
    api_key: String,
    model: String,
) -> Result<String, String> {
    if bytes.is_empty() {
        return Err("这张图是空的".into());
    }
    if base_url.trim().is_empty() || model.trim().is_empty() {
        return Err("OCR 接口还没配置：设置 → 模型 → OCR".into());
    }

    // 按扩展名给 MIME；认不出的一律按 jpeg 试 —— 多数网关只看字节，不看这个头
    let mime = match ext.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        _ => "image/jpeg",
    };
    let data_url = format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    );

    let content = serde_json::json!([
        {
            "type": "text",
            "text": "把这张图里的可见文字逐行原样抄写出来：保留换行与原有顺序，\
                     不要解释、不要总结、不要翻译、不要补全看不清的部分。\
                     如果图里没有文字，只回答：没有文字。"
        },
        { "type": "image_url", "image_url": { "url": data_url } }
    ]);

    let request = crate::net::AiRequest {
        base_url,
        api_key,
        model,
        messages: vec![crate::net::ChatMessage {
            role: "user".into(),
            content: Some(content),
            tool_calls: None,
            tool_call_id: None,
        }],
        // 抄写任务不需要发挥；温度压到 0，减少它自作主张改写数字
        temperature: Some(0.0),
        reasoning: None,
        max_tokens: Some(4_096),
        tools: None,
    };

    let raw = crate::net::chat_once(request).await?;
    let text = strip_fences(&raw);
    if text.is_empty() || text.contains("没有文字") {
        return Err("这张图里没有认出文字 —— 可能太糊、太小，或者不是文字图".into());
    }
    Ok(text)
}

/// 多模态模型爱把整段回答包进 ``` 代码块里；抄写任务不需要这层壳。
fn strip_fences(text: &str) -> String {
    let trimmed = text.trim();
    let body = trimmed
        .strip_prefix("```")
        .map(|rest| rest.split_once('\n').map(|(_, tail)| tail).unwrap_or(rest))
        .unwrap_or(trimmed);
    let body = body.trim_end();
    let body = body.strip_suffix("```").unwrap_or(body);
    body.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 端到端：真调一次 OCR。只在 Windows 上跑（动态加载只有那份实现）。
    #[cfg(all(feature = "ocr", target_os = "windows"))]
    #[test]
    fn ocr_returns_a_business_error_on_a_blank_image() {
        // 1x1 白图认不出字是正常现象，这条只验证「调用链不崩、错误是业务级的」
        let png: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        assert!(ocr_image(png.to_vec(), "png".into()).is_err());
    }

    #[test]
    fn ocr_api_rejects_unconfigured_endpoint() {
        let err = tauri::async_runtime::block_on(ocr_api(
            vec![1, 2, 3],
            "png".into(),
            "".into(),
            "".into(),
            "".into(),
        ))
        .unwrap_err();
        assert!(err.contains("还没配置"), "{err}");
    }

    #[test]
    fn fences_are_stripped() {
        assert_eq!(strip_fences("```\nABC\n```"), "ABC");
        assert_eq!(strip_fences("ABC"), "ABC");
    }
}
