"""Independent PyArrow fixtures for enumeration semantics and damaged-body shortcuts."""
from pathlib import Path
import datetime, decimal, struct
import pyarrow as pa
import pyarrow.parquet as pq

root = Path(__file__).parent / 'fixtures/m3'
root.mkdir(parents=True, exist_ok=True)
n = 120
table = pa.table({
    'union_cap': pa.array([i % 40 for i in range(60)] + [30 + i % 40 for i in range(60)], pa.int64()),
    'text': pa.array(([None, '', '<NULL>', '北京', 'a', 'a'] * 20)),
    'constant': pa.array([7] * n, pa.int32()),
    'all_null': pa.array([None] * n, pa.string()),
    'float': pa.array(([float('nan'), struct.unpack('d', struct.pack('Q', 0x7ff8000000000001))[0], -0., 0., float('inf'), -float('inf')] * 20)),
    'binary': pa.array(([b'a'*32+b'x', b'a'*32+b'y', None] * 40), pa.binary()),
    'nested': pa.array(([{'x': 1, 'y': 'a'}, {'x': 1, 'y': 'b'}, None] * 40)),
    'map': pa.array(([[('a', 1), ('a', 2)], [('a', 2), ('a', 1)], None] * 40), pa.map_(pa.string(), pa.int32())),
    'small_decimal': pa.array([decimal.Decimal('12.30'), decimal.Decimal('-2.10')] * 60, pa.decimal128(15, 2)),
    'big_decimal': pa.array([decimal.Decimal('9007199254740993.12')] * n, pa.decimal128(20, 2)),
    'date': pa.array([datetime.date(2026, 1, 1), None] * 60, pa.date32()),
    'timestamp': pa.array([datetime.datetime(2026, 1, 1)] * n, pa.timestamp('ns')),
    'time': pa.array([123] * n, pa.time64('ns')),
})
pq.write_table(table, root / 'enums.parquet', row_group_size=60, compression='zstd')
# Each dictionary has unused entries and independent IDs.
schema = pa.schema([('dict', pa.dictionary(pa.int32(), pa.string()))])
with pq.ParquetWriter(root / 'dictionaries.parquet', schema, compression='snappy') as w:
    for values, keys in [(['a', 'b', 'unused'], [0, 0, 1, None]), (['b', 'a', 'unused2'], [0, 1, 1, None])]:
        w.write_table(pa.table({'dict': pa.DictionaryArray.from_arrays(pa.array(keys, pa.int32()), pa.array(values))}, schema=schema))
pq.write_table(pa.table({'constant': [42] * 100}), root / 'constant_damaged.parquet', compression='zstd')
p = root / 'constant_damaged.parquet'
b = bytearray(p.read_bytes()); footer = int.from_bytes(b[-8:-4], 'little'); b[4:len(b)-8-footer] = b'\xff' * (len(b)-12-footer); p.write_bytes(b)
pq.write_table(pa.table({'text': pa.array([], pa.string())}), root / 'empty.parquet')
pq.write_table(pa.table({'text': ['x' * 9_000_000, 'y']}), root / 'resource.parquet', compression='zstd', write_statistics=False)
print(f'M3 fixtures: {root}')
