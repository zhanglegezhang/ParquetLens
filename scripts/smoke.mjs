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
  await expect(page.getByRole('heading', { name: '中文 样例.parquet' })).toBeVisible();
  await expect(page.locator('dd.numeric')).toContainText((await stat(fixture)).size.toLocaleString('zh-CN'));
  await expect(page.locator('dd.path')).toHaveText(fixture);
  passed.push('real exe: command-line Unicode/space path and exact byte count');
  await page.screenshot({ path: path.join(out, 'm0-file-light.png') });
  await page.getByRole('button', { name: '关闭文件', exact: true }).click();
  await expect(page.getByRole('heading', { name: '从一个文件，了解你的数据' })).toBeVisible();
  passed.push('close file returns to empty view');
  await page.screenshot({ path: path.join(out, 'm0-empty-light.png') });

  async function event(name, paths = []) {
    await page.evaluate(({ name, paths }) => {
      const listeners = window.__internal_unstable_listeners_object_id__?.[name];
      if (!listeners || !Object.getOwnPropertyNames(listeners).length) throw new Error(`no registered listener: ${name}`);
      window.__internal_unstable_listeners_function_id__(
        { event: name, payload: { paths, position: { x: 100, y: 100 } } }, Object.getOwnPropertyNames(listeners).map(Number));
    }, { name, paths });
  }
  await event('tauri://drag-enter', [fixture]);
  await expect(page.locator('.drop-overlay')).toBeVisible();
  await event('tauri://drag-leave');
  await expect(page.locator('.drop-overlay')).toHaveCount(0);
  await event('tauri://drag-drop', [fixture]);
  await expect(page.getByRole('heading', { name: '中文 样例.parquet' })).toBeVisible();
  passed.push('injected Tauri drag enter/leave/drop through registered listeners and real Rust IPC');
  await event('tauri://drag-drop', [path.join(root, 'README.md')]);
  await expect(page.getByRole('alert')).toContainText('请选择 .parquet 文件');
  await expect(page.getByRole('heading', { name: '中文 样例.parquet' })).toBeVisible();
  passed.push('invalid extension error preserves existing file');
  await event('tauri://drag-drop', [path.join(run, 'missing.parquet')]);
  await expect(page.getByRole('alert')).toContainText('文件不存在或已被移动');
  await page.getByText('技术细节', { exact: true }).click();
  await expect(page.locator('.error pre')).toBeVisible();
  await page.screenshot({ path: path.join(out, 'm0-error.png') });
  passed.push('missing file error and expandable technical details');
  await event('tauri://drag-drop', [fixture, fixture]);
  await expect(page.locator('.tabs .tab')).toHaveCount(1);
  await event('tauri://drag-drop', [fixture]);
  await expect(page.getByRole('alert')).toHaveCount(0);
  passed.push('duplicate multi-file input deduplicates; subsequent valid input recovers');

  await page.getByRole('button',{name:'关闭文件',exact:true}).click();
  const types = path.join(root,'tests/fixtures/m1/types.parquet');
  await event('tauri://drag-drop',[types]);
  await expect(page.getByRole('heading',{name:'types.parquet',exact:true})).toBeVisible();
  await expect(page.locator('.column-panel tbody tr')).toHaveCount(37);
  await page.getByPlaceholder('搜索列名…').fill('id64');
  await expect(page.locator('.column-panel tbody tr')).toHaveCount(1);
  await page.locator('.column-panel tbody').getByRole('button',{name:'id64',exact:true}).click();
  await expect(page.locator('aside')).toContainText('9007199254740993');
  await page.getByRole('button',{name:'复制单元格 最大值',exact:true}).click();
  await expect(page.locator('.copy-status')).toContainText('已复制');
  expect(await page.evaluate(()=>navigator.clipboard.readText())).toBe('9007199254740993');
  await page.getByRole('button',{name:'复制整表 TSV',exact:true}).click();
  await expect.poll(()=>page.evaluate(()=>navigator.clipboard.readText())).toContain('id64');
  const copied = await page.evaluate(()=>navigator.clipboard.readText());
  expect(copied.split('\r\n')).toHaveLength(2);
  expect(copied).toContain('9007199254740993');
  passed.push('M1 real clipboard: exact large integer cell, filtered whole-table TSV; search and details');
  await page.screenshot({path:path.join(out,'m1-details-light.png'),fullPage:true});
  await page.getByPlaceholder('搜索列名…').fill('');
  await page.getByRole('columnheader',{name:/序号/}).getByRole('button').click();
  await expect(page.locator('.column-panel tbody tr').first().locator('td').nth(1)).toHaveText('null_type');
  passed.push('M1 column header descending sort');
  await page.getByRole('button',{name:'关闭文件',exact:true}).click();
  await event('tauri://drag-drop',[path.join(root,'tests/fixtures/m1/wide.parquet')]);
  await expect(page.getByRole('heading',{name:'wide.parquet',exact:true})).toBeVisible();
  await expect(page.locator('.column-panel tbody tr')).toHaveCount(50);
  await page.getByPlaceholder('搜索列名…').fill('col_0600');
  await expect(page.locator('.column-panel tbody tr')).toHaveCount(1);
  await expect(page.locator('.column-panel tbody')).toContainText('600');
  await page.getByPlaceholder('搜索列名…').fill('');
  await page.getByRole('button',{name:'下一页',exact:true}).click();
  await expect(page.locator('.column-panel tbody tr').first().locator('td').nth(1)).toHaveText('col_0050');
  const ipc = await page.evaluate(async(p)=>{
    const f=await window.__TAURI_INTERNALS__.invoke('open_file',{path:p});
    const remainder=await window.__TAURI_INTERNALS__.invoke('get_columns',{id:f.id,offset:500,limit:500});
    let rejected=false;try {await window.__TAURI_INTERNALS__.invoke('get_columns',{id:f.id,offset:0,limit:501});}catch{rejected=true;}
    // M4 deduplicates the open session; the UI owns this id.
    return {first:f.columns.length,remainder:remainder.length,rejected};
  },path.join(root,'tests/fixtures/m1/wide.parquet'));
  expect(ipc).toEqual({first:500,remainder:101,rejected:true});
  passed.push('M1 601-column file: 500+101 IPC paging, 50-row UI paging, search beyond first IPC page');
  for (const name of ['encrypted_False.parquet','encrypted_True.parquet']) {
    await event('tauri://drag-drop',[path.join(root,'tests/fixtures/m1',name)]);
    await expect(page.getByRole('alert')).toContainText('v1 暂不支持加密文件');
  }
  await event('tauri://drag-drop',[path.join(root,'tests/fixtures/m1/corrupt.parquet')]);
  await expect(page.getByRole('alert')).toContainText('文件无法解析');
  await page.getByRole('button',{name:'关闭文件',exact:true}).click();
  await event('tauri://drag-drop',[path.join(root,'tests/fixtures/m1/no_stats.parquet')]);
  await expect(page.getByRole('heading',{name:'no_stats.parquet',exact:true})).toBeVisible();
  await expect(page.locator('.column-panel tbody tr').first()).toContainText('统计不完整');
  await page.getByRole('button',{name:'关闭文件',exact:true}).click();
  await event('tauri://drag-drop',[types]);
  await expect(page.getByRole('heading',{name:'types.parquet',exact:true})).toBeVisible();
  passed.push('M1 encrypted/corrupt file UI and missing-statistics display');

  const benchmark=path.join(root,'bench/data/benchmark.parquet');
  const uiTimes=[];
  for (let i=0;i<5;i++) {
    await page.getByRole('button',{name:'关闭文件',exact:true}).click();
    await expect(page.locator('.column-panel tbody')).toHaveCount(0);
    const ms=await page.evaluate(async(p)=>{
      const start=performance.now();
      const complete=new Promise(resolve=>{
        const observer=new MutationObserver(()=>{
          if (document.querySelectorAll('tbody tr').length===50 && document.querySelector('h1')?.textContent==='benchmark.parquet') {
            observer.disconnect();requestAnimationFrame(()=>requestAnimationFrame(()=>resolve(performance.now()-start)));
          }
        });observer.observe(document.body,{subtree:true,childList:true});
        setTimeout(()=>{observer.disconnect();resolve(-1);},5000);
      });
      const name='tauri://drag-drop';
      const ids=Object.getOwnPropertyNames(window.__internal_unstable_listeners_object_id__[name]).map(Number);
      window.__internal_unstable_listeners_function_id__({event:name,payload:{paths:[p],position:{x:100,y:100}}},ids);
      return complete;
    },benchmark);
    expect(ms).toBeGreaterThan(0);uiTimes.push(ms);
  }
  await writeFile(path.join(out,'m1-gui-bench.json'),JSON.stringify({file_bytes:(await stat(benchmark)).size,scope:'release exe; injected native event to 50 rendered rows + two animation frames; includes Rust I/O/aggregation, IPC and DOM; OS cache not cleared',samples_ms:uiTimes,max_ms:Math.max(...uiTimes)},null,2));
  passed.push(`M1 803MB GUI timings (ms): ${uiTimes.map(n=>n.toFixed(1)).join(', ')}`);
  await page.screenshot({path:path.join(out,'m1-benchmark.png'),fullPage:true});

  const longDir = path.join(run, ...Array.from({ length: 5 }, (_, i) => `${i}-${'long-segment-'.repeat(4)}`));
  await mkdir(longDir, { recursive: true });
  const longFile = path.join(longDir, '超长 路径.parquet');
  await copyFile(types,longFile);
  if (longFile.length <= 260) throw new Error('long path fixture too short');
  await event('tauri://drag-drop', [longFile]);
  await expect(page.getByRole('heading', { name: '超长 路径.parquet' })).toBeVisible();
  await expect(page.locator('dd.path')).toHaveText(longFile);
  passed.push(`real filesystem and Rust IPC: ${longFile.length}-character path`);

  await page.emulateMedia({ colorScheme: 'dark' });
  await page.screenshot({ path: path.join(out, 'm1-file-dark.png'),fullPage:true });
  await page.setViewportSize({ width: 680, height: 480 });
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
  expect(overflow).toBe(false);
  await page.screenshot({ path: path.join(out, 'm1-small-dark.png') });
  passed.push('dark media mode and 680px viewport: no horizontal overflow');
  expect(checksum(await readFile(fixture))).toBe(before);
  expect(errors).toEqual([]);
  passed.push('fixture SHA256 unchanged; no observed JS page errors');
  await writeFile(path.join(out, 'smoke.json'), JSON.stringify({ date: new Date().toISOString(), passed, errors, scope: 'Real release exe and Rust IPC. Tauri drag events injected; no physical OS drag or native dialog selection certification.' }, null, 2));
  console.log(JSON.stringify({ passed, errors }, null, 2));
} finally {
  await browser?.close();
  child.kill();
}

