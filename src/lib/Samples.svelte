<script lang="ts">
  import {onMount,untrack} from 'svelte';
  import {preferences,session} from './preferences.svelte';
  import {invoke} from '@tauri-apps/api/core';
  import {listen} from '@tauri-apps/api/event';
  import {save} from '@tauri-apps/plugin-dialog';
  import type {FileInfo} from './columns';
  import {t,type MessageKey} from './i18n';
  let {file}: {file:FileInfo}=$props();
  type Field={index:number;name:string;data_type:string;uuid:boolean};
  type Preview={fields:Field[];rows:(string|null)[][];total:number;offset:number};
  type Report={rows:number;columns:number;truncated:number;cleaned:number;precision_text:number;empty_strings:number;path:string;protected:number};
  let fields=$state<Field[]>([]),selected=$state<number[]>([]),mode=$state('head'),n=$state(preferences.sample_n),search=$state('');
  let preview=$state<Preview|null>(null),busy=$state(false),ready=$state(false),error=$state(''),status=$state(''),detail=$state<string|null>(null);
  let protect=$state(true);
  $effect(()=>{const time=preferences.time_mode;untrack(()=>{void time;reset();});});
  let report=$state<Report|null>(null),scroll=$state(0),progress=$state(0),request='',disposed=false;
  let viewport=$state<HTMLDivElement>();
  const pageSize=500,rowHeight=36;
  let filtered=$derived(fields.filter(f=>f.name.toLocaleLowerCase().includes(search.toLocaleLowerCase())));
  let start=$derived(Math.max(0,Math.floor(scroll/rowHeight)-3));
  let shown=$derived(preview?.rows.slice(start,start+22) ?? []);
  let valid=$derived(Number.isInteger(n)&&n>=1&&n<=1000000&&selected.length>0&&ready);
  function failure(cause:unknown){const e=cause as {code?:string;detail?:string};error=`${t((['cancelled','task_busy','export_failed','file_changed','parse_failed','unreadable','resource_limit','invalid_request','task_failed'].includes(e.code??'')?e.code:'unknown') as MessageKey)}${e.detail?'\n'+e.detail:''}`;}
  function reset(){preview=null;report=null;detail=null;status='';scroll=0;}
  function toggle(index:number){selected=selected.includes(index)?selected.filter(i=>i!==index):[...selected,index];reset();}
  function options(){return {mode,n,columns:[...selected]};}
  async function load(offset=0){
    if(busy||session.task||!valid)return;
    const timeMode=preferences.time_mode;busy=true;error='';report=null;status=t('previewLoading');request=crypto.randomUUID();progress=0;
    try{const result=await invoke<Preview>('get_preview',{id:file.id,options:options(),offset,limit:pageSize,request});
      if(disposed||timeMode!==preferences.time_mode)return;preview=result;scroll=0;if(viewport)viewport.scrollTop=0;status=t('previewReady');
    }catch(e){if(!disposed)failure(e);}finally{if(!disposed){busy=false;status='';}}
  }
  async function exportFile(csv=false){
    if(busy||session.task||!valid)return;
    busy=true;error='';report=null;progress=0;request=crypto.randomUUID();
    try{
      const outPath=await save({defaultPath:file.name.replace(/\.parquet$/i,'')+(csv?'.sample.csv':'.sample.xlsx'),filters:[{name:csv?'CSV':'Excel',extensions:[csv?'csv':'xlsx']}]});
      if(!outPath||disposed)return;
      status=t('exportWriting');
      const result=await invoke<Report>(csv?'export_sample_csv':'export_xlsx',{id:file.id,options:options(),outPath,request,protect});
      if(!disposed){report=result;status=t('exportDone');progress=1;}
    }catch(e){if(!disposed){status='';failure(e);}}finally{if(!disposed)busy=false;}
  }
  async function cancel(){status=t('cancelling');try{await invoke('cancel_task',{id:file.id,request});}catch(e){failure(e);}}
  async function copy(){if(detail===null)return;try{await navigator.clipboard.writeText(detail);status=t('copied');}catch{status=t('copyFailed');}}
  function brief(value:string|null){if(value===null)return 'NULL';if(value==='')return t('emptyString');let result='',count=0;for(const ch of value){if(count++===256)return result+'…';result+=ch;}return result;}
  onMount(()=>{
    let unlisten:(()=>void)|undefined;
    void(async()=>{try{
      const off=await listen<{request:string;done:number;total:number;phase:string}>('sample_progress',({payload:p})=>{
        if(p.request===request&&!disposed){progress=p.total?p.done/p.total:0;status=t(p.phase==='packaging'?'exportPackaging':'exportWriting');}
      });if(disposed){off();return;}unlisten=off;
      const result=await invoke<Field[]>('sample_fields',{id:file.id});
      if(!disposed){fields=result;selected=result.map(f=>f.index);ready=true;}
    }catch(e){if(!disposed)failure(e);}})();
    return()=>{disposed=true;unlisten?.();void invoke('cancel_task',{id:file.id,request}).catch(()=>{});};
  });
