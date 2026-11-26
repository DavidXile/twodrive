param(
    [Parameter(Mandatory = $true)][string]$State,
    [Parameter(Mandatory = $true)][string]$Root,
    [Parameter(Mandatory = $true)][string]$ExpectedSha256,
    [ValidateRange(120, 3600)][int]$TimeoutSeconds = 1800
)
# Run only against a disposable share and an already paired test server.
# TwoDrive carries files and bounded next/finish/abort signals, not remote shell.
$ErrorActionPreference = 'Stop'
$statePath = (Resolve-Path -LiteralPath $State).Path.TrimEnd('\')
$rootPath = (Resolve-Path -LiteralPath $Root).Path.TrimEnd('\')
$bundle = (Resolve-Path -LiteralPath $PSScriptRoot).Path.TrimEnd('\')
if (-not $bundle.StartsWith($rootPath + '\', [StringComparison]::OrdinalIgnoreCase)) {
    throw 'The validation bundle must be inside the disposable share.'
}
$runId = Split-Path $bundle -Leaf
if ($runId -notmatch '^td-egress-[a-f0-9]{32}$') { throw 'Unexpected validation bundle name.' }
$sourceExe = Join-Path $bundle 'twodrive-dev.exe'
if ((Get-FileHash -LiteralPath $sourceExe -Algorithm SHA256).Hash -ne $ExpectedSha256) {
    throw 'Executable SHA256 differs from the separately supplied expected value.'
}
$old = @(Get-CimInstance Win32_Process -Filter "Name='twodrive-dev.exe'" | Where-Object {
    $s = [regex]::Match($_.CommandLine, '--state\s+(?:"(?<p>[^"]+)"|(?<p>\S+))').Groups['p'].Value
    $r = [regex]::Match($_.CommandLine, '--root\s+(?:"(?<p>[^"]+)"|(?<p>\S+))').Groups['p'].Value
    $s.TrimEnd('\') -eq $statePath -and $r.TrimEnd('\') -eq $rootPath -and $_.CommandLine -match '(?:^|\s)serve(?:\s|$)'
})
if ($old.Count -ne 1 -or -not $old[0].ExecutablePath) { throw 'Expected exactly one matching existing test serve process.' }
$oldExe = $old[0].ExecutablePath
if ($oldExe.StartsWith($bundle + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'The original server must be outside this temporary bundle.' }
$oldArgs = [regex]::Match($old[0].CommandLine, '^\s*(?:"[^"]+"|\S+)\s*(?<args>.*)$').Groups['args'].Value
# Never mint a new invitation while restoring the already paired server.
$oldArgs = [regex]::Replace($oldArgs, '(?:^|\s)--invite-file(?:=\S+|\s+(?:"[^"]+"|\S+))', '')
$preserved = @{}
foreach ($name in @('identity.json', 'control.json')) {
    $file = Join-Path $statePath $name
    if (Test-Path -LiteralPath $file) { $preserved[$name] = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash }
}
$runtime = Join-Path ([IO.Path]::GetTempPath()) $runId
if (Test-Path -LiteralPath $runtime) { throw 'Temporary runtime already exists; refusing to overwrite it.' }
$exe = Join-Path $runtime 'twodrive-dev.exe'
$reportPath = Join-Path $bundle 'windows-report.json'
$controlPath = Join-Path $bundle 'control.json'
$report = [ordered]@{ run_id = $runId; status = 'starting'; executable_sha256 = $ExpectedSha256.ToLowerInvariant(); stages = [ordered]@{}; cleanup = [ordered]@{} }
$deadline = (Get-Date).AddSeconds($TimeoutSeconds)
$current = $null
$stoppedOld = $false

function Save-Report {
    $text = $report | ConvertTo-Json -Depth 10
    $temp = $reportPath + '.tmp'
    [IO.File]::WriteAllText($temp, $text, (New-Object Text.UTF8Encoding($false)))
    Move-Item -LiteralPath $temp -Destination $reportPath -Force
}
function Read-NetworkMessages([string]$path) {
    if (-not [IO.File]::Exists($path)) { return }
    # Get-Content adds PSDrive/PSProvider metadata to strings on PowerShell
    # 5.1. Deep JSON serialization follows those graphs and can exhaust RAM.
    # Start-Process keeps its redirected writer open. The reader must also
    # permit that existing write access, unlike File.ReadAllLines/OpenRead.
    $share = [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete
    $stream = New-Object IO.FileStream($path, [IO.FileMode]::Open, [IO.FileAccess]::Read, $share)
    $reader = New-Object IO.StreamReader($stream)
    try {
        while ($null -ne ($line = $reader.ReadLine())) {
            if ($line.StartsWith('network:')) { $line }
        }
    } finally { $reader.Dispose() }
}
function Route-Stamp {
    $routes = Get-NetRoute | Sort-Object AddressFamily, InterfaceIndex, DestinationPrefix, NextHop | Select-Object AddressFamily, InterfaceIndex, DestinationPrefix, NextHop, RouteMetric
    $bytes = [Text.Encoding]::UTF8.GetBytes(($routes | ConvertTo-Json -Compress))
    $sha = [Security.Cryptography.SHA256]::Create()
    try { return [BitConverter]::ToString($sha.ComputeHash($bytes)) } finally { $sha.Dispose() }
}
function Start-TestServer([string]$stage, [string[]]$extra) {
    $stdout = Join-Path $runtime ($stage + '.stdout')
    $stderr = Join-Path $runtime ($stage + '.stderr')
    $arguments = @('--state', $statePath, 'serve', '--root', $rootPath, '--write') + $extra
    foreach ($arg in $arguments) { if ($arg.Contains('"') -or $arg.EndsWith('\')) { throw 'Unsupported quoted/trailing-slash argument.' } }
    $quoted = ($arguments | ForEach-Object { '"' + $_ + '"' }) -join ' '
    $process = Start-Process -FilePath $exe -ArgumentList $quoted -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru -WindowStyle Hidden
    try {
        $readyDeadline = (Get-Date).AddSeconds(75)
        while ((Get-Date) -lt $readyDeadline) {
            $process.Refresh()
            if ($process.HasExited) { throw "Test server exited during $stage startup." }
            if ((Test-Path $stdout) -and ((Get-Content -LiteralPath $stdout -Raw) -match 'Serving device=')) {
                return @{ process = $process; stdout = $stdout; stderr = $stderr }
            }
            Start-Sleep -Milliseconds 250
        }
        throw "Test server startup timed out in $stage."
    } catch {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        $process.WaitForExit(10000) | Out-Null
        throw
    }
}
function Stop-TestServer {
    if ($null -ne $script:current) {
        $p = $script:current.process
        $p.Refresh()
        if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force; $p.WaitForExit(10000) | Out-Null }
        $script:current = $null
    }
}
function Wait-Stage([string]$stage, [string]$nextAction) {
    $initialRoutes = Route-Stamp
    $stats = [ordered]@{ samples = 0; public_tcp_samples = 0; physical_tcp_samples = 0; nonphysical_tcp_samples = 0; route_unchanged = $true; proxy_fake_ip_present = $false; network_messages = @() }
    $report.stages[$stage] = $stats
    $report.status = $stage + '_ready'
    while ((Get-Date) -lt $deadline) {
        $script:current.process.Refresh()
        if ($script:current.process.HasExited) { throw "Test server exited during $stage." }
        $adapters = @(Get-NetAdapter -IncludeHidden | Where-Object { $_.HardwareInterface -and $_.Status -eq 'Up' })
        $indices = @($adapters | ForEach-Object { $_.ifIndex })
        $addresses = @(Get-NetIPAddress | Where-Object { $_.InterfaceIndex -in $indices } | ForEach-Object { $_.IPAddress })
        $stats.proxy_fake_ip_present = @((Get-NetIPAddress -AddressFamily IPv4) | Where-Object { $_.IPAddress -match '^198\.(18|19)\.' }).Count -gt 0
        $connections = @(Get-NetTCPConnection -OwningProcess $script:current.process.Id -State Established -ErrorAction SilentlyContinue | Where-Object { $_.RemotePort -eq 443 -and $_.RemoteAddress -notin @('127.0.0.1', '::1') })
        foreach ($connection in $connections) {
            $stats.public_tcp_samples++
            if ($connection.LocalAddress -in $addresses) { $stats.physical_tcp_samples++ } else { $stats.nonphysical_tcp_samples++ }
        }
        $stats.samples++
        if ((Route-Stamp) -ne $initialRoutes) { $stats.route_unchanged = $false }
        $stats.network_messages = @(Read-NetworkMessages $script:current.stderr)
        Save-Report
        if (Test-Path -LiteralPath $controlPath) {
            try { $control = Get-Content -LiteralPath $controlPath -Raw | ConvertFrom-Json } catch { $control = $null }
            if ($null -ne $control -and $control.run_id -eq $runId) {
                if ($control.action -eq 'abort') { throw 'Linux verifier requested abort.' }
                if ($control.action -eq $nextAction) {
                    Remove-Item -LiteralPath $controlPath
                    # Let the WebDAV PUT acknowledgement leave before restarting.
                    Start-Sleep -Seconds 2
                    return
                }
            }
        }
        Start-Sleep -Seconds 1
    }
    throw 'Validation deadline elapsed.'
}

try {
    New-Item -ItemType Directory -Path $runtime | Out-Null
    Copy-Item -LiteralPath $sourceExe -Destination $exe
    Stop-Process -Id $old[0].ProcessId -Force
    $stoppedOld = $true
    Start-Sleep -Milliseconds 500
    $current = Start-TestServer 'auto' @()
    Wait-Stage 'auto' 'next'
    Stop-TestServer
    $current = Start-TestServer 'fallback' @('--network-interface', ('td-missing-' + [guid]::NewGuid().ToString('N')))
    Wait-Stage 'fallback' 'finish'
    $report.status = 'completed'
} catch {
    $report.status = 'failed'
    $report.error_type = $_.Exception.GetType().FullName
    Write-Warning $_.Exception.Message
} finally {
    try { Stop-TestServer } catch { $report.cleanup.test_server_stop_failed = $true }
    $unchanged = $true
    foreach ($name in $preserved.Keys) {
        try {
            if ((Get-FileHash -LiteralPath (Join-Path $statePath $name) -Algorithm SHA256).Hash -ne $preserved[$name]) { $unchanged = $false }
        } catch { $unchanged = $false }
    }
    $report.cleanup.identity_and_grants_preserved = $unchanged
    if ($stoppedOld) {
        try {
            $restored = Start-Process -FilePath $oldExe -ArgumentList $oldArgs -PassThru -WindowStyle Hidden
            Start-Sleep -Seconds 3
            $restored.Refresh()
            $report.cleanup.original_server_restarted = -not $restored.HasExited
        } catch { $report.cleanup.original_server_restarted = $false }
    }
    $bundleClean = $true
    foreach ($name in @('twodrive-dev.exe', 'verify-network.ps1', 'manifest.json', 'control.json', 'probe-auto.bin', 'probe-auto.bin.moved', 'probe-linux-fallback.bin', 'probe-linux-fallback.bin.moved', 'probe-windows-fallback.bin', 'probe-windows-fallback.bin.moved')) {
        $file = Join-Path $bundle $name
        try { if (Test-Path -LiteralPath $file) { Remove-Item -LiteralPath $file -Force } } catch { $bundleClean = $false }
    }
    $report.cleanup.bundle_payloads_removed = $bundleClean
    try { if (Test-Path -LiteralPath $runtime) { Remove-Item -LiteralPath $runtime -Recurse -Force } } catch { Write-Warning 'Temporary runtime cleanup failed.' }
    $report.cleanup.temporary_runtime_removed = -not (Test-Path -LiteralPath $runtime)
    Save-Report
}
Write-Host 'Validation ended. The Linux verifier will collect the report and remove this temporary bundle.'
if ($report.status -ne 'completed' -or -not $report.cleanup.identity_and_grants_preserved -or -not $report.cleanup.original_server_restarted -or -not $report.cleanup.temporary_runtime_removed -or -not $report.cleanup.bundle_payloads_removed -or $report.cleanup.test_server_stop_failed) { exit 1 }
