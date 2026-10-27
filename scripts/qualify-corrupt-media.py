#!/usr/bin/env python3
import argparse, json, subprocess, hashlib
from pathlib import Path
parser=argparse.ArgumentParser(description='Authored corrupt MPEG-4 controls; no media/model/device qualification')
parser.add_argument('--output', type=Path, required=True)
parser.add_argument('--ffmpeg', type=Path, required=True)
parser.add_argument('--ffprobe', type=Path, required=True)
args=parser.parse_args()
r=args.output.absolute();r.mkdir(mode=0o700)
f=args.ffmpeg.resolve(strict=True);p=args.ffprobe.resolve(strict=True);clean=r/'clean.mp4'
def run(argv):return subprocess.run(list(map(str,argv)),stdin=subprocess.DEVNULL,capture_output=True,timeout=30)
x=run([f,'-v','error','-nostdin','-f','lavfi','-i','testsrc2=size=64x64:rate=8:duration=2','-threads','1','-c:v','mpeg4','-g','16','-bf','0',clean]);assert x.returncode==0
q=run([p,'-v','error','-select_streams','v:0','-show_packets','-show_entries','packet=pos,size','-of','json',clean]);assert q.returncode==0
packets=json.loads(q.stdout)['packets'];raw=clean.read_bytes(); observations=[]
for ordinal in [1,3,7]:
 info=packets[ordinal];pos=int(info['pos']);size=int(info['size']); data=bytearray(raw);n=max(1,size//2);data[pos+8:pos+8+n]=bytes(n)
 damaged=r/f'damaged-{ordinal}.mp4';damaged.write_bytes(data)
 row={'ordinal':ordinal,'packet_pos':pos,'packet_size':size,'mutation_start':pos+8,'mutation_bytes':n}
 for strict in [False,True]:
  argv=[f,'-v','error','-nostdin']+(['-xerror'] if strict else [])+['-i',damaged,'-map','0:v:0','-an','-sn','-dn','-threads','1','-f','rawvideo','-pix_fmt','rgb24','pipe:1']
  x=run(argv);(r/f'decode-{ordinal}-{strict}.stderr').write_bytes(x.stderr)
  row[str(strict)]={'exit':x.returncode,'frames':len(x.stdout)//(64*64*3),'bytes':len(x.stdout),'sha256':hashlib.sha256(x.stdout).hexdigest(),'stderr_bytes':len(x.stderr)}
 observations.append(row)
(r/'observations.json').write_text(json.dumps(observations,indent=2)+'\n')
assert all(row['False']['exit']==0 and row['False']['frames']==16 and row['False']['stderr_bytes']>0 and row['True']['exit']!=0 for row in observations)
identities={str(tool): {'sha256': hashlib.sha256(tool.read_bytes()).hexdigest(), 'byte_len': tool.stat().st_size} for tool in [f,p]}
(r/'subjects.json').write_text(json.dumps({'qualified':False, 'physical_device_used':False, 'authored_synthetic_media':True, 'tools':identities, 'files':{q.name:{'sha256':hashlib.sha256(q.read_bytes()).hexdigest(),'byte_len':q.stat().st_size} for q in [clean]+sorted(r.glob('damaged-*.mp4'))}},indent=2)+'\n')
print('Three actual codec concealment/error controls recorded; qualified=false')
