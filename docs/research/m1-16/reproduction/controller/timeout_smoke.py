"""Principal-authorized single timeout qualification; no utility measurement."""
import json
from pathlib import Path
from participant import run_participant

def handler(name,args,cancel):
    if name!='experiment_echo' or args!={} or cancel.is_set():
        raise ValueError('unadmitted timeout-smoke callback')
    return {'canary':'M1_16_TIMEOUT_CANARY'}

if __name__=='__main__':
    result=run_participant(
        'Infrastructure cancellation calibration only. Execute exactly one code-mode cell containing: while (true) {} . Do not call other tools or do any other work. The controller will interrupt this deliberately infinite JavaScript loop.',
        [{'name':'experiment_echo','description':'Return a fixed infrastructure canary.','inputSchema':{'type':'object','properties':{},'additionalProperties':False}}],
        handler,Path(__file__).resolve().parent/'timeout-qualification',wall_seconds=10,max_output_tokens=1000)
    print(json.dumps({k:result.get(k) for k in ['status','failure_code','identity','model_turns_sent','turn_status','turn_error','usage','usage_coverage','admission_stopped','elapsed_seconds','cleanup']},indent=2))
