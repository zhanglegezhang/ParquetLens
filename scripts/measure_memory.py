"""Sample a Windows app's own process tree; no debug injection or system changes."""
import ctypes as C
from ctypes import wintypes as W
import json,sys,time
from pathlib import Path
class Entry(C.Structure):
 _fields_=[('dwSize',W.DWORD),('cntUsage',W.DWORD),('pid',W.DWORD),('heap',C.c_size_t),('module',W.DWORD),('threads',W.DWORD),('parent',W.DWORD),('priority',W.LONG),('flags',W.DWORD),('exe',W.WCHAR*260)]
class Memory(C.Structure):
 _fields_=[('cb',W.DWORD),('faults',W.DWORD)]+[(n,C.c_size_t) for n in ['peak_working','working','peak_paged','paged','peak_nonpaged','nonpaged','pagefile','peak_pagefile','private']]
k=C.WinDLL('kernel32',use_last_error=True);p=C.WinDLL('psapi',use_last_error=True)
k.CreateToolhelp32Snapshot.argtypes=[W.DWORD,W.DWORD];k.CreateToolhelp32Snapshot.restype=W.HANDLE
k.Process32FirstW.argtypes=[W.HANDLE,C.POINTER(Entry)];k.Process32NextW.argtypes=[W.HANDLE,C.POINTER(Entry)]
k.OpenProcess.argtypes=[W.DWORD,W.BOOL,W.DWORD];k.OpenProcess.restype=W.HANDLE;k.CloseHandle.argtypes=[W.HANDLE]
k.GetProcessTimes.argtypes=[W.HANDLE,C.POINTER(W.FILETIME),C.POINTER(W.FILETIME),C.POINTER(W.FILETIME),C.POINTER(W.FILETIME)]
def created(h):
 times=[W.FILETIME() for _ in range(4)]
 if not k.GetProcessTimes(h,*(C.byref(t) for t in times)):return None
 return (times[0].dwHighDateTime<<32)|times[0].dwLowDateTime
p.GetProcessMemoryInfo.argtypes=[W.HANDLE,C.POINTER(Memory),W.DWORD]
def sample(root):
 handle=k.OpenProcess(0x1000,False,root)
 root_created=created(handle) if handle else None
 if handle:k.CloseHandle(handle)
 snap=k.CreateToolhelp32Snapshot(2,0);entries=[];e=Entry();e.dwSize=C.sizeof(e)
 ok=k.Process32FirstW(snap,C.byref(e))
 while ok:
  entries.append((e.pid,e.parent,e.exe));ok=k.Process32NextW(snap,C.byref(e))
 k.CloseHandle(snap)
 ids={root}
 while True:
  new={pid for pid,parent,_ in entries if parent in ids}-ids
  if not new:break
  ids|=new
 result=[]
 for pid,parent,name in entries:
  if pid not in ids:continue
  h=k.OpenProcess(0x0400|0x0010,False,pid)
  if not h:continue
  born=created(h)
  if root_created is not None and (born is None or born<root_created):
   k.CloseHandle(h);continue
  m=Memory();m.cb=C.sizeof(m)
  if p.GetProcessMemoryInfo(h,C.byref(m),m.cb):result.append({'pid':pid,'name':name,'private':m.private,'working':m.working,'peak_private':m.peak_pagefile})
  k.CloseHandle(h)
 return {'private':sum(v['private'] for v in result),'working':sum(v['working'] for v in result),'processes':result}
if __name__=='__main__':
 pid=int(sys.argv[1]);out=Path(sys.argv[2]);stop=Path(sys.argv[3]);start=time.perf_counter();points=[]
 while not stop.exists() and time.perf_counter()-start<7200:
  point=sample(pid);point['seconds']=time.perf_counter()-start
  if len(sys.argv)>4:
   try:point['phase']=Path(sys.argv[4]).read_text(encoding='utf-8')
   except OSError:point['phase']='startup'
  if not point['processes']:break
  points.append(point);time.sleep(.05)
 result={'scope':'Own app process tree including WebView2; roughly 50ms sampling; includes runtime test work and CDP; working-set sum can double-count shared pages','samples':len(points),'max_private_bytes':max((v['private'] for v in points),default=0),'max_working_bytes':max((v['working'] for v in points),default=0),'peak_sample':max(points,key=lambda v:v['private'],default=None),'last_sample':points[-1] if points else None}
 result['phases']={phase:{'max_private_bytes':max(v['private'] for v in points if v.get('phase')==phase),'max_working_bytes':max(v['working'] for v in points if v.get('phase')==phase)} for phase in {v.get('phase') for v in points} if phase is not None}
 out.write_text(json.dumps(result,ensure_ascii=False,indent=2),encoding='utf-8')
