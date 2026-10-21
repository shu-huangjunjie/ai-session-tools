function Test-PortBindable([int]$port) {
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $port)
    try {
        $listener.Start()
        return $true
    } catch {
        return $false
    } finally {
        $listener.Stop()
    }
}

function Get-SafeAppPort([int]$preferredPort) {
    if (Test-PortBindable $preferredPort) { return $preferredPort }
    $lastPort = [Math]::Min($preferredPort + 79, 65535)
    for ($candidate = $preferredPort + 1; $candidate -le $lastPort; $candidate++) {
        if (Test-PortBindable $candidate) { return $candidate }
    }
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
    try {
        $listener.Start()
        return ($listener.LocalEndpoint).Port
    } finally {
        $listener.Stop()
    }
}
