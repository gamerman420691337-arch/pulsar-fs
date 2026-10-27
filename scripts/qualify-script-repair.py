#!/usr/bin/env python3
"""Actual standalone repair and history checks with authored neutral data."""
import argparse,hashlib,json,re,shutil,sqlite3,subprocess,time
from pathlib import Path
def identity(path):
    with path.open("rb") as f:return {"sha256":hashlib.file_digest(f,"sha256").hexdigest(),"byte_len":path.stat().st_size}
def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ["app","controller","output","evidence"]:p.add_argument("--"+name,type=Path,required=True)
    p.add_argument("--desktop-controller",type=Path)
    args=p.parse_args();root=args.output.absolute();root.mkdir(mode=0o700);evidence=args.evidence.absolute();evidence.mkdir(mode=0o700)
    app=args.app.resolve(strict=True);state=root/"state";env={"PATH":"/usr/bin:/bin","HOME":str(root),"TMPDIR":str(root),"PULSAR_STATE_DIR":str(state)}
    raw={"actions":[{"at":40,"pos":150},{"at":0,"pos":-5},{"at":20,"pos":2},{"at":10,"pos":1},{"at":20,"pos":100},{"at":100,"pos":50}]}
    source=root/"authored.funscript";source.write_text(json.dumps(raw)+"\n");pin=identity(source);records=[];engine=None;log=(root/"engine.stderr").open("w")
    def invoke(arguments,success=True):
        result=subprocess.run([str(app)]+list(map(str,arguments)),env=env,stdin=subprocess.DEVNULL,capture_output=True,text=True,timeout=45)
        assert len(result.stdout)+len(result.stderr)<1024*1024
        record={"arguments":list(map(str,arguments)),"exit":result.returncode,"stdout":result.stdout,"stderr":result.stderr};records.append(record);(root/f"cli-{len(records):03}.json").write_text(json.dumps(record,indent=2)+"\n")
        assert (result.returncode==0)==success,record
        return result
    def retain():
        for path in root.iterdir():
            if path.is_file() and path.suffix in [".json",".funscript",".stderr"]:shutil.copyfile(path,evidence/path.name)
    try:
        engine=subprocess.Popen([str(app),"engine"],env=env,stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=log);deadline=time.monotonic()+15
        while not (state/"engine.sock").exists():assert engine.poll() is None and time.monotonic()<deadline;time.sleep(0.025)
        result=invoke(["fix",source]);fixed=root/"authored.fixed.funscript"
        assert json.loads(fixed.read_text())["actions"]==[{"at":t,"pos":x} for t,x in [(0,0),(20,9),(40,18),(100,45)]]
        project=re.search(r"Repair project retained: ([A-Za-z0-9_.:-]+)\.",result.stderr).group(1)
        controller=subprocess.run([str(args.controller),str(state),project],env=env,stdin=subprocess.DEVNULL,capture_output=True,text=True,timeout=30)
        (root/"controller.json").write_text(json.dumps({"exit":controller.returncode,"stdout":controller.stdout,"stderr":controller.stderr},indent=2)+"\n");assert controller.returncode==0,controller.stderr
        # Read only the owned fixture DB; no private source data or auth values retained.
        with sqlite3.connect(f"file:{state/'projects.sqlite3'}?mode=ro",uri=True) as db:
            hashes=db.execute("SELECT sha256 FROM sources WHERE project=?",(project,)).fetchall();assert hashes==[(pin['sha256'],)]
            receipts=db.execute("SELECT e.authored_count,e.inherited_count FROM edit_candidate_lineage e JOIN candidates c ON c.id=e.candidate WHERE c.project=?",(project,)).fetchall();assert len(receipts)==1 and receipts[0][0]>0
            (root/"lineage.json").write_text(json.dumps({"raw_source_sha256":hashes[0][0],"edit_receipt_counts":receipts},indent=2)+"\n")
        invoke(["import-script",source],success=False)
        alternate=root/"slow.funscript";invoke(["fix",source,"--max-speed","100","--output",alternate])
        assert json.loads(alternate.read_text())["actions"]==[{"at":t,"pos":x} for t,x in [(0,0),(20,2),(40,4),(100,10)]]
        no_jitter=root/"no-jitter.funscript";invoke(["fix",source,"--jitter-window","0","--output",no_jitter])
        assert json.loads(no_jitter.read_text())["actions"]==[{"at":t,"pos":x} for t,x in [(0,0),(10,1),(20,5),(40,14),(100,41)]]
        def project_count():
            with sqlite3.connect(f"file:{state/'projects.sqlite3'}?mode=ro",uri=True) as db:return db.execute("SELECT COUNT(*) FROM projects").fetchone()[0]
        before_refusals=project_count()
        for extra in [["--max-speed","0"],["--max-speed","NaN"],["--jitter-window","-1"],["--jitter-threshold","101"]]:invoke(["fix",source,*extra,"--output",root/"refused.funscript"],success=False)
        invoke(["fix",source,"--output",source],success=False);invoke(["fix",source],success=False)
        assert identity(source)==pin and not (root/"refused.funscript").exists()
        assert project_count()==before_refusals,"policy/destination refusal created a project"
        (root/"preflight-effects.json").write_text(json.dumps({"before":before_refusals,"after":project_count(),"refusals":6})+"\n")
        for name,raw in [("negative",{"actions":[{"at":-1,"pos":0}]}),("empty",{"actions":[]})]:
            path=root/f"{name}.funscript";path.write_text(json.dumps(raw));invoke(["fix",path],success=False);assert not path.with_name(name+".fixed.funscript").exists()
        if args.desktop_controller is not None:
            desktop=subprocess.run([str(args.desktop_controller),str(state),str(source)],env=env,stdin=subprocess.DEVNULL,capture_output=True,text=True,timeout=30)
            (root/"desktop-controller.json").write_text(json.dumps({"exit":desktop.returncode,"stdout":desktop.stdout,"stderr":desktop.stderr},indent=2)+"\n");assert desktop.returncode==0,desktop.stderr
        (root/"result.json").write_text(json.dumps({"qualified":False,"app":identity(app),"controller":identity(args.controller),"raw_source":pin,"native_repairs":3,"history_and_lineage":True,"refusals":9,"commands":len(records),"physical_device_used":False,"desktop_shared_controller":args.desktop_controller is not None},indent=2)+"\n")
    finally:
        if engine is not None and engine.poll() is None:
            engine.terminate()
            try:engine.wait(timeout=5)
            except subprocess.TimeoutExpired:engine.kill();engine.wait(timeout=5)
        log.close();(root/"custody.json").write_text(json.dumps({"engine_joined":engine is None or engine.poll() is not None})+"\n");retain()
if __name__=="__main__":main()
