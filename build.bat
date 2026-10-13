@echo off
setlocal
cd /d "%~dp0"
python tools\build.py
if errorlevel 1 (
    echo Build failed. See the error above.
    exit /b 1
)
echo Ready: %~dp0AudioRouter.exe
