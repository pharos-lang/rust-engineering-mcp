"""Trusted actual SDK/gateway qualification, never participant inference."""
import sys,os,json,hashlib,threading,time,subprocess,signal,socket,errno
from pathlib import Path
sys.dont_write_bytecode=True
R=Path(__file__).resolve().parent.parent
sys.path.insert(0,str(R/'target/m1-16-controller'))
from broker import Driver,HOST_PROFILE
import containment
OUT=R/'target/m1-16-driver-qualification';OUT.mkdir(mode=0o700,exist_ok=True)
OUT=OUT/('run-'+str(time.time_ns()));OUT.mkdir(mode=0o700)
BASE=json.loads((R/'target/m1-16-benchmark/host-config.private.json').read_text())
BASE.update(rustsec_path=str(R/'target/m1-16-runtime/rustsec.json'),rustsec_sha256=json.loads((R/'target/m1-16-runtime/receipt.json').read_text())['rustsec_sha256'])
DOCKER='/Applications/Docker.app/Contents/Resources/bin/docker'
DC=OUT/'docker-config';DC.mkdir(mode=0o700,exist_ok=True);(DC/'config.json').write_text('{}\n')
def docker(args):
 p=subprocess.run([DOCKER,'--config',str(DC),'--host','unix://'+BASE['docker_socket'],*args],env={},capture_output=True,text=True,timeout=15);assert p.returncode==0,p.stderr;return p.stdout

def objects(kind):
 return docker([kind,'ls',*(['--all','--no-trunc'] if kind=='container' else []),'--filter','label=org.rust-mcp.execution=true','--filter','label=org.rust-mcp.rust-job','--format','{{json .}}']).splitlines()
def clean():
 assert not objects('container'),'containers remain';assert not objects('volume'),'volumes remain'
def save(name,value):
 (OUT/name).write_text(json.dumps(value,indent=2)+'\n')
def config(mode,root,label):
 state=OUT/(label+'-state');state.mkdir(mode=0o700);c=dict(BASE,mode=mode,root=str(root),state_root=str(state),stderr_path=str(OUT/(label+'.stderr')));return c
def write(path,text):
 path.parent.mkdir(mode=0o700,parents=True,exist_ok=True);fd=os.open(path,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600);os.write(fd,text.encode());os.close(fd)
def fixture(root,task,variant='reference'):
 base=R/'target/m1-16-corpus/repair'/task
 files=[]
 for name in ['Cargo.toml','Cargo.lock','src/lib.rs','tests/behavior.rs']:
  source=base/'hidden/behavior.rs' if name.startswith('tests/') else base/(variant if name=='src/lib.rs' else 'initial')/name
  text=source.read_text();write(root/name,text);files.append({'path':name,'text':text})
 return files
receipt={'status':'running','cases':[],'sdk':'rmcp 3.2.0','profile':HOST_PROFILE}
clean()
if '--cancel-only' not in sys.argv:
 root=OUT/'projects';root.mkdir(mode=0o700)
 for task in ['R01','R02','R03','R04']:fixture(root/task,task)
 fixture(root/'R01initial','R01','initial')
 receipt['input_files']={str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in root.rglob('*') if p.is_file()}
 d=Driver(config('mcp',root,'workflow'));cancel=threading.Event()
 def call(name,args,expected='passed'):
  start=time.monotonic();result=d.request({'op':'call','name':name,'arguments':args},cancel)
  print(name,args.get('project_ref',''),'',flush=True)
  row={'name':name,'arguments':args,'seconds':time.monotonic()-start,'result':result};receipt['cases'].append(row);save('workflow.json',receipt)
  payload=result.get('structuredContent',{});assert payload.get('status')==expected,row
  return payload['data']
 try:
  discovery=d.request({'op':'tools'},cancel);save('discovery.json',discovery);assert len(discovery['tools'])==13
  if '--discovery-only' in sys.argv:
   print(json.dumps({t['name']:t['inputSchema'] for t in discovery['tools']},indent=2))
  else:
   for task in ['R01','R02','R03','R04']:
    ref=call('rust.project.open',{'path':str(root/task)})['project_ref']
    for name in ['rust.project.inspect','rust.toolchain.inspect','rust.check','rust.fmt.check','rust.clippy','rust.test','rust.dependencies.audit','rust.quality.gate']:
     args={'project_ref':ref}
     if name=='rust.clippy':args['lint_profile']='strict'
     if name=='rust.test':args['timeout']=30
     if name=='rust.quality.gate':args['profile']='standard'
     data=call(name,args)
    if name=='rust.dependencies.audit':
     obs=data['observation'];assert obs['crates_io_scanned']==0 and obs['packages_total']==1 and obs['workspace_packages_excluded']==1 and obs['findings']==[] and obs['validation_complete']
   call('rust.diagnostics.explain',{'code':'E0502'})
   call('rust.catalog.status',{})
   call('rust.crate.search',{'query':'Unicode NFC','mode':'hybrid','limit':5})
   call('rust.crate.inspect',{'name':'unicode-normalization','section':'overview'})
   ref=call('rust.project.open',{'path':str(root/'R01initial')})['project_ref']
   failed=call('rust.check',{'project_ref':ref},'failed')
   save('failed-data.json',failed)
   def uris(v):
    if isinstance(v,dict):
     for x in v.values():yield from uris(x)
    elif isinstance(v,list):
     for x in v:yield from uris(x)
    elif isinstance(v,str) and v.startswith('rust-artifact://'):yield v
   found=set(uris(failed));assert found,'no resource URI'
   for uri in sorted(found):
    resource=d.request({'op':'resource','uri':uri},cancel);receipt['cases'].append({'resource':uri,'result':resource});assert resource.get('contents')
   receipt['status']='passed'
 finally:
  receipt['cleanup']=d.close();save('workflow.json',receipt);clean()

