import {chromium} from '@playwright/test';
import {spawn,execFileSync} from 'node:child_process';
import {mkdtemp,writeFile} from 'node:fs/promises';
import path from 'node:path';
import net from 'node:net';
const root=process.cwd(),label=process.env.BENCH_LABEL||'m5-final',exe=path.resolve(process.env.BENCH_EXE||'src-tauri/target/release/ParquetLens.exe');
const out=path.join(root,'test-results'),run=await mkdtemp(path.join(out,'startup-m5-'));const results=[];
for(const mode of ['first-webview-profile','existing-webview-profile'])for(let i=0;i<5;i++){
 const s=net.createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const port=s.address().port;await new Promise(r=>s.close(r));
 const t=performance.now();const child=spawn(exe,[],{env:{...process.env,LOCALAPPDATA:path.join(run,'local'),WEBVIEW2_USER_DATA_FOLDER:path.join(run,mode==='first-webview-profile'?`profile-${i}`:'profile-0'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`},stdio:'ignore'});let browser;
 try{const endpoint=`http://127.0.0.1:${port}`;let ok=false;for(let j=0;j<1000;j++){if(child.exitCode!==null)throw Error('App forwarded/exited; close existing app first');try{if((await fetch(endpoint+'/json/version')).ok){ok=true;break;}}catch{}await new Promise(r=>setTimeout(r,10));}if(!ok)throw Error('No debug endpoint');browser=await chromium.connectOverCDP(endpoint);const page=browser.contexts()[0].pages()[0];await page.waitForFunction(()=>!!window.__TAURI_INTERNALS__&&[...document.querySelectorAll('button')].some(b=>b.textContent.includes('Ctrl + O')&&!b.disabled));await page.evaluate(async()=>{await window.__TAURI_INTERNALS__.invoke('get_settings');await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));});results.push({mode,ms:performance.now()-t});}
 finally{await browser?.close();execFileSync(path.join(root,'.venv/Scripts/python.exe'),[path.join(root,'scripts/app_window.py'),String(child.pid),'close'],{windowsHide:true});if(child.exitCode===null)await new Promise(r=>child.once('exit',r));}
}
await writeFile(path.join(out,label+'-startup.json'),JSON.stringify({scope:'Process spawn to enabled open button, settings IPC and two frames. Includes CDP attachment overhead and warm OS cache. Fresh WebView profile separately measured, NOT post-reboot cold start.',results},null,2));
