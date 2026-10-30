"""Operate only a supplied test process's visible ParquetLens window."""
import ctypes as C
from ctypes import wintypes as W
import json,sys
pid=int(sys.argv[1]);action=sys.argv[2];user=C.WinDLL('user32',use_last_error=True)
callback=C.WINFUNCTYPE(W.BOOL,W.HWND,W.LPARAM)
user.GetWindowThreadProcessId.argtypes=[W.HWND,C.POINTER(W.DWORD)]
user.GetWindowTextW.argtypes=[W.HWND,W.LPWSTR,C.c_int]
user.IsWindowVisible.argtypes=[W.HWND];user.IsIconic.argtypes=[W.HWND]
user.ShowWindow.argtypes=[W.HWND,C.c_int];user.PostMessageW.argtypes=[W.HWND,W.UINT,W.WPARAM,W.LPARAM]
user.EnumWindows.argtypes=[callback,W.LPARAM]
result=[]
@callback
def visit(hwnd,_):
    found=W.DWORD();user.GetWindowThreadProcessId(hwnd,C.byref(found))
    title=C.create_unicode_buffer(256);user.GetWindowTextW(hwnd,title,256)
    if found.value==pid and user.IsWindowVisible(hwnd) and title.value=='ParquetLens':
        if action=='minimize':user.ShowWindow(hwnd,6)
        elif action=='close':user.PostMessageW(hwnd,0x10,0,0)
        result.append({'hwnd':hwnd,'minimized':bool(user.IsIconic(hwnd))})
    return True
user.EnumWindows(visit,0)
print(json.dumps(result))
