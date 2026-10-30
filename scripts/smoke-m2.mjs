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
const fixture = path.join(root, 'tests/fixtures/中文 样例.parquet');
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
const memory=spawn(path.join(root,'.venv/Scripts/python.exe'),[path.join(root,'scripts/measure_memory.py'),String(child.pid),path.join(out,'m2-runtime-memory.json'),memoryStop],{cwd:root,stdio:'ignore',windowsHide:true});
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

  async function openFile(file) {
    if(await page.getByRole('button',{name:'关闭文件',exact:true}).isVisible())await page.getByRole('button',{name:'关闭文件',exact:true}).click();
    await page.evaluate(file=>{
      const name='tauri://drag-drop';const listeners=window.__internal_unstable_listeners_object_id__[name];
      window.__internal_unstable_listeners_function_id__({event:name,payload:{paths:[file],position:{x:10,y:10}}},Object.getOwnPropertyNames(listeners).map(Number));
    },file);
    await expect(page.getByRole('heading',{name:path.basename(file),exact:true})).toBeVisible();
    await page.getByRole('button',{name:'样例与导出',exact:true}).click();
    await expect(page.getByRole('button',{name:'加载预览',exact:true})).toBeEnabled();
  }
  async function loadPreview(){
    await page.getByRole('button',{name:'加载预览',exact:true}).click();
    await expect(page.locator('.preview-table')).toBeVisible();
    await expect(page.getByRole('button',{name:'加载预览',exact:true})).toBeEnabled();
  }
  async function setSavePath(file){
    await page.evaluate(file=>{
      window.__m2Fetch??=window.fetch.bind(window);
      window.fetch=(url,options)=>decodeURIComponent(String(url)).includes('plugin:dialog|save')
        ?Promise.resolve(new Response(JSON.stringify(file),{headers:{'Tauri-Response':'ok','Content-Type':'application/json'}}))
        :window.__m2Fetch(url,options);
    },file);
  }
  await expect(page.getByRole('heading',{name:'中文 样例.parquet',exact:true})).toBeVisible();
  await page.setViewportSize({width:1200,height:900});
  await openFile(path.join(root,'tests/fixtures/m1/types.parquet'));
  await loadPreview();
  await expect(page.locator('.page-bar')).toContainText('6');
  await expect(page.locator('.preview-table')).toContainText('9007199254740993');
  await page.screenshot({path:path.join(out,'m2-preview-light.png'),fullPage:true});
  const typesOut=path.join(out,'m2-types.xlsx');await setSavePath(typesOut);
  await page.getByRole('button',{name:'导出样例 xlsx',exact:true}).click();
  await expect(page.locator('.export-report')).toContainText('导出完成');
  expect((await stat(typesOut)).size).toBeGreaterThan(1000);
  passed.push('all-type preview and UI xlsx export with real Rust IPC; save-dialog result injected');

  await openFile(path.join(root,'tests/fixtures/m2/uneven.parquet'));await loadPreview();
  await expect(page.locator('.page-bar')).toContainText('1–500');
  expect(await page.locator('.preview-table tbody tr').count()).toBeLessThan(30);
  await page.locator('.preview-scroll').evaluate(el=>el.scrollTop=el.scrollHeight);
  await expect(page.locator('.preview-table')).toContainText('499');
  await page.getByRole('button',{name:'下一页',exact:true}).click();
  await expect(page.locator('.page-bar')).toContainText('501–1000');
  await expect(page.locator('.preview-table')).toContainText('500');
  await page.locator('summary').filter({hasText:'选择顶层列'}).click();
  await page.getByRole('button',{name:'清空',exact:true}).click();
  await expect(page.getByRole('button',{name:'加载预览',exact:true})).toBeDisabled();
  await page.locator('.field-list label').filter({hasText:/^id$/}).getByRole('checkbox').check();
  await page.getByLabel('样例行数',{exact:true}).fill('10');
  await page.locator('.samples select').selectOption('uniform');await loadPreview();
  const ids=await page.locator('.preview-table tbody tr td:nth-child(2) button').allTextContents();
  expect(ids).toEqual(['0','1','2','3','4','5','6','603','604','605']);
  await page.getByLabel('样例行数',{exact:true}).fill('1000001');
  await expect(page.getByRole('button',{name:'加载预览',exact:true})).toBeDisabled();
  passed.push('virtualized 500-row paging, selection, uniform top-up identities and N validation');

  await openFile(path.join(root,'tests/fixtures/m2/edge.parquet'));await loadPreview();
  await page.locator('.preview-table tbody tr td:nth-child(2) button').first().click();
  expect((await page.getByLabel('完整值',{exact:true}).inputValue()).length).toBe(60000);
  await page.getByRole('button',{name:'复制单元格',exact:true}).click();
  expect((await page.evaluate(()=>navigator.clipboard.readText())).length).toBe(60000);
  await page.getByRole('button',{name:'关闭提示',exact:true}).click();
  const edgeOut=path.join(out,'m2-edge.xlsx');await setSavePath(edgeOut);
  await page.getByRole('button',{name:'导出样例 xlsx',exact:true}).click();
  await expect(page.locator('.export-report')).toContainText('截断单元格: 1');
  await expect(page.locator('.export-report')).toContainText('清理控制字符单元格: 1');
  passed.push('full long value and clipboard preserved; xlsx truncation/control cleaning reported');

  const benchmark=path.join(root,'bench/data/benchmark.parquet');await openFile(benchmark);
  const timings={head:[],uniform:[],xlsx:[]};
  for(const mode of ['head','uniform']) {
    await page.locator('.samples select').selectOption(mode);
    for(let i=0;i<5;i++){
      const ms=await page.evaluate(async()=>{
        const button=[...document.querySelectorAll('button')].find(b=>b.textContent==='加载预览');
        const start=performance.now();button.click();
        await new Promise(resolve=>setTimeout(resolve,0));
        while(button.disabled||!document.querySelector('.preview-table'))await new Promise(r=>setTimeout(r,5));
        await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));
        return performance.now()-start;
      });timings[mode].push(ms);
    }
  }
  await page.locator('.samples select').selectOption('head');
  const benchOut=path.join(out,'m2-benchmark.xlsx');await setSavePath(benchOut);
  for(let i=0;i<5;i++) {
    const ms=await page.evaluate(async()=>{
      const b=[...document.querySelectorAll('button')].find(b=>b.textContent==='导出样例 xlsx');const start=performance.now();b.click();
      await new Promise(r=>setTimeout(r,0));while(b.disabled)await new Promise(r=>setTimeout(r,5));
      if(!document.querySelector('.export-report'))throw new Error('export failed');return performance.now()-start;
    });timings.xlsx.push(ms);
  }
  const fullPreview=await page.evaluate(async benchmark=>{
    const invoke=window.__TAURI_INTERNALS__.invoke;const meta=await invoke('open_file',{path:benchmark});
    const fields=await invoke('sample_fields',{id:meta.id});const result={};
    for(const mode of ['head','uniform']){
      result[mode+'_1000_ipc']=[];
      for(let i=0;i<5;i++){
        const start=performance.now();
        for(const offset of [0,500]){
          const p=await invoke('get_preview',{id:meta.id,options:{mode,n:1000,columns:fields.map(f=>f.index)},offset,limit:500,request:crypto.randomUUID()});
          if(p.rows.length!==500)throw new Error('expected 500 rows per page');
        }
        result[mode+'_1000_ipc'].push(performance.now()-start);
      }
    }
    return result;
  },benchmark);
  Object.assign(timings,fullPreview);
  await writeFile(path.join(out,'m2-gui-bench.json'),JSON.stringify({file_bytes:(await stat(benchmark)).size,scope:'release exe; head/uniform: first 500 rows incl DOM/two frames; *_1000_ipc: all 1000 rows via 2 IPC pages; xlsx 1000x50 to completion; save dialog result injected; OS cache not cleared',samples_ms:timings},null,2));
  passed.push('803 MB real GUI preview/head/uniform and 1000x50 export benchmark');

  const cancelOut=path.join(run,'cancelled.xlsx');
  const cancellation=await page.evaluate(async({benchmark,cancelOut})=>{
    const invoke=window.__TAURI_INTERNALS__.invoke;const meta=await invoke('open_file',{path:benchmark});const options={mode:'head',n:1000000,columns:[0,1,2,3,4]};
    const request='cancel-test';const pending=invoke('export_xlsx',{id:meta.id,options,outPath:cancelOut,request}).then(()=>({ok:true}),e=>({code:e.code}));
    await new Promise(r=>setTimeout(r,100));
    const busy=await invoke('get_preview',{id:meta.id,options,offset:0,limit:500,request:'busy-test'}).then(()=>null,e=>e.code);
    const start=performance.now();await invoke('cancel_task',{id:meta.id,request});const result=await pending;const elapsed=performance.now()-start;
    return {busy,result,elapsed};
  },{benchmark,cancelOut});
  expect(cancellation.busy).toBe('task_busy');expect(cancellation.result.code).toBe('cancelled');
  expect(await stat(cancelOut).then(()=>true,()=>false)).toBe(false);
  passed.push(`IPC concurrent task rejected; million-row export cancellation ${cancellation.elapsed.toFixed(1)} ms; no target left`);

  // Destination failure uses real filesystem errors then retries through UI.
  await setSavePath(path.join(run,'missing-directory','fail.xlsx'));
  await page.getByRole('button',{name:'导出样例 xlsx',exact:true}).click();
  await expect(page.locator('.sample-error')).toContainText('导出失败');
  await setSavePath(benchOut);await page.getByRole('button',{name:'导出样例 xlsx',exact:true}).click();
  await expect(page.locator('.export-report')).toContainText('导出完成');
  await page.emulateMedia({colorScheme:'dark'});await loadPreview();
  await page.screenshot({path:path.join(out,'m2-preview-dark.png'),fullPage:true});
  await page.setViewportSize({width:680,height:480});
  expect(await page.evaluate(()=>document.documentElement.scrollWidth>window.innerWidth)).toBe(false);
  await page.screenshot({path:path.join(out,'m2-small-dark.png'),fullPage:true});
  passed.push('destination failure recovery; dark/small viewport without page overflow');
  expect(checksum(await readFile(fixture))).toBe(before);expect(errors).toEqual([]);
  await writeFile(path.join(out,'m2-smoke.json'),JSON.stringify({date:new Date().toISOString(),passed,errors,cancellation,scope:'Real release exe and IPC; native drag/save-dialog results injected, no physical dialog selection certification'},null,2));
  console.log(JSON.stringify({passed,errors,timings,cancellation},null,2));
} finally {
  await writeFile(memoryStop,'stop');
  await new Promise(resolve=>{if(memory.exitCode!==null)resolve();else memory.once('exit',resolve);});
  await browser?.close();child.kill();
}
