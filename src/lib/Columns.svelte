<script lang="ts">
  import EnumPanel, {type EnumSummary} from './EnumPanel.svelte';
  import { t, type MessageKey } from './i18n';
  import { compareValues, tsv, type ColumnInfo, type FileInfo } from './columns';
  let { file }:{file:FileInfo}=$props();
  const fields: {key:keyof ColumnInfo;label:MessageKey}[]=[
    {key:'index',label:'ordinal'},{key:'path',label:'columnName'},{key:'logical_type',label:'logicalType'},
    {key:'physical_type',label:'physicalType'},{key:'nullable',label:'nullable'},
    {key:'null_count',label:'nullCount'},{key:'null_rate',label:'nullRate'},
    {key:'min',label:'minimum'},{key:'max',label:'maximum'},{key:'distinct',label:'distinct'},
    {key:'compression',label:'compression'},{key:'encodings',label:'encodings'},
    {key:'compressed_bytes',label:'compressedSize'},{key:'compressed_ratio',label:'compressedRatio'},
    {key:'stats_complete',label:'statsStatus'},
  ];
  let search=$state('');let sort=$state<keyof ColumnInfo>('index');let descending=$state(false);
  let page=$state(0);let selected=$state(0);let copied=$state('');let copyFailed=$state(false);
  let selectionVersion=$state(0);let enumDistinct=$state<Record<number,string>>({});
  function enumeration(r:EnumSummary|null){if(!r){enumDistinct={};return;}if(r.column.nested||r.column.leaves.length!==1)return;const value=r.status==='complete'?(r.exact?String(r.distinct):`${t('enumSampleShort')} ${r.distinct}`):r.status==='high'?`>${r.cap}${r.exact?'':` (${t('enumSampleShort')})`}`:t(('enumStatus_'+r.status) as MessageKey);enumDistinct={...enumDistinct,[r.column.leaves[0]]:value};}
  let displayColumns=$derived(file.columns.map(c=>({...c,distinct:enumDistinct[c.index]??c.distinct})));
  const pageSize=50;
  let filtered=$derived(displayColumns.filter(c=>c.path.toLocaleLowerCase().includes(search.toLocaleLowerCase())));
  let ordered=$derived([...filtered].sort((a,b)=>{
    if (a[sort]===null || b[sort]===null) return compareValues(a[sort],b[sort]);
    const result=compareValues(a[sort],b[sort]); return (descending ? -result : result) || a.index-b.index;
  }));
  let visible=$derived(ordered.slice(page*pageSize,(page+1)*pageSize));
  let detail=$derived(displayColumns.find(c=>c.index===selected));
  function cell(c:ColumnInfo,key:keyof ColumnInfo):string {
    const value=c[key]; if (value===null || value===undefined) return '—';
    if (key==='nullable') return t(value ? 'yes' : 'no');
    if (key==='stats_complete') return t(value ? 'complete' : 'incomplete');
    if (key==='index') return String(c.index+1);
    if (key==='null_rate' || key==='compressed_ratio') return `${(Number(value)*100).toFixed(2)}%`;
    if (key==='min' || key==='max') return `${c.min_max_truncated ? '≈ ' : ''}${value==='' ? t('emptyString') : String(value)}`;
    return String(value);
  }
  async function copy(text:string) {
    try {await navigator.clipboard.writeText(text);copied=t('copied');copyFailed=false;}
    catch {copied=t('copyFailed');copyFailed=true;}
  }
  function copyAll() { void copy(tsv([fields.map(f=>t(f.label)),...ordered.map(c=>fields.map(f=>cell(c,f.key)))])); }
  function sortBy(key:keyof ColumnInfo) {descending=sort===key ? !descending : false;sort=key;page=0;}
</script>

<section class="overview" aria-label={t('fileOverview')}>
  <div><span>{t('rows')}</span><strong>{BigInt(file.num_rows).toLocaleString('zh-CN')}</strong></div>
  <div><span>{t('columns')}</span><strong>{file.column_count}</strong></div>
  <div><span>Row groups</span><strong>{file.row_groups}</strong></div>
  <div><span>{t('formatVersion')}</span><strong>{file.version}</strong></div>
  <div class="creator"><span>{t('createdBy')}</span><strong title={file.created_by??'—'}>{file.created_by??'—'}</strong></div>
