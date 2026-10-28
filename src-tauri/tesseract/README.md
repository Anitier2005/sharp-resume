# tesseract —— 进程内 OCR 的本地资产

这份目录让 `ocr_image` 命令**不依赖系统安装的 tesseract.exe**：
tesseract-sys / leptonica-sys 在构建期链接下面的导入库，运行期加载旁边的 DLL，
语言数据随包分发。总共约 17MB。

## 布局

| 路径 | 内容 | 用途 |
|---|---|---|
| `bin/x64/tesseract50.dll` | tesseract 5.0.0 | 运行期加载，build.rs 会拷到 exe 旁 |
| `bin/x64/leptonica-1.82.0.dll` | leptonica 1.82.0 | 同上（tesseract50.dll 依赖它） |
| `lib/x64/tesseract50.lib` | 从 DLL 导出表生成的导入库 | 链接期 |
| `lib/x64/lept.lib` | 同上 | 链接期 |
| `include/tesseract/*.h` | tesseract 5.0.0 头文件 | bindgen 生成绑定时解析 |
| `include/leptonica/*.h` + `include/*.h` | leptonica 1.82.0 头文件 | 同上（两处都放：`leptonica/allheaders.h` 与 `<endianness.h>` 两种引用风格都要能命中） |
| `tessdata/*.traineddata` | tessdata_fast 的 eng 与 chi_sim | OCR 运行期按 `chi_sim+eng` 加载 |

## 构建期需要的环境变量

`tesseract-sys` / `leptonica-sys` 的 build script 靠环境变量找库（Windows 上默认走
vcpkg，本机没有）。`.tools/build.bat` 与 `.tools/dev-build.cmd` 已经设好：

```bat
set "LIBCLANG_PATH=<pip 装的 libclang 目录>"   rem bindgen 运行时要 libclang.dll
set "TESSERACT_INCLUDE_PATHS=%~dp0..\src-tauri\tesseract\include"
set "TESSERACT_LINK_PATHS=%~dp0..\src-tauri\tesseract\lib\x64"
set "TESSERACT_LINK_LIBS=tesseract50"
set "LEPTONICA_INCLUDE_PATH=%~dp0..\src-tauri\tesseract\include"
set "LEPTONICA_LINK_PATHS=%~dp0..\src-tauri\tesseract\lib\x64"
set "LEPTONICA_LINK_LIBS=lept"
```

缺 `LIBCLANG_PATH` 时 bindgen 会直接报
「couldn't find any valid shared libraries matching: clang.dll / libclang.dll」。
本机的来源：`python -m pip install libclang`（18.1.1），
dll 在 site-packages 的 `clang/native/` 下。

## 来源与许可

- **tesseract 5.0.0**：二进制取自 NuGet 包 `Tesseract` 5.2.0（charlesw 打包的官方构建），
  头文件取自 [tesseract-ocr/tesseract](https://github.com/tesseract-ocr/tesseract) 的 `5.0.0` tag。
  许可：Apache-2.0。
- **leptonica 1.82.0**：二进制同上包，头文件取自
  [DanBloomberg/leptonica](https://github.com/DanBloomberg/leptonica) 的 `1.82.0` tag。
  许可：BSD 2-clause。
- **tessdata_fast**（eng、chi_sim）：
  [tesseract-ocr/tessdata_fast](https://github.com/tesseract-ocr/tessdata_fast)。
  许可：Apache-2.0。
- 两个头文件是 cmake 模板，按本平台手工生成：
  `include/tesseract/version.h`（填 5.0.0）、`include/endianness.h`（小端）。
- 导入库由 `dumpbin /exports x.dll` 整理成 `.def`（首行 `LIBRARY x.dll`）后
  `lib /def:… /machine:x64` 生成；DLL 的 zlib/png/jpeg/tiff 均已静态链入，无第三方运行时依赖。

## 升级方法

换版本 = 换上面三样东西并保持同名布局。注意 tesseract 的头文件版本必须与 DLL 实际
导出的 ABI 一致（C API 跨小版本稳定，跨大版本要重验）；`version.h` 里的版本号要与
DLL 匹配，否则 `TessVersion()` 返回值和编译期常量对不上。
