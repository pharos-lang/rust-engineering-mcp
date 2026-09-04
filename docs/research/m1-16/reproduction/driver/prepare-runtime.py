import importlib.util,json,hashlib,os,shutil,sys,time
from pathlib import Path
sys.dont_write_bytecode=True
R=Path(__file__).resolve().parents[2]; OUT=R/'target/m1-16-runtime';OUT.mkdir(mode=0o700)
for name in ('catalog','index','trust'): (OUT/name).mkdir(mode=0o700)
emit=R/'target/m1-16-driver/research-output';trust=OUT/'trust/trust.json';shutil.copyfile(emit/'trust.json',trust);trust.chmod(0o600)
spec=importlib.util.spec_from_file_location('g',R/'scripts/test-doctor.py');g=importlib.util.module_from_spec(spec);spec.loader.exec_module(g)
binary=R/'target/m1-15-candidate/local/bin/rust-engineering-mcp';base=['/usr/bin/sandbox-exec','-p','(version 1) (allow default) (deny network*)',str(binary),'catalog'];flags=['--store',str(OUT/'catalog'),'--trust',str(trust)]
receipt={'binary_sha256':hashlib.file_digest(binary.open('rb'),'sha256').hexdigest(),'host_network_profile':'(version 1) (allow default) (deny network*)','steps':[]}
for label,args in [('import',['import',str(emit/'research.tar.zst'),*flags,'--json']),('rebuild',['rebuild-index',*flags,'--index-store',str(OUT/'index'),'--model-dir','/private/tmp/rust-mcp-e5-m009/onnx','--json'])]:
 start=time.monotonic();c=g.Capture(base+args,{},OUT,1024*1024)
 try:
  exit=c.finish(start+600);c.bounded();out=json.loads(c.data['stdout']);assert exit==0 and out['status']=='passed' and not c.data['stderr'],(exit,out)
  receipt['steps'].append({'label':label,'seconds':time.monotonic()-start,'exit_code':exit,'output':out});print(label+' passed',flush=True)
 finally:c.force_stop();c.save(OUT,label)
now=int(time.time());doc={'format_version':1,'sequence':1,'source_id':'research-fixture-m1-16-std-only-audit-not-global-rustsec','created_at':now,'observed_at':now,'records':[{'path':'crates/rsa/RUSTSEC-2023-0071.md','markdown':(R/'crates/catalog-adapter/tests/fixtures/rustsec/RUSTSEC-2023-0071.md').read_text()}]}
p=OUT/'rustsec.json';p.write_text(json.dumps(doc)+'\n');p.chmod(0o600);receipt['rustsec_sha256']='sha256:'+hashlib.sha256(p.read_bytes()).hexdigest();receipt['rustsec_scope']='one existing advisory fixture; zero third-party packages in immutable std-only corpus, not global advisory coverage';receipt['status']='passed';(OUT/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
