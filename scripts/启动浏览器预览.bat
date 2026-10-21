@echo off
cd /d "%~dp0.."
title Context Vault - Vite Browser Preview
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "$env:Path = [System.Environment]::GetEnvironmentVariable('Path','User') + ';' + [System.Environment]::GetEnvironmentVariable('Path','Machine') + ';C:\Program Files\nodejs;' + $env:LOCALAPPDATA + '\Programs\nodejs;' + $env:Path; npm run dev"
if %ERRORLEVEL% NEQ 0 pause
