#!/usr/bin/env python3
"""Owned Linux CLI/IPC learned proposal probe; not package/UI/effect qualification."""
import argparse, hashlib, json, os, pathlib, selectors, signal, subprocess, time, uuid

def digest(path):
    h=hashlib.sha256()
    with path.open('rb') as f:
        for block in iter(lambda:f.read(1048576),b''):h.update(block)
    return h.hexdigest()

def call(argv,env,deadline=40):
    process=subprocess.Popen(argv,env=env,stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.PIPE,start_new_session=True)
    buffers={};selector=selectors.DefaultSelector()
    try:
        for stream in (process.stdout,process.stderr):os.set_blocking(stream.fileno(),False);selector.register(stream,selectors.EVENT_READ);buffers[stream]=bytearray()
        end=time.monotonic()+deadline
        while selector.get_map():
            if time.monotonic()>=end:raise TimeoutError('CLI observation deadline')
            for key,_ in selector.select(.05):
                chunk=os.read(key.fileobj.fileno(),65536)
                if not chunk:selector.unregister(key.fileobj)
                else:
                    buffers[key.fileobj].extend(chunk)
                    if len(buffers[key.fileobj])>2097152:raise RuntimeError('CLI egress cap')
        code=process.wait(timeout=max(.01,end-time.monotonic()))
        return {'argv':argv,'exit_code':code,'stdout':buffers[process.stdout].decode(),'stderr':buffers[process.stderr].decode()}
    finally:
        if process.poll() is None:
            os.killpg(process.pid,signal.SIGKILL);process.wait(timeout=5)
        selector.close();process.stdout.close();process.stderr.close()

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--application',type=pathlib.Path,required=True);parser.add_argument('--worker',type=pathlib.Path,required=True);parser.add_argument('--runtime',type=pathlib.Path,required=True);parser.add_argument('--model',type=pathlib.Path,required=True);parser.add_argument('--output',type=pathlib.Path,required=True);parser.add_argument('--trace-startup',action='store_true');args=parser.parse_args()
    root=args.output.resolve();root.mkdir(mode=0o700);calls=[];server=None;success=False;supervision={'server_pid':None,'terminal':None,'success':False,'join_confirmed':False}
    try:
        identity=lambda path,label:{'artifact_id':label,'sha256':digest(path),'byte_len':path.stat().st_size}
        runtime=identity(args.runtime,'pinned-runtime');model=identity(args.model,'pinned-model')
        assert runtime['sha256']=='f332257812cf358954a1701c9fb205e5ac50b4fbdf0e2c09944cc25ea58459ae'
        assert model['sha256']=='9465e63a22add5354d9bb4b99e90117043c7124007664907259bd16d043bb031'
        policy={'schema_version':1,'approve_inference':True,'worker_sha256':digest(args.worker),'worker_bytes':args.worker.stat().st_size,'runtime_path':str(args.runtime.resolve()),'runtime':runtime,'model_path':str(args.model.resolve()),'model':model,'cpu_threads':4,'wall_time_ms':45000,'worker_memory_bytes':2147483648}
        policy_path=root/'owner-policy.json';policy_path.write_text(json.dumps(policy));policy_path.chmod(0o600)
        env={'PATH':'/usr/bin:/bin','HOME':os.environ['HOME'],'TMPDIR':str(root),'PULSAR_STATE_DIR':str(root/'state'),'PULSAR_WORKER_EXECUTABLE':str(args.worker.resolve()),'PULSAR_ASSISTANT_OWNER_POLICY':str(policy_path)}
        def cli(*words,expect=0):
            result=call([str(args.application.resolve()),*words],env);calls.append(result);assert result['exit_code']==expect,result['stderr'];return json.loads(result['stdout']) if result['stdout'].lstrip().startswith('{') else result['stdout']
        log=(root/'server.log').open('wb')
        server_argv=[str(args.application.resolve()),'engine']
        if args.trace_startup:server_argv=['strace','-f','-tt','-T','-e','trace=fsync,fdatasync,flock,syncfs,sync','-o',str(root/'startup-sync-lock.trace'),*server_argv]
        server=subprocess.Popen(server_argv,env=env,stdin=subprocess.DEVNULL,stdout=log,stderr=log,start_new_session=True);log.close()
        supervision['server_pid']=server.pid
        supervision['boot_id']=pathlib.Path('/proc/sys/kernel/random/boot_id').read_text().strip()
        try:supervision['start_ticks']=pathlib.Path(f'/proc/{server.pid}/stat').read_text().rsplit(')',1)[1].split()[19]
        except OSError:supervision['start_ticks']=None
        (root/'supervision.json').write_text(json.dumps(supervision)+'\n')
        end=time.monotonic()+50
        while not (root/'state/engine.sock').exists():
            assert server.poll() is None,'engine exited before ready';assert time.monotonic()<end,'listener readiness deadline';time.sleep(.05)
        project=cli('project','create','Assistant CLI qualification')['value']['project_id']
        profiles=cli('assistant','profiles',project)['value'];assert len(profiles)==1
        before=cli('project','open',project)['value'];intent=root/'start-intent.json';request='cli-proposal-'+uuid.uuid4().hex
        cli('assistant','prepare',project,'--profile',profiles[0]['profile_id'],'--revision','0','--prompt','Please set generation sample stride to 8.','--request-id',request,'--output',str(intent))
        saved=intent.read_bytes();assert intent.stat().st_mode&0o777==0o600
        refused=call([str(args.application.resolve()),'assistant','prepare',project,'--profile',profiles[0]['profile_id'],'--revision','0','--prompt','different','--request-id',request,'--output',str(intent)],env);calls.append(refused);assert refused['exit_code']!=0 and intent.read_bytes()==saved
        accepted=cli('assistant','start','--intent',str(intent),'--approve-inference')['value'];assert accepted['request_id']==request
        end=time.monotonic()+60
        while True:
            report=cli('assistant','status',project,accepted['attempt_id'])['value']
            if report['state']!='running':break
            assert time.monotonic()<end,'assistant status deadline';time.sleep(.1)
        assert report['state']=='completed',report
        assert report['proposal']=={'kind':'set_sample_stride','stride':8},report
        assert cli('project','open',project)['value']==before
        replay=cli('assistant','start','--intent',str(intent),'--approve-inference')['value'];assert replay['attempt_id']==accepted['attempt_id']
        entries=cli('assistant','list',project)['value'];assert len(entries)==1 and entries[0]['state']=='completed'
        resolved=cli('assistant','resolve','--intent',str(intent),'--acknowledge-withdrawing-unpublished')['value'];assert resolved['outcome']['state']=='accepted' and resolved['outcome']['report']['attempt_id']==accepted['attempt_id']
        negative=root/'unpublished-intent.json';negative_id='unpublished-'+uuid.uuid4().hex
        cli('assistant','prepare',project,'--profile',profiles[0]['profile_id'],'--revision','0','--prompt','No action','--request-id',negative_id,'--output',str(negative))
        withdrawal=cli('assistant','resolve','--intent',str(negative),'--acknowledge-withdrawing-unpublished')['value'];assert withdrawal['outcome']['state']=='withdrawn'
        repeated=cli('assistant','resolve','--intent',str(negative),'--acknowledge-withdrawing-unpublished')['value'];assert repeated==withdrawal
        cli('assistant','start','--intent',str(negative),'--approve-inference',expect=1)
        assert len(cli('assistant','list',project)['value'])==1
        cancelled=cli('assistant','cancel',project,accepted['attempt_id'],'--request-id','terminal-cancel-'+uuid.uuid4().hex)['value'];assert cancelled['state']=='completed'
        (root/'result.json').write_text(json.dumps({'scope':'Real CLI/private owner setup/IPC/learned proposal; no application effects or qualification','report':report,'unchanged_project':True,'exact_replay':True,'non_overwrite':True,'terminal_cancel_preserved':True,'accepted_resolution':True,'permanent_unpublished_withdrawal':True,'withdrawal_replay':True,'withdrawn_start_refused':True,'application_sha256':digest(args.application),'worker_sha256':policy['worker_sha256']},indent=2)+'\n');success=True
    except BaseException as error:
        supervision['failure']=str(error)
        raise
    finally:
        if server is not None:
            if server.poll() is None:
                try:
                    os.killpg(server.pid,signal.SIGTERM)
                    try:server.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        os.killpg(server.pid,signal.SIGKILL)
                        try:server.wait(timeout=5)
                        except subprocess.TimeoutExpired:supervision['shutdown_failure']='owned process retirement unconfirmed after TERM/KILL bounded waits'
                except ProcessLookupError:pass
            supervision['terminal']=server.poll()
            supervision['join_confirmed']=supervision['terminal'] is not None
        supervision['success']=success
        (root/'calls.json').write_text(json.dumps(calls,indent=2)+'\n')
        (root/'supervision.json').write_text(json.dumps(supervision)+'\n')
        if success and not supervision['join_confirmed']:raise RuntimeError('owned server retirement is unconfirmed')
if __name__=='__main__':main()
