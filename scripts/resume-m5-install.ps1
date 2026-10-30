# Resume the M5 install check after correcting the portable-vs-NSIS marker expectation.
# The initial Install phase already passed fresh install/uninstall and 0.4 -> 0.5.0 -> 0.5.1 -> 0.6.0.
$ErrorActionPreference='Stop'
$root=Split-Path $PSScriptRoot -Parent
$run=Join-Path $root 'test-results/install-m5'
$install=Join-Path $run '应用 安装'
$personal=Join-Path $env:LOCALAPPDATA 'ParquetLens'
function Assert($ok,$message){if(!$ok){throw $message}}
Assert (Test-Path (Join-Path $run 'personal-before-hashes.json')) 'Missing original data backup evidence'
$key=Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\ParquetLens'
Assert ($key.DisplayVersion -eq '0.6.0' -and $key.InstallLocation.Trim('"') -eq $install) 'Unexpected test installation'
& (Join-Path $root '.venv/Scripts/python.exe') (Join-Path $root 'scripts/verify-m5-installed.py')
Assert ($LASTEXITCODE -eq 0) 'Unexpected binary difference'
$expected='{"version":1,"cap":333,"sample_n":25,"time_mode":"utc","theme":"dark","language":"zh-CN","recent":[]}' + [Environment]::NewLine
$hash=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($expected)))
Assert ((Get-FileHash (Join-Path $personal 'settings.json')).Hash -eq $hash) 'Upgrade altered original test settings bytes'
Assert ((Get-Content (Join-Path $personal 'cache/acceptance-sentinel') -Raw).Trim() -eq 'M5 acceptance cache sentinel') 'Upgrade changed cache sentinel'
Assert (!(Test-Path 'HKCU:\Software\Classes\SystemFileAssociations\.parquet\shell\ParquetLens')) 'Shell verb remains'
$keys=@('HKCU:\Software\Classes\.parquet','HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.parquet\UserChoice')
$snapshot=@($keys | ForEach-Object {if(Test-Path $_){Get-ItemProperty $_ | Select-Object * -ExcludeProperty PS*}else{$null}}) | ConvertTo-Json -Depth 8 -Compress
Assert ($snapshot -eq (Get-Content (Join-Path $run 'association-before.json') -Raw).Trim()) 'Default association changed'
@{fresh_install_no_verb=$true;fresh_uninstall_no_verb=$true;legacy_m4_upgrade_removes_verb=$true;m3_install=$true;m4_upgrade=$true;m4_002_to_m5_upgrade=$true;installed_exe_matches_except_bundle_marker=$true;upgrade_preserves_data=$true;no_shell_verb=$true;default_association_unchanged=$true;install_path=$install;resumed_after_correcting_bundle_marker_expectation=$true} | ConvertTo-Json | Set-Content (Join-Path $run 'acceptance.json')
Write-Output 'Remaining upgrade assertions passed; portable and NSIS differ only by the framework bundle marker.'
