from pathlib import Path
from zipfile import ZipFile
import xml.etree.ElementTree as ET
from collections import deque
import pyarrow.parquet as pq
import json
root=Path.cwd(); ns={'s':'http://schemas.openxmlformats.org/spreadsheetml/2006/main'}
first=[];last=deque(maxlen=2);count=0;data=None
with ZipFile(root/'test-results/m2-million.xlsx') as book, book.open('xl/worksheets/sheet1.xml') as xml:
 for event,element in ET.iterparse(xml,events=('start','end')):
  if event=='start' and element.tag.endswith('}sheetData'):data=element
  if event=='end' and element.tag.endswith('}row'):
   values=[]
   for cell in element.findall('s:c',ns):
    values.append(''.join(cell.itertext()))
   if count==0:assert values==['random_0','random_1']
   else:
    row=tuple(int(v) for v in values)
    if count<=2:first.append(row)
    last.append(row)
   count+=1
   if data is not None:data.clear()
assert count==1000001,count
expected_first=[];expected_last=deque(maxlen=2);seen=0
for batch in pq.ParquetFile(root/'bench/data/benchmark.parquet').iter_batches(batch_size=65536,columns=['random_0','random_1']):
 take=min(batch.num_rows,1000000-seen)
 if not take:break
 if not seen:expected_first=list(zip(batch.column(0).slice(0,2).to_pylist(),batch.column(1).slice(0,2).to_pylist()))
 expected_last.extend(zip(batch.column(0).slice(max(0,take-2),min(take,2)).to_pylist(),batch.column(1).slice(max(0,take-2),min(take,2)).to_pylist()))
 seen+=take
 if seen==1000000:break
assert first==expected_first
assert list(last)==list(expected_last)
result={'rows':count-1,'columns':2,'first_two_and_last_two_match_pyarrow_exactly':True,'xlsx_bytes':(root/'test-results/m2-million.xlsx').stat().st_size}
(root/'test-results/m2-stress-readback.json').write_text(json.dumps(result,indent=2),encoding='utf-8');print(result)
