@echo off
setlocal
echo ========================================
echo   M3U8 Downloader Build Script
echo ========================================

del ".\m3u8-downloader-gui.exe"

echo [1/2] Building m3u8-downloader-gui (Release)...
cargo build --release -p m3u8-downloader-gui
if %ERRORLEVEL% neq 0 (
    echo.
    echo [ERROR] Build failed!
    pause
    exit /b %ERRORLEVEL%
)

echo [2/2] Deploying executable...
copy /y "target\release\m3u8-downloader-gui.exe" ".\m3u8-downloader-gui.exe"
if %ERRORLEVEL% neq 0 (
    echo.
    echo [ERROR] Failed to copy executable to current directory.
    pause
    exit /b %ERRORLEVEL%
)

echo.
echo [SUCCESS] Build complete!
echo Executable: .\m3u8-downloader-gui.exe
echo.
pause
