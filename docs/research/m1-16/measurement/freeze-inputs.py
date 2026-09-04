"""Principal-approved one-time freeze creation after recorded qualification."""
import sys,json,subprocess,importlib.util,hashlib,platform
from datetime import datetime,timezone
from pathlib import Path
sys.dont_write_bytecode=True
R=Path(__file__).resolve().parent.parent
sys.path.insert(0,str(R/'target/m1-16-controller'))
s=importlib.util.spec_from_file_location('runner',R/'target/m1-16-controller/run-study.py');m=importlib.util.module_from_spec(s);s.loader.exec_module(m)
c=json.loads((R/'target/m1-16-study-config.draft.json').read_text())
qual=R/'docs/research/m1-16/qualification'
c['qualification_approved']=True
c['qualification_receipts'] += [str(qual/name) for name in ['principal-disposition.md','opus-followup.md','final-local-tests.json','participant-final-echo.json','catalog-setup.json','positive-observer-cancel-raw.json','positive-observer-cancel-mcp.json','positive-observer-network-controls.json']]
config=R/'target/m1-16-study-config.json';m.write_json(config,c)
files=set()
for key in ['catalog_store','catalog_trust','model_dir','index_store','rustsec_path']:
 p=Path(c['driver'][key]);files.update(m.tree_files(p) if p.is_dir() else [p])
files.update(m.tree_files(c['corpus_root']));files.update(map(Path,c['qualification_receipts']))
files.update([m.PINNED_CLI,m.PINNED_CODE_HOST,m.containment.DOCKER,m.broker.BINARY,Path(c['driver']['server_binary']),Path(c['projection']),Path(sys.executable).resolve()])
for name in ['participant.py','test_participant.py','broker.py','broker_tests.py','run-study.py','evaluate.py','containment.py','analyze.py','test_analyze.py']:
 files.add(R/'target/m1-16-controller'/name)
for name in ['Cargo.toml','Cargo.lock','src/main.rs','src/tests.rs','src/bin/research-bundle.rs']:files.add(R/'target/m1-16-driver'/name)
for name in ['AGENTS.md','Cargo.toml','Cargo.lock','rust-toolchain.toml','docs/spec/rust-engineering-mcp-propuesta-v0.3.md','docs/validation/M1-16-protocol.md','docs/adr/ADR-046-bounded-utility-experiment.md']:
 p=R/name
 if p.is_file():files.add(p)
for name in subprocess.check_output(['git','ls-files','crates','scripts'],cwd=R,text=True).splitlines():files.add(R/name)
files.update(m.tree_files(R/'docs/research/m1-16/reproduction'))
freeze={'protocol_version':2,'approved':True,'approved_at':datetime.now(timezone.utc).isoformat(),'principal_disposition':str(qual/'principal-disposition.md'),'source_commit':subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip(),'product_compiled_source_commit':'01a90ab6','python':sys.version,'platform':platform.platform(),'image':'sha256:8fac70723a8d04b6ec9633ab721806b8a55f4f083a1b3f988c61bf6a00fa1909','config_sha256':m.digest(config),'files':[{'path':str(p),'sha256':m.digest(p),'bytes':p.stat().st_size} for p in sorted(files)]}
path=R/'target/m1-16-study-freeze.json';m.write_json(path,freeze);sha=m.digest(path)
m.verify(config,path,sha)
(R/'target/m1-16-study-freeze.sha256').write_text(sha+'\n')
print(json.dumps({'freeze_sha256':sha,'files':len(files),'source_commit':freeze['source_commit'],'planned_runs':m.schedule(),'processes_started':0}))
