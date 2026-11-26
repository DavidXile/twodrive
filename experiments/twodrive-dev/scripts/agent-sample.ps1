param([Parameter(Mandatory=$true)][int]$ProcessId, [Parameter(Mandatory=$true)][string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
function Route-Stamp {
    $routes = Get-NetRoute | Sort-Object AddressFamily, InterfaceIndex, DestinationPrefix, NextHop | Select-Object AddressFamily, InterfaceIndex, DestinationPrefix, NextHop, RouteMetric
    $sha = [Security.Cryptography.SHA256]::Create()
    try { [BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes(($routes | ConvertTo-Json -Compress)))) } finally { $sha.Dispose() }
}
$stats = [ordered]@{ samples=0; public_tcp_samples=0; physical_tcp_samples=0; nonphysical_tcp_samples=0; route_unchanged=$true; proxy_fake_ip_present=$false }
try {
    $initial = Route-Stamp
    while (-not (Test-Path -LiteralPath (Join-Path $OutputDirectory 'stop')) -and (Get-Process -Id $ProcessId -ErrorAction SilentlyContinue)) {
        $indices = @(Get-NetAdapter -IncludeHidden | Where-Object { $_.HardwareInterface -and $_.Status -eq 'Up' } | ForEach-Object { [int]$_.ifIndex })
        $addresses = @(Get-NetIPAddress | Where-Object { $_.InterfaceIndex -in $indices } | ForEach-Object { [string]$_.IPAddress })
        $stats.proxy_fake_ip_present = @(Get-NetIPAddress -AddressFamily IPv4 | Where-Object { $_.IPAddress -match '^198\.(18|19)\.' }).Count -gt 0
        $connections = @(Get-NetTCPConnection -OwningProcess $ProcessId -State Established -ErrorAction SilentlyContinue | Where-Object { $_.RemotePort -eq 443 -and $_.RemoteAddress -notin @('127.0.0.1','::1') })
        foreach ($c in $connections) { $stats.public_tcp_samples++; if ($c.LocalAddress -in $addresses) { $stats.physical_tcp_samples++ } else { $stats.nonphysical_tcp_samples++ } }
        $stats.samples++
        if ((Route-Stamp) -ne $initial) { $stats.route_unchanged=$false }
        [IO.File]::WriteAllText((Join-Path $OutputDirectory 'stats.json'), ($stats | ConvertTo-Json -Compress))
        Start-Sleep -Seconds 1
    }
} catch {
    $stats.error_type = [string]$_.Exception.GetType().FullName
    [IO.File]::WriteAllText((Join-Path $OutputDirectory 'stats.json'), ($stats | ConvertTo-Json -Compress))
}