</section>
<div class="workspace">
  <section class="column-panel">
    <div class="toolbar">
      <label><span class="sr-only">{t('searchColumns')}</span><input placeholder={t('searchColumns')} bind:value={search} oninput={()=>page=0} /></label>
      <span>{filtered.length} / {file.column_count}</span>
      <button onclick={copyAll} disabled={!ordered.length}>{t('copyTable')}</button>
    </div>
    <p class="hint">{t('cellHint')}</p>
    <div class="table-scroll">
      <table>
        <thead><tr>{#each fields as field}<th aria-sort={sort===field.key ? (descending?'descending':'ascending') : 'none'}><button onclick={()=>sortBy(field.key)}>{t(field.label)}{sort===field.key ? (descending?' ↓':' ↑') : ''}</button></th>{/each}</tr></thead>
        <tbody>{#each visible as c (c.index)}<tr class:selected={c.index===selected}>
          {#each fields as field}<td class:numeric={['index','null_count','null_rate','compressed_bytes','compressed_ratio','distinct'].includes(field.key)}>
            <button class="cell" class:missing={c[field.key]===null} title={cell(c,field.key)} onclick={()=>{selected=c.index;selectionVersion++;}} ondblclick={()=>copy(cell(c,field.key))} onkeydown={(e)=>{if(e.ctrlKey&&e.key.toLowerCase()==='c'){e.preventDefault();void copy(cell(c,field.key));}}}>{cell(c,field.key)}</button>
          </td>{/each}
        </tr>{/each}</tbody>
      </table>
      {#if !visible.length}<p class="no-results">{t('noColumns')}</p>{/if}
    </div>
    <div class="pagination"><span>{t('footerOnly')}</span><div><button disabled={page===0} onclick={()=>page--}>{t('previous')}</button><span>{page+1} / {Math.max(1,Math.ceil(ordered.length/pageSize))}</span><button disabled={(page+1)*pageSize>=ordered.length} onclick={()=>page++}>{t('next')}</button></div></div>
  </section>
  <aside aria-label={t('columnDetails')}>
    {#if detail}
      <div class="detail-title"><small>{t('columnDetails')}</small><h2>{detail.path}</h2><p>{detail.logical_type}</p></div>
      <EnumPanel {file} {selected} {selectionVersion} onresult={enumeration}/>
      <dl>{#each fields.filter(f=>!['index','path','logical_type'].includes(f.key)) as field}<div><dt>{t(field.label)}</dt><dd><span class:missing={detail[field.key]===null}>{cell(detail,field.key)}</span><button class="copy-cell" aria-label={`${t('copyCell')} ${t(field.label)}`} title={t('copyCell')} onclick={()=>copy(cell(detail!,field.key))}>⧉</button></dd></div>{/each}</dl>
      {#each detail.notes as note}<p class="note">{t(('note_'+note) as MessageKey)}</p>{/each}

    {:else}<p>{t('selectColumn')}</p>{/if}
  </aside>
</div>
{#if copied}<p class="copy-status" class:failed={copyFailed} role="status">{copied}</p>{/if}

<style>
  .overview{display:flex;flex-wrap:wrap;gap:24px;padding:18px 22px;background:var(--surface);border:1px solid var(--line);border-radius:9px;margin:16px 0}.overview div{display:flex;flex-direction:column;gap:5px;min-width:85px}.overview span{color:var(--muted);font-size:11px}.overview strong{font:600 18px Consolas,"Microsoft YaHei",monospace}.overview .creator{flex:1;min-width:180px}.creator strong{font:12px "Segoe UI",sans-serif;overflow-wrap:anywhere;line-height:1.6}
  .workspace{display:grid;grid-template-columns:minmax(0,1fr) 290px;gap:16px;min-width:0;align-items:start}.column-panel{border:1px solid var(--line);border-radius:9px;overflow:hidden;background:var(--surface);min-width:0}.toolbar{display:flex;align-items:center;gap:12px;padding:14px}.toolbar label{flex:1;min-width:90px}.toolbar input{width:100%;background:var(--bg);color:var(--text);border:1px solid var(--line);border-radius:5px;padding:9px 10px;font:inherit}.toolbar span{font-size:11px;color:var(--muted);white-space:nowrap}button{border:1px solid var(--line);background:var(--surface);color:var(--text);border-radius:5px;padding:7px 9px;font-size:12px}button:disabled{opacity:.35;cursor:default}.hint{font-size:11px;color:var(--muted);padding:0 14px 12px;margin:0}.table-scroll{overflow:auto;max-height:520px;min-height:230px;border-top:1px solid var(--line);border-bottom:1px solid var(--line)}table{border-collapse:separate;border-spacing:0;white-space:nowrap;font-size:12px;width:100%}th{position:sticky;top:0;z-index:1;background:var(--bg);border-bottom:1px solid var(--line);text-align:left}th button{font-weight:600;border:0;background:transparent;padding:13px 12px;white-space:nowrap}td{border-bottom:1px solid var(--line);padding:0;height:37px}.cell{display:block;border:0;border-radius:0;background:transparent;padding:10px 12px;max-width:240px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;width:100%;text-align:left;font-size:12px}.numeric .cell{text-align:right;font-family:Consolas,monospace}.selected td{background:var(--soft)}tr:hover td{background:var(--soft)}.missing{color:var(--muted)}.pagination{display:flex;align-items:center;justify-content:space-between;gap:12px;padding:12px;font-size:11px;color:var(--muted)}.pagination>div{display:flex;align-items:center;gap:10px}.pagination button{white-space:nowrap}.no-results{padding:50px;color:var(--muted);text-align:center}
  aside{background:var(--surface);border:1px solid var(--line);border-radius:9px;padding:18px;min-width:0;max-height:650px;overflow:auto}.detail-title small{font-size:11px;color:var(--muted)}h2{font-size:18px;margin:10px 0;overflow-wrap:anywhere}.detail-title p{font:12px Consolas,monospace;color:var(--accent);overflow-wrap:anywhere;margin:8px 0 18px}dl{margin:0}dl>div{padding:10px 0;border-top:1px solid var(--line)}dt{font-size:11px;color:var(--muted);margin-bottom:5px}dd{margin:0;display:flex;gap:8px;align-items:start;font:12px Consolas,"Microsoft YaHei",monospace;line-height:1.6}dd>span{flex:1;min-width:0;overflow-wrap:anywhere;white-space:pre-wrap;user-select:text}.copy-cell{padding:0 5px;flex-shrink:0;color:var(--muted)}.note{font-size:11px;line-height:1.7;color:var(--muted);background:var(--soft);padding:10px;border-radius:5px}.copy-status{margin:10px 0 0;color:var(--success);font-size:12px}.failed{color:#b35b35}.sr-only{position:absolute;width:1px;height:1px;overflow:hidden;clip-path:inset(50%)}
  @media(max-width:900px){.workspace{grid-template-columns:minmax(0,1fr)}aside{max-height:400px}.overview{gap:16px}.table-scroll{max-height:390px}}
</style>
