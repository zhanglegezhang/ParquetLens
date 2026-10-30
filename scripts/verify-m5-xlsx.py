"""Independent readback of the last benchmark XLSX (1000 x 50 cells)."""
import json,sys,datetime
from pathlib import Path
from zipfile import ZipFile
import xml.etree.ElementTree as ET
import pyarrow.parquet as pq
root=Path(__file__).resolve().parents[1]
label=sys.argv[1] if len(sys.argv)>1 else 'm5-final'
r=json.loads((root/'test-results'/f'{label}.json').read_text(encoding='utf-8'))
if 'output_xlsx' in r:p=Path(r['output_xlsx'])
else:
 start=datetime.datetime.fromisoformat(r['date'].replace('Z','+00:00')).timestamp()
 candidates=[p/'bench.xlsx' for p in (root/'test-results').glob('m5-*') if p.is_dir() and abs(p.stat().st_birthtime-start)<5 and (p/'bench.xlsx').exists()]
 assert len(candidates)==1,candidates
 p=candidates[0]
name=list(r['datasets'])[-1]
expected=pq.ParquetFile(root/'bench/data'/f'{name}.parquet').read_row_group(0).slice(0,1000).to_pylist()
ns={'s':'http://schemas.openxmlformats.org/spreadsheetml/2006/main'}
with ZipFile(p) as z:
 shared=[]
 if 'xl/sharedStrings.xml' in z.namelist():shared=[''.join(e.itertext()) for e in ET.fromstring(z.read('xl/sharedStrings.xml'))]
 sheet=ET.fromstring(z.read('xl/worksheets/sheet1.xml'))
 assert not sheet.findall('.//s:f',ns)
 rows=sheet.findall('s:sheetData/s:row',ns)
 assert len(rows)==1001,len(rows)
 cells={}
 for row in rows:
  for c in row.findall('s:c',ns):
   v=c.find('s:v',ns)
   cells[c.get('r')]=''.join(c.find('s:is',ns).itertext()) if c.get('t')=='inlineStr' else shared[int(v.text)] if c.get('t')=='s' else v.text if v is not None else None
 def letter(i):
  s=''
  while i:i,n=divmod(i-1,26);s=chr(65+n)+s
  return s
 fields=list(expected[0])
 for i,f in enumerate(fields,1):assert cells.get(letter(i)+'1')==f
 for j,row in enumerate(expected,2):
  for i,f in enumerate(fields,1):
   want=None if row[f] is None else str(row[f]);got=cells.get(letter(i)+str(j));assert got==want,(j,f,got,want)
report={'xlsx':str(p),'dataset':name,'cells_compared':50000,'rows':1000,'columns':50,'no_formulas':True,'oracle':'Independent PyArrow + XLSX XML'}
(root/'test-results'/f'{label}-xlsx-readback.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps(report,ensure_ascii=False,indent=2))
