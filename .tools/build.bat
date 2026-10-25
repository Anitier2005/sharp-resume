@echo off
rem ============================================================================
rem Sharp Resume · Windows release 构建
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

if not defined MSVC_ROOT set "MSVC_ROOT=C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\VC\Tools\MSVC\14.51.36231"
if not defined SDK_ROOT  set "SDK_ROOT=C:\Program Files (x86)\Windows Kits\10"
if not defined SDK_VER   set "SDK_VER=10.0.28000.0"

set "CL_DIR=%MSVC_ROOT%\bin\Hostx86\x64"
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

pushd "%~dp0..\src-tauri"
rem custom-protocol 必须带上：否则 release 版不内嵌 dist/，启动后去连 devUrl，窗口全黑。
cargo build --release --features custom-protocol
set "RC=%ERRORLEVEL%"
popd

if not "%RC%"=="0" exit /b %RC%

echo.
echo [ok] src-tauri\target\release\sharp-resume.exe
exit /b 0
