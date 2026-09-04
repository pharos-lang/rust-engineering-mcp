import sys,json,hashlib
from pathlib import Path
sys.dont_write_bytecode=True
from participant import run_participant
OUT=Path(__file__).resolve().parent/'postreview-smoke'
TOOLS=[{'name':'experiment_echo','description':'Return one fixed calibration canary; no effects.','inputSchema':{'type':'object','properties':{},'additionalProperties':False,'required':[]}}]
calls=[]
def echo(name,args,cancel):
 assert name=='experiment_echo' and args=={} and not cancel.is_set()
 calls.append((name,args));return {'canary':'m116-post-review-transport'}
r=run_participant('This is a disjoint infrastructure calibration, not a study task. Call experiment_echo exactly once with {} and then report its canary. Do not call any other tool.',TOOLS,echo,OUT,wall_seconds=90,max_output_tokens=1000)
assert r['status']=='completed' and len(calls)==1 and r['watchdog_joined'],r['status']
(OUT/'source-identity.json').write_text(json.dumps({'participant_sha256':hashlib.sha256(Path(__file__).with_name('participant.py').read_bytes()).hexdigest(),'echo_calls':len(calls),'status':'passed'})+'\n')
print('PASS actual post-review participant echo; bounded transport/cleanup',flush=True)
