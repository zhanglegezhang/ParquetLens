import {chromium,expect} from '@playwright/test';
import {spawn,execFileSync} from 'node:child_process';
import {readFile,writeFile,mkdir,copyFile} from 'node:fs/promises';
import path from 'node:path';
import net from 'node:net';
import {fileURLToPath} from 'node:url';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const run=path.join(root,'test-results/install-m5');
const report=JSON.parse(await readFile(path.join(run,'acceptance.json'),'utf8'));
const exe=path.join(report.install_path,'ParquetLens.exe');
const fixture=path.join(run,'中文 空格 样例.parquet');
await copyFile(path.join(root,'tests/fixtures/m1/types.parquet'),fixture);
const server=net.createServer();await new Promise(r=>server.listen(0,'127.0.0.1',r));const port=server.address().port;await new Promise(r=>server.close(r));
const env={...process.env,WEBVIEW2_USER_DATA_FOLDER:path.join(run,'webview'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`};
const child=spawn(exe,[],{cwd:root,env,stdio:'ignore'});let browser;
try{
  const endpoint=`http://127.0.0.1:${port}`;
  for(let i=0;i<100;i++){try{if((await fetch(endpoint+'/json/version')).ok)break;}catch{}await new Promise(r=>setTimeout(r,200));}
  browser=await chromium.connectOverCDP(endpoint);let page;
  for(let i=0;i<50;i++){page=browser.contexts()[0].pages().find(p=>p.url().includes('tauri.localhost'));if(page)break;await new Promise(r=>setTimeout(r,100));}
  if(!page)throw new Error('Installed app page missing');
  const ipc=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
  await expect(page.locator('.tabs .tab')).toHaveCount(0);
  const prefs=await ipc('get_settings');expect(prefs.preferences.cap).toBe(333);expect(prefs.preferences.theme).toBe('dark');
  // Launch the installed executable directly with a Unicode/space path.
  const forwarded=spawn(exe,[fixture],{cwd:root,env,stdio:'ignore'});
  await new Promise((resolve,reject)=>{forwarded.once('error',reject);forwarded.once('exit',code=>code===0?resolve():reject(new Error(`Second instance exited ${code}`)));});
  await expect(page.getByRole('heading',{name:'中文 空格 样例.parquet',exact:true})).toBeVisible();await expect(page.locator('.tabs .tab')).toHaveCount(1);
  await page.screenshot({path:path.join(run,'installed-cli-open.png'),fullPage:true});
  report.installed_settings_loaded=true;report.command_line_unicode_space_path=true;report.command_line_to_existing_instance=true;report.no_auto_restored_tabs=true;
  await writeFile(path.join(run,'runtime.json'),JSON.stringify(report,null,2));
  console.log(JSON.stringify(report,null,2));
}finally{
  await browser?.close();
  execFileSync(path.join(root,'.venv/Scripts/python.exe'),[path.join(root,'scripts/app_window.py'),String(child.pid),'close'],{windowsHide:true});
  await Promise.race([new Promise(r=>child.once('exit',r)),new Promise((_,reject)=>setTimeout(()=>reject(new Error('Installed app did not close')),10000))]);
}
