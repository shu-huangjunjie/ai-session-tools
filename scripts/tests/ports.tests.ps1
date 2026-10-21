$ErrorActionPreference = 'Stop'
. (Join-Path (Split-Path -Parent $PSScriptRoot) 'ports.ps1')
$occupied = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
try {
    $occupied.Start()
    $occupiedPort = ($occupied.LocalEndpoint).Port
    $selectedPort = Get-SafeAppPort $occupiedPort
    if ($selectedPort -eq $occupiedPort) { throw 'Selected an occupied port.' }
    if (Test-PortBindable $occupiedPort) { throw 'The original listener was interrupted.' }
    if (-not (Test-PortBindable $selectedPort)) { throw 'The selected port is not available.' }
    Write-Output 'PASS: another listener stays running while an available port is selected.'
} finally {
    $occupied.Stop()
}
