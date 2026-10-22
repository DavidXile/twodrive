#requires -Version 7.0
param(
 [Parameter(Mandatory=$true)][int]$GuiProcessId,
 [Parameter(Mandatory=$true)][string]$GuiExecutable,
 [Parameter(Mandatory=$true)][string]$PackageVerification,
 [Parameter(Mandatory=$true)][string]$Root,
 [Parameter(Mandatory=$true)][string]$OutputDirectory,
 [Parameter(Mandatory=$true)][ValidateSet('preflight','baseline','details','properties','thumbnails','pin','unpin','release','handle-guard','offline','retry','reconnect','dirty','exit')][string]$Stage,
 [int]$ExplorerProcessId=0,
 [long]$ExplorerWindowHandle=0
)
# Observer only: never starts a process, registers, hydrates, writes source data,
# restarts Explorer, changes view settings, or reports an API state as a glyph.
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class FixtureEvidenceNative {
 [StructLayout(LayoutKind.Sequential)] public struct Rect {public int Left,Top,Right,Bottom;}
 [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] public struct RootInfo {
  public long FileId;public ushort Hydration,HydrationModifier,Population,PopulationModifier;
  public uint InSync,HardLink,ProviderStatus;
  [MarshalAs(UnmanagedType.ByValTStr,SizeConst=256)]public string ProviderName;
  [MarshalAs(UnmanagedType.ByValTStr,SizeConst=256)]public string Version;
  public uint IdentityLength;public byte Identity;
 }
 [DllImport("user32.dll")]public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr c);
 [DllImport("user32.dll")]public static extern bool GetWindowRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")]public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint flags);
 [DllImport("user32.dll")]public static extern bool IsIconic(IntPtr h);
 [DllImport("user32.dll")]public static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
 [DllImport("shell32.dll",CharSet=CharSet.Unicode)]static extern IntPtr CommandLineToArgvW(string command,out int count);
 [DllImport("kernel32.dll")]static extern IntPtr LocalFree(IntPtr p);
 public static string[] Args(string command){int count;IntPtr array=CommandLineToArgvW(command,out count);if(array==IntPtr.Zero)throw new Exception("Cannot parse owner command");try{string[] args=new string[count];for(int i=0;i<count;i++)args[i]=Marshal.PtrToStringUni(Marshal.ReadIntPtr(array,i*IntPtr.Size));return args;}finally{LocalFree(array);}}
 [DllImport("cldapi.dll",CharSet=CharSet.Unicode)]public static extern int CfGetSyncRootInfoByPath(string path,int kind,IntPtr buffer,uint length,out uint returned);
 public static object RootState(string path) {
  IntPtr buffer=Marshal.AllocHGlobal(8192);
  try {uint returned;int hr=CfGetSyncRootInfoByPath(path,1,buffer,8192,out returned);
   if(hr<0)return new {hresult=String.Format("0x{0:x8}",unchecked((uint)hr)),registered=hr==unchecked((int)0x80070186)?(bool?)false:null,file_id=0UL,context=""};
   int lengthOffset=(int)Marshal.OffsetOf<RootInfo>("IdentityLength"),offset=(int)Marshal.OffsetOf<RootInfo>("Identity");
   int length=Marshal.ReadInt32(buffer,lengthOffset);
   if(length<0 || length>4096 || offset+length>returned || returned>8192)throw new Exception("Invalid root metadata");
   byte[] bytes=new byte[length];Marshal.Copy(IntPtr.Add(buffer,offset),bytes,0,length);
   string context=Encoding.UTF8.GetString(bytes);
   if(context!="twodrive-cfapi-generated-visual-v1")context="foreign-or-other-dataset";
   return new {hresult="0x00000000",registered=true,file_id=unchecked((ulong)Marshal.ReadInt64(buffer)),context=context};
  }finally{Marshal.FreeHGlobal(buffer);}
 }
}
'@
function Get-FullTaskPath([string]$Path){[IO.Path]::GetFullPath($Path).TrimEnd('\')}
$rootPath=Get-FullTaskPath $Root
$output=Get-FullTaskPath $OutputDirectory
$state=[IO.Path]::ChangeExtension($rootPath,'engine-state')
if($output -eq $rootPath -or $output.StartsWith($rootPath+'\',[StringComparison]::OrdinalIgnoreCase) -or $output -eq $state -or $output.StartsWith($state+'\',[StringComparison]::OrdinalIgnoreCase)){throw 'Evidence must stay outside fixture/cache state'}
$package=Get-Content -Raw -LiteralPath $PackageVerification -Encoding utf8 | ConvertFrom-Json
if($package.source_commit -notmatch '^[a-f0-9]{40}$'){throw 'Exact package source required'}
$gui=Get-Process -Id $GuiProcessId
$guiPath=Get-FullTaskPath $GuiExecutable
if($gui.Path -ne $guiPath -or [IO.Path]::GetFileName($guiPath) -ne 'twodrive_dav.exe'){throw 'Unexpected GUI identity'}
if((Get-FileHash -LiteralPath $guiPath -Algorithm SHA256).Hash.ToLower() -ne $package.runtime_files.'twodrive_dav.exe'.sha256){throw 'GUI package digest differs'}
if($gui.MainWindowHandle -eq [IntPtr]::Zero -or $gui.MainWindowTitle -ne 'TwoDrive Development'){throw 'No verified GUI window'}
$rootBefore=[FixtureEvidenceNative]::RootState($rootPath)
function Read-Phase {
 $path=Join-Path $state 'native-phases.jsonl'
 if(!(Test-Path -LiteralPath $path)){throw 'Live visual phase log missing'}
 if((Get-Item -LiteralPath $path).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Phase log is a reparse point'}
 foreach($line in @(Get-Content -LiteralPath $path -Tail 3 -Encoding utf8)[-1..-3]){
  try{$record=$line|ConvertFrom-Json -ErrorAction Stop;if($record.schema_version -eq 1){return $record}}catch{}
 }
 throw 'No complete versioned phase record'
}
function Assert-Phase($Record) {
 if($Record.snapshot.dataset -ne 'visual_v1' -or [uint64]$Record.snapshot.binding.root_file_id -ne $rootBefore.file_id){throw 'Phase/root binding differs'}
 if($Record.snapshot.observation_log_error -or $Record.snapshot.session_expired){throw 'Visual observation unavailable/expired'}
 if([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()-[long]$Record.utc_ms -gt 10000){throw 'Phase snapshot is stale'}
 if(@($Record.snapshot.items).Count -ne 3 -or @($Record.snapshot.directories).Count -ne 1){throw 'Unexpected visual dataset shape'}
 if($Record.snapshot.max_session_seconds -ne 2700){throw 'Expected finite45-minute visual session'}
 $owner=Get-CimInstance Win32_Process -Filter "ProcessId=$([int]$Record.snapshot.owner_pid)"
 $expectedEngine=Join-Path ([IO.Path]::GetDirectoryName($guiPath)) 'twodrive-dav-engine.exe'
 if(!$owner -or $owner.ParentProcessId -ne $GuiProcessId -or $owner.ExecutablePath -ne $expectedEngine){throw 'Native owner is not this GUI child engine'}
 if((Get-FileHash -LiteralPath $expectedEngine -Algorithm SHA256).Hash.ToLower() -ne $package.runtime_files.'twodrive-dav-engine.exe'.sha256){throw 'Owner package digest differs'}
 $arguments=[FixtureEvidenceNative]::Args($owner.CommandLine);$rootIndex=[Array]::IndexOf($arguments,'--native-local-fixture-root')
 if($rootIndex -lt 0 -or $rootIndex+1 -ge $arguments.Count -or (Get-FullTaskPath $arguments[$rootIndex+1]) -ne $rootPath){throw 'GUI owner root argument differs'}
}
$before=$null
if($Stage -ne 'preflight'){
 if($Stage -eq 'exit'){
  if($rootBefore.hresult -ne '0x80070186'){throw 'Exit capture requires confirmed unregistration'}
 }else{
  if(!$rootBefore.registered -or $rootBefore.context -ne 'twodrive-cfapi-generated-visual-v1'){throw 'Not the approved active visual dataset'}
  $before=Read-Phase;Assert-Phase $before
 }
}
if(Test-Path -LiteralPath $output){throw 'Use a NEW phase evidence directory'}
[void][IO.Directory]::CreateDirectory($output)
function Save-TargetWindow([IntPtr]$Handle,[int]$ExpectedProcess,[string]$Name) {
 [uint32]$owner=0;[void][FixtureEvidenceNative]::GetWindowThreadProcessId($Handle,[ref]$owner)
 if($owner -ne $ExpectedProcess){throw 'Window process changed'}
 if([FixtureEvidenceNative]::IsIconic($Handle)){throw "Target $Name is minimized; observer never restores or focuses windows"}
 $rect=[FixtureEvidenceNative+Rect]::new()
 if(![FixtureEvidenceNative]::GetWindowRect($Handle,[ref]$rect)){throw 'Cannot inspect target geometry'}
 $width=$rect.Right-$rect.Left;$height=$rect.Bottom-$rect.Top
 if($width -lt 100 -or $height -lt 100 -or $width -gt 8000 -or $height -gt 8000){throw "Invalid target $Name geometry ${width}x${height}; no screenshot accepted"}
 $bitmap=[Drawing.Bitmap]::new($width,$height);$graphics=[Drawing.Graphics]::FromImage($bitmap);$dc=$graphics.GetHdc()
 try{$ok=[FixtureEvidenceNative]::PrintWindow($Handle,$dc,2)}finally{$graphics.ReleaseHdc($dc)}
 try{if(!$ok){throw 'Target capture failed'};$bitmap.Save((Join-Path $output $Name),[Drawing.Imaging.ImageFormat]::Png)}finally{$graphics.Dispose();$bitmap.Dispose()}
 return @{file=$Name;width=$width;height=$height;process_id=$ExpectedProcess;visual_inspection='required; no glyph assertion'}
}
$oldDpi=[FixtureEvidenceNative]::SetThreadDpiAwarenessContext([IntPtr](-4))
try{
 $captures=@(Save-TargetWindow $gui.MainWindowHandle $GuiProcessId 'gui.png')
 $explorerStatus='not-requested'
 if($ExplorerProcessId -ne 0){
  $explorer=Get-Process -Id $ExplorerProcessId
  if($explorer.Path -ne "$env:WINDIR\explorer.exe"){throw 'Unexpected Explorer identity'}
  $explorerHandle=if($ExplorerWindowHandle -ne 0){[IntPtr]$ExplorerWindowHandle}else{$explorer.MainWindowHandle}
  [uint32]$windowOwner=0;[void][FixtureEvidenceNative]::GetWindowThreadProcessId($explorerHandle,[ref]$windowOwner)
  if($explorerHandle -eq [IntPtr]::Zero -or $windowOwner -ne $ExplorerProcessId){throw 'Explicit Explorer window identity differs'}
  $shell=New-Object -ComObject Shell.Application
  try{$locations=@($shell.Windows()|Where-Object {[long]$_.HWND -eq $explorerHandle.ToInt64()}|ForEach-Object {[string]$_.Document.Folder.Self.Path})}finally{[void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($shell)}
  if($locations.Count -ne 1){throw 'Ambiguous Explorer tabs/window; no Explorer capture'}
  $location=Get-FullTaskPath $locations[0]
  if($location -ne $rootPath -and !$location.StartsWith($rootPath+'\',[StringComparison]::OrdinalIgnoreCase)){throw 'Explorer is outside the exact fixture'}
  $captures+=Save-TargetWindow $explorerHandle $ExplorerProcessId 'explorer.png';$explorerStatus='location-verified; target captured'
 }
}finally{if($oldDpi -ne [IntPtr]::Zero){[void][FixtureEvidenceNative]::SetThreadDpiAwarenessContext($oldDpi)}}
$after=$null
if($before){$after=Read-Phase;Assert-Phase $after}
$rootAfter=[FixtureEvidenceNative]::RootState($rootPath)
$assessment=if($Stage -eq 'thumbnails'){'on-demand reads may be expected; compare exact provider/native byte deltas'}elseif($Stage -in @('baseline','details','properties')){'metadata/browsing stage: data-byte growth requires caller attribution and investigation'}else{'record observed state; no automatic native-glyph pass'}
$report=[ordered]@{schema_version=1;stage=$Stage;utc_ms=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds();source_commit=$package.source_commit;os_build=[Environment]::OSVersion.Version.ToString();capture_method='DPI-aware target PrintWindow';root_before=$rootBefore;root_after=$rootAfter;before=$before;after=$after;captures=$captures;explorer=$explorerStatus;assessment=$assessment;registration_performed=$false;private_evidence=$true}
$report|ConvertTo-Json -Depth 20|Set-Content -LiteralPath (Join-Path $output 'phase-evidence.json') -Encoding utf8
[pscustomobject]@{stage=$Stage;captures=$captures.Count;registration_performed=$false;visual_pass_claimed=$false;source_commit=$package.source_commit}|ConvertTo-Json
