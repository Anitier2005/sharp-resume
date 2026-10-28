fn main() {
    tauri_build::build();

    // 把进程内 OCR 的运行时伴侣（两个 DLL + tessdata 训练数据）拷到产物旁边。
    //
    // 为什么在 build.rs 而不是打包脚本：dev（target/debug）与 release（target/release）
    // 一条路都走这里，谁也不用记着多拷一步。tesseract50.dll 是**加载期链接**的 ——
    // 程序一启动就要找到它，找不到直接起不来，所以必须待在 exe 旁边；
    // cargo test 的可执行文件落在 deps/ 子目录，Windows 找 DLL 先看 exe 所在目录，
    // 因此 deps/ 里也要有一份，否则 `cargo test` 全体秒挂（0xc0000135）。
    // 只有带 OCR 的构建才需要这些运行时伴侣（DLL + 训练数据）。
    // 默认构建不编 tesseract，也就不该往产物旁边丢 17MB 用不上的文件。
    if std::env::var_os("CARGO_FEATURE_OCR").is_none() {
        return;
    }

    use std::path::PathBuf;

    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    // OUT_DIR = target/<档>/build/<pkg>-<hash>/out → 上溯三级就是 target/<档>/
    let profile_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"))
        .ancestors()
        .nth(3)
        .expect("OUT_DIR 层级不对")
        .to_path_buf();
    let bin_src = manifest.join("tesseract/bin/x64");
    let tessdata_src = manifest.join("tesseract/tessdata");

    let mut targets = vec![profile_dir.clone(), profile_dir.join("deps")];
    if !targets[1].is_dir() {
        targets.pop(); // 还没编过测试目标时 deps/ 不存在，别凭空建目录
    }

    println!("cargo:rerun-if-changed=tesseract/");
    for dir in &targets {
        for name in ["tesseract50.dll", "leptonica-1.82.0.dll"] {
            let from = bin_src.join(name);
            let to = dir.join(name);
            // 应用正在运行时 DLL 被锁定，覆盖会失败 —— 不让这个错误打断构建：
            // 反正内容一致，锁着的那份照样能用；真换了版本，用户重启应用后再编即可
            if from.is_file() && std::fs::copy(&from, &to).is_err() {
                println!("cargo:warning=OCR DLL 拷贝被跳过（目标被占用？）：{}", to.display());
            }
        }
        for name in ["eng.traineddata", "chi_sim.traineddata"] {
            let from = tessdata_src.join(name);
            let to = dir.join("tessdata").join(name);
            if from.is_file() && !to.is_file() {
                let _ = std::fs::create_dir_all(to.parent().unwrap());
                let _ = std::fs::copy(&from, &to);
            }
        }
    }
}
