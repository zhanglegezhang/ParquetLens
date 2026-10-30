<script lang="ts">
  import {onMount,untrack} from 'svelte';
  import {invoke} from '@tauri-apps/api/core';
  import {listen} from '@tauri-apps/api/event';
  import {save} from '@tauri-apps/plugin-dialog';
  import {t,type MessageKey} from './i18n';
  import {preferences,session} from './preferences.svelte';
  import {tsv,type FileInfo} from './columns';
  type Column={root:number;leaves:number[];name:string;data_type:string;candidate:boolean;nested:boolean;numeric:boolean};
  type Entry={value:string|null;count:string};
  export type EnumSummary={id:string;column:Column;status:string;exact:boolean;cap:number;mode:string;k:number;scanned:string;scope_rows:string;file_rows:string;groups:number;scope_groups:number;file_groups:number;distinct:number|null;source:string;message:string|null};
  type Page={rows:Entry[];total:number;offset:number};
  let {file,selected,selectionVersion,onresult}:{file:FileInfo;selected:number;selectionVersion:number;onresult:(r:EnumSummary|null)=>void}=$props();
  let catalog=$state<Column[]>([]),results=$state<Record<number,EnumSummary>>({}),ready=$state(false),busy=$state(false),cap=$state(preferences.cap),mode=$state('exact'),k=$state(3);
  let search=$state(''),sort=$state('count'),descending=$state(true),page=$state<Page|null>(null),offset=$state(0),detail=$state<string|null>(null),error=$state(''),status=$state(''),warnings=$state<string[]>([]),progress=$state({done:0,total:0,scanned_groups:0});
  let protect=$state(true);
  $effect(()=>{const revision=session.cacheRevision;untrack(()=>{void revision;results={};onresult(null);});});
  let request='',disposed=false,runningAll=false,runningRoot=-1,queued=$state<number|null>(null),pageRequest=0;
  let current=$derived(catalog.find(c=>c.leaves.includes(selected)));
  let result=$derived(current?results[current.root]:undefined);
  let valid=$derived(Number.isInteger(cap)&&cap>=50&&cap<=5000&&Number.isInteger(k)&&k>=1);
  function fail(e:unknown){const v=e as {code?:string;detail?:string};const key=(['cancelled','task_busy','export_failed','cache_failed','file_changed','parse_failed','unreadable','invalid_request','task_failed'].includes(v.code??'')?v.code:'unknown') as MessageKey;error=t(key)+(v.detail?'\n'+v.detail:'');}
  function accept(r:EnumSummary){const previous=results[r.column.root];if(previous?.id===r.id&&previous.status===r.status&&previous.scanned===r.scanned&&previous.source===r.source)return;results={...results,[r.column.root]:r};onresult(r);}
  function choose(root:number){if(results[root]||!catalog[root]?.candidate||(session.task&&!busy))return;if(busy){if(!runningAll&&runningRoot!==root){queued=root;void cancel();}return;}void run([root]);}
  $effect(()=>{const c=current;const version=selectionVersion;if(ready&&c&&version>0)untrack(()=>choose(c.root));});
  $effect(()=>{const time=preferences.time_mode;void time;const r=result;const q=search;const s=sort;const d=descending;const off=offset;
    untrack(()=>{const token=++pageRequest;page=null;detail=null;if(!r||r.status!=='complete')return;
      void invoke<Page>('enum_values',{id:file.id,resultId:r.id,options:{offset:off,limit:50,search:q,sort:s,descending:d}}).then(p=>{if(!disposed&&token===pageRequest)page=p;}).catch(e=>{if(!disposed&&token===pageRequest)fail(e);});
    });
  });
  function reset(){results={};onresult(null);page=null;offset=0;search='';warnings=[];error='';if(current&&selectionVersion>0&&valid)void run([current.root]);}
  async function run(columns:number[],force=false,all=false){
    if(!valid||!ready||busy||session.task||columns.length===0)return;
    request=crypto.randomUUID();busy=true;runningAll=all;runningRoot=columns.length===1?columns[0]:-1;error='';warnings=[];status=t('enumScanning');progress={done:0,total:mode==='quick'?Math.min(k,file.row_groups):file.row_groups,scanned_groups:0};
    for(const root of columns)delete results[root];results={...results};offset=0;
    try{const out=await invoke<{results:EnumSummary[];warnings:string[];cancelled:boolean}>('analyze_enums',{id:file.id,options:{columns,cap,mode,k,force},request});if(disposed)return;for(const r of out.results)accept(r);warnings=out.warnings;status=t(out.cancelled?'enumCancelled':'enumDone');}
    catch(e){if(!disposed){fail(e);results={};onresult(null);}}
    finally{if(!disposed)busy=false;}
  }
  $effect(()=>{const next=queued;if(next!==null&&!busy&&!session.task)untrack(()=>{queued=null;if(!disposed&&!results[next])void run([next]);});});
  async function cancel(){status=t('cancelling');try{await invoke('cancel_task',{id:file.id,request});}catch(e){if(!disposed)fail(e);}}
  async function clearCache(){try{await invoke('clear_enum_cache');session.cacheRevision++;results={};page=null;onresult(null);status=t('cacheCleared');warnings=[];}catch(e){fail(e);}}
  function ratio(count:string){if(!result||result.scope_rows==='0')return '—';return `${(Number(count)/Number(result.scope_rows)*100).toFixed(2)}%`;}
  function brief(s:string|null){if(s===null)return '<NULL>';if(s==='')return t('emptyString');let out='',n=0;for(const c of s){if(n++>=128)return out+'…';out+=c;}return out;}
  async function copy(value:string){try{await navigator.clipboard.writeText(value);status=t('copied');}catch{status=t('copyFailed');}}
  async function copyAll(){if(!result)return;const selectedResult=result;const rows:Entry[]=[];try{for(let at=0;;at+=500){const p=await invoke<Page>('enum_values',{id:file.id,resultId:selectedResult.id,options:{offset:at,limit:500,search,sort,descending}});rows.push(...p.rows);if(at+500>=p.total)break;}await copy(tsv([[t('enumValue'),t('enumIsNull'),t('enumCount'),t('enumRatio')],...rows.map(v=>[v.value??'<NULL>',v.value===null?t('yes'):t('no'),v.count,ratio(v.count)])]));}catch(e){fail(e);}}
  async function exportFile(csv=false){if(!result||busy||session.task)return;busy=true;request=crypto.randomUUID();error='';status=t('exportWriting');
    try{const path=await save({defaultPath:`${file.name}.${result.column.name}.enums.${csv?'csv':'xlsx'}`.replace(/[<>:"/\\|?*]/g,'_'),filters:[{name:csv?'CSV':'Excel',extensions:[csv?'csv':'xlsx']}]});if(!path||disposed)return;
      const r=await invoke<{rows:number;truncated:number;cleaned:number;protected:number}>(csv?'export_enum_csv':'export_enum_xlsx',{id:file.id,resultId:result.id,outPath:path,request,protect});if(!disposed)status=`${t('exportDone')} · ${r.rows} · ${t('truncatedCells')}: ${r.truncated} · ${t('cleanedCells')}: ${r.cleaned} · ${t('protectedCells')}: ${r.protected??0}`;
    }catch(e){if(!disposed)fail(e);}finally{if(!disposed)busy=false;}
  }
  onMount(()=>{const off:(()=>void)[]=[];void(async()=>{try{
    const a=await listen<{request:string;result:EnumSummary}>('enum_result',e=>{if(!disposed&&e.payload.request===request)accept(e.payload.result);});if(disposed){a();return;}off.push(a);
    const b=await listen<{request:string;progress:typeof progress}>('enum_progress',e=>{if(!disposed&&e.payload.request===request)progress=e.payload.progress;});if(disposed){b();return;}off.push(b);
    const data=await invoke<Column[]>('enum_catalog',{id:file.id});if(!disposed){catalog=data;ready=true;}
  }catch(e){if(!disposed)fail(e);}})();return()=>{disposed=true;off.forEach(f=>f());void invoke('cancel_task',{id:file.id,request}).catch(()=>{});};});
</script>

<section class="enum-panel" aria-label={t('enumValues')}>
  <h3>{t('enumValues')}</h3>
  <div class="enum-controls"><label>cap <input aria-label="cap" type="number" min="50" max="5000" step="1" bind:value={cap} onchange={reset} disabled={busy}/></label><label>{t('enumMode')}<select aria-label={t('enumMode')} bind:value={mode} onchange={reset} disabled={busy}><option value="exact">{t('enumExact')}</option><option value="quick">{t('enumQuick')}</option></select></label>
    {#if mode==='quick'}<label>K <input aria-label="K" type="number" min="1" step="1" bind:value={k} onchange={reset} disabled={busy}/></label>{/if}
  </div>
  <div class="enum-buttons"><button disabled={!ready||!valid||busy||!!session.task||!current} onclick={()=>current&&run([current.root])}>{t(current?.candidate?'enumAnalyze':'enumForceAnalyze')}</button><button disabled={!ready||!valid||busy||!!session.task||!catalog.some(c=>c.candidate)} onclick={()=>run(catalog.filter(c=>c.candidate).map(c=>c.root),false,true)}>{t('enumAnalyzeAll')}</button><button disabled={!ready||!valid||busy||!!session.task||!current} onclick={()=>current&&run([current.root],true)}>{t('enumRecompute')}</button><button onclick={clearCache}>{t('clearEnumCache')}</button></div>
  {#if current?.nested}<p class="enum-note">{t('enumNestedHint')} <strong>{current.name}</strong></p>{/if}
  {#if !result&&!busy}<p class="enum-note">{t(current?.candidate?'enumClickHint':'enumManualHint')}</p>{/if}
  {#if result}
    <div class="enum-summary" data-status={result.status}>
      <strong>{t(('enumStatus_'+result.status) as MessageKey)}{result.status==='high'?` > ${result.cap}`:''}</strong>
      <span>{result.exact?t('enumExact'):t('enumSampleWarning')} · {t(('enumSource_'+result.source) as MessageKey)}</span>
      <span>{result.status==='complete'?`${t('distinct')}: ${result.distinct} · `:''}{t('enumCoverage')}: {result.scope_groups}/{result.file_groups} RG · {result.scope_rows}/{result.file_rows} {t('rows')}</span>
      {#if result.status==='resource'}<p>{t('enumResourceHint')}</p>{/if}
    </div>
    {#if result.status==='complete'}
      <input class="enum-search" aria-label={t('enumSearch')} placeholder={t('enumSearch')} bind:value={search} oninput={()=>offset=0}/>
      <div class="enum-sort"><select aria-label={t('enumSort')} bind:value={sort} onchange={()=>offset=0}><option value="count">{t('enumCount')}</option><option value="value">{t('enumValue')}</option></select><button onclick={()=>{descending=!descending;offset=0;}}>{descending?'↓':'↑'}</button><button onclick={copyAll}>{t('copyEnums')}</button></div>
      {#if page}<div class="enum-table-scroll"><table class="enum-table"><thead><tr><th>{t('enumValue')}</th><th>{t('enumCount')}</th><th>{t('enumRatio')}</th></tr></thead><tbody>{#each page.rows as row}<tr><td><button class:null-value={row.value===null} title={row.value===null?t('enumIsNull'):row.value} onclick={()=>detail=row.value===null?'<NULL>':row.value}>{brief(row.value)}{row.value===null?` (${t('enumIsNull')})`:''}</button></td><td>{row.count}</td><td>{ratio(row.count)}</td></tr>{/each}</tbody></table></div>
        <div class="enum-pagination"><button disabled={offset===0} onclick={()=>offset=Math.max(0,offset-50)}>{t('previous')}</button><span>{page.total?offset+1:0}–{offset+page.rows.length} / {page.total}</span><button disabled={offset+50>=page.total} onclick={()=>offset+=50}>{t('next')}</button></div>
      {/if}
    {/if}
    {#if ['complete','high'].includes(result.status)}<button disabled={busy} onclick={()=>exportFile(false)}>{t('exportEnumXlsx')}</button><button disabled={busy||!!session.task} onclick={()=>exportFile(true)}>{t('exportEnumCsv')}</button><label class="enum-note"><input type="checkbox" bind:checked={protect} disabled={busy}/>{t('csvProtect')}</label><p class="enum-note">{t('csvHint')}</p>{/if}
  {/if}
  {#if detail!==null}<div class="enum-detail"><textarea readonly value={detail} aria-label={t('enumFullValue')}></textarea><button onclick={()=>copy(detail??'')}>{t('copyCell')}</button><button onclick={()=>detail=null}>{t('dismiss')}</button></div>{/if}
  {#if error}<p class="enum-error" role="alert">{error}</p>{/if}
  {#each warnings as warning}<p class="enum-note">{t('cacheWarning')} {warning}</p>{/each}
  {#if busy}<div class="enum-task" role="status"><span>{status} · {progress.done}/{progress.total} RG</span><progress max={progress.total||1} value={progress.done}></progress><button onclick={cancel}>{t('cancelTask')}</button></div>{:else if status}<p class="enum-note" role="status">{status}</p>{/if}
</section>
<style>
  .enum-panel{margin:14px 0 20px;border-top:1px solid var(--line);padding-top:8px}h3{font-size:14px;margin:10px 0}.enum-controls,.enum-buttons,.enum-sort,.enum-pagination{display:flex;gap:6px;align-items:center;flex-wrap:wrap;margin:8px 0}.enum-controls label{font-size:11px;display:flex;gap:5px;align-items:center}.enum-controls input{width:70px}input,select,button,textarea{font:inherit;font-size:11px;color:var(--text);background:var(--surface);border:1px solid var(--line);border-radius:5px;padding:6px}button{cursor:pointer}button:disabled{opacity:.4;cursor:default}.enum-note{font-size:11px;color:var(--muted);line-height:1.7;overflow-wrap:anywhere}.enum-summary{display:flex;flex-direction:column;gap:5px;font-size:11px;background:var(--soft);padding:10px;border-radius:6px;margin:10px 0}.enum-search{width:100%}.enum-table-scroll{overflow:auto;max-height:270px}.enum-table{border-collapse:collapse;width:100%;font-size:11px;table-layout:fixed}.enum-table th{text-align:left;border-bottom:1px solid var(--line);padding:7px 2px}.enum-table th:first-child{width:48%}.enum-table td{padding:5px 2px;border-bottom:1px solid var(--line);text-align:right;font-family:Consolas,'Microsoft YaHei',monospace;overflow-wrap:anywhere}.enum-table td button{width:100%;max-width:170px;border:0;text-align:left;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;background:transparent}.null-value{color:var(--muted)}.enum-pagination{font-size:10px;justify-content:space-between}.enum-pagination button{font-size:10px;padding:4px}.enum-detail textarea{width:100%;height:100px;resize:vertical;margin:8px 0}.enum-detail button{margin-right:5px}.enum-error{font-size:11px;color:#bf6f5f;white-space:pre-wrap;overflow-wrap:anywhere}.enum-task{position:fixed;bottom:0;left:0;right:0;z-index:30;background:var(--surface);border-top:1px solid var(--line);box-shadow:0 -2px 12px #0002;padding:12px 24px;display:flex;align-items:center;gap:12px;font-size:12px}.enum-task progress{max-width:260px;flex:1}
</style>

