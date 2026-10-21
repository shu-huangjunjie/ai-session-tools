@echo off
chcp 65001 >nul
cd /d "%~dp0.."
title Context Vault - Tauri Dev Mode
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0run.ps1"
if %ERRORLEVEL% NEQ 0 (
    echo.
    echo =======================================================
    echo Launch failed or terminated with code %ERRORLEVEL%.
    echo =======================================================
    pause
)
