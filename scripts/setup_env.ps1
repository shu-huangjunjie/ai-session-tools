# Environment setup & diagnostic script for Context Vault (TS + Rust)
# Run with: powershell -ExecutionPolicy Bypass -File .\scripts\setup_env.ps1

Write-Host "=================================================" -ForegroundColor Cyan
Write-Host " Context Vault (TS + Rust) 环境检查与安装配置" -ForegroundColor Cyan
Write-Host "=================================================" -ForegroundColor Cyan

$NodeInstalled = $false
$RustInstalled = $false

# 1. Check Node.js
try {
    $nodeVer = node -v 2>$null
    if ($nodeVer) {
        Write-Host "[√] 检测到 Node.js: $nodeVer" -ForegroundColor Green
        $NodeInstalled = $true
    }
} catch {}

if (-not $NodeInstalled) {
    Write-Host "[!] 未检测到 Node.js (v18+)" -ForegroundColor Yellow
    Write-Host "    正在尝试使用 winget 安装 Node.js LTS..." -ForegroundColor Gray
    try {
        winget install --id OpenJS.NodeJS.LTS --silent --accept-source-agreements --accept-package-agreements
        Write-Host "[√] Node.js 安装完成，请重新打开终端加载环境变量。" -ForegroundColor Green
    } catch {
        Write-Host "    [×] winget 自动安装受限，请访问官网手动下载并安装 Node.js LTS:" -ForegroundColor Red
        Write-Host "        https://nodejs.org/" -ForegroundColor Yellow
    }
}

# 2. Check Rust
try {
    $rustVer = rustc --version 2>$null
    if ($rustVer) {
        Write-Host "[√] 检测到 Rust: $rustVer" -ForegroundColor Green
        $RustInstalled = $true
    }
} catch {}

if (-not $RustInstalled) {
    Write-Host "[!] 未检测到 Rust 编译环境" -ForegroundColor Yellow
    Write-Host "    正在尝试使用 winget 安装 Rustup..." -ForegroundColor Gray
    try {
        winget install --id Rustlang.Rustup --silent --accept-source-agreements --accept-package-agreements
        Write-Host "[√] Rustup 安装完成，请重新打开终端运行 'rustup default stable'。" -ForegroundColor Green
    } catch {
        Write-Host "    [×] winget 自动安装受限，请访问官网手动下载 rustup-init.exe:" -ForegroundColor Red
        Write-Host "        https://rustup.rs/" -ForegroundColor Yellow
    }
}

Write-Host "`n依赖安装完成后，在项目根目录运行：" -ForegroundColor Cyan
Write-Host "  npm install" -ForegroundColor White
Write-Host "  .\scripts\run.ps1" -ForegroundColor White
Write-Host "=================================================" -ForegroundColor Cyan