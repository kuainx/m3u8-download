@echo off
setlocal enabledelayedexpansion

REM 支持拖放, 先进入脚本所在目录
cd /d %~dp0

REM 检查参数
if "%~1"=="" (
    echo Usage: %~nx0 ^<folder_dir^>
    echo Example: %~nx0 "C:\MyVideos"
    pause
    exit /b 1
)

REM 验证文件夹是否存在
set "folder=%~1"
if not exist "%folder%" (
    echo Error: Folder "%folder%" does not exist.
    pause
    exit /b 1
)

REM 检查ffmpeg.exe是否存在
if not exist "ffmpeg.exe" (
    echo Error: ffmpeg.exe not found in the current directory.
    echo Please ensure ffmpeg.exe is in the same directory as this script.
    pause
    exit /b 1
)

echo Starting metadata repair for MP4 files in "%folder%"
echo.

REM 遍历文件夹中所有MP4文件
for %%f in ("%folder%\*.mp4") do (
    echo Processing: "%%~nxf"
    
    REM 检测元数据是否损坏（检查ffmpeg输出中是否包含特定字符串）
    ffmpeg -i "%%f" 2>&1 | findstr /C:"Detected creation time before 1970" >nul
    
    if errorlevel 1 (
        echo   No metadata issues detected.
    ) else (
        echo   Detected metadata corruption. Attempting repair...
        set "tempfile=%%~dpnf_fixed.mp4"
        
        REM 使用ffmpeg修复元数据（复制流，不重新编码）
        ffmpeg -i "%%f" -c copy "!tempfile!" -y 2>nul
        
        if errorlevel 1 (
            echo   Repair failed: ffmpeg encountered an error.
            if exist "!tempfile!" del "!tempfile!"
        ) else (
			if exist "!tempfile!" (
				REM 直接覆盖原文件
				del "%%f"
				move "!tempfile!" "%%f" >nul
				echo   Successfully repaired. Original file has been replaced.
			) else (
				echo   Repair failed: temporary file was not created.
			)
        )
    )
    echo.
)

echo Metadata repair process completed.
endlocal
pause
