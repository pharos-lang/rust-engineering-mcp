"""Disjoint live model+broker+gateway calibration, excluded from utility study."""
import sys,json,threading,hashlib,time
from pathlib import Path
sys.dont_write_bytecode=True
import broker,participant,containment
R=Path(__file__).resolve().parents[2]
OUT=R/'target/m1-16-integrated-qualification'/str(time.time_ns());OUT.mkdir(mode=0o700,parents=True)
base=json.loads((R/'target/m1-16-benchmark/host-config.private.json').read_text())
for key in ['mode','root','state_root']:base.pop(key,None)
base.update(rustsec_path=str(R/'target/m1-16-runtime/rustsec.json'),rustsec_sha256=json.loads((R/'target/m1-16-runtime/receipt.json').read_text())['rustsec_sha256'])
projection=json.loads((R/'target/m1-16-driver/research-output/baseline-projection.json').read_text())
files={'Cargo.toml':'[package]\nname = "m116_calibration_canary"\nversion = "0.1.0"\nedition = "2024"\n','Cargo.lock':'version = 4\n\n[[package]]\nname = "m116_calibration_canary"\nversion = "0.1.0"\n','src/lib.rs':'pub fn canary() -> u32 {\n    1\n}\n'}
for arm in ['A','B']:
 out=OUT/arm;out.mkdir(mode=0o700);state=out/'state';state.mkdir(mode=0o700)
 receipt={'purpose':'disjoint infrastructure calibration, not utility run','arm':arm,'status':'running'}
 receipt['docker_before']=containment.observe(base['docker_socket'],out);assert receipt['docker_before']['absent']
 ws=broker.Workspace(out,files);driver=instance=None
 try:
  driver=broker.Driver(dict(base,mode='raw' if arm=='A' else 'mcp',root=str(ws.root),state_root=str(state),stderr_path=str(out/'server.stderr')))
  instance=broker.Broker(arm,ws,driver,projection,allow_project_code=True,strict_clippy=True)
  prompt='Disjoint infrastructure calibration only. Read src/lib.rs with read_project_file, submit_patch a complete properly formatted replacement that changes canary to return 2, then validate the submitted source. No extra function or test. '
  prompt+=('Use raw_validate stage quality exactly once.' if arm=='A' else 'Use rust_project_open with '+str(ws.root)+' then rust_quality_gate profile standard exactly once, with the returned project_ref.')
  prompt+=' Only the supplied tools. Six candidates/validations,64 calls, test<=30s,strict Clippy. Report the actual gate outcome briefly.'
  receipt['participant']=participant.run_participant(prompt,instance.tools(threading.Event()),instance.handle,out/'participant',wall_seconds=180,max_output_tokens=3000)
  receipt['source_final']=ws.read('src/lib.rs')
  receipt['status']='completed'
 finally:
  if instance:receipt['broker']=instance.receipt()
  if driver:receipt['cleanup']=driver.close()
  ws.close();receipt['docker_after']=containment.observe(base['docker_socket'],out)
  receipt['identities']={str(p.relative_to(R)):hashlib.sha256(p.read_bytes()).hexdigest() for p in [Path(__file__),Path(participant.__file__),Path(broker.__file__),Path(containment.__file__),broker.BINARY]}
  (out/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
 assert receipt['status']=='completed' and receipt['participant']['status']=='completed' and not receipt['participant']['infrastructure_failed'],out
 assert not receipt['cleanup']['cleanup_failed'] and receipt['docker_after']['absent'],out
 assert '    2\n' in receipt['source_final'] and len(receipt['broker']['candidates'])==1,out
 vals=receipt['broker']['validation_requests'];assert len(vals)==1,out
 result=vals[0]['result']
 if arm=='A':assert all(s['result']['exit_code']==0 for s in result['stages']),out
 else:assert result['structuredContent']['status']=='passed',out
 print('integrated',arm,'PASS',out,flush=True)
