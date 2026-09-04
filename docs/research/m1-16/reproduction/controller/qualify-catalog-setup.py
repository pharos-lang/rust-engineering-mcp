import sys,json,threading,importlib.util,time
from pathlib import Path
sys.dont_write_bytecode=True
import broker,containment
spec=importlib.util.spec_from_file_location('runner',Path(__file__).with_name('run-study.py'));runner=importlib.util.module_from_spec(spec);spec.loader.exec_module(runner)
R=Path(__file__).resolve().parents[2];c=json.loads((R/'target/m1-16-study-config.draft.json').read_text())
out=R/'target/m1-16-catalog-setup-qualification'/str(time.time_ns());out.mkdir(mode=0o700,parents=True)
ws=broker.Workspace(out,runner.SELECTION_FILES);state=out/'state';state.mkdir(mode=0o700);d=None;r={}
try:
 r['before']=containment.observe(c['driver']['docker_socket'],out);assert r['before']['absent']
 d=broker.Driver(runner.driver_config(c,ws,'mcp',state,out/'server.stderr'))
 r['result']=d.request({'op':'call','name':'rust.catalog.status','arguments':{}},threading.Event())
 r['verified']=runner.check_catalog(r['result'],c['expected_catalog']);r['status']='passed'
finally:
 if d:r['cleanup']=d.close()
 ws.close();r['after']=containment.observe(c['driver']['docker_socket'],out)
 (out/'receipt.json').write_text(json.dumps(r,indent=2)+'\n')
assert r['status']=='passed' and not r['cleanup']['cleanup_failed'] and r['after']['absent']
print('PASS actual catalog setup check',out,flush=True)
