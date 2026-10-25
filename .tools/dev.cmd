@echo off
rem ============================================================================
rem Sharp Resume · 开发模式（热重载）
rem
rem 和 build.bat 是同一套问题的两面：cargo 要 cl.exe 与 CRT 导入库，
rem 而本机既没有把 MSVC 放进 PATH，SDK 的 ucrt 库还不全（详见 build.bat 注释）。
rem 这里复用 .tools\libstub 的空壳库，然后把环境交给 tauri dev。
rem
rem 热重载范围：
rem   - 前端（tsx / ts / css）：保存即生效，窗口不刷新、状态不丢
rem   - Rust（src-tauri/**）：tauri 自动增量重建并重启窗口（debug 档，十几秒）
rem   - tauri.conf.json：会重启窗口
rem
rem 只改界面时更轻的做法是 npm run dev，用浏览器开 http://localhost:1420，
rem 同样是热更新，只是没有文件对话框等 Tauri 能力（自动降级为浏览器下载）。
rem
rem 用法：.tools\dev.cmd
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

set "INCLUDE=%MSVC_ROOT%\include;%SDK_ROOT%\Include\%SDK_VER%\ucrt;%SDK_ROOT%\Include\%SDK_VER%\um;%SDK_ROOT%\Include\%SDK_VER%\shared;%SDK_ROOT%\Include\%SDK_VER%\winrt;%SDK_ROOT%\Include\%SDK_VER%\cppwinrt"
set "LIB=%MSVC_ROOT%\lib\x64;%SDK_ROOT%\Lib\%SDK_VER%\ucrt\x64;%SDK_ROOT%\Lib\%SDK_VER%\um\x64;%~dp0libstub"
set "PATH=%CL_DIR%;%RC_DIR%;%PATH%"

pushd "%~dp0.."
call npm run tauri dev
set "RC=%ERRORLEVEL%"
popd

exit /b %RC%
