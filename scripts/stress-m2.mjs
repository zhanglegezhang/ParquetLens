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
  env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`, WEBVIEW2_USER_DATA_FOLDER: path.join(run, 'webview') },
  stdio: 'ignore',
});
const memoryStop=path.join(run,'memory.stop');
const memory=spawn(path.join(root,'.venv/Scripts/python.exe'),[path.join(root,'scripts/measure_memory.py'),String(child.pid),path.join(out,'m2-stress-memory.json'),memoryStop],{cwd:root,stdio:'ignore',windowsHide:true});
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


  await expect(page.getByRole('heading',{name:'中文 样例.parquet',exact:true})).toBeVisible();
  const result=await page.evaluate(async({source,target})=>{
    const invoke=window.__TAURI_INTERNALS__.invoke;const meta=await invoke('open_file',{path:source});const start=performance.now();
    const report=await invoke('export_xlsx',{id:meta.id,options:{mode:'head',n:1000000,columns:[0,1]},outPath:target,request:'million-full'});
    const elapsed=performance.now()-start;await invoke('close_file',{id:meta.id});return {report,elapsed_ms:elapsed};
  },{source:path.join(root,'bench/data/benchmark.parquet'),target:path.join(out,'m2-million.xlsx')});
  expect(result.report.rows).toBe(1000000);expect(result.report.columns).toBe(2);expect(errors).toEqual([]);
  await writeFile(path.join(out,'m2-stress.json'),JSON.stringify({scope:'Release actual Rust IPC, complete 1,000,000-row x 2-column export from 803MB fixture, not the 1000x50 benchmark',...result},null,2));console.log(JSON.stringify(result));
} finally {
  await writeFile(memoryStop,'stop');
  await new Promise(resolve=>{if(memory.exitCode!==null)resolve();else memory.once('exit',resolve);});
  await browser?.close();child.kill();
}
