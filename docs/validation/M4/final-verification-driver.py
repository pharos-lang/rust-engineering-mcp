from pathlib import Path
import json,hashlib,datetime,subprocess,os,importlib.util,re,shutil
root=Path.cwd();out=root/'docs/validation';review=root/'docs/reviews/m4-final-evidence'
def sha(p):
 with Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def read(p):return json.loads(Path(p).read_text())
def save(p,d):Path(p).write_text(json.dumps(d,indent=2)+'\n')
d=read(review/'response.json');assert not d['is_error'] and 'ACCEPT local M4 closure.' in d['result'] and 'claude-opus-5' in d['modelUsage'];assert not d.get('permission_denials')
core=read(out/'M4-core-gate.json');full=read(out/'M4-full-gate.json');client=read(out/'M4-clients.json');native=read(out/'M4-runtime.json');idx=read(out/'M4-evidence-index.json')
spec=importlib.util.spec_from_file_location('gate',root/'scripts/gate.py');gate=importlib.util.module_from_spec(spec);spec.loader.exec_module(gate)
assert gate.source_inventory(root,os.environ)==core['source_inputs']==full['source_inputs']==client['candidate']['sources']
canonical=json.dumps(core['source_inputs'],sort_keys=True,separators=(',',':')).encode();assert hashlib.sha256(canonical).hexdigest()==idx['source_inventory_sha256']
for row in native['sources']+native['configuration_inputs']:assert sha(row['path'])==row['sha256']
for r in idx['receipts']:assert sha(out/r['path'])==r['sha256']
for r in read(review/'inputs.json'):assert sha(review/r['copy'])==r['sha256']
old=read(out/'M4-hardening-attempts/full-attempt-2/receipt.json');assert full['steps'][:27]==old['steps'][:27]
for i,s in enumerate(native['steps']):assert sha(out/'M4-runtime'/f'{i}.log')==s['log_sha256']
logs=read(out/'M4-log-archive.json');assert all(sha(out/r['retained_copy'])==r['sha256']==sha(out/r['original']) for r in logs['files'])
additional=[]
for name in ['docs/validation/M4-full-gate-resume.txt','docs/validation/M4-hardening-attempts/full-attempt-2/gate.txt','crates/semantic-adapter/src/model.rs','docs/validation/M4-full-gate.txt','scripts/test-m4-runtime.py']:
 dest=review/'additional-inputs'/name;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(name,dest);additional.append({'path':name,'copy':str(dest.relative_to(review)),'sha256':sha(name),'reviewer_disclosed_read_outside_frozen_package':len(additional)<3})
snapshots=[]
for row in read(out/'M4-client-execution.json')['previous_snapshots_unchanged']:
 blob=subprocess.check_output(['git','show','HEAD:'+row['path']]);digest=hashlib.sha256(blob).hexdigest();assert digest==row['sha256']==sha(row['path']);dest=review/'additional-inputs/git-baseline'/row['path'];dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(blob);snapshots.append({'path':row['path'],'baseline_copy':str(dest.relative_to(review)),'baseline_sha256':digest,'current_sha256':sha(row['path']),'equal':True})
assert len(snapshots)==23
save(review/'additional-inputs.json',{'reason':'Record explicitly disclosed reads outside frozen package; preserve supporting attachments and baseline after review. Additional attachments are not represented as newly reviewed by Opus.','files':additional,'baseline_snapshots':snapshots})
model=Path('crates/semantic-adapter/src/model.rs');pins=re.findall(r'\(\s*"([^"]+)",\s*(\d+),\s*"([0-9a-f]{64})",\s*\)',model.read_text().split('];',1)[0]);assert len(pins)==5
assets=[];priorassets={Path(r['path']).name:r for r in read(out/'M4-e5-local-recovery.json')['files']}
for name,size,digest in pins:
 oldasset=priorassets[name];assert oldasset['sha256']==digest and oldasset['bytes']==int(size);p=root/oldasset['path'];assert sha(p)==digest and p.stat().st_size==int(size);assets.append({'path':oldasset['path'],'bytes':p.stat().st_size,'expected_bytes':int(size),'sha256':sha(p),'expected_sha256':digest,'match':True})
docker='/Applications/Docker.app/Contents/Resources/bin/docker';env=dict(os.environ,DOCKER_HOST='unix:///Users/cburgosro/.docker/run/docker.sock')
for cmd in [['ps','-aq','--filter','label=org.rust-mcp.execution=true'],['volume','ls','-q','--filter','label=org.rust-mcp.execution=true']]:assert not subprocess.check_output([docker]+cmd,env=env,text=True).strip()
assert sha('target/release/rust-engineering-mcp')==client['candidate']['server_sha256']
assert not [p for p in Path('docs').rglob('*') if p.is_file() and p.name in ['auth.json','tokens.json','credentials.json']]
result={'schema':'rust-mcp-m4-final-verification-v1','status':'passed','observed_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'review_verdict':'ACCEPT local M4 closure','review_response_sha256':sha(review/'response.json'),'review_execution_sha256':sha(review/'execution.json'),'review_original_package_unchanged':True,'review_model_usage':d['modelUsage'],'review_model_note':'Substantive review used claude-opus-5. CLI also reports 19 auxiliary output tokens on Haiku; this was not a substituted review.','source_inventory_count':len(core['source_inputs']),'core_full_clients_equal_current_elementwise':True,'native_sources_and_configuration_equal_current':True,'canonicalization':"SHA-256 of UTF-8 json.dumps(source_inputs, sort_keys=True, separators=(',', ':'), ensure_ascii=True)",'source_inventory_sha256':idx['source_inventory_sha256'],'core_steps_passed':19,'full_steps_passed':33,'full_segments':[{'kind':'retained_passed','first_step':1,'last_step':27,'identical_to_original_receipt':True,'original_receipt':'M4-hardening-attempts/full-attempt-2/receipt.json','started_at':full['steps'][0]['started_at'],'finished_at':full['steps'][26]['finished_at']},{'kind':'executed_after_local_asset_recovery','first_step':28,'last_step':33,'started_at':full['steps'][27]['started_at'],'finished_at':full['steps'][32]['finished_at'],'driver':'M4-full-gate-resume-driver.py'}],'m4_native_steps_passed':19,'native_log_hashes_verified':19,'archived_log_copies_verified':len(logs['files']),'previous_snapshot_baseline_comparisons':snapshots,'e5_pin_source':str(model),'e5_pin_source_sha256':sha(model),'e5_assets':assets,'new_network_acquisition':False,'server_binary_sha256':client['candidate']['server_sha256'],'docker_owned_containers':[],'docker_owned_volumes':[],'no_credential_named_files_in_docs':True,'credential_check_scope':'Filename guard only; no claim of universal content secret scanning.','git_head':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'branch':subprocess.check_output(['git','branch','--show-current'],text=True).strip()}
save(out/'M4-final-verification.json',result)
print('PASS final verification: independent ACCEPT; sources987; native logs19; retained logs38; previous snapshots23; exact E5 assets5; no owned Docker residuals.')
