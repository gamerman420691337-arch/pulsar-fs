"""Actual isolated CLI/SessionClient/Engine/worker axis-pattern exercise.

Use private RAM TMPDIR and a network-disabled namespace. Output must be empty.
No physical device or rendered-window effects; all motion is explicitly synthetic.
"""
import argparse,hashlib,json,os,pathlib,subprocess,tempfile,time,shutil,sqlite3,math
p=argparse.ArgumentParser()
for n in ('binary','controller','runtime','output'):p.add_argument('--'+n,type=pathlib.Path,required=True)
a=p.parse_args();assert all(v.is_absolute() for v in vars(a).values());assert a.output.is_dir() and not any(a.output.iterdir()), 'empty owned output directory required';root=pathlib.Path(tempfile.mkdtemp(prefix='axis-patterns-',dir=os.environ['TMPDIR']));records=[];engine=None;log=None
axes=['stroke','sway','surge','roll','pitch','yaw'];text='\n'.join(f'pattern=triangle axis={axis} duration=2s frequency=1hz amplitude={0.02*(i+1):.2f} offset=0.5 sample_hz=8' for i,axis in enumerate(axes))+'\n'
env={'PATH':'/usr/bin:/bin','PULSAR_STATE_DIR':str(root/'state'),'PULSAR_FFMPEG':str(a.runtime/'runtimes/ffmpeg'),'PULSAR_FFPROBE':str(a.runtime/'runtimes/ffprobe'),'PULSAR_ORT_DYLIB':str(a.runtime/'runtimes/libonnxruntime.so.1')}
def cli(args,good=True):
 q=subprocess.run([str(a.binary),*args],env=env,capture_output=True,text=True,timeout=90);records.append({'args':args,'exit_code':q.returncode,'stdout':q.stdout,'stderr':q.stderr});assert not good or q.returncode==0,q.stderr;return q
def value(args):return json.loads(cli(args).stdout)['value']
def check(program):
 assert [t['axis'] for t in program['tracks']]==axes
 for i,t in enumerate(program['tracks']):
  assert len(t['actions'])==17 and not t.get('gaps')
  for k,action in enumerate(t['actions']):
   wave=[0,.5,1,.5,0,-.5,-1,-.5][k%8]
   expected=.5+.02*(i+1)*wave
   assert action['time']==k*125000000 and action['evidence']=='synthesized' and abs(action['position']-expected)<1e-12,action
try:
 log=(root/'engine.log').open('w');engine=subprocess.Popen([str(a.binary),'engine'],env=env,stdout=subprocess.DEVNULL,stderr=log)
 deadline=time.monotonic()+30
 while not (root/'state/engine.sock').exists():assert engine.poll() is None and time.monotonic()<deadline;time.sleep(.02)
 project=value(['project','create','Axis pattern fixture']);pid=project['project_id'];rev=project['revision'];assert project['program']['tracks']==[]
 file=root/'patterns.txt';file.write_text(text)
 candidate=value(['pattern-candidate',pid,str(file),'--revision',str(rev)]);check(candidate['program']);assert candidate['base_revision']==rev
 untouched=value(['project','open',pid]);assert untouched['revision']==rev and untouched['program']==project['program']
 for bad in (text.replace('axis=yaw','axis=stroke'),text.replace('axis=yaw duration=2s','axis=yaw duration=3s')):
  file.write_text(bad);before=sqlite3.connect(root/'state/projects.sqlite3').execute('SELECT COUNT(*) FROM jobs').fetchone()[0];assert cli(['pattern-candidate',pid,str(file),'--revision',str(rev)],False).returncode!=0;after=sqlite3.connect(root/'state/projects.sqlite3').execute('SELECT COUNT(*) FROM jobs').fetchone()[0];assert before==after
 file.write_text(text);assert cli(['pattern-candidate',pid,str(file),'--revision',str(rev+999)],False).returncode!=0
 committed=value(['commit-candidate',pid,candidate['candidate_id'],'--revision',str(rev)]);check(committed['program'])
 for i,axis in enumerate(axes):
  out=root/f'{axis}.funscript';cli(['export-axis',pid,str(out),'--axis',axis]);actual=json.loads(out.read_text())['actions'];expected=[]
  for k in range(17):
   wave=[0,.5,1,.5,0,-.5,-1,-.5][k%8]
   expected.append({'at':k*125,'pos':int(round(50+2*(i+1)*wave))})
  assert actual==expected,(axis,actual,expected)
 controller=subprocess.run([str(a.controller),str(root/'state'),pid,str(file)],env=env,capture_output=True,text=True,timeout=90);records.append({'controller':str(a.controller),'exit_code':controller.returncode,'stdout':controller.stdout,'stderr':controller.stderr});assert controller.returncode==0,controller.stderr;control=json.loads(controller.stdout);assert control['undo_redo']=='PASS' and control['pattern_program']==candidate['program']
 engine.terminate();engine.wait(timeout=10);log.close();log=None
 log=(root/'restart.log').open('w');engine=subprocess.Popen([str(a.binary),'engine'],env=env,stdout=subprocess.DEVNULL,stderr=log);deadline=time.monotonic()+30
 while True:
  assert engine.poll() is None and time.monotonic()<deadline
  opened=cli(['project','open',pid],False)
  if opened.returncode==0:restored=json.loads(opened.stdout)['value'];break
  time.sleep(.05)
 check(restored['program']);assert restored['program']==committed['program']
 with sqlite3.connect(root/'state/projects.sqlite3') as db:
  jobs=db.execute('SELECT id,state FROM jobs').fetchall()
 assert len(jobs)==2 and all(state=='completed' for _,state in jobs),jobs
 result={'qualified':False,'axes':axes,'independently_checked_actions':102,'selected_axis_exports':6,'master_untouched_before_explicit_commit':True,'invalid_inputs_no_jobs':True,'revision_conflict_refused':True,'cli_controller_agreement':True,'undo_redo_restart':'PASS','pattern_input_sha256':hashlib.sha256(text.encode()).hexdigest(),'jobs':jobs,'scope':'actual CLI/SessionClient/Engine/worker synthetic patterns; no rendered desktop, real target or physical qualification'}
 a.output.mkdir(parents=True,exist_ok=True);(a.output/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result))
finally:
 if engine is not None and engine.poll() is None:
  engine.terminate()
  try:engine.wait(timeout=10)
  except subprocess.TimeoutExpired:engine.kill();engine.wait(timeout=5)
 if log:log.close()
 a.output.mkdir(parents=True,exist_ok=True);(a.output/'commands.json').write_text(json.dumps(records,indent=2)+'\n')
 for n in ('engine.log','restart.log'):
  if (root/n).exists():shutil.copy2(root/n,a.output/n)
 shutil.rmtree(root)
