"""PE on-disk section sizes; no executable loading."""
import json,struct,sys
from pathlib import Path
p=Path(sys.argv[1]);data=p.read_bytes();pe=struct.unpack_from('<I',data,0x3c)[0];count=struct.unpack_from('<H',data,pe+6)[0];optional=struct.unpack_from('<H',data,pe+20)[0];start=pe+24+optional
sections=[]
for i in range(count):
 pos=start+40*i;name=data[pos:pos+8].rstrip(b'\0').decode('ascii');virtual,rva,raw,offset=struct.unpack_from('<IIII',data,pos+8);sections.append(dict(name=name,virtual_bytes=virtual,disk_bytes=raw))
print(json.dumps(dict(path=str(p),bytes=len(data),sections=sections),indent=2))
