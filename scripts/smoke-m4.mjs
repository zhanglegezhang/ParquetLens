// Runtime smoke against the real release exe and Rust IPC. OS drag events are
// injected at Tauri's JS event boundary; this does not certify physical dragging.
import { chromium, expect } from '@playwright/test';
import { spawn, execFileSync } from 'node:child_process';
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
const memory=spawn(path.join(root,'.venv/Scripts/python.exe'),[path.join(root,'scripts/measure_memory.py'),String(child.pid),path.join(out,'m4-runtime-memory.json'),memoryStop],{cwd:root,stdio:'ignore',windowsHide:true});
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

  const ipc=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
  const active=()=>page.locator('.file-panel:not([hidden])');
  async function drop(files){await page.evaluate(paths=>{const name='tauri://drag-drop';window.__internal_unstable_listeners_function_id__({event:name,payload:{paths,position:{x:10,y:10}}},Object.getOwnPropertyNames(window.__internal_unstable_listeners_object_id__[name]).map(Number));},files);}
  async function saveTo(file){await page.evaluate(file=>{window.__m4Fetch??=window.fetch.bind(window);window.fetch=(url,options)=>decodeURIComponent(String(url)).includes('plugin:dialog|save')?Promise.resolve(new Response(JSON.stringify(file),{headers:{'Tauri-Response':'ok','Content-Type':'application/json'}})):window.__m4Fetch(url,options);},file);}
  async function second(args){const p=spawn(path.join(root,'src-tauri/target/release/ParquetLens.exe'),args,{cwd:root,env:{...process.env,LOCALAPPDATA:path.join(run,'localapp')},stdio:'ignore'});await new Promise((resolve,reject)=>{p.once('exit',code=>code===0?resolve():reject(new Error('second instance exit '+code)));p.once('error',reject);});}
  const native=action=>JSON.parse(execFileSync(path.join(root,'.venv/Scripts/python.exe'),[path.join(root,'scripts/app_window.py'),String(child.pid),action],{encoding:'utf8',windowsHide:true}));
  await expect(page.getByRole('heading',{name:'enums.parquet',exact:true})).toBeVisible();
  await page.setViewportSize({width:1200,height:900});
  await second(['tests/fixtures/m1/types.parquet',fixture]);await expect(page.locator('.tabs .tab')).toHaveCount(2);await expect(page.getByRole('heading',{name:'enums.parquet',exact:true})).toBeVisible();
  expect(native('minimize')[0].minimized).toBe(true);await second([]);await expect.poll(()=>native('status')[0]?.minimized).toBe(false);
  passed.push('real second process exits, relative/multiple paths forwarded, repeated path deduplicates, no-argument launch restores minimized primary');
  await page.getByRole('button',{name:'设置',exact:true}).click();const settings=page.getByRole('dialog',{name:'设置'});
  await settings.getByLabel('默认枚举 cap',{exact:true}).fill('333');await settings.getByLabel('默认样例行数',{exact:true}).fill('25');await settings.getByLabel('时间显示',{exact:true}).selectOption('local');await settings.getByLabel('主题',{exact:true}).selectOption('dark');await settings.getByRole('button',{name:'保存设置',exact:true}).click();await expect(settings).toContainText('设置已保存');await settings.getByRole('button',{name:'关闭提示',exact:true}).click();expect(await page.evaluate(()=>document.documentElement.dataset.theme)).toBe('dark');
  const prefs=await ipc('get_settings');expect(prefs.preferences.cap).toBe(333);expect(prefs.preferences.sample_n).toBe(25);expect(JSON.parse(await readFile(path.join(run,'localapp/ParquetLens/settings.json'),'utf8')).theme).toBe('dark');
  await page.locator('.tabs').getByRole('button',{name:'types.parquet',exact:true}).click();const types=await ipc('open_file',{path:path.join(root,'tests/fixtures/m1/types.parquet')});const cols=await ipc('display_columns',{id:types.id,offset:0,limit:500});const utc=cols.find(c=>c.path==='ts_ns_UTC');expect(utc.min).toMatch(/[+-]\d\d:\d\d$/);expect(utc.min).toContain('.999999999');const naive=cols.find(c=>c.path==='ts_ns_naive');expect(naive.min).not.toMatch(/[+-]\d\d:\d\d$/);
  await active().getByRole('button',{name:'样例与导出',exact:true}).click();await expect(active().getByLabel('样例行数',{exact:true})).toHaveValue('25');await active().getByRole('button',{name:'加载预览',exact:true}).click();await expect(active().locator('.preview-table')).toContainText('.999999999');
  await drop([path.join(root,'tests/fixtures/m2/edge.parquet')]);await expect(page.getByRole('heading',{name:'edge.parquet',exact:true})).toBeVisible();await expect(active().getByLabel('cap',{exact:true})).toHaveValue('333');await active().getByRole('button',{name:'样例与导出',exact:true}).click();await expect(active().getByLabel('样例行数',{exact:true})).toHaveValue('25');
  const csvProtected=path.join(out,'m4-edge-protected.csv');await saveTo(csvProtected);await active().getByRole('button',{name:'导出样例 CSV',exact:true}).click();await expect(active().locator('.export-report')).toContainText('保护文本数: 4');const data=await readFile(csvProtected);expect(data.subarray(0,3).toString('hex')).toBe('efbbbf');expect(data.toString('utf8')).toContain("'=cmd");expect(data.toString('utf8')).toContain('中😀'.repeat(20000));
  await active().getByLabel('保护公式样式文本（添加单引号）',{exact:true}).uncheck();await saveTo(path.join(out,'m4-edge-raw.csv'));await active().getByRole('button',{name:'导出样例 CSV',exact:true}).click();await expect(active().locator('.export-report')).toContainText('保护文本数: 0');
  passed.push('settings persisted; theme/manual local time with nanoseconds; defaults in new panels; raw/protected CSV without long-text truncation');
  await page.locator('.tabs').getByRole('button',{name:'enums.parquet',exact:true}).click();await active().getByRole('button',{name:'数据字典',exact:true}).click();await active().getByLabel('数据字典 cap',{exact:true}).fill('50');await active().getByLabel('数据字典样例行数',{exact:true}).fill('7');await saveTo(path.join(out,'m4-dictionary.xlsx'));await active().getByRole('button',{name:'导出数据字典 xlsx',exact:true}).click();await expect(active().locator('.dictionary-report')).toContainText('导出完成');expect((await stat(path.join(out,'m4-dictionary.xlsx'))).size).toBeGreaterThan(5000);
  await active().getByRole('button',{name:'列与枚举',exact:true}).click();await active().getByLabel('分析模式',{exact:true}).selectOption('quick');await active().getByLabel('K',{exact:true}).fill('1');await active().getByLabel('K',{exact:true}).press('Tab');await active().locator('.column-panel .cell').filter({hasText:/^text$/}).click();await expect(active().locator('.enum-summary')).toContainText('样本结果');await expect(active().locator('.enum-table')).toContainText('a');await saveTo(path.join(out,'m4-enums.csv'));await active().getByRole('button',{name:'导出此列枚举 CSV',exact:true}).click();await expect(active().locator('.enum-panel')).toContainText('导出完成');
  await active().getByRole('button',{name:'数据字典',exact:true}).click();await active().getByLabel('仅导出已有枚举结果',{exact:true}).check();await active().getByLabel('包含样例',{exact:true}).uncheck();await saveTo(path.join(out,'m4-dictionary-existing.xlsx'));await active().getByRole('button',{name:'导出数据字典 xlsx',exact:true}).click();await expect(active().locator('.dictionary-report')).toContainText('导出完成');await page.screenshot({path:path.join(out,'m4-dictionary-dark.png'),fullPage:true});
  passed.push('dictionary UI default full analysis with sample, existing sample-only statuses, enum CSV, preserved state across view/tab changes');
  const benchmark=path.join(root,'bench/data/benchmark.parquet');await drop([benchmark]);await expect(page.getByRole('heading',{name:'benchmark.parquet',exact:true})).toBeVisible();const bench=await ipc('open_file',{path:benchmark});
  await page.evaluate(id=>{window.__m4Pending=window.__TAURI_INTERNALS__.invoke('analyze_enums',{id,options:{columns:Array.from({length:45},(_,i)=>i+5),cap:5000,mode:'exact',k:3,force:true},request:'tab-background'});},bench.id);
  await expect(page.locator('.global-task')).toBeVisible();await page.locator('.tabs').getByRole('button',{name:'enums.parquet',exact:true}).click();await expect(page.getByRole('heading',{name:'enums.parquet',exact:true})).toBeVisible();expect((await ipc('task_status')).owner).toBe(bench.id);
  const cancelStart=performance.now();await page.getByRole('button',{name:'关闭标签 benchmark.parquet',exact:true}).click();await expect(page.locator('.global-task')).toHaveCount(0);const cancelMs=performance.now()-cancelStart;const cancelled=await page.evaluate(()=>window.__m4Pending);expect(cancelled.cancelled).toBe(true);
  passed.push(`tab switch preserves background job; closing owner cancels and drains (${cancelMs.toFixed(1)} ms incl UI)`);
  const copies=[];for(let i=0;i<11;i++){const file=path.join(run,`tab-${i}.parquet`);await copyFile(fixture,file);copies.push(file);}await drop(copies);await expect(page.locator('.tabs .tab')).toHaveCount(10);await expect(page.locator('.error')).toContainText('最多打开 10 个文件');expect((await ipc('get_settings')).preferences.recent).toHaveLength(10);await page.getByRole('button',{name:'关闭文件',exact:true}).click();await expect(page.locator('.tabs .tab')).toHaveCount(9);await drop([copies[10]]);await expect(page.getByRole('heading',{name:'tab-10.parquet',exact:true})).toBeVisible();await expect(page.locator('.tabs .tab')).toHaveCount(10);
  passed.push('10-tab limit rejects additional files without eviction, close/reopen recovery, 10-item recent list');
  while(await page.locator('.tabs .tab').count()){const before=await page.locator('.tabs .tab').count();await page.getByRole('button',{name:'关闭文件',exact:true}).click();await expect(page.locator('.tabs .tab')).toHaveCount(before-1);}
  const benchMeta=await ipc('open_file',{path:benchmark});const start=performance.now();await ipc('export_dictionary',{id:benchMeta.id,options:{include_sample:true,sample:{mode:'head',n:1000,columns:Array.from({length:50},(_,i)=>i)},cap:200,existing_only:false,manual_columns:[]},outPath:path.join(out,'m4-benchmark-dictionary.xlsx'),request:'dictionary-bench'});const dictionaryMs=performance.now()-start;const csvStart=performance.now();await ipc('export_sample_csv',{id:benchMeta.id,options:{mode:'head',n:1000,columns:Array.from({length:50},(_,i)=>i)},outPath:path.join(out,'m4-benchmark.csv'),protect:true,request:'csv-bench'});const csvMs=performance.now()-csvStart;await ipc('close_file',{id:benchMeta.id});
  await page.getByRole('button',{name:'设置',exact:true}).click();await page.getByRole('dialog').getByRole('button',{name:'清空最近记录',exact:true}).click();await expect(page.getByRole('dialog')).toContainText('最近文件记录已清空');expect((await ipc('get_settings')).preferences.recent).toEqual([]);await page.getByRole('dialog').getByRole('button',{name:'清空枚举缓存',exact:true}).click();await expect.poll(async()=>(await ipc('enum_cache_info')).bytes).toBe(0);await page.getByRole('dialog').getByLabel('主题',{exact:true}).selectOption('light');await page.getByRole('dialog').getByRole('button',{name:'保存设置',exact:true}).click();await expect(page.getByRole('dialog')).toContainText('设置已保存');await page.screenshot({path:path.join(out,'m4-settings-light.png'),fullPage:true});await page.setViewportSize({width:680,height:480});expect(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth)).toBe(false);await page.screenshot({path:path.join(out,'m4-small-settings.png'),fullPage:true});
  const exitFile=path.join(out,'m4-exit-cancel.csv');await writeFile(exitFile,'old destination');const exitMeta=await ipc('open_file',{path:benchmark});await page.evaluate(({id,outPath})=>{window.__m4Exit=window.__TAURI_INTERNALS__.invoke('export_sample_csv',{id,options:{mode:'head',n:1000000,columns:Array.from({length:50},(_,i)=>i)},outPath,protect:true,request:'exit-cancel'}).catch(()=>null);},{id:exitMeta.id,outPath:exitFile});await expect.poll(async()=>!!(await ipc('task_status'))).toBe(true);const exitStart=performance.now();native('close');await Promise.race([new Promise(resolve=>child.once('exit',resolve)),new Promise((_,reject)=>setTimeout(()=>reject(new Error('exit did not drain task')),10000))]);expect(child.exitCode).toBe(0);expect(await readFile(exitFile,'utf8')).toBe('old destination');passed.push(`normal window close cancels active CSV export and preserves existing destination (${(performance.now()-exitStart).toFixed(1)} ms)`);
  expect(checksum(await readFile(fixture))).toBe(before);expect(errors).toEqual([]);await writeFile(path.join(out,'m4-smoke.json'),JSON.stringify({date:new Date().toISOString(),passed,errors,timings:{dictionary_ms:dictionaryMs,csv_1000x50_ms:csvMs,close_cancel_ui_ms:cancelMs},profile:path.join(run,'localapp'),scope:'Real release app/IPC and second exe; native drag/save result injected. Own isolated profile/cache. Warm OS cache.'},null,2));console.log(JSON.stringify({passed,timings:{dictionaryMs,csvMs,cancelMs}},null,2));
} finally {await writeFile(memoryStop,'stop');await new Promise(resolve=>{if(memory.exitCode!==null)resolve();else memory.once('exit',resolve);});await browser?.close();child.kill();}

