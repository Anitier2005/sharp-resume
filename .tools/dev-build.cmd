@echo off
rem ============================================================================
rem Sharp Resume · debug 编译（可见控制台）
rem
rem 和 build.bat 的区别只有一处：这里编 debug 档、不带 custom-protocol，
rem 产物给 tauri dev 用。存在的意义是「进度能看见」—— 全量编译几十个 crate
rem 时，日志直接打在窗口里，不用猜是不是卡住。
rem
rem 用法：.tools\dev-build.cmd
rem 产物：src-tauri\target\debug\sharp-resume.exe
rem ============================================================================
setlocal

rem 工具链位置会随 VS 重装变化（本机 2026-10-26 从 C 盘 BuildTools 换成了 E 盘 Community），
rem 按候选顺序探测，找到第一个存在的 VC\Tools\MSVC\<版本> 就用它。
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
  echo        set MSVC_ROOT to your VC\Tools\MSVC^<version^> folder and retry.
  pause
  exit /b 1
)

set "INCLUDE=%MSVC_ROOT%\include;%SDK_ROOT%\Include\%SDK_VER%\ucrt;%SDK_ROOT%\Include\%SDK_VER%\um;%SDK_ROOT%\Include\%SDK_VER%\shared;%SDK_ROOT%\Include\%SDK_VER%\winrt;%SDK_ROOT%\Include\%SDK_VER%\cppwinrt"
set "LIB=%MSVC_ROOT%\lib\x64;%SDK_ROOT%\Lib\%SDK_VER%\ucrt\x64;%SDK_ROOT%\Lib\%SDK_VER%\um\x64;%~dp0libstub"
set "PATH=%CL_DIR%;%RC_DIR%;%PATH%"

rem 进程内 OCR（tesseract-sys）的构建期依赖，与 build.bat 同一套；说明看那边
if not defined LIBCLANG_PATH (
  for /f "delims=" %%D in ('python -c "import clang,os;print(os.path.join(os.path.dirname(clang.__file__),'native'))" 2^>nul') do set "LIBCLANG_PATH=%%D"
)
set "TESSERACT_INCLUDE_PATHS=%~dp0..\src-tauri\tesseract\include"
set "TESSERACT_LINK_PATHS=%~dp0..\src-tauri\tesseract\lib\x64"
set "TESSERACT_LINK_LIBS=tesseract50"
set "LEPTONICA_INCLUDE_PATH=%~dp0..\src-tauri\tesseract\include"
set "LEPTONICA_LINK_PATHS=%~dp0..\src-tauri\tesseract\lib\x64"
set "LEPTONICA_LINK_LIBS=lept"
for %%C in ("%MSVC_ROOT%\..\..\..\..\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin") do (
  if exist "%%~C\cmake.exe" set "PATH=%PATH%;%%~C"
)

echo ============================================================
echo  MSVC : %MSVC_ROOT%
echo  SDK  : %SDK_ROOT% %SDK_VER%
echo  开始时间 %DATE% %TIME%
echo ============================================================
echo.

pushd "%~dp0..\src-tauri"
set "FEATURES="
if /i "%~1"=="ocr" set "FEATURES=--features ocr"
rem lite by default; with built-in OCR: dev-build.cmd ocr
cargo build --no-default-features %FEATURES%
set "RC=%ERRORLEVEL%"
popd

echo.
echo ============================================================
if "%RC%"=="0" echo  [OK] 编译成功�������� src-tauri\target\debug\sharp-resume.exe
if not "%RC%"=="0" echo  [FAIL] 编译失败，exit=%RC%，上面的红字就是原因
echo  结束时间 %DATE% %TIME%
echo ============================================================
echo.
echo 本窗口保留，方便回看输出。按任意键关闭。
pause >nul
exit /b %RC%
