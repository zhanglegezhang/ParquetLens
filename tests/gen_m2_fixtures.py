from pathlib import Path
import pyarrow as pa
import pyarrow.parquet as pq
from decimal import Decimal
root=Path('tests/fixtures/m2');root.mkdir(exist_ok=True)
schema=pa.schema([('id',pa.int64()),('label',pa.string()),('large_id',pa.uint64())])
with pq.ParquetWriter(root/'uneven.parquet',schema,compression='snappy') as writer:
 start=0
 for length in [2,1,600,600]:
  writer.write_table(pa.table({'id':range(start,start+length),'label':[f'行 {i}' for i in range(start,start+length)],'large_id':[2**64-1]*length},schema=schema));start+=length
v={
 'long_text':pa.array(['中😀'*20000,'=cmd','+cmd','-cmd','@cmd','\x00a\x01b\t\nc']),
 'precise_float':pa.array([1.2345678901234567,1e-20,-0.0,float('nan'),float('inf'),1.25]),
 'small_decimal':pa.array([Decimal('1.20'),Decimal('-5.60'),None,Decimal('0.00'),Decimal('999.99'),Decimal('3.14')],type=pa.decimal128(10,2)),
 'nested':pa.array([[9007199254740993,None],[],None,[1],[2],[3]],type=pa.list_(pa.int64())),
 'map':pa.array([[(1,'a'),(1,'b')],[],None,[(2,'c')],[],[]],type=pa.map_(pa.int32(),pa.string())),
 'historical':pa.array([-25568,-25509,-25508,0,1,2932896],type=pa.date32()),
}
pq.write_table(pa.table(v),root/'edge.parquet',row_group_size=2)

# Damage only selected data chunks; footer stays valid for projection/early-stop tests.
source=root/'uneven.parquet'
metadata=pq.read_metadata(source)
for name,group,column in [('corrupt_column',0,1),('corrupt_tail',3,0)]:
 data=bytearray(source.read_bytes());chunk=metadata.row_group(group).column(column)
 offset=chunk.dictionary_page_offset if chunk.has_dictionary_page else chunk.data_page_offset
 data[offset:offset+chunk.total_compressed_size]=b'\xff'*chunk.total_compressed_size
 (root/(name+'.parquet')).write_bytes(data)
