"""Generate real ~800 MB compressed data, 20M rows / 50 columns. Bounded batches."""
import argparse, json, time
from pathlib import Path
import numpy as np
import pyarrow as pa
import pyarrow.parquet as pq
parser=argparse.ArgumentParser()
parser.add_argument('--out',default='bench/data/benchmark.parquet')
parser.add_argument('--rows',type=int,default=20_000_000)
args=parser.parse_args()
target=Path(args.out);target.parent.mkdir(parents=True,exist_ok=True)
rng=np.random.default_rng(20261029)
start=time.perf_counter()
writer=None
try:
    for offset in range(0,args.rows,250_000):
        n=min(250_000,args.rows-offset)
        # Five incompressible int64 columns ~800 MB over 20M rows;
        # remaining columns provide deterministic low-cardinality statistics.
        cols={f'random_{i}':pa.array(rng.integers(-(2**63),2**63-1,size=n,dtype=np.int64)) for i in range(5)}
        cols.update({f'category_{i}':pa.array(np.full(n,(offset//250_000+i)%200,dtype=np.int32)) for i in range(45)})
        table=pa.table(cols)
        if writer is None: writer=pq.ParquetWriter(target,table.schema,compression='snappy',use_dictionary=[f'category_{i}' for i in range(45)])
        writer.write_table(table,row_group_size=250_000)
finally:
    if writer:writer.close()
report={'bytes':target.stat().st_size,'rows':args.rows,'columns':50,'row_groups':pq.read_metadata(target).num_row_groups,'seconds':time.perf_counter()-start,'compression':'snappy','seed':20261029}
target.with_suffix('.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
print(json.dumps(report,indent=2))