</script>

<section class="samples" aria-label={t('sampleView')}>
  <div class="controls">
    <label>{t('sampleMode')}<select bind:value={mode} onchange={reset} disabled={busy}><option value="head">{t('headRows')}</option><option value="uniform">{t('uniformRows')}</option></select></label>
    <label>{t('sampleCount')}<input aria-label={t('sampleCount')} type="number" min="1" max="1000000" step="1" bind:value={n} oninput={reset} disabled={busy}/></label>
    <button class="primary" onclick={()=>load()} disabled={busy||!!session.task||!valid}>{t('loadPreview')}</button>
    <button class="primary" onclick={()=>exportFile(false)} disabled={busy||!!session.task||!valid||selected.length>16384}>{t('exportXlsx')}</button>
  </div>
  <div class="controls"><label><input type="checkbox" bind:checked={protect} disabled={busy}/>{t('csvProtect')}</label><button onclick={()=>exportFile(true)} disabled={busy||!!session.task||!valid}>{t('exportCsv')}</button></div><p class="hint">{t('csvHint')}</p>
  <p class="hint">{mode==='uniform'?t('uniformHint'):t('headHint')} {t('sampleLimitHint')}</p>
  <details class="selection"><summary>{t('chooseColumns')} · {selected.length} / {fields.length}</summary>
    <div class="selection-tools"><input aria-label={t('searchColumns')} placeholder={t('searchColumns')} bind:value={search}/><button disabled={busy} onclick={()=>{selected=fields.map(f=>f.index);reset();}}>{t('selectAll')}</button><button disabled={busy} onclick={()=>{selected=[];reset();}}>{t('clearSelection')}</button></div>
    <div class="field-list">{#each filtered as field}<label title={field.data_type}><input type="checkbox" checked={selected.includes(field.index)} disabled={busy} onchange={()=>toggle(field.index)}/><span>{field.name}</span></label>{/each}</div>
  </details>
  {#if error}<div class="sample-error" role="alert">{error}</div>{/if}
  {#if preview}
    <div class="page-bar"><span>{t('sampleRows')}: {preview.total.toLocaleString()} · {preview.total?preview.offset+1:0}–{preview.offset+preview.rows.length}</span><span><button disabled={busy||preview.offset===0} onclick={()=>load(Math.max(0,preview!.offset-pageSize))}>{t('previous')}</button><button disabled={busy||preview.offset+pageSize>=preview.total} onclick={()=>load(preview!.offset+pageSize)}>{t('next')}</button></span></div>
    <div class="preview-scroll" bind:this={viewport} onscroll={()=>scroll=viewport?.scrollTop??0}>
      <table class="preview-table"><thead><tr><th class="ordinal">#</th>{#each preview.fields as f}<th title={f.data_type}>{f.name}<small>{f.data_type}</small></th>{/each}</tr></thead>
        <tbody><tr aria-hidden="true" style:height={`${start*rowHeight}px`}><td colspan={preview.fields.length+1}></td></tr>
          {#each shown as row,i}<tr style:height={`${rowHeight}px`}><td class="ordinal">{preview.offset+start+i+1}</td>{#each row as value}<td><button class:null-value={value===null} onclick={()=>detail=value} title={t('viewFullValue')}>{brief(value)}</button></td>{/each}</tr>{/each}
          <tr aria-hidden="true" style:height={`${Math.max(0,preview.rows.length-start-shown.length)*rowHeight}px`}><td colspan={preview.fields.length+1}></td></tr>
        </tbody></table>
    </div>
    <p class="hint">{t('previewHint')}</p>
  {:else}<div class="preview-empty">{t('previewEmpty')}</div>{/if}
  {#if detail!==null}<div class="value-detail"><div>{t('fullValue')} <button onclick={copy}>{t('copyCell')}</button><button onclick={()=>detail=null}>{t('dismiss')}</button></div><textarea readonly value={detail} aria-label={t('fullValue')}></textarea></div>{/if}
  {#if report}<div class="export-report" role="status"><strong>{t('exportDone')}</strong><p>{report.path}</p><p>{report.rows} {t('sampleRows')} × {report.columns} {t('exportColumns')} · {t('truncatedCells')}: {report.truncated} · {t('cleanedCells')}: {report.cleaned} · {t('precisionCells')}: {report.precision_text} · {t('emptyCells')}: {report.empty_strings} · {t('protectedCells')}: {report.protected??0}</p></div>{/if}
  <div class="task-bar" class:running={busy} role="status"><span>{status||t('sampleReady')}</span>{#if busy}<progress max="1" value={progress||undefined}></progress><button onclick={cancel}>{t('cancelTask')}</button>{/if}</div>
</section>

<style>
  .samples{margin-top:16px;background:var(--surface);border:1px solid var(--line);border-radius:12px;padding:18px;min-width:0}.controls,.selection-tools,.page-bar,.task-bar{display:flex;align-items:center;gap:12px;flex-wrap:wrap}.controls label{display:flex;align-items:center;gap:8px}.controls input{width:130px}input,select,button,textarea{font:inherit;color:var(--text);background:var(--surface);border:1px solid var(--line);border-radius:5px;padding:7px 10px}button{cursor:pointer}.primary{background:var(--accent);color:var(--surface)}.hint{font-size:12px;color:var(--muted);line-height:1.7}.selection{border-top:1px solid var(--line);padding:12px 0}summary{cursor:pointer}.selection-tools{margin:12px 0}.field-list{display:grid;grid-template-columns:repeat(auto-fill,minmax(180px,1fr));gap:8px;max-height:200px;overflow:auto}.field-list label{display:flex;align-items:center;gap:8px;min-width:0}.field-list span{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.sample-error{white-space:pre-wrap;overflow-wrap:anywhere;background:var(--soft);color:var(--text);border:1px solid #bd6c5c;padding:12px;margin:12px 0}.page-bar{justify-content:space-between;margin:12px 0}.page-bar button{margin-left:8px}.preview-scroll{height:420px;overflow:auto;border:1px solid var(--line);position:relative}.preview-table{border-collapse:separate;border-spacing:0;table-layout:fixed;width:max-content;min-width:100%;font-family:Consolas,'Microsoft YaHei',monospace}.preview-table th{position:sticky;top:0;z-index:1;background:var(--soft);text-align:left;width:210px;max-width:210px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;height:52px;padding:6px 10px;border-bottom:1px solid var(--line)}th small{display:block;color:var(--muted);font-size:10px;overflow:hidden;text-overflow:ellipsis}.preview-table td{padding:0;border-bottom:1px solid var(--line);height:inherit;max-width:210px}.preview-table td button{display:block;width:210px;border:0;border-radius:0;height:35px;text-align:right;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;background:transparent;font-family:inherit}.preview-table td button:hover{background:var(--soft)}.preview-table .ordinal{width:65px;min-width:65px;max-width:65px;text-align:right;padding-right:10px;color:var(--muted);font-size:12px}.null-value{color:var(--muted)}.preview-empty{padding:36px;text-align:center;color:var(--muted)}.value-detail{margin:12px 0}.value-detail button{margin-left:8px}.value-detail textarea{width:100%;height:150px;margin-top:8px;font-family:Consolas,monospace}.export-report{background:var(--soft);padding:12px;overflow-wrap:anywhere;margin:12px 0}.task-bar{padding-top:12px;border-top:1px solid var(--line);font-size:12px;color:var(--muted)}.task-bar.running{position:fixed;bottom:0;left:0;right:0;z-index:20;padding:12px 24px;background:var(--surface);box-shadow:0 -2px 12px #0002}.task-bar progress{flex:1;max-width:300px}@media(max-width:740px){.samples{padding:12px}.controls{align-items:stretch}.preview-scroll{height:360px}}
</style>
