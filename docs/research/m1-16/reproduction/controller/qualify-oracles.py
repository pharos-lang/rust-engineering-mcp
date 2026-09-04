import sys,json,os,hashlib,time,threading
from pathlib import Path
sys.dont_write_bytecode=True
from broker import Driver
R=Path(__file__).resolve().parents[2];OUT=R/'target/m1-16-oracles';OUT.mkdir(mode=0o700);(OUT/'raw-state').mkdir(mode=0o700)
config={'mode':'raw','server_binary':str(R/'target/m1-15-candidate/local/bin/rust-engineering-mcp'),'root':str(OUT),'state_root':str(OUT/'raw-state'),'docker_socket':'<LOCAL_HOME>/.docker/run/docker.sock'}
d=Driver(config);rows=[];receipt={'status':'running','driver_binary_sha256':hashlib.file_digest((R/'target/debug/m1-16-trusted-driver').open('rb'),'sha256').hexdigest(),'cases':rows};cancel=threading.Event()
def source(task,variant):
 p=R/'target/m1-16-corpus/repair'/task
 return [{'path':name,'text':(p/'initial'/name).read_text()} for name in ('Cargo.toml','Cargo.lock')]+[{'path':'src/lib.rs','text':(p/variant/'src/lib.rs').read_text()},{'path':'tests/behavior.rs','text':(p/'hidden/behavior.rs').read_text()}]
try:
 for task,stage,diagnostic in [('R01','check','E0502'),('R02','check','E0597'),('R03','clippy','useless_vec'),('R04','test',None)]:
  for variant,stages in [('initial',[stage]),('reference',['fmt','check','clippy','test'])]:
   files=source(task,variant)
   for command in stages:
    start=time.monotonic();out=d.request({'op':'execute','files':files,'command':command},cancel)
    passed=out['termination']=='exited' and out['exit_code']==0
    row={'task':task,'variant':variant,'command':command,'passed':passed,'elapsed_seconds':time.monotonic()-start,'source_files':[{'path':f['path'],'sha256':hashlib.sha256(f['text'].encode()).hexdigest()} for f in files],'result':out};rows.append(row)
    (OUT/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n');print(task,variant,command,'exit',out['exit_code'],flush=True)
    assert out['termination']=='exited',out
    assert passed==(variant=='reference'),out
    if variant=='initial' and diagnostic:assert diagnostic in out['stdout']+out['stderr'],out
 receipt['status']='passed'
finally:
 receipt['driver_cleanup']=d.close();(OUT/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
