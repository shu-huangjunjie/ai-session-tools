@echo off
chcp 65001 >nul
cd /d "%~dp0"
title Context Vault

if exist "%~dp0releases\desktop\Context-Vault.exe" (
    start "" "%~dp0releases\desktop\Context-Vault.exe"
    exit
) else (
    echo [错误] 未找到 releases\desktop\Context-Vault.exe
    pause
)
