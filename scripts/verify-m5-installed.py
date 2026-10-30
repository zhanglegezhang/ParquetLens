"""Verify Tauri's documented NSIS marker is the only installed/portable difference."""
from pathlib import Path
import hashlib,json
root=Path(__file__).resolve().parents[1]
a=root/'src-tauri/target/release/ParquetLens.exe';b=root/'test-results/install-m5/应用 安装/ParquetLens.exe'
x=a.read_bytes();y=b.read_bytes();old=b'__TAURI_BUNDLE_TYPE_VAR_UNK';new=b'__TAURI_BUNDLE_TYPE_VAR_NSS'
assert x.count(old)==1, 'Unexpected portable marker count'
assert x.replace(old,new,1)==y, 'Differences beyond expected NSIS bundle marker'
report={'portable_sha256':hashlib.sha256(x).hexdigest(),'installed_sha256':hashlib.sha256(y).hexdigest(),'different_byte_offsets':[i for i,(v,w) in enumerate(zip(x,y)) if v!=w],'portable_marker':old.decode(),'installed_marker':new.decode(),'identical_except_bundle_marker':True,'evidence':'tauri-utils 2.10.2 src/platform.rs:357-368; tauri bundle log patches NSIS type'}
(root/'test-results/install-m5/binary-comparison.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
print(json.dumps(report,indent=2))
