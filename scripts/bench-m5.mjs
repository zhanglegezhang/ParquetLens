import {chromium} from '@playwright/test';
import {spawn,execFileSync} from 'node:child_process';
import {mkdtemp,writeFile,stat,copyFile,readFile} from 'node:fs/promises';
import path from 'node:path';
import net from 'node:net';
import {createHash} from 'node:crypto';
const root=process.cwd(),out=path.join(root,'test-results'),label=process.env.BENCH_LABEL||'m5-final',reps=Number(process.env.BENCH_REPS||20);
const run=await mkdtemp(path.join(out,'m5-'));const server=net.createServer();await new Promise(r=>server.listen(0,'127.0.0.1',r));const port=server.address().port;await new Promise(r=>server.close(r));
const exe=path.resolve(process.env.BENCH_EXE||'src-tauri/target/release/ParquetLens.exe');
const child=spawn(exe,[],{cwd:root,env:{...process.env,LOCALAPPDATA:path.join(run,'local'),WEBVIEW2_USER_DATA_FOLDER:path.join(run,'webview'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`},stdio:'ignore'});
const stop=path.join(run,'stop'),phase=path.join(run,'phase');const monitor=spawn(path.join(root,'.venv/Scripts/python.exe'),[path.join(root,'scripts/measure_memory.py'),String(child.pid),path.join(out,label+'-memory.json'),stop,phase],{windowsHide:true,stdio:'ignore'});let browser;const report={label,output_xlsx:path.join(run,'bench.xlsx'),date:new Date().toISOString(),reps,exe,sha256:createHash('sha256').update(await readFile(exe)).digest('hex'),bytes:(await stat(exe)).size,scope:'One unrecorded full warmup per dataset, then real release IPC; UI timing uses in-page click/predicate/two animation frames, CDP attached. Not cold-disk or cold-start evidence.',datasets:{}};
try{
 const endpoint=`http://127.0.0.1:${port}`;for(let i=0;i<200;i++){if(child.exitCode!==null)throw Error('App exited/forwarded; close existing instance before benchmark');try{if((await fetch(endpoint+'/json/version')).ok)break;}catch{}await new Promise(r=>setTimeout(r,100));}
 browser=await chromium.connectOverCDP(endpoint);const page=browser.contexts()[0].pages()[0];await page.waitForURL(/tauri.localhost/);await page.waitForFunction(()=>!!window.__TAURI_INTERNALS__);report.errors=[];page.on('pageerror',e=>report.errors.push(e.message));
 for(const name of (process.env.BENCH_DATASETS||'benchmark,mixed-snappy,mixed-zstd').split(',')){
 const file=path.join(root,'bench/data',name+'.parquet');const oracle=JSON.parse(await readFile(path.join(out,'m5-data-oracle.json'),'utf8'))[name];
 await writeFile(phase,name+' IPC scan-preview-export');
 report.datasets[name]=await page.evaluate(async({file,reps,output,oracle})=>{
 let recording=false;const ipc=window.__TAURI_INTERNALS__.invoke, metrics={};const time=async(name,fn)=>{const t=performance.now();const r=await fn();if(recording)(metrics[name]??=[]).push(performance.now()-t);return r;};
 for(let i=-1;i<reps;i++){
 recording=i>=0;const m=await time('open_metadata_ipc',()=>ipc('open_file',{path:file}));const id=m.id;
 const fields=await ipc('sample_fields',{id});const columns=fields.map(f=>f.index);
 for(const mode of ['head','uniform']) await time(mode+'_1000_ipc',async()=>{for(const offset of [0,500]){const p=await ipc('get_preview',{id,options:{mode,n:1000,columns},offset,limit:500,request:crypto.randomUUID()});if(p.rows.length!==500)throw Error('preview rows');}});
 await time('xlsx_1000x50',()=>ipc('export_xlsx',{id,options:{mode:'head',n:1000,columns},outPath:output,request:crypto.randomUUID()}));
 const opts={cap:200,mode:'exact',k:3,force:true};
 const single=await time('single_exact',()=>ipc('analyze_enums',{id,options:{...opts,columns:[5]},request:crypto.randomUUID()}));if(single.results[0].status!=='complete')throw Error('single result');
 const all=await time('all_exact',()=>ipc('analyze_enums',{id,options:{...opts,columns},request:crypto.randomUUID()}));if(all.results.length!==50||all.results.filter(r=>r.status==='high').length!==5)throw Error('all status');
 const hit=await time('cache_ipc',()=>ipc('analyze_enums',{id,options:{...opts,columns:[5],force:false},request:crypto.randomUUID()}));if(hit.results[0].source!=='cache')throw Error('not cache');
 const values=await ipc('enum_values',{id,resultId:hit.results[0].id,options:{offset:0,limit:500,search:'',sort:'count',descending:true}});if(values.rows.reduce((n,r)=>n+BigInt(r.count),0n)!==BigInt(m.num_rows))throw Error('counts');const actual=new Map(values.rows.map(v=>[v.value,v.count]));if(actual.size!==oracle.category_0_counts.length||oracle.category_0_counts.some(v=>actual.get(v.value)!==v.count))throw Error('Independent PyArrow frequency mismatch');
 await ipc('close_file',{id});
 }
 return metrics;
 },{file,reps,output:path.join(run,'bench.xlsx'),oracle});
 await writeFile(phase,name+' UI');
 if(!process.env.BENCH_SKIP_UI) report.datasets[name].ui=await page.evaluate(async({file,reps})=>{
 const wait=async(fn)=>{const deadline=performance.now()+30000;while(!fn()){if(performance.now()>deadline)throw Error('UI condition timeout: '+document.body.innerText.slice(0,2500));await new Promise(r=>setTimeout(r,2));}};
 const frames=()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));
 const visible=sel=>[...document.querySelectorAll(sel)].filter(e=>e.getClientRects().length);
 const button=text=>visible('button').find(e=>e.textContent.trim()===text);
 const metrics={open_display:[],cache_display:[],head_display:[],uniform_display:[]};

 for(let i=0;i<reps;i++){
  let t=performance.now();const event='tauri://drag-drop';const ls=window.__internal_unstable_listeners_object_id__[event];window.__internal_unstable_listeners_function_id__({event,payload:{paths:[file],position:{x:10,y:10}}},Object.getOwnPropertyNames(ls).map(Number));
  await wait(()=>visible('.column-panel tbody tr').length===50&&button('关闭文件')&&!button('关闭文件').disabled);await frames();metrics.open_display.push(performance.now()-t);
  const cell=visible('.column-panel .cell').find(e=>e.textContent.trim()==='category_0');t=performance.now();cell.click();await wait(()=>visible('.enum-summary').some(e=>e.textContent.includes('缓存命中'))&&visible('.enum-table tbody tr').length===50);await frames();metrics.cache_display.push(performance.now()-t);
  button('样例与导出').click();await wait(()=>button('加载预览')&&!button('加载预览').disabled);
  for(const mode of ['head','uniform']){
   const select=visible('select').find(e=>[...e.options].some(o=>o.value==='uniform'));select.value=mode;select.dispatchEvent(new Event('change',{bubbles:true}));await frames();await wait(()=>visible('.preview-table').length===0);t=performance.now();button('加载预览').click();await wait(()=>!button('加载预览').disabled&&visible('.preview-table tbody tr').length>0);await frames();metrics[mode+'_display'].push(performance.now()-t);
  }
  button('关闭文件').click();await wait(()=>visible('.tabs .tab').length===0);
 }
 return metrics;
 },{file,reps});
 await writeFile(path.join(out,label+'.json'),JSON.stringify(report,null,2));console.log(label,name,'done');
 }
 const memory=()=>JSON.parse(execFileSync(path.join(root,'.venv/Scripts/python.exe'),['-c',"import sys,json;sys.path.insert(0,'scripts');from measure_memory import sample;print(json.dumps(sample(int(sys.argv[1]))))",String(child.pid)],{cwd:root,encoding:'utf8',windowsHide:true}));
 await writeFile(phase,'ten-tab lifecycle');
 report.lifecycle={empty_after_bench:memory(),rounds:[]};
 const copies=[];for(let i=0;i<10;i++){const f=path.join(run,`memory-${i}.parquet`);await copyFile(path.join(root,'tests/fixtures/m1/types.parquet'),f);copies.push(f);}
 for(let round=0;round<10;round++){
  await page.evaluate(async files=>{const event='tauri://drag-drop',ls=window.__internal_unstable_listeners_object_id__[event];window.__internal_unstable_listeners_function_id__({event,payload:{paths:files,position:{x:10,y:10}}},Object.getOwnPropertyNames(ls).map(Number));},copies);
  await page.waitForFunction(()=>document.querySelectorAll('.tabs .tab').length===10&&![...document.querySelectorAll('button')].find(b=>b.textContent.trim()==='关闭文件').disabled);
  const opened=memory();
  for(let i=10;i>0;i--){await page.getByRole('button',{name:'关闭文件',exact:true}).click();await page.waitForFunction(n=>document.querySelectorAll('.tabs .tab').length===n,i-1);}
  await new Promise(r=>setTimeout(r,1000));report.lifecycle.rounds.push({opened,closed:memory()});
 }
 await new Promise(r=>setTimeout(r,30000));report.lifecycle.after_30s_idle=memory();if(report.errors.length)throw Error('Page errors: '+report.errors.join('; '));
 await writeFile(path.join(out,label+'.json'),JSON.stringify(report,null,2));
}finally{await writeFile(stop,'stop');await new Promise(r=>monitor.exitCode!==null?r():monitor.once('exit',r));await browser?.close();execFileSync(path.join(root,'.venv/Scripts/python.exe'),[path.join(root,'scripts/app_window.py'),String(child.pid),'close'],{windowsHide:true});if(child.exitCode===null)await new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(Error('App did not close')),10000);child.once('exit',()=>{clearTimeout(timer);resolve();});});}

