"""Independent XML verification of the UI-generated M3 workbook."""
from pathlib import Path
from zipfile import ZipFile
import xml.etree.ElementTree as ET

p = Path(__file__).resolve().parents[1] / 'test-results/m3-ui-enums.xlsx'
ns = {'s': 'http://schemas.openxmlformats.org/spreadsheetml/2006/main'}
with ZipFile(p) as z:
    sheet = ET.fromstring(z.read('xl/worksheets/sheet1.xml'))
    assert not sheet.findall('.//s:f', ns), 'No formula execution'
    assert sheet.find('.//s:pane', ns).get('ySplit') == '1'
    assert sheet.find('s:autoFilter', ns) is not None
    assert all(float(c.get('width')) < 61 for c in sheet.findall('s:cols/s:col', ns))
    rows = sheet.findall('s:sheetData/s:row', ns)
    assert len(rows) == 6
    def text(c):
        return ''.join(t.text or '' for t in c.findall('.//s:t', ns))
    count_sum = 0
    ratio_sum = 0.
    null_rows = 0
    empty_rows = 0
    for row in rows[1:]:
        cells = {c.get('r')[0]: c for c in row}
        count_sum += int(cells['D'].find('s:v', ns).text)
        ratio_sum += float(cells['E'].find('s:v', ns).text)
        is_null = cells['C'].find('s:v', ns).text == '1'
        null_rows += is_null
        empty_rows += not is_null and ('B' not in cells or text(cells['B']) == '')
        assert '采样' in text(cells['F']) and '60 / 120' in text(cells['F'])
    assert count_sum == 60 and abs(ratio_sum - 1) < 1e-12
    assert null_rows == 1 and empty_rows == 1
print('M3 xlsx XML: 5 values; NULL/empty distinct; 60 sample rows; percentages 100%; text/no formulas; frozen header/filter/width passed')
