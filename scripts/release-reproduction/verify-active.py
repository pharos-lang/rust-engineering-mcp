import importlib.util,json,os,time,hashlib,signal,socket,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]; OUT=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('gate',ROOT/'scripts/test-doctor.py'); g=importlib.util.module_from_spec(spec); spec.loader.exec_module(g)
profile='(version 1) (allow default) (deny network-outbound (remote ip "*:*"))'
for family,address in ((socket.AF_INET,('127.0.0.1',0)),(socket.AF_INET6,('::1',0))):
 with socket.socket(family,socket.SOCK_STREAM) as listener:
  listener.bind(address); listener.listen(); port=listener.getsockname()[1]
  with socket.socket(family,socket.SOCK_STREAM) as client: client.connect(listener.getsockname())
  code=f'import socket\ns=socket.socket({int(family)},socket.SOCK_STREAM)\ntry: s.connect(({address[0]!r},{port}))\nexcept PermissionError: pass\nelse: raise SystemExit("IP deny not enforced")\n'
  g.run(['/usr/bin/sandbox-exec','-p',profile,sys.executable,'-c',code],{},OUT,10)
config=OUT/'active-docker-config';config.mkdir(mode=0o700);(config/'config.json').write_text('{}\n')
docker=[g.DOCKER,'--config',str(config),'--host','unix://<LOCAL_HOME>/.docker/run/docker.sock']
def cleanup():
 for kind in ('container','volume'):
  args=[kind,'ls']+(['--all'] if kind=='container' else [])+['--filter','label=org.rust-mcp.execution=true','--format','{{.ID}}']
  g.require(not g.run(docker+args,{},OUT,15).strip(),'Owned Docker objects remain')
cleanup(); rows=[]
receipt={'scope':'local-release-candidate-active-doctor','network_profile':profile,'controls':'IPv4/IPv6 connect succeeds outside profile and PermissionError inside; Unix socket allowed','status':'running','results':rows}
try:
 for feature in ('core','local'):
  binary=OUT/'installation'/feature/'bin/rust-engineering-mcp'; state=OUT/('active-state-'+feature);state.mkdir(mode=0o700)
  flags=['--catalog-store',str(OUT/'installation'/feature/'catalog'),'--catalog-trust',str(OUT/'installation'/feature/'trust/fixture-trust.json')]
  if feature=='local': flags+=['--catalog-model-dir',str(OUT/'installation/local/model')]
  command=['/usr/bin/sandbox-exec','-p',profile,str(binary),'doctor','--active','--json','--docker',g.DOCKER,'--docker-socket','<LOCAL_HOME>/.docker/run/docker.sock','--rust-image',g.IMAGE,'--state-root',str(state),*flags]
  start=time.monotonic(); cap=g.Capture(command,{},OUT,256*1024);forced=False
  try:
   code=cap.finish(start+1200);report=g.report_from(cap);g.require(code==0,'active doctor nonzero');g.validate_success(report);g.require(not cap.data['stderr'],'unexpected stderr');cleanup()
   rows.append({'feature':feature,'exit_code':code,'elapsed_seconds':time.monotonic()-start,'binary_sha256':hashlib.file_digest(binary.open('rb'),'sha256').hexdigest(),'report':report,'cleanup':True})
   print('PASS release active '+feature,flush=True)
  finally:
   if cap.child.poll() is None:
    cap.send(signal.SIGINT)
    try:cap.finish(time.monotonic()+300)
    except RuntimeError:forced=True
   cap.force_stop();cap.save(OUT,'release-active-'+feature)
   if forced:raise RuntimeError('forced cleanup; failed')
 receipt['status']='passed'
finally:
 receipt['script_sha256']=hashlib.sha256(Path(__file__).read_bytes()).hexdigest();(OUT/'active-release-receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
