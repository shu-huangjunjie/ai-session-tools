# Build script for Context Vault (TS + Rust)
# Run with: powershell -ExecutionPolicy Bypass -File .\scripts\build.ps1
# Update a portable folder elsewhere (its data and backups are kept):
#   .\scripts\build.ps1 -PortableDir "G:\tools\Context restoration app\Context Vault"

param(
    [string]$PortableDir = ""
)

$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

# Refresh PATH from registry so newly installed Node/Rust are immediately available
$env:Path = [System.Environment]::GetEnvironmentVariable("Path","User") + ";" + [System.Environment]::GetEnvironmentVariable("Path","Machine")

Write-Host "=================================================" -ForegroundColor Cyan
Write-Host " Context Vault (TS + Rust) 便携程序构建脚本" -ForegroundColor Cyan
Write-Host "=================================================" -ForegroundColor Cyan

$ProjectRoot = Split-Path -Parent $PSScriptRoot
Set-Location $ProjectRoot

# 1. Install frontend dependencies if needed
if (-not (Test-Path "node_modules")) {
    Write-Host "[1/3] 正在安装前端依赖 (npm install)..." -ForegroundColor Yellow
    npm ci
    if ($LASTEXITCODE -ne 0) { throw "npm ci failed with exit code $LASTEXITCODE" }
}

# 2. Build frontend assets with relative path
Write-Host "[2/3] 正在构建前端静态资产 (npm run build)..." -ForegroundColor Yellow
npm run build
if ($LASTEXITCODE -ne 0) { throw "Frontend build failed with exit code $LASTEXITCODE" }

# 3. Build Tauri binary
Write-Host "[3/3] 正在编译 Rust 后端与打包便携程序..." -ForegroundColor Yellow
Set-Location "$ProjectRoot\src-tauri"
# Only the MSVC toolchain links the WebView2 loader statically; MinGW builds load
# WebView2Loader.dll at run time, so it has to be shipped next to the exe. Use the
# MSVC toolchain whenever it and the Visual Studio C++ build tools are installed,
# even if another toolchain is the default.
$ToolchainArgs = @()
$VsWhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$HasMsvcTools = (Test-Path $VsWhere) -and
    (& $VsWhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath)
if ($HasMsvcTools -and (Get-Command rustup -ErrorAction SilentlyContinue) -and
    (rustup toolchain list | Select-String -SimpleMatch 'stable-x86_64-pc-windows-msvc')) {
    $ToolchainArgs = @('+stable-x86_64-pc-windows-msvc')
}
cargo @ToolchainArgs build --release --locked
if ($LASTEXITCODE -ne 0) { throw "Rust build failed with exit code $LASTEXITCODE" }
$RustHost = ((rustc @ToolchainArgs -vV | Select-String '^host:').ToString() -split '\s+')[1]
Set-Location $ProjectRoot
$LoaderDll = $null
if ($RustHost -notlike '*msvc') {
    $LoaderDll = Get-ChildItem "$ProjectRoot\src-tauri\target\release\build\webview2-com-sys-*\out\x64\WebView2Loader.dll" |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if (-not $LoaderDll) { throw "Missing WebView2Loader.dll for the $RustHost build" }
    Write-Host "    $RustHost 工具链：随程序附带 WebView2Loader.dll" -ForegroundColor Yellow
}

$OutDir = if ($PortableDir) { $PortableDir } else { Join-Path $ProjectRoot "releases\portable" }
$DesktopOutDir = Join-Path $ProjectRoot "releases\desktop"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
New-Item -ItemType Directory -Force -Path $DesktopOutDir | Out-Null

# The C runtime is linked statically (src-tauri\.cargo\config.toml), and so is the
# WebView2 loader in MSVC builds; then the exe is the only file the app needs besides
# its data folder.
$ReleaseExe = Join-Path $ProjectRoot "src-tauri\target\release\context-vault.exe"

if (Test-Path $ReleaseExe) {
    Copy-Item $ReleaseExe -Destination (Join-Path $OutDir "Context-Vault.exe") -Force
    Copy-Item $ReleaseExe -Destination (Join-Path $DesktopOutDir "Context-Vault.exe") -Force
    foreach ($dir in @($OutDir, $DesktopOutDir)) {
        if ($LoaderDll) {
            Copy-Item $LoaderDll.FullName -Destination $dir -Force
        } else {
            # Left over from an earlier MinGW build; the MSVC exe does not load it.
            Remove-Item (Join-Path $dir "WebView2Loader.dll") -ErrorAction SilentlyContinue
        }
    }
    # A data folder next to the exe switches the app to portable mode
    New-Item -ItemType Directory -Force -Path (Join-Path $OutDir "data") | Out-Null
    Write-Host "`n[√] 便携版程序构建成功！输出目录：" -ForegroundColor Green
    Write-Host "    $OutDir\Context-Vault.exe" -ForegroundColor White
    Write-Host "    整个文件夹即为完整程序，复制到任何 Windows 10/11 电脑上双击 Context-Vault.exe 即可运行。" -ForegroundColor Cyan
} else {
    throw "Missing build output: $ReleaseExe"
}
