param(
    [Parameter(Mandatory=$true)][string]$OutputDirectory,
    [switch]$Offline
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$output = [IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $output) { throw 'Output directory must be new' }
if (!(Test-Path -LiteralPath (Split-Path $output -Parent))) { throw 'Output parent must exist' }
$manifest = Join-Path $repo 'experiments/windows-cfapi/Cargo.toml'
$oldFlags = $env:CARGO_ENCODED_RUSTFLAGS
$oldTarget = $env:CARGO_TARGET_DIR
try {
    # Delimit arguments without shell splitting, including paths containing spaces.
    $flags = @()
    if ($env:USERPROFILE) { $flags += "--remap-path-prefix=$env:USERPROFILE=user" }
    $flags += "--remap-path-prefix=$repo=source"
    $env:CARGO_ENCODED_RUSTFLAGS = $flags -join [char]31
    $env:CARGO_TARGET_DIR = Join-Path $repo 'experiments/windows-cfapi/target'
    $arguments = @('build','--manifest-path',$manifest,'--features','fixture-probe','--release','--locked','-j','1')
    if ($Offline) { $arguments += '--offline' }
    & cargo @arguments
    if ($LASTEXITCODE) { throw 'CFAPI probe build failed' }
} finally {
    $env:CARGO_ENCODED_RUSTFLAGS = $oldFlags
    $env:CARGO_TARGET_DIR = $oldTarget
}
$binary = Join-Path $repo 'experiments/windows-cfapi/target/release/twodrive-cfapi-probe.exe'
$bytes = [IO.File]::ReadAllBytes($binary)
foreach ($encoding in @([Text.Encoding]::UTF8, [Text.Encoding]::Unicode)) {
    $strings = $encoding.GetString($bytes)
    foreach ($privatePath in @($repo, $env:USERPROFILE)) {
        if ($privatePath -and $strings.Contains($privatePath)) { throw 'Private source path embedded in binary' }
    }
}
$capabilityLines = @(& $binary)
if ($LASTEXITCODE) { throw 'Read-only native capability query failed' }
$capabilities = $capabilityLines[-1] | ConvertFrom-Json
if (!$capabilities.experimental -or $capabilities.production_mount -or !$capabilities.registration_requires_explicit_opt_in) {
    throw 'Unexpected probe capabilities'
}
New-Item -ItemType Directory -Path $output | Out-Null
Copy-Item -LiteralPath $binary -Destination $output
Copy-Item -LiteralPath (Join-Path $repo 'experiments/windows-cfapi/README.md') -Destination $output
$readme = Join-Path $output 'README.md'
$text = [IO.File]::ReadAllText($readme).Replace('../../doc/windows-cfapi-2026-10-21.md', 'windows-cfapi-2026-10-21.md')
[IO.File]::WriteAllText($readme, $text, [Text.UTF8Encoding]::new($false))
Copy-Item -LiteralPath (Join-Path $repo 'doc/windows-cfapi-2026-10-21.md') -Destination $output
Copy-Item -LiteralPath (Join-Path $repo 'LICENSE') -Destination $output
$capabilityLines | Set-Content -LiteralPath (Join-Path $output 'capabilities.jsonl') -Encoding utf8
Write-Output "Built isolated probe $($capabilities.version); no registration performed."
