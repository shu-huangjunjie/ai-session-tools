# Development runner for Context Vault (TS + Rust)
# Run with: powershell -ExecutionPolicy Bypass -File .\scripts\run.ps1

$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$OutputEncoding = [System.Text.Encoding]::UTF8

# Set Console Title
try {
    $host.ui.RawUI.WindowTitle = "Context Vault (TS + Rust) - 开发模式"
} catch {}

# Refresh PATH from registry so newly installed Node/Rust are immediately available in any shell
$userPath = [System.Environment]::GetEnvironmentVariable("Path", "User")
$sysPath = [System.Environment]::GetEnvironmentVariable("Path", "Machine")
$extraPaths = @(
    "C:\Program Files\nodejs",
    "$env:LOCALAPPDATA\Programs\nodejs",
    "$env:USERPROFILE\.cargo\bin"
)
$env:Path = "$userPath;$sysPath;$($extraPaths -join ';');$env:Path"

$ProjectRoot = Split-Path -Parent $PSScriptRoot
Set-Location $ProjectRoot

Write-Host "=================================================" -ForegroundColor Cyan
Write-Host "  Context Vault (TS + Rust) 开发调试环境启动器" -ForegroundColor Cyan
Write-Host "  [*] 日常使用可双击根目录 [启动应用.bat] 或 releases\desktop\Context-Vault.exe" -ForegroundColor Yellow
Write-Host "=================================================" -ForegroundColor Cyan
Write-Host ""

. (Join-Path $PSScriptRoot "ports.ps1")

$DefaultPort = 28420
Write-Host "[1/3] 正在检测并分配独立可用端口（避开系统冲突）..." -ForegroundColor Cyan
$AppPort = Get-SafeAppPort $DefaultPort
Write-Host "      本次运行绑定端口: $AppPort" -ForegroundColor Green

# 同步端口环境变量供 Vite 读取
$env:VITE_PORT = $AppPort.ToString()

$DevConfigPath = $null

# 2. 依赖检查
if (-not (Test-Path "node_modules")) {
    Write-Host "[2/3] 首次运行，正在安装前端依赖 (npm install)..." -ForegroundColor Cyan
    npm ci
    if ($LASTEXITCODE -ne 0) { throw "npm ci failed with exit code $LASTEXITCODE" }
} else {
    Write-Host "[2/3] 前端依赖库就绪。" -ForegroundColor Green
}

# 3. 启动应用
$hasRust = $false
try {
    if (Get-Command cargo -ErrorAction SilentlyContinue) {
        $hasRust = $true
    }
} catch {}

try {
    if ($hasRust) {
        Write-Host "[3/3] 启动 Tauri 桌面端开发环境（带热重载与本地 SQLite 后端）..." -ForegroundColor Green
        Write-Host "提示：测试期间请保持此窗口打开。关闭桌面窗口或按 Ctrl+C 可退出。" -ForegroundColor Gray
        Write-Host ""
        $DevConfigPath = Join-Path $env:TEMP ("context-vault-dev-" + [guid]::NewGuid().ToString('N') + ".json")
        $DevOverride = @{ build = @{ devUrl = "http://127.0.0.1:$AppPort" } } | ConvertTo-Json -Depth 3
        [IO.File]::WriteAllText($DevConfigPath, $DevOverride, [Text.UTF8Encoding]::new($false))
        npm run tauri -- dev --config $DevConfigPath
        if ($LASTEXITCODE -ne 0) { throw "Tauri development mode failed with exit code $LASTEXITCODE" }
    } else {
        Write-Host "[3/3] 未检测到 Rust/Cargo，启动纯 Web 界面预览调试模式..." -ForegroundColor Yellow
        npm run dev
    }
} catch {
    Write-Host ""
    Write-Host "[!] 运行过程中遇到异常: $_" -ForegroundColor Red
} finally {
    if ($DevConfigPath -and (Test-Path -LiteralPath $DevConfigPath)) { Remove-Item -LiteralPath $DevConfigPath }
    Write-Host ""
    Write-Host "=================================================" -ForegroundColor Cyan
    Write-Host " 服务已停止。按任意键关闭此窗口..." -ForegroundColor Gray
    Write-Host "=================================================" -ForegroundColor Cyan
    try {
        $null = [System.Console]::ReadKey($true)
    } catch {
        Read-Host "按回车键退出"
    }
}
