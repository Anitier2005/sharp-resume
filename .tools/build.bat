@echo off
rem ============================================================================
rem Sharp Resume · Windows release 构建（lite：不含内置 OCR）
rem 需要内置 OCR 的版本请用 build-ocr.bat（多 17MB，且需要 libclang 那套构建依赖）
rem
rem 为什么需要这个脚本（两条都会让 cargo build 直接失败）：
rem
rem   1) cargo 找 cl.exe 依赖 VS 开发者命令行（vswhere → 查注册表）。
rem      受限环境里 reg.exe 会被拦截，vswhere 拿不到安装路径，
rem      报 `failed to find tool "cl.exe"`。这里直接把 MSVC 与 Windows SDK 的
rem      include/lib/bin 拼进环境变量，绕开注册表查询。
rem
rem   2) 本机 Windows SDK（10.0.28000.0）的 ucrt 库是**残缺的**，只有 8 个 lib，
rem      缺 legacy_stdio_definitions.lib / oldnames.lib / msvcrt.lib。
rem      本工程用静态 CRT（static_vcruntime），实际只需要前两个且用不到里面的
rem      符号，因此 .tools\libstub 放了两个空壳导入库顶上；真实库存在时会优先命中
rem      SDK 里的那份（libstub 排在 LIB 最后），不影响正常机器。
rem
rem 用法：
rem   .tools\build.bat                       构建 release exe
rem   set MSVC_ROOT=... & set SDK_VER=...    覆盖自动探测的路径
rem
rem 产物：src-tauri\target\release\sharp-resume.exe
rem 打包：回到仓库根目录跑 npm run package:clean
rem ============================================================================
setlocal

rem 工具链位置会随 VS 重装变化（本机 2026-10-26 从 C 盘 BuildTools 换成了 E 盘 Community），
rem 所以按候选顺序探测，找到第一个存在的 VC\Tools\MSVC\<版本> 就用它。
if not defined MSVC_ROOT (
  for %%B in (
    "E:\Program Files\Microsoft Visual Studio\18\Community"
    "C:\Program Files\Microsoft Visual Studio\18\Community"
    "C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools"
  ) do (
    if not defined MSVC_ROOT if exist "%%~B\VC\Tools\MSVC" (
      for /d %%V in ("%%~B\VC\Tools\MSVC\*") do if not defined MSVC_ROOT set "MSVC_ROOT=%%~V"
    )
  )
)
if not defined SDK_ROOT  set "SDK_ROOT=C:\Program Files (x86)\Windows Kits\10"
if not defined SDK_VER   set "SDK_VER=10.0.28000.0"

rem Community 装的是 64 位宿主（bin\Hostx64\x64），BuildTools 之前只有 32 位宿主
set "CL_DIR=%MSVC_ROOT%\bin\Hostx64\x64"
if not exist "%CL_DIR%\cl.exe" set "CL_DIR=%MSVC_ROOT%\bin\Hostx86\x64"
set "RC_DIR=%SDK_ROOT%\bin\%SDK_VER%\x64"

if not exist "%CL_DIR%\cl.exe" (
  echo [FAIL] found no cl.exe at "%CL_DIR%"
  echo        set MSVC_ROOT to your VC\Tools\MSVC\<version> folder and retry.
  exit /b 1
)
if not exist "%RC_DIR%\rc.exe" (
  echo [FAIL] found no rc.exe at "%RC_DIR%"
  echo        set SDK_ROOT / SDK_VER and retry.
  exit /b 1
)

set "INCLUDE=%MSVC_ROOT%\include;%SDK_ROOT%\Include\%SDK_VER%\ucrt;%SDK_ROOT%\Include\%SDK_VER%\um;%SDK_ROOT%\Include\%SDK_VER%\shared;%SDK_ROOT%\Include\%SDK_VER%\winrt;%SDK_ROOT%\Include\%SDK_VER%\cppwinrt"
set "LIB=%MSVC_ROOT%\lib\x64;%SDK_ROOT%\Lib\%SDK_VER%\ucrt\x64;%SDK_ROOT%\Lib\%SDK_VER%\um\x64;%~dp0libstub"
set "PATH=%CL_DIR%;%RC_DIR%;%PATH%"

rem ----------------------------------------------------------------------------
rem 进程内 OCR（tesseract-sys）的构建期依赖。三样缺一不可：
rem   1) LIBCLANG_PATH —— bindgen 运行时要 libclang.dll（pip install libclang 装的那份）
rem   2) TESSERACT_* / LEPTONICA_* —— C 库的头文件与导入库位置（vendor 在 src-tauri/tesseract/）
rem      注意值必须是**反斜杠**路径：link.exe 的 /LIBPATH 不认正斜杠
rem   3) VS 自带的 CMake 进 PATH —— 本身不参与 OCR 构建，但依赖树里要用 cmake 的
rem      构建脚本没了它会直接失败，顺手一起给
rem 环境变量已定义时（外部 CI 想换 libclang 位置）尊重外部值。
rem ----------------------------------------------------------------------------
if not defined LIBCLANG_PATH (
  for /f "delims=" %%D in ('python -c "import clang,os;print(os.path.join(os.path.dirname(clang.__file__),'native'))" 2^>nul') do set "LIBCLANG_PATH=%%D"
)
set "TESSERACT_INCLUDE_PATHS=%~dp0..\src-tauri\tesseract\include"
set "TESSERACT_LINK_PATHS=%~dp0..\src-tauri\tesseract\lib\x64"
set "TESSERACT_LINK_LIBS=tesseract50"
set "LEPTONICA_INCLUDE_PATH=%~dp0..\src-tauri\tesseract\include"
set "LEPTONICA_LINK_PATHS=%~dp0..\src-tauri\tesseract\lib\x64"
set "LEPTONICA_LINK_LIBS=lept"
rem VS 自带的 CMake（MSVC_ROOT 上溯四级是 VS 根）：依赖树里要 cmake 的构建脚本没了它会直接失败
for %%C in ("%MSVC_ROOT%\..\..\..\..\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin") do (
  if exist "%%~C\cmake.exe" set "PATH=%PATH%;%%~C"
)

pushd "%~dp0..\src-tauri"
rem custom-protocol 必须带上：否则 release 版不内嵌 dist/，启动后去连 devUrl，窗口全黑。
rem 默认 lite：不带 ocr feature，tesseract 不参与编译，产物旁也不会有 DLL/tessdata。
cargo build --release --features custom-protocol
set "RC=%ERRORLEVEL%"
popd

if not "%RC%"=="0" exit /b %RC%

echo.
echo [ok] src-tauri\target\release\sharp-resume.exe  (lite, no OCR)
exit /b 0
