"""Independent PyArrow writer for M1 metadata/format tests. No user data."""
from pathlib import Path
from decimal import Decimal
import datetime as dt
import json
import pyarrow as pa
import pyarrow.parquet as pq

ROOT = Path(__file__).resolve().parent / 'fixtures' / 'm1'
ROOT.mkdir(parents=True, exist_ok=True)
data = {}
def add(name, values, typ):
    data[name] = pa.array(values, type=typ)

for bits in (8,16,32,64):
    add(f'i{bits}', [-(2**(bits-1)), -1, None, 0, 1, 2**(bits-1)-1], getattr(pa, f'int{bits}')())
    add(f'u{bits}', [0, 1, None, 2**bits-1, 2, 3], getattr(pa, f'uint{bits}')())
add('id64',[9007199254740993,None,1000000000000001,9007199254740992,3,4],pa.int64())
add('bool',[False,True,None,False,True,True],pa.bool_())
add('f32',[-1.25,1.5,None,0.0,-0.0,2.5],pa.float32())
add('f64',[-1e-20,1e20,None,0.0,1.25,2.5],pa.float64())
add('decimal128',[Decimal('-99999999999999999999999999999999.123456'),Decimal('0.000001'),None,Decimal('99999999999999999999999999999999.123456'),Decimal('-0.000001'),Decimal('1')],pa.decimal128(38,6))
add('decimal256',[Decimal('-'+'9'*50+'.1234567890'),Decimal('0.0000000001'),None,Decimal('9'*50+'.1234567890'),Decimal('-0.0000000001'),Decimal('1')],pa.decimal256(60,10))
add('text',['', '中文🦀', None, '=cmd', 'hello\tworld\nline', '🦀'],pa.string())
add('date32',[dt.date(1800,1,1),dt.date(1970,1,1),None,dt.date(2024,2,29),dt.date(2000,1,1),dt.date(1900,1,1)],pa.date32())
add('date64',[dt.date(1800,1,1),dt.date(1970,1,1),None,dt.date(2024,2,29),dt.date(2000,1,1),dt.date(1900,1,1)],pa.date64())
for unit in ('ms','us','ns'):
    for tz in (None,'UTC'):
        add(f'ts_{unit}_{tz or "naive"}',[-1,0,None,123456789,1,2],pa.timestamp(unit,tz))
add('time32',[0,1,None,1000,123456,86399999],pa.time32('ms'))
add('time64us',[0,1,None,1000,123456,86399999999],pa.time64('us'))
add('time64ns',[0,1,None,1000,123456,86399999999999],pa.time64('ns'))
add('binary',[b'',b'\x00\xff',None,b'\x01',b'\xff'*20,b'\x02'],pa.binary())
add('fixed16',[bytes(16),bytes(range(16)),None,b'\xff'*16,b'\x01'*16,b'\x02'*16],pa.binary(16))
add('uuid',[bytes(16),bytes(range(16)),None,b'\xff'*16,b'\x01'*16,b'\x02'*16],pa.uuid())
data['dictionary'] = pa.array(['甲','乙',None,'甲','','乙']).dictionary_encode()
add('list',[[1,2],[],None,[3,None],[4],[5]],pa.list_(pa.int32()))
add('struct',[{'a':1,'b':'甲'},None,{'a':None,'b':'乙'},{'a':4,'b':None},{'a':2,'b':''},{'a':3,'b':'丙'}],pa.struct([('a',pa.int32()),('b',pa.string())]))
add('map',[[('a',1)],[],None,[('b',2)],[],[('a',3)]],pa.map_(pa.string(),pa.int32()))
add('all_null',[None]*6,pa.int64())
add('null_type',[None]*6,pa.null())
table=pa.table(data)
pq.write_table(table,ROOT/'types.parquet',row_group_size=2,compression='zstd',version='2.6',use_dictionary=True)
pq.write_table(table,ROOT/'no_stats.parquet',row_group_size=2,compression='snappy',write_statistics=False)
pq.write_table(table.slice(0,0),ROOT/'empty.parquet')
pq.write_table(pa.table({'x':[float('nan'),float('nan')],'y':[float('-inf'),float('inf')]}),ROOT/'special_float.parquet')
pq.write_table(pa.table({'only_null':pa.array([None,None],type=pa.int64())}),ROOT/'all_null.parquet')
for codec in ('snappy','gzip','zstd','lz4','brotli'):
    pq.write_table(pa.table({'x':[1,2,None,4]}),ROOT/f'{codec}.parquet',compression=codec,row_group_size=2)
pq.write_table(pa.table({f'col_{i:04}':[i] for i in range(601)}),ROOT/'wide.parquet')
(ROOT/'corrupt.parquet').write_bytes(b'PAR1'+b'invalid'*4+b'PAR1')
(ROOT/'zero.parquet').write_bytes(b'')

# Genuine encrypted footer and plaintext-footer encrypted-column files.
import base64
import pyarrow.parquet.encryption as pe
class TestKms(pe.KmsClient):
    def wrap_key(self,key,master_key_identifier): return base64.b64encode(key)
    def unwrap_key(self,key,master_key_identifier): return base64.b64decode(key)
factory=pe.CryptoFactory(lambda _:TestKms())
for plaintext in (False,True):
    config=pe.EncryptionConfiguration(footer_key='test-footer',column_keys={'test-column':['x']},plaintext_footer=plaintext,internal_key_material=True)
    props=factory.file_encryption_properties(pe.KmsConnectionConfig(kms_instance_id='test'),config)
    pq.write_table(pa.table({'x':[1,2,3]}),ROOT/f'encrypted_{plaintext}.parquet',encryption_properties=props)

expected={}
for name in ['i8','u8','i16','u16','i32','u32','i64','u64','id64','decimal128','decimal256']:
    vals=[x.as_py() for x in table[name] if x.as_py() is not None]
    expected[name]={'min':str(min(vals)),'max':str(max(vals)),'null_count':'1'}
(ROOT/'expected.json').write_text(json.dumps(expected,ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps({'pyarrow':pa.__version__,'columns':len(data),'leaf_columns':pq.read_metadata(ROOT/'types.parquet').num_columns,'files':len(list(ROOT.glob('*.parquet')))},indent=2))
