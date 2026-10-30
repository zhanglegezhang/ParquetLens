"""Independent Python CSV/XML readback of actual M4 UI exports."""
import csv
import json
from pathlib import Path
from zipfile import ZipFile
import xml.etree.ElementTree as ET
import pyarrow.parquet as pq

root = Path(__file__).resolve().parents[1]
out = root / 'test-results'
ns = {'s': 'http://schemas.openxmlformats.org/spreadsheetml/2006/main'}
checks = []

def workbook(name):
    with ZipFile(out / name) as z:
        book = ET.fromstring(z.read('xl/workbook.xml'))
        names = [s.get('name') for s in book.findall('s:sheets/s:sheet', ns)]
        result = {}
        for i, name in enumerate(names, 1):
            sheet = ET.fromstring(z.read(f'xl/worksheets/sheet{i}.xml'))
            assert not sheet.findall('.//s:f', ns)
            assert sheet.find('.//s:pane', ns).get('ySplit') == '1'
            assert sheet.find('s:autoFilter', ns) is not None
            assert all(float(c.get('width')) < 61 for c in sheet.findall('s:cols/s:col', ns))
            rows = []
            for row in sheet.findall('s:sheetData/s:row', ns):
                values = []
                for cell in row:
                    if cell.get('t') == 'inlineStr':
                        values.append(''.join(t.text or '' for t in cell.findall('.//s:t', ns)))
                    else:
                        v = cell.find('s:v', ns)
                        values.append(v.text if v is not None else '')
                rows.append(values)
            result[name] = rows
        return result

full = workbook('m4-dictionary.xlsx')
assert list(full) == ['列清单', '枚举值', '样例']
assert len(full['样例']) == 8
assert len(full['列清单']) - 1 == pq.ParquetFile(root / 'tests/fixtures/m3/enums.parquet').metadata.num_columns
assert any('基数 > 50' in str(row) for row in full['枚举值'])
assert any('未分析' in str(row) for row in full['枚举值'])
existing = workbook('m4-dictionary-existing.xlsx')
assert list(existing) == ['列清单', '枚举值']
assert any('采样结果' in str(row) and '60 / 120' in str(row) for row in existing['枚举值'])
checks.append('dictionary sheets, all leaf metadata, sample size, high/missing/sample statuses, freeze/filter/width and no formulas')

for name in ['m4-edge-protected.csv', 'm4-edge-raw.csv', 'm4-enums.csv', 'm4-benchmark.csv']:
    data = (out / name).read_bytes()
    assert data[:3] == b'\xef\xbb\xbf'
    with (out / name).open(encoding='utf-8-sig', newline='') as f:
        rows = list(csv.reader(f))
    assert len(rows) > 1
    if 'edge' in name:
        assert any('中😀' * 20000 in value for row in rows for value in row)
        expected = "'=cmd" if 'protected' in name else '=cmd'
        assert any(expected == value for row in rows for value in row)
    if name == 'm4-enums.csv':
        assert sum(int(row[3]) for row in rows[1:]) == 60
        assert sum(row[2] == 'true' for row in rows[1:]) == 1
    if name == 'm4-benchmark.csv':
        assert len(rows) == 1001 and len(rows[0]) == 50
        expected = pq.ParquetFile(root / 'bench/data/benchmark.parquet').read_row_group(0).slice(0, 1000).to_pydict()
        for col, values in enumerate(expected.values()):
            assert [row[col] for row in rows[1:]] == [str(v) for v in values]
checks.append('CSV BOM, parsed embedded delimiters/long Unicode, raw/protected text, NULL count, 1000x50 values equal PyArrow')
bench = workbook('m4-benchmark-dictionary.xlsx')
assert len(bench['样例']) == 1001 and len(bench['列清单']) == 51
checks.append('benchmark dictionary 50 columns and 1000 samples')
(out / 'm4-export-verification.json').write_text(json.dumps({'passed': checks}, ensure_ascii=False, indent=2), encoding='utf-8')
print('\n'.join(checks))
