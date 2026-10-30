import {invoke} from '@tauri-apps/api/core';
export type Preferences={version:number;cap:number;sample_n:number;time_mode:string;theme:string;language:string;recent:string[]};
export const defaults:Preferences={version:1,cap:200,sample_n:1000,time_mode:'utc',theme:'system',language:'zh-CN',recent:[]};
export const preferences=$state<Preferences>({...defaults});
export const session=$state<{warning:string|null;task:{owner:string;request:string}|null;progress:string;revision:number;cacheRevision:number}>({warning:null,task:null,progress:'',revision:0,cacheRevision:0});
export function accept(snapshot:{preferences:Preferences;warning:string|null}){Object.assign(preferences,snapshot.preferences);session.warning=snapshot.warning;session.revision++;}
export async function refresh(){accept(await invoke('get_settings'));}
