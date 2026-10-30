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
exe = root / os.environ.get('BENCH_EXE', 'src-tauri/target/release/ParquetLens.exe')
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

report = {'scope':'Normal no-CDP release launch; five repetitions, own process tree incl WebView2 at 8s and 20s; no forced GC or working-set trim; warm OS cache', 'exe':str(exe), 'runs':[]}
for i in range(5):
    proc = subprocess.Popen([str(exe)], cwd=root, env=env)
    try:
        time.sleep(8)
        assert proc.poll() is None
        first = sample(proc.pid)
        time.sleep(12)
        second = sample(proc.pid)
        close(proc)
        report['runs'].append({'at_8s':first, 'at_20s':second, 'normal_close':True})
    finally:
        if proc.poll() is None: proc.terminate()
    (root / ('test-results/'+os.environ.get('BENCH_LABEL','m5-final')+'-idle.json')).write_text(json.dumps(report, indent=2), encoding='utf-8')
print(json.dumps(report, indent=2))
