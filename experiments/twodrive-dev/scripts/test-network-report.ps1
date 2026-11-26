param(
    [string]$ScriptPath = (Join-Path $PSScriptRoot 'verify-network.ps1'),
    [switch]$LegacyReportDepth
)
$ErrorActionPreference = 'Stop'
# Load only trusted repository function definitions, never the server/restart
# entry point. Network/process operations in Wait-Stage are isolated below.
$tokens = $null
$parseErrors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($ScriptPath, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count -gt 0) { throw 'Verification script syntax errors.' }
$functions = @($ast.FindAll({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] }, $true))
foreach ($name in @('Save-Report', 'Wait-Stage', 'Read-NetworkMessages')) {
    $definition = @($functions | Where-Object { $_.Name -eq $name })
    if ($name -eq 'Read-NetworkMessages' -and $LegacyReportDepth -and $definition.Count -eq 0) { continue }
    if ($definition.Count -ne 1) { throw ('Expected exactly one function: ' + $name) }
    $source = $definition[0].Extent.Text
    # Bound the old provider graph to depth 3; do not reproduce an OOM in CI.
    if ($LegacyReportDepth -and $name -eq 'Save-Report') { $source = $source.Replace('-Depth 10', '-Depth 3') }
    . ([scriptblock]::Create($source))
}
function Route-Stamp { 'unchanged-synthetic-route' }
function Get-NetAdapter { [pscustomobject]@{ HardwareInterface = $true; Status = 'Up'; ifIndex = 11 } }
function Get-NetIPAddress {
    [pscustomobject]@{ InterfaceIndex = 11; IPAddress = '192.0.2.2' }
    [pscustomobject]@{ InterfaceIndex = 99; IPAddress = '198.18.0.1' }
}
function Get-NetTCPConnection { }
function Start-Sleep { }
$temporary = Join-Path ([IO.Path]::GetTempPath()) ('td-report-regression-' + [guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($temporary) | Out-Null
try {
    $runId = 'td-egress-' + ('0' * 32)
    $controlPath = Join-Path $temporary 'control.json'
    [IO.File]::WriteAllText($controlPath, ('{"run_id":"' + $runId + '","action":"next"}'))
    $child = $null
    $logPath = Join-Path $temporary 'synthetic.stderr'
    $expected = @('network: route=physical interface=synthetic fallback=reported-system/proxy')
    [IO.File]::WriteAllLines($logPath, @('non-network diagnostic') + $expected)
    $reportPath = Join-Path $temporary 'report.json'
    $report = [ordered]@{ run_id = $runId; status = 'starting'; stages = [ordered]@{}; cleanup = [ordered]@{} }
    $deadline = (Get-Date).AddSeconds(30)
    $script:current = @{ process = [Diagnostics.Process]::GetCurrentProcess(); stderr = $logPath }
    Wait-Stage 'auto' 'next'
    $json = [IO.File]::ReadAllText($reportPath)
    $decoded = $json | ConvertFrom-Json
    $messages = @($decoded.stages.auto.network_messages)
    if ($messages.Count -ne 1 -or $messages[0] -isnot [string] -or $messages[0] -ne $expected[0]) {
        throw 'Report network_messages must contain only plain strings.'
    }
    if ($json.Length -gt 2048 -or $json -match 'PSDrive|PSProvider|PSPath') { throw 'Report contains provider metadata or exceeds its bounded size.' }
    if ($decoded.status -ne 'auto_ready' -or $decoded.stages.auto.samples -ne 1 -or -not $decoded.stages.auto.proxy_fake_ip_present) { throw 'Stage report lost required values.' }
    if (-not $LegacyReportDepth) {
        if (@(Read-NetworkMessages (Join-Path $temporary 'missing')).Count -ne 0) { throw 'Missing log should have no messages.' }
        [IO.File]::WriteAllLines($logPath, @())
        if (@(Read-NetworkMessages $logPath).Count -ne 0) { throw 'Empty log should have no messages.' }
        [IO.File]::WriteAllLines($logPath, $expected + @('network: route=system/proxy fallback; controlled fixture'))
        if (@(Read-NetworkMessages $logPath).Count -ne 2) { throw 'Multiple network messages were lost.' }
    }
    if (-not $LegacyReportDepth) {
        # Use the same live redirection mechanism as the actual serve script.
        $liveLog = Join-Path $temporary 'live.stderr'
        $hostExe = [Diagnostics.Process]::GetCurrentProcess().MainModule.FileName
        $message = 'network: route=physical interface=writer-fixture'
        $command = "[Console]::Error.WriteLine('$message'); Start-Sleep -Seconds 120"
        $child = Start-Process -FilePath $hostExe -ArgumentList @('-NoProfile', '-Command', ('"' + $command + '"')) -RedirectStandardError $liveLog -RedirectStandardOutput (Join-Path $temporary 'live.stdout') -PassThru -WindowStyle Hidden
        $ready = (Get-Date).AddSeconds(15)
        while (-not (Test-Path -LiteralPath $liveLog) -or (Get-Item -LiteralPath $liveLog).Length -eq 0) {
            $child.Refresh()
            if ($child.HasExited -or (Get-Date) -gt $ready) { throw 'Redirected fixture did not start.' }
            [Threading.Thread]::Sleep(50)
        }
        try {
            $messages = @(Read-NetworkMessages $liveLog)
            if ($messages.Count -ne 1 -or $messages[0] -ne $message) { throw 'Active writer log lost its message.' }
            $script:current = @{ process = $child; stderr = $liveLog }
            [IO.File]::WriteAllText($controlPath, ('{"run_id":"' + $runId + '","action":"next"}'))
            Wait-Stage 'auto' 'next'
            $decoded = [IO.File]::ReadAllText($reportPath) | ConvertFrom-Json
            if (@($decoded.stages.auto.network_messages)[0] -ne $message) { throw 'Active stage/report lost its message.' }
        } catch {
            if ($null -ne $_.Exception.InnerException -and ($_.Exception.InnerException.HResult -band 0xffff) -eq 32) {
                throw 'Expected sharing violation reproduced by active redirected log reader.'
            }
            throw
        }
        $child.Refresh()
        if ($child.HasExited) { throw 'Writer must still be running when the report succeeds.' }
        Write-Host 'Active Start-Process writer and stage/report regression passed.'
    }
    Write-Host ('Compact stage/report regression passed on PowerShell ' + $PSVersionTable.PSVersion.ToString())
} finally {
    if ($null -ne $child) {
        $child.Refresh()
        if (-not $child.HasExited) { Stop-Process -Id $child.Id -Force; $child.WaitForExit(10000) | Out-Null }
        $child.Dispose()
    }
    Remove-Item -LiteralPath $temporary -Recurse -Force
}
