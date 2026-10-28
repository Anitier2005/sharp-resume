fn main() {
    // OCR 资源已经改为编译期 include_bytes! 嵌进 exe（运行时释放），
    // 这里不再往产物旁拷 DLL / tessdata —— 也就不需要 CARGO_FEATURE_OCR 分支。
    tauri_build::build();
}
