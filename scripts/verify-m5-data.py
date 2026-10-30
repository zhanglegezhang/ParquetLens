import json,collections
from pathlib import Path
import pyarrow.parquet as pq
import pyarrow.compute as pc
report={}
for name in ['benchmark','mixed-snappy','mixed-zstd','stress-snappy','stress-zstd']:
 p=Path('bench/data')/(name+'.parquet');f=pq.ParquetFile(p);counts=collections.Counter()
 for batch in f.iter_batches(batch_size=65536,columns=['category_0']):
  for item in pc.value_counts(batch.column(0)).to_pylist(): counts[item['values']]+=item['counts']
 report[name]={'rows':f.metadata.num_rows,'columns':f.metadata.num_columns,'groups':f.metadata.num_row_groups,'bytes':p.stat().st_size,'category_0_counts':[{'value':None if k is None else str(k),'count':str(v)} for k,v in counts.items()]}
assert report['mixed-snappy']['category_0_counts']==report['mixed-zstd']['category_0_counts']
a=pq.ParquetFile('bench/data/mixed-snappy.parquet');b=pq.ParquetFile('bench/data/mixed-zstd.parquet')
for index in [0,39]: assert a.read_row_group(index).equals(b.read_row_group(index))
Path('test-results/m5-data-oracle.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
print('Five datasets counted independently; Snappy/Zstd first/last complete row groups equal')
