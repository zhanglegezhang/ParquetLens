$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
foreach ($entry in @(@('smoke.mjs','m1'),@('smoke-m2.mjs','m2'),@('smoke-m3.mjs','m3'),@('smoke-m4.mjs','m4'))) {
  node (Join-Path scripts $entry[0]) *> (Join-Path test-results ('m5-regression-'+$entry[1]+'.log'))
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
  Start-Sleep -Milliseconds 500
}
foreach ($name in @('verify_xlsx_xml.py','verify_enum_xlsx.py','verify_m4_exports.py')) {
  .venv/Scripts/python.exe (Join-Path tests $name) *> (Join-Path test-results ('m5-'+$name+'.log'))
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
$env:BENCH_EXE='src-tauri/target/release/ParquetLens.exe'
Remove-Item Env:BENCH_DATASETS,Env:BENCH_SKIP_UI -ErrorAction SilentlyContinue
$env:BENCH_LABEL='m5-final'
$env:BENCH_REPS='20'
node scripts/bench-m5.mjs *> test-results/m5-final.log
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
node scripts/startup-m5.mjs *> test-results/m5-final-startup.log
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
.venv/Scripts/python.exe scripts/idle_m5.py *> test-results/m5-final-idle.log
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$env:BENCH_LABEL='m5-stress'
$env:BENCH_REPS='1'
$env:BENCH_DATASETS='stress-snappy,stress-zstd'
$env:BENCH_SKIP_UI='1'
node scripts/bench-m5.mjs *> test-results/m5-stress.log
exit $LASTEXITCODE
