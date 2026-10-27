#!/usr/bin/env python3
"""Owned no-network Handy preflight and shared desktop-controller checks."""
import argparse, hashlib, json, os, subprocess, time
from pathlib import Path

def pin(path):
    with path.open('rb') as source:
        return {'sha256': hashlib.file_digest(source, 'sha256').hexdigest(), 'byte_len': path.stat().st_size}

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['app','controller','output','evidence']:parser.add_argument('--'+name,type=Path,required=True)
    args=parser.parse_args();app=args.app.resolve(strict=True);controller=args.controller.resolve(strict=True)
    root=args.output.absolute();out=args.evidence.absolute();root.mkdir(mode=0o700);out.mkdir(mode=0o700)
    state=root/'state';env=dict(os.environ,PULSAR_STATE_DIR=str(state),TMPDIR=str(root))
    for name in ['PULSAR_FIXTURE_ABSENT_HANDY_API','PULSAR_FIXTURE_ABSENT_HANDY_CONNECTION']:env.pop(name,None)
    records=[];server=None
    try:
        with (out/'engine.log').open('wb') as log:server=subprocess.Popen([str(app),'engine'],env=env,stdin=subprocess.DEVNULL,stdout=log,stderr=log)
        deadline=time.monotonic()+30
        while not (state/'engine.sock').exists():
            assert server.poll() is None and time.monotonic()<deadline
            time.sleep(.05)
        result=subprocess.run([str(controller),str(state)],env=env,stdin=subprocess.DEVNULL,capture_output=True,text=True,timeout=30)
        (out/'controller.json').write_text(json.dumps({'exit':result.returncode,'stdout':result.stdout,'stderr':result.stderr},indent=2)+'\n')
        assert result.returncode==0
        report=json.loads(result.stdout);assert report['credential_preflight_refused'] and report['master_unchanged'] and not report['qualified']
        cases=[('absent-credentials',['--api-key-env','PULSAR_FIXTURE_ABSENT_HANDY_API','--connection-key-env','PULSAR_FIXTURE_ABSENT_HANDY_CONNECTION','--allow-online-inspection'],'credential is absent from the engine environment'),('same-name',['--api-key-env','FIXTURE_SAME','--connection-key-env','FIXTURE_SAME','--allow-online-inspection'],'distinct environment variable names'),('missing-consent',['--api-key-env','PULSAR_FIXTURE_ABSENT_HANDY_API','--connection-key-env','PULSAR_FIXTURE_ABSENT_HANDY_CONNECTION'],'--allow-online-inspection')]
        for name,words,expected in cases:
            result=subprocess.run([str(app),'handy-inspect',*words],env=env,stdin=subprocess.DEVNULL,capture_output=True,text=True,timeout=30)
            record={'name':name,'exit':result.returncode,'stdout':result.stdout,'stderr':result.stderr};records.append(record)
            assert result.returncode!=0 and expected in result.stderr,record
        (out/'cli.json').write_text(json.dumps(records,indent=2)+'\n')
        (out/'result.json').write_text(json.dumps({'qualified':False,'physical_device_used':False,'online_connection_tested':False,'native_rendered_gui':'UNKNOWN','app':pin(app),'controller':pin(controller),'cli_refusals':len(records),'production_shared_controller':True,'master_unchanged':True},indent=2)+'\n')
    finally:
        if server is not None and server.poll() is None:
            server.terminate()
            try:server.wait(timeout=10)
            except subprocess.TimeoutExpired:server.kill();server.wait(timeout=10)
        (out/'custody.json').write_text(json.dumps({'engine_joined':server is not None and server.poll() is not None})+'\n')
    assert server is not None and server.poll() is not None

if __name__=='__main__':main()
