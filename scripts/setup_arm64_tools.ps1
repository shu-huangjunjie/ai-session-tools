# Download verified Microsoft ARM64 compiler packages into the project, without
# changing Visual Studio or requiring administrator privileges. Uses the catalog
# belonging to an already-installed Visual Studio C++ x64 Build Tools instance.
param([string]$OutputRoot = '')

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
if (-not $OutputRoot) { $OutputRoot = Join-Path $projectRoot 'releases\build-tools\msvc-arm64' }
$OutputRoot = [IO.Path]::GetFullPath($OutputRoot)
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
if (-not (Test-Path -LiteralPath $vswhere)) { throw 'Install Visual Studio C++ x64 Build Tools first.' }
$instance = (& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -format json | ConvertFrom-Json) | Select-Object -First 1
if (-not $instance) { throw 'No Visual Studio C++ x64 Build Tools instance found.' }
$catalogPath = Join-Path $env:ProgramData "Microsoft\VisualStudio\Packages\_Instances\$($instance.instanceId)\catalog.json"
$catalog = Get-Content -LiteralPath $catalogPath -Raw | ConvertFrom-Json
$vcVersion = (Get-Content -LiteralPath (Join-Path $instance.installationPath 'VC\Auxiliary\Build\Microsoft.VCToolsVersion.default.txt') -Raw).Trim()
$vcParts = $vcVersion.Split('.')
$vcMajorMinor = [regex]::Escape("$($vcParts[0]).$($vcParts[1])")
$tools = $catalog.packages | Where-Object { $_.id -match "^Microsoft\.VC\.$vcMajorMinor\.\d+\.\d+\.Tools\.HostX64\.TargetARM64\.base$" } |
    Sort-Object { [version]$_.version } -Descending | Select-Object -First 1
if (-not $tools) { throw "No ARM64 tools in the installed Visual Studio $vcVersion catalog." }
$crtId = $tools.id -replace '\.Tools\.HostX64\.TargetARM64\.base$', '.CRT.ARM64.Desktop.base'
$resourcesId = $tools.id -replace '\.base$', '.Res.base'
$crt = $catalog.packages | Where-Object { $_.id -eq $crtId } | Select-Object -First 1
$storeCrtId = $crtId -replace '\.Desktop\.base$', '.Store.base'
$storeCrt = $catalog.packages | Where-Object { $_.id -eq $storeCrtId } | Select-Object -First 1
$resources = $catalog.packages | Where-Object { $_.id -eq $resourcesId -and $_.language -eq 'en-US' } | Select-Object -First 1
if (-not $crt -or -not $storeCrt -or -not $resources) { throw 'ARM64 CRT or English compiler resources are missing from the catalog.' }
# The Store CRT component also carries common ARM64 compatibility libraries such
# as oldnames.lib, which desktop applications need when linking the static CRT.
$packages = @($crt, $storeCrt, $tools, $resources)
$downloads = Join-Path $OutputRoot 'downloads'
$toolchain = Join-Path $OutputRoot 'toolchain'
New-Item -ItemType Directory -Path $downloads,$toolchain -Force | Out-Null
Add-Type -AssemblyName System.IO.Compression.FileSystem
foreach ($package in $packages) {
    foreach ($payload in $package.payloads) {
        $url = [Uri]$payload.url
        if ($url.Scheme -ne 'https' -or $url.Host -ne 'download.visualstudio.microsoft.com') { throw 'Unexpected Microsoft package source.' }
        $archive = Join-Path $downloads ([IO.Path]::GetFileName($payload.fileName))
        if (-not (Test-Path -LiteralPath $archive)) {
            Write-Output "Downloading $($package.id)..."
            Invoke-WebRequest -Uri $url -OutFile $archive -UseBasicParsing
        }
        if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne $payload.sha256) {
            throw "Checksum mismatch: $archive. Remove this incomplete download and retry."
        }
        $extracted = Join-Path $OutputRoot $package.id
        if (-not (Test-Path -LiteralPath $extracted)) { [IO.Compression.ZipFile]::ExtractToDirectory($archive, $extracted) }
        $contents = Join-Path $extracted 'Contents'
        foreach ($file in (Get-ChildItem -LiteralPath $contents -File -Recurse)) {
            $relative = $file.FullName.Substring($contents.Length + 1)
            $destination = [IO.Path]::GetFullPath((Join-Path $toolchain $relative))
            if (-not $destination.StartsWith($toolchain + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
                throw 'Package file escaped the isolated toolchain directory.'
            }
            New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
            Copy-Item -LiteralPath $file.FullName -Destination $destination -Force
        }
        Write-Output "Verified $($package.id)"
    }
}
$manifest = $packages | Select-Object id,version,payloads | ConvertTo-Json -Depth 6
[IO.File]::WriteAllText((Join-Path $OutputRoot 'package-manifest.json'), $manifest, [Text.UTF8Encoding]::new($false))
Write-Output "ARM64 toolchain ready: $toolchain"
