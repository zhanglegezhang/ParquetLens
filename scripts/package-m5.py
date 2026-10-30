"""Assemble M5 delivery from the already tested binaries and explicit evidence only."""
from pathlib import Path
import shutil,hashlib,json
root=Path(__file__).resolve().parents[1];out=root/'test-results';dest=root/'dist/M5'
assert not dest.exists(), 'M5 delivery already exists; inspect before overwriting'
report=(out/'m5-report.md').read_text(encoding='utf-8')
accept=json.loads((out/'install-m5/acceptance.json').read_text(encoding='utf-8-sig'))
assert accept['registry_restored'] and accept['opt_in_cleanup']
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
exe=root/'src-tauri/target/release/ParquetLens.exe'
bench=json.loads((out/'m5-final.json').read_text(encoding='utf-8'))
assert digest(exe)==bench['sha256'], 'Release binary changed after final benchmark'
dest.mkdir();verify=dest/'verification';verify.mkdir()
shutil.copy2(exe,dest/'ParquetLens.exe')
shutil.copy2(root/'src-tauri/target/release/bundle/nsis/ParquetLens_0.6.0_x64-setup.exe',dest/'ParquetLens_0.6.0_x64-setup.exe')
intro='''# ParquetLens 0.6.0

Windows x64 自用版，本地离线查看 Parquet。包含列统计/类型、样例预览、枚举与缓存、Excel/CSV/数据字典导出、取消、设置和单实例；不注册右键菜单，不接管默认文件关联。

- 直接运行：双击 `ParquetLens.exe`，通过“打开文件”或 Ctrl+O 选择文件。
- 安装使用：运行 `ParquetLens_0.6.0_x64-setup.exe`；安装到当前用户，升级保留设置/缓存。卸载默认保留数据，可明确选择清理。
- 需要本机 WebView2。本机 Windows 11 验收通过；没有签名、自动更新或公开发布。

校验见 `SHA256SUMS.txt`；`SOURCE-SHA256SUMS.txt` 的路径相对项目根目录。下面是完整实测与尚未达标的性能项。

'''
(dest/'README.md').write_text(intro+report,encoding='utf-8')
names=['m5-summary.json','m5-environment.json','m5-data-oracle.json','m5-verify.log','m5-build.log','m5-install-final.log','m5-installed-runtime-final.log','m5-uninstall-final.log','smoke.json','m2-smoke.json','m3-smoke.json','m4-smoke.json','m2-runtime-memory.json','m3-runtime-memory.json','m4-runtime-memory.json','m4-settings-light.png','m3-enums-light.png','m2-preview-dark.png']
for label in ['m5-baseline','m5-s','m5-z','m5-final','m5-stress']:
 for suffix in ['.json','-memory.json','-sections.json','-idle.json','-startup.json','-xlsx-readback.json','.log']:
  p=out/(label+suffix)
  if p.exists():names.append(p.name)
for p in out.glob('m5-regression-*.log'):names.append(p.name)
for p in out.glob('m5-verify_*.log'):names.append(p.name)
for name in sorted(set(names)):
 p=out/name;assert p.is_file(),name;shutil.copy2(p,verify/name)
for name in ['acceptance.json','runtime.json','binary-comparison.json','installed-cli-open.png']:
 shutil.copy2(out/'install-m5'/name,verify/('install-'+name))
shutil.copy2(out/'install-m5-initial/recovery.json',verify/'initial-acceptance-recovery.json')
(verify/'initial-acceptance-notes.txt').write_text('Initial acceptance stopped at an over-strict raw binary hash assertion: Tauri NSIS changes only UNK to NSS (3 bytes). Verification now requires exact equality after that specific marker normalization and records both hashes. The first uninstall restoration then encountered a read-only RegistryKey.SetValue handle; Set-Item corrected it. Original data and 304 WebView files were hash-verified restored. After both script fixes, the entire fresh install / legacy upgrade / installed runtime / reinstall / default and opt-in uninstall sequence was rerun from the beginning and passed. No application binary or installer rebuild was needed. Initial backups remain outside delivery.\n',encoding='utf-8')
files=set()
for folder in ['src','src-tauri/src','src-tauri/tests','src-tauri/icons','src-tauri/capabilities','static']:
 p=root/folder
 if p.exists():files.update(f for f in p.rglob('*') if f.is_file())
for pattern in ['scripts/*.ps1','scripts/*.mjs','scripts/*.py','bench/*.py','tests/*.py','tests/*.mjs','src-tauri/Cargo.*','src-tauri/*.rs','src-tauri/*.json','src-tauri/*.nsh','src-tauri/*.manifest','package*.json','*.config.*','tsconfig.json','README.md','AGENTS.md','.cargo/config.toml']:
 files.update(f for f in root.glob(pattern) if f.is_file())
(dest/'SOURCE-SHA256SUMS.txt').write_text(''.join(f'{digest(p)}  {p.relative_to(root).as_posix()}\n' for p in sorted(files)),encoding='utf-8')
artifacts=sorted(p for p in dest.rglob('*') if p.is_file())
(dest/'SHA256SUMS.txt').write_text(''.join(f'{digest(p)}  {p.relative_to(dest).as_posix()}\n' for p in artifacts),encoding='utf-8')
print(json.dumps({'delivery':str(dest),'files':len(artifacts)+1,'source_files':len(files),'exe_sha256':digest(dest/'ParquetLens.exe'),'setup_sha256':digest(dest/'ParquetLens_0.6.0_x64-setup.exe')},indent=2))
