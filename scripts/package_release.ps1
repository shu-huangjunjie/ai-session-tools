# Build and package a clean portable release plus an allowlisted source archive.
param([switch]$SkipBuild)

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$version = (Get-Content -LiteralPath (Join-Path $projectRoot 'package.json') -Raw | ConvertFrom-Json).version
$packageDir = Join-Path $projectRoot 'releases\packages'
$portableName = "Context-Vault-$version-Windows-x64"
$sourceName = "Context-Vault-$version-source"
$portableStage = Join-Path $packageDir $portableName
$portableZip = Join-Path $packageDir "$portableName.zip"
$sourceZip = Join-Path $packageDir "$sourceName.zip"
$sumsPath = Join-Path $packageDir "SHA256SUMS-$version.txt"
foreach ($destination in @($portableStage, $portableZip, $sourceZip, $sumsPath)) {
    if (Test-Path -LiteralPath $destination) { throw "Already exists: $destination" }
}

if (-not $SkipBuild) { & (Join-Path $PSScriptRoot 'build.ps1') }
$releaseRoot = Join-Path $projectRoot 'releases\portable'
$releaseExe = Join-Path $releaseRoot 'Context-Vault.exe'
if (-not (Test-Path -LiteralPath $releaseExe)) { throw "Missing build: $releaseExe" }
$binaryVersion = (Get-Item -LiteralPath $releaseExe).VersionInfo.ProductVersion
if ($binaryVersion -notlike "$version*") { throw "Binary version $binaryVersion does not match $version" }

# Start with an empty data directory; existing runtime folders are never archived.
New-Item -ItemType Directory -Path (Join-Path $portableStage 'data') -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $portableStage 'backups') -Force | Out-Null
Copy-Item -LiteralPath $releaseExe -Destination (Join-Path $portableStage 'Context-Vault.exe')
$loader = Join-Path $releaseRoot 'WebView2Loader.dll'
if (Test-Path -LiteralPath $loader) { Copy-Item -LiteralPath $loader -Destination $portableStage }
$instructions = @"
Context Vault $version - Windows x64 便携版

使用方法
1. 将整个压缩包解压到可写目录，双击 Context-Vault.exe。
2. data 目录保存本地会话数据库，backups 目录保存备份；移动应用时请整体移动文件夹。
3. 此发布包使用空数据目录，可在应用中扫描或导入会话。
4. Windows 10/11 需要 Microsoft Edge WebView2 运行时；缺少时应用会显示安装提示。

从旧版本更新
先退出旧程序，再将本包的 Context-Vault.exe 复制到原应用目录中替换程序。
请保留原目录中的 data、backups 等文件夹。首次运行会自动备份并升级旧数据库。

本版更新
- 会话及工作文件夹支持分别置顶与取消置顶，重启后保留。
- 清空数据库支持数量预览、确认和自动备份。
- 批量保存支持带地址栏的目录选择窗口，并记住上次成功保存位置。
- 修复通过应用打开 VS Code 时的 Windows 路径前缀兼容问题。
"@
[IO.File]::WriteAllText((Join-Path $portableStage '使用说明.txt'), $instructions, [Text.UTF8Encoding]::new($false))

Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
function Write-Archive([string]$Destination, [string]$RootName, [string]$BasePath, [string[]]$Files, [string[]]$EmptyDirs = @()) {
    $zip = [IO.Compression.ZipFile]::Open($Destination, [IO.Compression.ZipArchiveMode]::Create)
    try {
        $null = $zip.CreateEntry("$RootName/")
        foreach ($directory in $EmptyDirs) { $null = $zip.CreateEntry("$RootName/$directory/") }
        foreach ($relative in ($Files | Sort-Object -Unique)) {
            $entry = "$RootName/" + $relative.Replace('\', '/')
            $null = [IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
                $zip, (Join-Path $BasePath $relative), $entry, [IO.Compression.CompressionLevel]::Optimal)
        }
    } finally { $zip.Dispose() }
}
$portableFiles = @(Get-ChildItem -LiteralPath $portableStage -File | ForEach-Object Name)
Write-Archive $portableZip $portableName $portableStage $portableFiles @('data', 'backups')

$sourceFolders = @('assets', 'public', 'src', 'scripts', 'src-tauri/src', 'src-tauri/tests',
    'src-tauri/icons', 'src-tauri/capabilities', 'src-tauri/.cargo')
$sourceFiles = @('.gitignore', '.gitattributes', 'README.md', 'CHANGELOG.md', '启动应用.bat', 'index.html',
    'package.json', 'package-lock.json', 'postcss.config.js', 'tailwind.config.js',
    'tsconfig.json', 'tsconfig.node.json', 'vite.config.ts', 'vitest.config.ts',
    'src-tauri/build.rs', 'src-tauri/Cargo.toml', 'src-tauri/Cargo.lock', 'src-tauri/tauri.conf.json')
foreach ($folder in $sourceFolders) {
    $folderPath = Join-Path $projectRoot $folder
    foreach ($file in (Get-ChildItem -LiteralPath $folderPath -File -Recurse -Force)) {
        if ($file.Name -match '^(\.env($|\.)|.*\.(db|sqlite|sqlite3)(-wal|-shm)?$|.*\.log$)') {
            throw "Unexpected local data in source folder: $($file.FullName)"
        }
        $sourceFiles += $file.FullName.Substring($projectRoot.Length + 1)
    }
}
Write-Archive $sourceZip $sourceName $projectRoot $sourceFiles
$sums = foreach ($archive in @($portableZip, $sourceZip)) {
    "{0}  {1}" -f (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant(), (Split-Path -Leaf $archive)
}
[IO.File]::WriteAllLines($sumsPath, $sums, [Text.UTF8Encoding]::new($false))
Get-Item -LiteralPath $portableZip, $sourceZip, $sumsPath | Select-Object FullName, Length
