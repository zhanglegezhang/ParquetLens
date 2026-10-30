"""Deterministic mixed row groups; bounded generation, Snappy and Zstd."""
import argparse,json,time
from pathlib import Path
import numpy as np
import pyarrow as pa
import pyarrow.parquet as pq
parser=argparse.ArgumentParser()
parser.add_argument('--groups',type=int,default=40)
parser.add_argument('--prefix',choices=['mixed','stress'],default='mixed')
args=parser.parse_args()
if not 1<=args.groups<=1000:parser.error('--groups must be 1..1000')
out=Path('bench/data');out.mkdir(parents=True,exist_ok=True)
rng=np.random.default_rng(20261030); writers={};start=time.perf_counter()
try:
 for g in range(args.groups):
  n=250000
  cols={f'random_{i}':pa.array(rng.integers(-(2**63),2**63-1,n,dtype=np.int64)) for i in range(5)}
  codes=rng.integers(0,100,n,dtype=np.int32);null=rng.random(n)<.05
  for i in range(40):cols[f'category_{i}']=pa.array((codes+i)%100,mask=null)
  for i in range(5):cols[f'text_{i}']=pa.DictionaryArray.from_arrays(pa.array((codes+i)%100,mask=null),pa.array([f'类别-{j:03d}' for j in range(100)]))
  table=pa.table(cols)
  for codec in ['snappy','zstd']:
   if codec not in writers:writers[codec]=pq.ParquetWriter(out/f'{args.prefix}-{codec}.parquet',table.schema,compression=codec,use_dictionary=list(cols)[5:])
   writers[codec].write_table(table,row_group_size=n)
finally:
 for w in writers.values():w.close()
report={c:{'bytes':(out/f'{args.prefix}-{c}.parquet').stat().st_size,'rows':args.groups*250000,'columns':50,'groups':args.groups,'seed':20261030,'null_fraction':.05,'distinct':100,'constant_groups':False} for c in writers}
report['generation_seconds']=time.perf_counter()-start
(out/f'{args.prefix}.json').write_text(json.dumps(report,indent=2),encoding='utf-8');print(json.dumps(report,indent=2))
