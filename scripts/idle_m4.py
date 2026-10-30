"""Normal no-CDP launch, own process-tree memory, WM_CLOSE exit and restart."""
import ctypes as C
from ctypes import wintypes as W
import json, os, subprocess, time, tempfile
from pathlib import Path
from measure_memory import sample

root = Path(__file__).resolve().parents[1]
env = dict(os.environ)
env.pop('WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS', None)
env['WEBVIEW2_USER_DATA_FOLDER'] = tempfile.mkdtemp(prefix='idle-m4-', dir=root / 'test-results')
env['LOCALAPPDATA'] = tempfile.mkdtemp(prefix='idle-m4-settings-', dir=root / 'test-results')
exe = root / 'src-tauri/target/release/ParquetLens.exe'
user = C.WinDLL('user32', use_last_error=True)
callback = C.WINFUNCTYPE(W.BOOL, W.HWND, W.LPARAM)
user.GetWindowThreadProcessId.argtypes = [W.HWND, C.POINTER(W.DWORD)]
user.PostMessageW.argtypes = [W.HWND, W.UINT, W.WPARAM, W.LPARAM]
user.EnumWindows.argtypes = [callback, W.LPARAM]
user.GetWindowTextW.argtypes = [W.HWND, W.LPWSTR, C.c_int]
user.IsWindowVisible.argtypes = [W.HWND]

def close(proc):
    @callback
    def visit(hwnd, _):
        pid = W.DWORD()
        user.GetWindowThreadProcessId(hwnd, C.byref(pid))
        title=C.create_unicode_buffer(256)
        user.GetWindowTextW(hwnd,title,256)
        if pid.value == proc.pid and user.IsWindowVisible(hwnd) and title.value=='ParquetLens':
            user.PostMessageW(hwnd, 0x10, 0, 0)
        return True
    user.EnumWindows(visit, 0)
    proc.wait(timeout=10)
    assert proc.returncode == 0

report = {'scope':'Delivery exe, normal startup without CDP, isolated WebView profile to avoid sharing with existing app instances; process tree Private Bytes at 8 seconds; not a cold-start timing measurement'}
proc = subprocess.Popen([str(exe)], cwd=root, env=env)
try:
    time.sleep(8)
    assert proc.poll() is None
    report['memory'] = sample(proc.pid)
    close(proc)
    report['normal_close'] = True
finally:
    if proc.poll() is None: proc.terminate()
proc = subprocess.Popen([str(exe)], cwd=root, env=env)
try:
    time.sleep(3)
    assert proc.poll() is None
    close(proc)
    report['restart_and_close'] = True
finally:
    if proc.poll() is None: proc.terminate()
(root / 'test-results/m4-idle.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
print(json.dumps(report, indent=2))

