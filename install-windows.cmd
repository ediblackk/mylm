@echo off
REM mylm Windows Installation Script (Batch)
REM Usage: Just double-click or run: install-windows.cmd

echo ================================================================================
echo    mylm Windows Installation Script
echo ================================================================================
echo.

REM Check for PowerShell 7
set "PS7PATH=C:\Program Files\PowerShell\7\pwsh.exe"
if exist "%PS7PATH%" (
    echo Found PowerShell 7 at: %PS7PATH%
    echo Launching installer...
    "%PS7PATH%" -ExecutionPolicy Bypass -File "%~dp0install.ps1"
    pause
) else (
    echo PowerShell 7 not found at: %PS7PATH%
    echo.
    echo Please install PowerShell 7 first:
    echo   Option 1: winget install Microsoft.PowerShell
    echo   Option 2: Download from https://aka.ms/powershell
    echo.
    pause
)
