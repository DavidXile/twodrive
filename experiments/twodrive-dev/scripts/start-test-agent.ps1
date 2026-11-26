param(
    [Parameter(Mandatory=$true)][string]$State,
    [Parameter(Mandatory=$true)][string]$ExpectedSha256,
    [Parameter(Mandatory=$true)][string]$Owner,
    [ValidateRange(120,7200)][int]$Lifetime=7200
)
# One bootstrap: the original paired serve stays running throughout.
$ErrorActionPreference='Stop'
function Native-Hash([string]$path) {
    $stream=[IO.File]::OpenRead($path)
    $sha=[Security.Cryptography.SHA256]::Create()
    try { [BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-','') } finally { $sha.Dispose(); $stream.Dispose() }
}
$bundle=(Resolve-Path -LiteralPath $PSScriptRoot).ProviderPath
if ([IO.Path]::GetFileName($bundle) -notmatch '^td-control-[a-f0-9]{32}$') { throw 'Unexpected control bundle name.' }
$statePath=(Resolve-Path -LiteralPath $State).ProviderPath
if ($statePath.StartsWith($bundle + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Private state must be outside the shared control directory.' }
$source=[IO.Path]::Combine($bundle,'twodrive-dev.exe')
if ((Native-Hash $source) -ne $ExpectedSha256) { throw 'Agent executable SHA256 mismatch.' }
$preserved=@{}
foreach ($name in @('identity.json','control.json')) { $preserved[$name]=(Native-Hash ([IO.Path]::Combine($statePath,$name))) }
$runtime=[IO.Path]::Combine([IO.Path]::GetTempPath(),[IO.Path]::GetFileName($bundle))
if ([IO.Directory]::Exists($runtime)) { throw 'Agent runtime already exists; refusing overwrite.' }
$previousTemp=$env:TEMP
$previousTmp=$env:TMP
$result=[ordered]@{ agent_exit_code=$null; identity_and_grants_preserved=$false; bootstrap_runtime_removed=$false; executable_payload_removed=$false }
try {
    [IO.Directory]::CreateDirectory($runtime) | Out-Null
    $exe=[IO.Path]::Combine($runtime,'twodrive-dev.exe')
    [IO.File]::Copy($source,$exe,$false)
    if ((Native-Hash $exe) -ne $ExpectedSha256) { throw 'Private executable copy differs from expected SHA256.' }
    # All child state/sample files remain inside this owned runtime.
    $env:TEMP=$runtime
    $env:TMP=$runtime
    & $exe --state $statePath test-agent --queue $bundle --owner $Owner --lifetime $Lifetime
    $result.agent_exit_code=$LASTEXITCODE
} finally {
    $env:TEMP=$previousTemp
    $env:TMP=$previousTmp
    $same=$true
    foreach ($name in $preserved.Keys) { if ((Native-Hash ([IO.Path]::Combine($statePath,$name))) -ne $preserved[$name]) { $same=$false } }
    $result.identity_and_grants_preserved=$same
    [IO.Directory]::Delete($runtime,$true)
    $result.bootstrap_runtime_removed=-not ([IO.Directory]::Exists($runtime))
    [IO.File]::Delete($source)
    $result.executable_payload_removed=-not ([IO.File]::Exists($source))
    # The Linux controller collects this small receipt and removes the bundle.
    [IO.File]::WriteAllText(([IO.Path]::Combine($bundle,'bootstrap-cleanup.json')), ($result | ConvertTo-Json -Compress))
}