# Real gateway cancellation, serial fresh driver/server and job nonce per arm.
for mode in ['raw','mcp']:
 slow=OUT/('slow-'+mode);slow.mkdir(mode=0o700)
 files=fixture(slow,'R01')
 behavior='#[test]\nfn bounded_sleep() { std::thread::sleep(std::time::Duration::from_secs(20)); }\n'
 (slow/'tests/behavior.rs').write_text(behavior)
 for f in files:
  if f['path']=='tests/behavior.rs':f['text']=behavior
 owner=Driver(config(mode,slow,'cancel-'+mode));event=threading.Event();row={'mode':mode,'signal':'SIGINT via broker cancellation','status':'running','source_hashes':{f['path']:hashlib.sha256(f['text'].encode()).hexdigest() for f in files}}
 result={};worker=None
 try:
  if mode=='raw':
   warm=owner.request({'op':'execute','files':files,'command':'check'},event);assert warm['exit_code']==0
   request={'op':'execute','files':files,'command':'test'}
  else:
   owner.request({'op':'tools'},event)
   opened=owner.request({'op':'call','name':'rust.project.open','arguments':{'path':str(slow)}},event)
   ref=opened['structuredContent']['data']['project_ref']
   warm=owner.request({'op':'call','name':'rust.check','arguments':{'project_ref':ref}},event);assert warm['structuredContent']['status']=='passed'
   request={'op':'call','name':'rust.test','arguments':{'project_ref':ref,'timeout':30}}
  def invoke():
   try:result['response']=owner.request(request,event)
   except BaseException as exc:result['failure']=type(exc).__name__
  worker=threading.Thread(target=invoke);worker.start();deadline=time.monotonic()+25;observed=None
  while time.monotonic()<deadline and worker.is_alive():
   for line in objects('container'):
    obj=json.loads(line)
    if 'test' in obj.get('Command','') and obj.get('State')=='running':
     top=docker(['top',obj['ID'],'-eo','pid,args'])
     if 'cargo' in top and 'test' in top:observed={'container':obj,'top':top};break
   if observed:break
   time.sleep(.05)
  assert observed,'actual running cargo test not observed'
  row['observed_running_cargo']=observed
  row['active_read_only_observer']=containment.observe(BASE['docker_socket'],OUT)
  assert not row['active_read_only_observer']['absent'] and all(row['active_read_only_observer']['objects'].values()),row
  start=time.monotonic();event.set();worker.join(timeout=310)
  assert not worker.is_alive(),'driver cancellation worker not joined'
  row['cancel_join_seconds']=time.monotonic()-start;row['outcome']=result;row['cleanup']=owner.cleanup
  assert owner.cleanup and owner.cleanup['execution_joined'] and not owner.cleanup['cleanup_failed'],row
  if mode=='mcp':assert owner.cleanup['server_joined'],row
  clean();row['docker_objects_absent']=True;row['status']='passed'
 except BaseException as exc:
  row['failure_kind']=type(exc).__name__;row['failure_code']=str(exc)[:1000]
  raise
 finally:
  event.set()
  if worker is not None and worker.is_alive():worker.join(timeout=310)
  if worker is not None and worker.is_alive():raise RuntimeError('qualification_worker_not_joined')
  if owner.cleanup is None:owner.cancel_and_join()
  row['cleanup']=owner.cleanup;save('cancel-'+mode+'.json',row)
 print('cancel',mode,row['status'],flush=True)

controls=[]
for family,host in [(socket.AF_INET,'127.0.0.1'),(socket.AF_INET6,'::1')]:
 with socket.socket(family,socket.SOCK_STREAM) as listener:
  listener.bind((host,0));listener.listen(4);port=listener.getsockname()[1]
  code='import socket,json\ns=socket.socket('+str(int(family))+',socket.SOCK_STREAM);s.settimeout(2)\ntry:\n s.connect(('+repr(host)+','+str(port)+'));print(json.dumps({"connected":True}))\nexcept OSError as e:print(json.dumps({"connected":False,"errno":e.errno}))\nfinally:s.close()'
  def probe(prefix):
   p=subprocess.run(prefix+[sys.executable,'-c',code],capture_output=True,text=True,env={},timeout=5);assert p.returncode==0;return json.loads(p.stdout)
  outside=probe([]);inside=probe(['/usr/bin/sandbox-exec','-p',HOST_PROFILE]);assert outside['connected'] and not inside['connected'] and inside['errno'] in [errno.EPERM,errno.EACCES]
  controls.append({'family':int(family),'outside':outside,'inside':inside})
save('network-controls.json',{'status':'passed','controls':controls,'profile':HOST_PROFILE})
print('qualification output',OUT,flush=True)
