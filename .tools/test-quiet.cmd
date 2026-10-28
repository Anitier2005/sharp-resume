@echo off
rem Sharp Resume - cargo check wrapper (same env as build.bat, no linking)
rem Usage: .tools\check.cmd [extra cargo args]
@echo off
rem ============================================================================
rem
rem
rem
rem
rem
rem ============================================================================
setlocal

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
for %%C in ("%MSVC_ROOT%\..\..\..\..\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin") do (
  if exist "%%~C\cmake.exe" set "PATH=%PATH%;%%~C"
)

pushd "%~dp0..\src-tauri"
cargo test --lib %*
set "RC=%ERRORLEVEL%"
popd
exit /b %RC%
