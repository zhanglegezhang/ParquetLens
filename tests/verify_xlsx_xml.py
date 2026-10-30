"""Independent XLSX XML checks; calamine semantic read-back lives in Rust tests."""
from pathlib import Path
from zipfile import ZipFile
import xml.etree.ElementTree as ET
import json
root=Path(__file__).resolve().parent.parent
ns={'s':'http://schemas.openxmlformats.org/spreadsheetml/2006/main'}
checks=[]
for name in ['m2-types.xlsx','m2-edge.xlsx','m2-benchmark.xlsx']:
 with ZipFile(root/'test-results'/name) as book:
  sheet=ET.fromstring(book.read('xl/worksheets/sheet1.xml'))
  styles=ET.fromstring(book.read('xl/styles.xml'))
  assert not sheet.findall('.//s:f',ns), 'No data cell may become a formula'
  pane=sheet.find('s:sheetViews/s:sheetView/s:pane',ns)
  assert pane is not None and pane.attrib.get('ySplit')=='1'
  assert sheet.find('s:autoFilter',ns) is not None
  assert all(float(c.attrib['width'])<=61 for c in sheet.findall('s:cols/s:col',ns))
  formats=styles.find('s:cellXfs',ns)
  fonts=styles.find('s:fonts',ns)
  first=sheet.find('s:sheetData/s:row/s:c',ns)
  font=int(formats[int(first.attrib['s'])].attrib['fontId'])
  assert fonts[font].find('s:b',ns) is not None
  for cell in sheet.findall('.//s:c',ns):
   value=''.join(cell.itertext())
   if value.startswith(('=cmd','+cmd','-cmd','@cmd')):
    assert cell.attrib.get('t')=='inlineStr'
   if cell.attrib.get('t')=='inlineStr':
    assert len(value.encode('utf-16-le'))//2<=32767
  if name=='m2-benchmark.xlsx':
   assert len(sheet.findall('s:sheetData/s:row',ns))==1001
   assert sheet.find('s:autoFilter',ns).attrib['ref']=='A1:AX1001'
  checks.append(name+': no formulas; string types/limits; frozen header; bold header; filter; bounded widths')
(root/'test-results'/'m2-xlsx-xml.json').write_text(json.dumps({'passed':checks},ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps({'passed':checks},ensure_ascii=False))
