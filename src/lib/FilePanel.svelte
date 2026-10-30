<script lang="ts">
 import Columns from './Columns.svelte';import Samples from './Samples.svelte';import Dictionary from './Dictionary.svelte';import {t} from './i18n';import type {FileInfo} from './columns';
 let {file}:{file:FileInfo}=$props();let view=$state('columns'),samples=$state(false),dictionary=$state(false);
</script>
<nav class="views"><button class:active={view==='columns'} onclick={()=>view='columns'}>{t('columnView')}</button><button class:active={view==='samples'} onclick={()=>{view='samples';samples=true;}}>{t('sampleView')}</button><button class:active={view==='dictionary'} onclick={()=>{view='dictionary';dictionary=true;}}>{t('dictionary')}</button></nav>
<div hidden={view!=='columns'}><Columns {file}/></div>{#if samples}<div hidden={view!=='samples'}><Samples {file}/></div>{/if}{#if dictionary}<div hidden={view!=='dictionary'}><Dictionary {file}/></div>{/if}
<style>.views{display:flex;gap:8px;flex-wrap:wrap;margin:16px 0}.views button{background:var(--surface);color:var(--text);padding:9px 16px;border:1px solid var(--line);border-radius:6px}.views button.active{background:var(--soft);color:var(--accent);border-color:var(--accent)}</style>
