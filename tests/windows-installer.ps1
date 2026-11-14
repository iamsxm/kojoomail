param([string]$MakeNsis = "$env:LOCALAPPDATA\tauri\NSIS\Bin\makensis.exe")

$ErrorActionPreference = 'Stop'
if (-not (Test-Path -LiteralPath $MakeNsis)) { throw "NSIS compiler not found: $MakeNsis" }
$qaRoot = Join-Path ([IO.Path]::GetTempPath()) ("kojoomail-installer-test-" + [guid]::NewGuid().ToString('N'))
$testProcesses = [Collections.Generic.List[Diagnostics.Process]]::new()
$hookPath = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../src-tauri/windows/hooks.nsh'))
$pingPath = Join-Path $env:WINDIR 'System32/ping.exe'

function Assert-True($Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}
function New-Case([string]$Name) {
    $caseDir = Join-Path $qaRoot $Name
    New-Item -ItemType Directory -Path $caseDir -Force | Out-Null
    return $caseDir
}
function Start-Fixture([string]$Directory, [string]$Name) {
    $binaryPath = Join-Path $Directory $Name
    if (-not (Test-Path -LiteralPath $binaryPath)) { Copy-Item -LiteralPath $pingPath -Destination $binaryPath }
    $child = Start-Process -FilePath $binaryPath -ArgumentList '-t', '127.0.0.1' -WindowStyle Hidden -PassThru
    $testProcesses.Add($child)
    return $child
}
function Invoke-Installer([string]$Directory, [switch]$Uninstall) {
    $startInfo = [Diagnostics.ProcessStartInfo]::new()
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    # /D and _?= must be last, with no quotation marks even for a spaced path.
    $startInfo.FileName = if ($Uninstall) { Join-Path $Directory 'uninstall.exe' } else { Join-Path $qaRoot 'fixture.exe' }
    $startInfo.Arguments = if ($Uninstall) { "/S _?=$Directory" } else { "/S /D=$Directory" }
    # The hooks must not depend on PATH, taskkill.exe or PowerShell.
    $startInfo.EnvironmentVariables['PATH'] = ''
    $installer = [Diagnostics.Process]::Start($startInfo)
    try {
        if (-not $installer.WaitForExit(45000)) { $installer.Kill(); throw 'Test installer timed out' }
        return $installer.ExitCode
    } finally { $installer.Dispose() }
}
function Assert-Installed([string]$Directory) {
    Assert-True ((Get-Content -LiteralPath (Join-Path $Directory 'kojoomail.exe') -Raw).Trim() -eq 'new-main') 'Main binary was not replaced'
    Assert-True ((Get-Content -LiteralPath (Join-Path $Directory 'kojoomail-mcp.exe') -Raw).Trim() -eq 'new-mcp') 'MCP binary was not replaced'
    Assert-True (@(Get-ChildItem -LiteralPath $Directory -Filter '*.kojoomail-install-old').Count -eq 0) 'Backups were not cleaned up'
}

