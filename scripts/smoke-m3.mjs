// Runtime smoke against the real release exe and Rust IPC. OS drag events are
// injected at Tauri's JS event boundary; this does not certify physical dragging.
import { chromium, expect } from '@playwright/test';
import { spawn } from 'node:child_process';
import { mkdir, stat, writeFile, mkdtemp, readFile, copyFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import net from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const fixture = path.join(root, 'tests/fixtures/m3/enums.parquet');
const out = path.join(root, 'test-results');
await mkdir(out, { recursive: true });
const run = await mkdtemp(path.join(out, 'runtime-'));
const checksum = (data) => createHash('sha256').update(data).digest('hex');
const before = checksum(await readFile(fixture));
const server = net.createServer();
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const port = server.address().port;
await new Promise(resolve => server.close(resolve));
const child = spawn(path.join(root, 'src-tauri/target/release/ParquetLens.exe'), [fixture], {
  cwd: root,
  env: { ...process.env, LOCALAPPDATA: path.join(run,'localapp'), WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`, WEBVIEW2_USER_DATA_FOLDER: path.join(run, 'webview') },
  stdio: 'ignore',
});
const memoryStop=path.join(run,'memory.stop');
const memory=spawn(path.join(root,'.venv/Scripts/python.exe'),[path.join(root,'scripts/measure_memory.py'),String(child.pid),path.join(out,'m3-runtime-memory.json'),memoryStop],{cwd:root,stdio:'ignore',windowsHide:true});
let browser;
const passed = [];
const errors = [];
try {
  const endpoint = `http://127.0.0.1:${port}`;
  let connected = false;
  for (let attempt = 0; attempt < 100; attempt++) {
    try { const response = await fetch(`${endpoint}/json/version`); if (response.ok) { connected = true; break; } } catch {}
    if (child.exitCode !== null) throw new Error(`app exited ${child.exitCode}`);
    await new Promise(resolve => setTimeout(resolve, 200));
  }
  if (!connected) throw new Error('WebView2 test endpoint did not start');
  browser = await chromium.connectOverCDP(endpoint);
  const context = browser.contexts()[0];
  await context.grantPermissions(['clipboard-read','clipboard-write'],{origin:'http://tauri.localhost'});
  let page;
  for (let attempt = 0; attempt < 50; attempt++) {
    page = context.pages().find(p => p.url().includes('tauri.localhost'));
    if (page) break;
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  if (!page) throw new Error('app page missing');
  page.on('pageerror', error => errors.push(error.message));

  const timings={};
  const ipc=(command,args)=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
  await expect(page.getByRole('heading',{name:'enums.parquet',exact:true})).toBeVisible();
  await page.setViewportSize({width:1200,height:900});
  await page.getByLabel('cap',{exact:true}).fill('50');await page.getByLabel('cap',{exact:true}).press('Tab');
  await page.locator('.column-panel .cell').filter({hasText:/^text$/}).click();
  await expect(page.locator('.enum-summary')).toHaveAttribute('data-status','complete');
  await expect(page.locator('.enum-table')).toContainText('33.33%');
  await expect(page.locator('.enum-table')).toContainText('<空字符串>');
  await expect(page.locator('.enum-table')).toContainText('<NULL> (空值)');
  await page.getByRole('button',{name:'复制枚举 TSV',exact:true}).click();
  await expect.poll(()=>page.evaluate(async()=>{const value=await navigator.clipboard.readText();return value.includes('北京')&&value.includes('33.33%');})).toBe(true);
  await page.getByLabel('搜索枚举值…',{exact:true}).fill('北京');await expect(page.locator('.enum-table tbody tr')).toHaveCount(1);
  await page.getByLabel('搜索枚举值…',{exact:true}).fill('');await expect(page.locator('.enum-table tbody tr')).toHaveCount(5);
  await page.screenshot({path:path.join(out,'m3-enums-light.png'),fullPage:true});
  passed.push('UI selection auto analysis; NULL/empty/literal NULL; percentages; search; TSV copy');
  await page.getByRole('button',{name:'分析此列',exact:true}).click();await expect(page.locator('.enum-summary')).toContainText('缓存命中');
  await page.getByRole('button',{name:'重新计算',exact:true}).click();await expect(page.locator('.enum-summary')).toContainText('数据扫描');
  await page.getByLabel('分析模式',{exact:true}).selectOption('quick');await page.getByLabel('K',{exact:true}).fill('1');await page.getByLabel('K',{exact:true}).press('Tab');
  await expect(page.locator('.enum-summary')).toContainText('样本结果');await expect(page.locator('.enum-summary')).toContainText('60/120');
  const target=path.join(out,'m3-ui-enums.xlsx');await page.evaluate(file=>{const previous=window.fetch.bind(window);window.fetch=(url,options)=>decodeURIComponent(String(url)).includes('plugin:dialog|save')?Promise.resolve(new Response(JSON.stringify(file),{headers:{'Tauri-Response':'ok','Content-Type':'application/json'}})):previous(url,options);},target);
  await page.getByRole('button',{name:'导出此列枚举 xlsx',exact:true}).click();await expect(page.locator('.enum-panel')).toContainText('导出完成');expect((await stat(target)).size).toBeGreaterThan(1000);
  await page.getByRole('button',{name:'清空枚举缓存',exact:true}).click();await expect(page.locator('.enum-panel')).toContainText('枚举缓存已清空');
  passed.push('cache hit/recompute/clear UI; quick scope; real xlsx export via injected save destination');
  await page.getByLabel('分析模式',{exact:true}).selectOption('exact');await expect(page.getByRole('button',{name:'分析此列',exact:true})).toBeEnabled();
  await page.locator('.column-panel .cell').filter({hasText:/^union_cap$/}).click();await expect(page.locator('.enum-summary')).toHaveAttribute('data-status','high');await expect(page.locator('.enum-summary')).toContainText('> 50');
  await page.locator('.column-panel .cell').filter({hasText:/^float$/}).click();await expect(page.getByRole('button',{name:'强制分析此列',exact:true})).toBeEnabled();await page.getByRole('button',{name:'强制分析此列',exact:true}).click();await expect(page.locator('.enum-table')).toContainText('NaN');
  await page.getByRole('button',{name:'分析全部候选列',exact:true}).click();await expect(page.getByRole('button',{name:'分析全部候选列',exact:true})).toBeEnabled();await expect(page.locator('.enum-error')).toHaveCount(0);
  await page.emulateMedia({colorScheme:'dark'});await page.screenshot({path:path.join(out,'m3-enums-dark.png'),fullPage:true});await page.setViewportSize({width:680,height:480});expect(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth)).toBe(false);await page.screenshot({path:path.join(out,'m3-small-dark.png'),fullPage:true});
  passed.push('global cap stops with no partial table; float manual NaN; dark and 680px viewport');
  const bench=await ipc('open_file',{path:path.join(root,'bench/data/benchmark.parquet')});
  const cat=await ipc('enum_catalog',{id:bench.id});expect(cat).toHaveLength(50);
  async function timed(columns,force,mode='exact',k=3){const start=performance.now();const result=await ipc('analyze_enums',{id:bench.id,options:{columns,cap:200,mode,k,force},request:crypto.randomUUID()});return {elapsed_ms:performance.now()-start,result};}
  const single=await timed([5],true);expect(single.result.results[0].status).toBe('complete');expect(single.result.results[0].scanned).toBe('20000000');timings.single_ms=single.elapsed_ms;
  const all=await timed(cat.map(c=>c.root),true);expect(all.result.results.filter(r=>r.status==='high')).toHaveLength(5);expect(all.result.results.filter(r=>r.status==='complete')).toHaveLength(45);expect(all.result.results.filter(r=>r.status==='high').every(r=>Number(r.scanned)<20000000)).toBe(true);timings.all_ms=all.elapsed_ms;
  const cached=await timed([5],false);expect(cached.result.results[0].source).toBe('cache');timings.cache_ipc_ms=cached.elapsed_ms;
  const quick=await timed([5],true,'quick',3);expect(quick.result.results[0].exact).toBe(false);expect(quick.result.results[0].scope_groups).toBe(3);timings.quick_ms=quick.elapsed_ms;
  const cancellation=await page.evaluate(async id=>{const invoke=window.__TAURI_INTERNALS__.invoke;let settled=false;const task=invoke('analyze_enums',{id,options:{columns:Array.from({length:45},(_,i)=>i+5),cap:5000,mode:'exact',k:3,force:true},request:'m3-cancel'}).then(r=>{settled=true;return r;});await new Promise(r=>setTimeout(r,100));const early=settled;const start=performance.now();await invoke('cancel_task',{id,request:'m3-cancel'});const result=await task;const elapsed_ms=performance.now()-start;const after=await invoke('analyze_enums',{id,options:{columns:[0],cap:50,mode:'quick',k:1,force:true},request:'m3-after-cancel'});return {early,elapsed_ms,cancelled:result.cancelled,statuses:result.results.map(r=>r.status),next_status:after.results[0].status};},bench.id);
  expect(cancellation.early).toBe(false);expect(cancellation.cancelled).toBe(true);expect(cancellation.statuses).toContain('cancelled');expect(cancellation.next_status).toBe('high');timings.cancel_ms=cancellation.elapsed_ms;
  await ipc('close_file',{id:bench.id});
  await page.getByRole('button',{name:'关闭文件',exact:true}).click();
  await page.setViewportSize({width:1200,height:900});
  await page.evaluate(file=>{const name='tauri://drag-drop';const listeners=window.__internal_unstable_listeners_object_id__[name];window.__internal_unstable_listeners_function_id__({event:name,payload:{paths:[file],position:{x:10,y:10}}},Object.getOwnPropertyNames(listeners).map(Number));},path.join(root,'bench/data/benchmark.parquet'));
  await expect(page.getByRole('heading',{name:'benchmark.parquet',exact:true})).toBeVisible();
  const displayStart=performance.now();await page.locator('.column-panel .cell').filter({hasText:/^category_0$/}).click();await expect(page.locator('.enum-summary')).toContainText('缓存命中');await expect(page.locator('.enum-table tbody tr')).toHaveCount(50);await page.evaluate(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))));timings.cache_display_ms=performance.now()-displayStart;

  passed.push('803MB benchmark: exact single/all, global cap 5 columns, quick 3 RG, cache IPC, real worker cancellation and immediate next task');
  expect(checksum(await readFile(fixture))).toBe(before);expect(errors).toEqual([]);
  await writeFile(path.join(out,'m3-smoke.json'),JSON.stringify({date:new Date().toISOString(),passed,errors,timings,cancellation,scope:'Real release exe/IPC, isolated WebView2. Save dialog destination injected. Timings use warm OS cache; memory includes test WebView2.'},null,2));console.log(JSON.stringify({passed,timings,cancellation},null,2));
} finally {
  await writeFile(memoryStop,'stop');await new Promise(resolve=>{if(memory.exitCode!==null)resolve();else memory.once('exit',resolve);});await browser?.close();child.kill();
}
