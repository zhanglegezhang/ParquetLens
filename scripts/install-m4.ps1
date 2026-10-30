# Requires an installer rebuilt after removal of right-click integration; old 0.5.0 bundles are not valid inputs.
param([ValidateSet('Install','Uninstall')][string]$Phase = 'Install')
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$run = Join-Path $root 'test-results/install-m4-002'
$install = Join-Path $run '应用 安装'
$personal = Join-Path $env:LOCALAPPDATA 'ParquetLens'
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\ParquetLens'
$productKey = 'HKCU:\Software\parquetlens\ParquetLens'
$verbKey = 'HKCU:\Software\Classes\SystemFileAssociations\.parquet\shell\ParquetLens'
$setup = Join-Path $root 'src-tauri/target/release/bundle/nsis/ParquetLens_0.5.1_x64-setup.exe'
function Assert($condition, $message) { if (!$condition) { throw $message } }
if ($Phase -eq 'Install') {
  Assert ((Get-Item $setup).LastWriteTimeUtc -ge (Get-Item (Join-Path $root 'src-tauri/installer-hooks.nsh')).LastWriteTimeUtc) 'Installer predates current hooks. Rebuild only when authorized before running acceptance.'
}

function RunSetup($file) {
  $proc = Start-Process -FilePath $file -ArgumentList "/S /NS /D=$install" -WindowStyle Hidden -PassThru -Wait
  Assert ($proc.ExitCode -eq 0) "Installer exit $($proc.ExitCode)"
}
function RemoveInstall($extra = '') {
  $file = Join-Path $install 'uninstall.exe'
  $proc = Start-Process -FilePath $file -ArgumentList "/S $extra _?=$install" -WindowStyle Hidden -PassThru -Wait
  Assert ($proc.ExitCode -eq 0) "Uninstaller exit $($proc.ExitCode)"
  Assert (!(Test-Path (Join-Path $install 'ParquetLens.exe'))) 'Executable remains after uninstall'
}
function AssociationSnapshot {
  $keys = @('HKCU:\Software\Classes\.parquet','HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.parquet\UserChoice')
  @($keys | ForEach-Object { if (Test-Path $_) { Get-ItemProperty $_ | Select-Object * -ExcludeProperty PS* } else { $null } }) | ConvertTo-Json -Depth 8 -Compress
}
if ($Phase -eq 'Install') {
  Assert (!(Test-Path $uninstallKey)) 'Existing installed application detected; preserve it and revise acceptance plan.'
  Assert (!(Test-Path $productKey)) 'Existing installer preferences detected; preserve them and revise acceptance plan.'
  Assert (!(Test-Path $verbKey)) 'Existing shell verb detected; preserve it and revise acceptance plan.'
  New-Item -ItemType Directory -Path $run -Force | Out-Null
  if (Test-Path -LiteralPath $personal) {
    $backup = Join-Path $run 'personal-before'
    Assert (!(Test-Path -LiteralPath $backup)) 'Existing personal-data backup; inspect before retry'
    Assert ((Resolve-Path -LiteralPath $personal).Path -eq [IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'ParquetLens'))) 'Unexpected personal-data path'
    Assert ([IO.Path]::GetFullPath($backup).StartsWith([IO.Path]::GetFullPath($root) + [IO.Path]::DirectorySeparatorChar)) 'Backup outside workspace'
    @(Get-ChildItem -LiteralPath $personal -File -Recurse | ForEach-Object { @{relative=[IO.Path]::GetRelativePath($personal,$_.FullName);hash=(Get-FileHash -LiteralPath $_.FullName).Hash} }) | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $run 'personal-before-hashes.json')
    Move-Item -LiteralPath $personal -Destination $backup
  }
  AssociationSnapshot | Set-Content (Join-Path $run 'association-before.json') -Encoding utf8
  # Fresh install must not create a verb; normal uninstall must also leave none.
  RunSetup $setup
  Assert (!(Test-Path $verbKey)) 'Fresh install registered a shell verb'
  RemoveInstall
  Assert (!(Test-Path $verbKey)) 'Fresh uninstall left a shell verb'
  RunSetup (Join-Path $root 'dist/M3/ParquetLens_0.4.0_x64-setup.exe')
  Assert ((Get-ItemProperty $uninstallKey).DisplayVersion -eq '0.4.0') 'M3 install version mismatch'
  New-Item -ItemType Directory -Path (Join-Path $personal 'cache') -Force | Out-Null
  '{"version":1,"cap":333,"sample_n":25,"time_mode":"utc","theme":"dark","language":"zh-CN","recent":[]}' | Set-Content (Join-Path $personal 'settings.json') -Encoding utf8NoBOM
  'M4 acceptance cache sentinel' | Set-Content (Join-Path $personal 'cache/acceptance-sentinel')
  $hash = (Get-FileHash (Join-Path $personal 'settings.json')).Hash
  RunSetup (Join-Path $root 'dist/M4/ParquetLens_0.5.0_x64-setup.exe')
  Assert ((Get-ItemProperty $uninstallKey).DisplayVersion -eq '0.5.0') 'Legacy M4 install version mismatch'
  Assert ((Get-Item "$verbKey\command").GetValue('') -eq "`"$install\ParquetLens.exe`" `"%1`"") 'Legacy M4 did not register expected owned verb'
  Assert ((Get-FileHash (Join-Path $personal 'settings.json')).Hash -eq $hash) 'Legacy upgrade altered settings'
  RunSetup $setup
  Assert ((Get-ItemProperty $uninstallKey).DisplayVersion -eq '0.5.1') 'M4 upgrade version mismatch'
  Assert ((Get-FileHash (Join-Path $personal 'settings.json')).Hash -eq $hash) 'Upgrade altered settings'
  Assert (Test-Path (Join-Path $personal 'cache/acceptance-sentinel')) 'Upgrade removed cache'
  Assert (!(Test-Path $verbKey)) 'Installer registered a removed shell feature'
  Assert ((AssociationSnapshot) -eq (Get-Content (Join-Path $run 'association-before.json') -Raw).Trim()) 'Default association changed'
  @{fresh_install_no_verb=$true;fresh_uninstall_no_verb=$true;legacy_m4_upgrade_removes_verb=$true;m3_install=$true;m4_upgrade=$true;upgrade_preserves_data=$true;no_shell_verb=$true;default_association_unchanged=$true;install_path=$install} | ConvertTo-Json | Set-Content (Join-Path $run 'acceptance.json')
  Write-Output "Installed for native acceptance: $install"
} else {
  Assert ((Get-ItemProperty $uninstallKey).DisplayVersion -eq '0.5.1') 'Expected test installation missing'
  Assert ((Get-ItemProperty $uninstallKey).InstallLocation.Trim('"') -eq $install) 'Refusing to uninstall a different installation'
  $hash = (Get-FileHash (Join-Path $personal 'settings.json')).Hash
  RemoveInstall
  Assert ((Get-FileHash (Join-Path $personal 'settings.json')).Hash -eq $hash) 'Default uninstall altered settings'
  Assert (Test-Path (Join-Path $personal 'cache/acceptance-sentinel')) 'Default uninstall removed cache'
  Assert (!(Test-Path $verbKey)) 'Shell verb remains after uninstall'
  RunSetup $setup
  Assert ((Get-FileHash (Join-Path $personal 'settings.json')).Hash -eq $hash) 'Reinstall altered settings'
  Assert (!(Test-Path $verbKey)) 'Reinstall registered a removed shell feature'
  # NSIS also removes its WebView profile when personal-data cleanup is chosen.
  # Preserve any profile that predates this acceptance run, verifying every byte.
  $profiles = @()
  foreach ($base in @($env:LOCALAPPDATA,$env:APPDATA)) {
    $source = Join-Path $base 'local.parquetlens.desktop'
    if (Test-Path -LiteralPath $source) {
      $backup = Join-Path $run ('webview-backup-' + $profiles.Count)
      Assert (!(Test-Path -LiteralPath $backup)) 'Existing acceptance backup; inspect before retry'
      Copy-Item -LiteralPath $source -Destination $backup -Recurse
      $files = @(Get-ChildItem -LiteralPath $source -File -Recurse | ForEach-Object {
        $relative = [IO.Path]::GetRelativePath($source,$_.FullName)
        $originalHash = (Get-FileHash -LiteralPath $_.FullName).Hash
        Assert ((Get-FileHash -LiteralPath (Join-Path $backup $relative)).Hash -eq $originalHash) 'Profile backup checksum mismatch'
        @{relative=$relative;hash=$originalHash}
      })
      $profiles += @{source=$source;backup=$backup;files=$files}
    }
  }
  RemoveInstall '/DELETEAPPDATA'
  foreach ($profile in $profiles) {
    New-Item -ItemType Directory -Path $profile.source -Force | Out-Null
    Get-ChildItem -LiteralPath $profile.backup -Force | Copy-Item -Destination $profile.source -Recurse -Force
    foreach ($file in $profile.files) {
      Assert ((Get-FileHash -LiteralPath (Join-Path $profile.source $file.relative)).Hash -eq $file.hash) 'Profile restore checksum mismatch'
    }
  }
  Assert (!(Test-Path (Join-Path $personal 'settings.json'))) 'Opt-in cleanup left settings'
  Assert (!(Test-Path (Join-Path $personal 'cache/acceptance-sentinel'))) 'Opt-in cleanup left cache'
  Assert (!(Test-Path $verbKey)) 'Opt-in uninstall left shell verb'
  Assert (!(Test-Path $uninstallKey)) 'Uninstall registration remains'
  Assert (!(Test-Path $productKey)) 'Installer preferences remain'
  Assert ((AssociationSnapshot) -eq (Get-Content (Join-Path $run 'association-before.json') -Raw).Trim()) 'Default association changed'
  $backup = Join-Path $run 'personal-before'
  if (Test-Path -LiteralPath $backup) {
    Assert ((Resolve-Path -LiteralPath $backup).Path.StartsWith([IO.Path]::GetFullPath($root) + [IO.Path]::DirectorySeparatorChar)) 'Backup outside workspace'
    Assert ([IO.Path]::GetFullPath($personal) -eq [IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'ParquetLens'))) 'Unexpected restore path'
    if (Test-Path -LiteralPath $personal) { Remove-Item -LiteralPath $personal } # empty only
    Move-Item -LiteralPath $backup -Destination $personal
    foreach ($file in (Get-Content (Join-Path $run 'personal-before-hashes.json') -Raw | ConvertFrom-Json)) {
      Assert ((Get-FileHash -LiteralPath (Join-Path $personal $file.relative)).Hash -eq $file.hash) 'Original personal data restore mismatch'
    }
  }
  $report = Get-Content (Join-Path $run 'acceptance.json') -Raw | ConvertFrom-Json -AsHashtable
  $report.default_uninstall_preserves_data=$true
  $report.reinstall_preserves_data=$true
  $report.opt_in_cleanup=$true
  $report.registry_restored=$true
  $report.preexisting_personal_data_restored=$true
  $report.preexisting_webview_files_restored=($profiles.files | Measure-Object).Count
  $report | ConvertTo-Json | Set-Content (Join-Path $run 'acceptance.json')
  Write-Output 'Install/upgrade/uninstall acceptance passed; test registration and personal data removed.'
}
