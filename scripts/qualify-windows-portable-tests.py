#!/usr/bin/env python3
"""Cross-built portable tests under isolated Wine; never native Windows qualification."""
import argparse, hashlib, json, os, re, subprocess
from pathlib import Path

def identity(path):
    with path.open('rb') as source:
        return {'sha256':hashlib.file_digest(source,'sha256').hexdigest(),'byte_len':path.stat().st_size}

def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['build-log','scratch','evidence']:p.add_argument('--'+name,type=Path,required=True)
    args=p.parse_args();root=args.scratch.absolute();out=args.evidence.absolute();root.mkdir(mode=0o700);out.mkdir(mode=0o700)
    prefix=root/'wine';runtime=root/'xdg';runtime.mkdir(mode=0o700)
    paths=[Path(path) for path in re.findall(r'Executable .*?\((/[^()]+\.exe)\)',args.build_log.read_text())]
    assert paths and len(paths)<=128 and len(set(paths))==len(paths)
    assert all('/x86_64-pc-windows-gnu/debug/deps/' in str(path) and path.is_file() for path in paths)
    env=dict(os.environ,WINEPREFIX=str(prefix),WINEARCH='win64',WINEDEBUG='-all',XDG_RUNTIME_DIR=str(runtime))
    env.pop('DISPLAY',None);env.pop('WAYLAND_DISPLAY',None)
    records=[];pins={str(path):identity(path) for path in paths};custody={'prefix':str(prefix),'qualified':False,'joined':False}
    try:
        boot=subprocess.run(['wineboot','--init'],env=env,stdin=subprocess.DEVNULL,capture_output=True,text=True,timeout=60)
        (out/'wineboot.json').write_text(json.dumps({'exit':boot.returncode,'stdout':boot.stdout,'stderr':boot.stderr},indent=2)+'\n')
        assert boot.returncode==0
        for path in paths:
            run=subprocess.run(['wine',str(path),'--test-threads=1'],env=env,stdin=subprocess.DEVNULL,capture_output=True,text=True,timeout=60)
            name=path.stem;record={'path':str(path),'exit':run.returncode,'stdout':run.stdout,'stderr':run.stderr};records.append(record)
            (out/(name+'.json')).write_text(json.dumps(record,indent=2)+'\n')
            print(name,run.returncode,flush=True)
        (out/'result.json').write_text(json.dumps({'qualified':False,'native_windows_tested':False,'execution':'isolated Wine emulation','executables':len(records),'passed_executables':sum(record['exit']==0 for record in records),'failed_executables':sum(record['exit']!=0 for record in records),'binary_identities':pins},indent=2)+'\n')
    finally:
        stop=subprocess.run(['wineserver','-k'],env=env,stdin=subprocess.DEVNULL,capture_output=True,text=True,timeout=15)
        joined=subprocess.run(['wineserver','-w'],env=env,stdin=subprocess.DEVNULL,capture_output=True,text=True,timeout=15)
        custody.update(stop_exit=stop.returncode,wait_exit=joined.returncode,joined=joined.returncode==0)
        (out/'custody.json').write_text(json.dumps(custody,indent=2)+'\n')
    assert len(records)==len(paths) and all(record['exit']==0 for record in records), 'Portable emulated test failures retained'

if __name__=='__main__':main()
