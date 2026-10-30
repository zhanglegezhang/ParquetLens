export interface ColumnInfo {
  index:number; path:string; logical_type:string; physical_type:string; nullable:boolean; repeated:boolean;
  null_count:string|null; null_rate:number|null; min:string|null; max:string|null;
  min_max_truncated:boolean; stats_complete:boolean; distinct:string|null; compression:string;
  encodings:string; compressed_bytes:string; compressed_ratio:number; notes:string[];
}
export interface FileInfo {
  id:string; name:string; path:string; size_bytes:string; num_rows:string; row_groups:number;
  created_by:string|null; version:number; column_count:number; columns:ColumnInfo[]; metadata_bytes_read:number;
}
// Compare decimal strings without converting large integers or Decimals to Number.
export function compareValues(a:unknown,b:unknown):number {
  if (a===null || a===undefined) return b===null || b===undefined ? 0 : 1;
  if (b===null || b===undefined) return -1;
  if (typeof a==='number' && typeof b==='number') return a-b;
  if (typeof a==='boolean' && typeof b==='boolean') return Number(a)-Number(b);
  const x=String(a), y=String(b);
  if (/^-?\d+(\.\d+)?$/.test(x) && /^-?\d+(\.\d+)?$/.test(y)) {
    const norm=(s:string) => {
      const negative=s.startsWith('-'); const [whole,frac='']=s.replace(/^-/,'').split('.');
      const integer=whole.replace(/^0+/,'') || '0';
      return {negative:negative && /[1-9]/.test(integer+frac),integer,frac};
    };
    const p=norm(x),q=norm(y);
    if (p.negative!==q.negative) return p.negative ? -1 : 1;
    let result=p.integer.length-q.integer.length;
    if (!result) result=p.integer<q.integer ? -1 : p.integer>q.integer ? 1 : 0;
    if (!result) {
      const n=Math.max(p.frac.length,q.frac.length),pf=p.frac.padEnd(n,'0'),qf=q.frac.padEnd(n,'0');
      result=pf<qf ? -1 : pf>qf ? 1 : 0;
    }
    return p.negative ? -result : result;
  }
  return x.localeCompare(y,'zh-CN',{numeric:false});
}
export function tsv(rows:string[][]):string {
  return rows.map(row=>row.map(v=>/[\t\n\r"]/.test(v) ? `"${v.replaceAll('"','""')}"` : v).join('\t')).join('\r\n');
}