try {
    New-Item -ItemType Directory -Path $qaRoot | Out-Null
    Set-Content -LiteralPath (Join-Path $qaRoot 'main.txt') -Value 'new-main'
    Set-Content -LiteralPath (Join-Path $qaRoot 'mcp.txt') -Value 'new-mcp'
    @"
Unicode true
!include "$hookPath"
Name "Kojoo Mail isolated installer test"
OutFile "$qaRoot\fixture.exe"
RequestExecutionLevel user
SilentInstall silent
InstallDir "`$EXEDIR\unused"
Section
 SetOutPath `$INSTDIR
 !insertmacro NSIS_HOOK_PREINSTALL
 ; Make the relaunch race much wider than the real extraction gap.
 Sleep 700
 File /oname=kojoomail.exe "$qaRoot\main.txt"
 File /oname=kojoomail-mcp.exe "$qaRoot\mcp.txt"
 !insertmacro NSIS_HOOK_POSTINSTALL
 WriteUninstaller "`$INSTDIR\uninstall.exe"
SectionEnd
Section Uninstall
 !insertmacro NSIS_HOOK_PREUNINSTALL
 Delete "`$INSTDIR\kojoomail.exe"
 Delete "`$INSTDIR\kojoomail-mcp.exe"
 Delete "`$INSTDIR\uninstall.exe"
 RMDir "`$INSTDIR"
SectionEnd
"@ | Set-Content -LiteralPath (Join-Path $qaRoot 'fixture.nsi') -Encoding utf8
    & $MakeNsis /V2 (Join-Path $qaRoot 'fixture.nsi')
    Assert-True ($LASTEXITCODE -eq 0) 'NSIS hook compilation failed'

    $clean = New-Case 'clean install with spaces'
    Assert-True ((Invoke-Installer $clean) -eq 0) 'Fresh installation failed'
    Assert-Installed $clean
    Write-Output 'PASS: clean installation without existing binaries or PATH'

    $active = New-Case 'running installation'
    $other = New-Case 'other installation'
    $unrelated = Start-Fixture $other 'kojoomail-mcp.exe'
    $main = Start-Fixture $active 'kojoomail.exe'
    $legacy = Start-Fixture $active 'Kojoo Mail.exe'
    $mcp = Start-Fixture $active 'kojoomail-mcp.exe'
    $mcp2 = Start-Fixture $active 'kojoomail-mcp.exe'
    Assert-True ((Invoke-Installer $active) -eq 0) 'Installation with running processes failed'
    foreach ($child in @($main, $legacy, $mcp, $mcp2)) { $child.Refresh(); Assert-True $child.HasExited 'A target process survived preparation' }
    $unrelated.Refresh()
    Assert-True (-not $unrelated.HasExited) 'Another installation was terminated'
    Assert-Installed $active
    Write-Output 'PASS: all target processes stop; another installation keeps running'

    $interrupted = New-Case 'interrupted extraction'
    $oldBridge = Start-Fixture $interrupted 'kojoomail-mcp.exe'
    Move-Item -LiteralPath (Join-Path $interrupted 'kojoomail-mcp.exe') -Destination (Join-Path $interrupted 'kojoomail-mcp.exe.kojoomail-install-old')
    Assert-True ((Invoke-Installer $interrupted) -eq 0) 'Interrupted installation recovery failed'
    $oldBridge.Refresh()
    Assert-True $oldBridge.HasExited 'Recovered old bridge is still running'
    Assert-Installed $interrupted
    # Also exercise a leftover backup alongside a successfully extracted binary.
    Copy-Item -LiteralPath $pingPath -Destination (Join-Path $interrupted 'kojoomail-mcp.exe.kojoomail-install-old')
    Assert-True ((Invoke-Installer $interrupted) -eq 0) 'Stale backup recovery failed'
    Assert-Installed $interrupted
    Write-Output 'PASS: interrupted extraction and stale backup recovery'

    $blocked = New-Case 'blocked preparation'
    $blockedMain = Join-Path $blocked 'kojoomail.exe'
    $blockedMcp = Join-Path $blocked 'kojoomail-mcp.exe'
    Set-Content -LiteralPath $blockedMain 'old-main'
    Set-Content -LiteralPath $blockedMcp 'old-mcp'
    (Get-Item -LiteralPath $blockedMcp).IsReadOnly = $true
    Assert-True ((Invoke-Installer $blocked) -eq 2) 'Blocked preparation did not fail with a nonzero exit code'
    Assert-True ((Get-Content -LiteralPath $blockedMain -Raw).Trim() -eq 'old-main') 'Main binary was not restored'
    Assert-True ((Get-Content -LiteralPath $blockedMcp -Raw).Trim() -eq 'old-mcp') 'MCP binary was modified despite failure'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $blocked 'uninstall.exe'))) 'Extraction continued despite failure'
    (Get-Item -LiteralPath $blockedMcp).IsReadOnly = $false
    Assert-True ((Invoke-Installer $blocked) -eq 0) 'Retry after removing obstruction failed'
    Assert-Installed $blocked
    Write-Output 'PASS: failed preparation rolls back before extraction; retry succeeds'

    # A supervisor repeatedly tries the configured executable after disconnection.
    $relaunch = New-Case 'automatic reconnect'
    $relaunchMcp = Start-Fixture $relaunch 'kojoomail-mcp.exe'
    $watcherPath = Join-Path $qaRoot 'watcher.ps1'
    @'
