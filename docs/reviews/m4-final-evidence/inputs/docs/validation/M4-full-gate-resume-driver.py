from pathlib import Path
import importlib.util, json, hashlib, os, subprocess, sys, datetime
ROOT=Path('/Users/cburgosro/Projects/rust-mcp');os.chdir(ROOT)
spec=importlib.util.spec_from_file_location('gate',ROOT/'scripts/gate.py');gate=importlib.util.module_from_spec(spec);spec.loader.exec_module(gate)
source=ROOT/'docs/validation/M4-hardening-attempts/full-attempt-2/receipt.json'
report=json.loads(source.read_text());assert report['mode']=='full' and report['status']=='failed'
assert len(report['steps'])==28 and report['steps'][-1]['name']=='semantic' and report['steps'][-1]['status']=='failed'
assert all(x['status']=='passed' and x['exit_code']==0 for x in report['steps'][:27])
assert report['steps'][25]['name']=='m4-runtime' and report['steps'][26]['name']=='audit-data'
keys=['HOME','PATH','TMPDIR','CARGO_HOME','RUSTUP_HOME','SDKROOT','DEVELOPER_DIR','CARGO_TARGET_DIR','RUST_MCP_TEST_SOCKET','RUST_MCP_E5_DIR','ORT_LIB_LOCATION']
env={k:v for k,v in os.environ.items() if k in keys}
env.update(CARGO_INCREMENTAL='0',ORT_SKIP_DOWNLOAD='1',CARGO_TERM_COLOR='never',RUST_MCP_TEST_SOCKET='/Users/cburgosro/.docker/run/docker.sock',RUST_MCP_E5_DIR=str(ROOT/'target/m1-15-candidate/assets/model'),ORT_LIB_LOCATION='/Users/cburgosro/Library/Caches/ort.pyke.io/dfbin/aarch64-apple-darwin/612739f75438dc0a075461e1fb454226b4a1eb175e60a7271ba966bbbb972cd4')
cargo=subprocess.check_output(['rustup','which','--toolchain','1.98.1','cargo'],env=env,text=True).strip();env['PATH']=str(Path(cargo).parent)+os.pathsep+env.get('PATH','');env['RUSTC']=str(Path(cargo).with_name('rustc'))
assert gate.source_inventory(ROOT,env)==report['source_inputs'],'source mismatch: do not reuse prior steps'
for key,binary in [('cargo',cargo),('rustc',env['RUSTC'])]:assert subprocess.check_output([binary,'--version'],env=env,text=True).strip()==report[key]
report['resumption']={'started_at':gate.utc_now(),'retained_passed_steps':27,'remaining_steps':6,'prior_failed_receipt':str(source.relative_to(ROOT)),'prior_failed_receipt_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'same_source_inventory_verified':True,'gate_script_sha256':hashlib.sha256((ROOT/'scripts/gate.py').read_bytes()).hexdigest(),'driver_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'reason':'Configured temporary E5 directory was empty; exact previously provisioned local assets reverified by size and hash. No code or test condition changed.','asset_recovery_receipt':'docs/validation/M4-e5-local-recovery.json','asset_recovery_receipt_sha256':hashlib.sha256((ROOT/'docs/validation/M4-e5-local-recovery.json').read_bytes()).hexdigest(),'network_acquisition':False,'retained_steps_are_not_skips':True}
report['steps']=report['steps'][:27];report['status']='running';report.pop('error',None);report.pop('finished_at',None)
out=ROOT/'target/M4-full-gate.json'
def save():out.write_text(json.dumps(report,indent=2)+'\n')
save()
try:
 for name,script in [('semantic','test-semantic.py'),('catalog','test-catalog.py'),('catalog-status','test-catalog-status.py'),('crate-search','test-crate-search.py'),('crate-inspect','test-crate-inspect.py'),('doctor','test-doctor.py')]:gate.run_step(report,save,name,[sys.executable,'scripts/'+script],env)
 assert len(report['steps'])==33 and all(x['status']=='passed' and x['exit_code']==0 for x in report['steps'])
 report['source_inputs_unchanged']=gate.source_inventory(ROOT,env)==report['source_inputs'];assert report['source_inputs_unchanged']
 report['status']='passed';report['finished_at']=gate.utc_now();save();print('PASS source-identical resumed full gate: 27 retained passed steps + 6 freshly executed = 33 passed',flush=True)
except BaseException as error:
 report['status']='failed';report['error']=str(error);report['finished_at']=gate.utc_now();save();raise
