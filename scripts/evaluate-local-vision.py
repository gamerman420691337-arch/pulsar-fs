#!/usr/bin/env python3
"""Bounded Linux CPU pixel-control experiment; not a product or qualification gate.

Requires bwrap and Pillow. No downloads, secrets, user media or device effects.
Uses the existing deadline/pipe/address-space supervisor. No cgroup RAM claim.
"""
import argparse
import importlib.util
import json
from pathlib import Path
from PIL import Image, ImageDraw

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--runtime', type=Path, required=True)
parser.add_argument('--runtime-sha256', required=True)
parser.add_argument('--model', type=Path, required=True)
parser.add_argument('--projector', type=Path, required=True)
parser.add_argument('--output', type=Path, required=True)
a = parser.parse_args()
for path in (a.runtime, a.model, a.projector, a.output):
    if not path.is_absolute(): parser.error('all paths must be absolute')
spec = importlib.util.spec_from_file_location('bounded', Path(__file__).with_name('evaluate-local-assistant.py'))
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)
expected = [(a.runtime, a.runtime_sha256, None),
            (a.model, '2a31195d3769c0b0fd0a4906201666108834848db768af11de1d2cef7cd35e65', 175054528),
            (a.projector, '0802360aca1748f112ea510b8ff277c65b1361c8ef30ed89b83c9c7a60d08e96', 190031616)]
for path, sha, size in expected:
    if (size is not None and path.stat().st_size != size) or m.digest(path) != sha:
        parser.error('asset identity mismatch: ' + str(path))
a.output.mkdir(exist_ok=False)
r = a.output
for shape in ['circle', 'square']:
    im = Image.new('RGB', (384,384), 'white')
    d = ImageDraw.Draw(im)
    getattr(d, 'ellipse' if shape == 'circle' else 'rectangle')((80,80,304,304),fill='red')
    im.save(r/(shape+'.png'))
(r/'schema.json').write_text(json.dumps({'type':'object','properties':{'shape':{'type':'string','enum':['circle','square','unknown']},'color':{'type':'string','enum':['red','blue','unknown']}},'required':['shape','color'],'additionalProperties':False}))
(r/'prompt.txt').write_text('Inspect the image. Identify the shape and its color. Return only JSON with shape and color. Use unknown if uncertain.')
(r/'assets.json').write_text(json.dumps([{'path':str(path),'sha256':sha,'bytes':path.stat().st_size} for path,sha,size in expected],indent=2)+'\n')
results=[]

for shape in ['circle','square']:
    argv=['/usr/bin/bwrap','--die-with-parent','--unshare-all','--new-session','--ro-bind',str(a.runtime),'/runtime','--ro-bind',str(a.model),'/model.gguf','--ro-bind',str(a.projector),'/mmproj.gguf','--ro-bind',str(r/(shape+'.png')),'/image.png','--ro-bind',str(r/'prompt.txt'),'/prompt.txt','--ro-bind',str(r/'schema.json'),'/schema.json','--proc','/proc','--dev','/dev','--tmpfs','/tmp','--chdir','/','/runtime','-m','/model.gguf','--mmproj','/mmproj.gguf','--image','/image.png','-f','/prompt.txt','--json-schema-file','/schema.json','--no-mmproj-offload','-ngl','0','-c','2048','-n','128','-t','2','-tb','2','--seed','42','--temp','0']
    code,out,failure,elapsed=m.supervise(argv,120,262144)
    for stream,data in out.items(): (r/(shape+'.'+stream)).write_bytes(data)
    try:
        def unique(pairs):
            d={}
            for k,v in pairs:
                if k in d: raise ValueError('duplicate')
                d[k]=v
            return d
        decoded=json.loads(out['stdout'],object_pairs_hook=unique)
        if not isinstance(decoded, dict) or set(decoded) != {'shape', 'color'}:
            raise ValueError('unexpected JSON fields')
        if decoded['shape'] not in ['circle', 'square', 'unknown'] or decoded['color'] not in ['red', 'blue', 'unknown']:
            raise ValueError('unsupported JSON value')
    except Exception as e: decoded={'parse_error':str(e)}
    result=dict(case=shape,exit=code,failure=failure,elapsed_seconds=elapsed,output=decoded,pass_case=code==0 and failure is None and decoded=={'shape':shape,'color':'red'},argv=argv)
    results.append(result);print(json.dumps(result),flush=True)
(r/'results.json').write_text(json.dumps(results,indent=2)+'\n')

raise SystemExit(0 if all(item['pass_case'] for item in results) else 1)