param($Directory, $InitialPid)
$current = Get-Process -Id $InitialPid -ErrorAction SilentlyContinue
Set-Content -LiteralPath (Join-Path $Directory 'watcher-ready') 'ready'
try {
    while (-not (Test-Path -LiteralPath (Join-Path $Directory 'watcher-stop'))) {
        if ($current) { $current.Refresh() }
        if (-not $current -or $current.HasExited) {
            Add-Content -LiteralPath (Join-Path $Directory 'attempts.txt') 'reconnect'
            try { $current = Start-Process -FilePath (Join-Path $Directory 'kojoomail-mcp.exe') -ArgumentList '-t', '127.0.0.1' -WindowStyle Hidden -PassThru -ErrorAction Stop } catch { $current = $null }
        }
        Start-Sleep -Milliseconds 20
    }
} finally { if ($current -and -not $current.HasExited) { $current.Kill(); $current.WaitForExit() } }
'@ | Set-Content -LiteralPath $watcherPath
    $watcher = Start-Process -FilePath "$env:WINDIR\System32\WindowsPowerShell\v1.0\powershell.exe" -ArgumentList '-NoProfile', '-NonInteractive', '-File', "`"$watcherPath`"", '-Directory', "`"$relaunch`"", '-InitialPid', $relaunchMcp.Id -WindowStyle Hidden -PassThru
    $testProcesses.Add($watcher)
    for ($poll = 0; $poll -lt 100 -and -not (Test-Path -LiteralPath (Join-Path $relaunch 'watcher-ready')); $poll++) { Start-Sleep -Milliseconds 50 }
    Assert-True (Test-Path -LiteralPath (Join-Path $relaunch 'watcher-ready')) 'Supervisor did not start'
    try { Assert-True ((Invoke-Installer $relaunch) -eq 0) 'Automatic reconnect blocked installation' }
    finally { Set-Content -LiteralPath (Join-Path $relaunch 'watcher-stop') 'stop' }
    Assert-True ($watcher.WaitForExit(10000)) 'Supervisor did not exit'
    Assert-True (Test-Path -LiteralPath (Join-Path $relaunch 'attempts.txt')) 'Supervisor never attempted reconnection'
    $relaunchMcp.Refresh()
    Assert-True $relaunchMcp.HasExited 'Initial bridge survived automatic reconnect test'
    Assert-Installed $relaunch
    Write-Output 'PASS: automatic client reconnection cannot lock the extraction path'

    $uninstall = New-Case 'running uninstall'
    Assert-True ((Invoke-Installer $uninstall) -eq 0) 'Uninstall fixture setup failed'
    Copy-Item -LiteralPath $pingPath -Destination (Join-Path $uninstall 'kojoomail.exe') -Force
    Copy-Item -LiteralPath $pingPath -Destination (Join-Path $uninstall 'kojoomail-mcp.exe') -Force
    $uninstallMain = Start-Fixture $uninstall 'kojoomail.exe'
    $uninstallMcp = Start-Fixture $uninstall 'kojoomail-mcp.exe'
    Assert-True ((Invoke-Installer $uninstall -Uninstall) -eq 0) 'Uninstall with running binaries failed'
    foreach ($child in @($uninstallMain, $uninstallMcp)) { $child.Refresh(); Assert-True $child.HasExited 'Uninstall left a running target process' }
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $uninstall 'kojoomail-mcp.exe'))) 'Uninstall left the bridge executable'
    Assert-True (@(Get-ChildItem -LiteralPath $uninstall -Filter '*.kojoomail-install-old').Count -eq 0) 'Uninstall left old images'
    Write-Output 'PASS: uninstall terminates main and MCP processes and removes old images'
} finally {
    foreach ($child in $testProcesses) {
        try { if (-not $child.HasExited) { $child.Kill(); $child.WaitForExit() } } catch { Write-Warning $_ }
        $child.Dispose()
    }
    # Verify the exact cleanup boundary before recursive deletion on Windows.
    $resolvedQa = [IO.Path]::GetFullPath($qaRoot)
    $tempBoundary = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\kojoomail-installer-test-'
    if (-not $resolvedQa.StartsWith($tempBoundary, [StringComparison]::OrdinalIgnoreCase)) { throw 'Unsafe test cleanup path' }
    if (Test-Path -LiteralPath $resolvedQa) { Remove-Item -LiteralPath $resolvedQa -Recurse -Force }
}
