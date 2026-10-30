"""Summarize raw repetitions without removing slow samples; nearest-rank P95."""
import json,math,statistics
from pathlib import Path
root=Path(__file__).resolve().parents[1]
out=root/'test-results'
def stats(values):
    ordered=sorted(values)
    return {'n':len(values),'median':round(statistics.median(values),2),'p95':round(ordered[math.ceil(.95*len(ordered))-1],2),'max':round(max(values),2)}
report={}
for label in ['m5-baseline','m5-s','m5-z','m5-final','m5-stress']:
    result={}
    p=out/(label+'.json')
    if p.exists():
        raw=json.loads(p.read_text(encoding='utf-8'))
        result['exe_bytes']=raw['bytes'];result['scope']=raw['scope'];result['datasets']={}
        for name,metrics in raw['datasets'].items():
            result['datasets'][name]={key:stats(v) for key,v in metrics.items() if key!='ui'}
            result['datasets'][name].update({key:stats(v) for key,v in metrics.get('ui',{}).items()})
        if 'lifecycle' in raw:
            life=raw['lifecycle'];result['lifecycle_private_MB']={'after_bench':round(life['empty_after_bench']['private']/1e6,2),'rounds_closed':[round(r['closed']['private']/1e6,2) for r in life['rounds']],'after_30s':round(life.get('after_30s_idle',{}).get('private',0)/1e6,2)}
    for suffix in ['memory','idle','startup']:
        p=out/(label+'-'+suffix+'.json')
        if not p.exists():continue
        raw=json.loads(p.read_text(encoding='utf-8'))
        if suffix=='memory':result['peak_private_MB']=round(raw['max_private_bytes']/1e6,2);result['peak_working_MB']=round(raw['max_working_bytes']/1e6,2)
        elif suffix=='idle':
            result['idle_private_MB']=stats([r['at_20s']['private']/1e6 for r in raw['runs']]);result['idle_working_MB']=stats([r['at_20s']['working']/1e6 for r in raw['runs']])
        else:result['startup_ms']={mode:stats([r['ms'] for r in raw['results'] if r['mode']==mode]) for mode in {r['mode'] for r in raw['results']}}
    if result:report[label]=result
(out/'m5-summary.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps(report,ensure_ascii=False,indent=2))
