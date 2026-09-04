"""Fixed native retrieval benchmark. Run only after principal approves import/index.
No LLM, Docker command, project tool or acquisition. Uses the real rmcp SDK driver.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import signal
import statistics
import subprocess
import sys
import threading
import time

ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'target/m1-16-controller'))
from broker import Driver, BINARY, HOST_PROFILE, encoded
SERVER=ROOT/'target/m1-15-candidate/local/bin/rust-engineering-mcp'
QUERIES=ROOT/'target/m1-16-catalog/queries-qrels.draft.json'
MODES=('lexical','semantic','hybrid')
REPETITIONS=3
SAMPLE_SECONDS=.05

def sha(path):
    digest=hashlib.sha256()
    with Path(path).open('rb') as stream:
        while chunk:=stream.read(1024*1024):digest.update(chunk)
    return digest.hexdigest()

def read_queries():
    value=json.loads(QUERIES.read_text());rows=value['queries']
    if len(rows)!=8 or len({r['id'] for r in rows})!=8:raise ValueError('expected8uniquequeries')
    for row in rows:
        text=row['query']
        if not text or len(text.encode())>256 or len(text.split())>16 or any(ord(c)<32 or ord(c)==127 for c in text):raise ValueError('query_budget')
        if not row['accepted_identities'] or len(row['qrels'])!=16:raise ValueError('closed_corpus_labels')
    return rows

class RssSampler:
    def __init__(self,pid):
        self.pid=pid;self.samples=[];self.errors=0;self.stop_event=threading.Event()
        self.thread=threading.Thread(target=self.run,daemon=True)
        self.thread.start()
    def run(self):
        while not self.stop_event.is_set():
            before=time.monotonic()
            try:
                response=subprocess.run(['/bin/ps','-o','rss=','-p',str(self.pid)],capture_output=True,text=True,timeout=1)
                value=response.stdout.strip()
                if response.returncode==0 and value.isdecimal():self.samples.append({'monotonic':before,'rss_kib':int(value)})
                elif response.returncode not in [0,1]:self.errors+=1
            except (OSError,subprocess.TimeoutExpired):self.errors+=1
            self.stop_event.wait(SAMPLE_SECONDS)
    def close(self):
        self.stop_event.set();self.thread.join(timeout=2)
    def during(self,start,end):
        values=[s['rss_kib'] for s in self.samples if start<=s['monotonic']<=end]
        return {'samples':len(values),'peak_observed_rss_kib':max(values) if values else None}

def payload(response):
    value=response.get('structuredContent')
    if not isinstance(value,dict):raise ValueError('missing_structuredContent')
    if value.get('status')!='passed' or response.get('isError') is True:raise ValueError('tool_not_passed')
    return value['data']

def relevance(search,query):
    judgments={r['identity']:r['relevance'] for r in query['qrels']}
    ranked=[]
    for position,row in enumerate(search['results'][:5],1):
        facts=row['facts'];identity=facts['name']+'@'+facts['selected_version']['version']
        if identity not in judgments:raise ValueError('unexpected_identity_outside_qrels')
        ranked.append({'rank':position,'identity':identity,'relevance':judgments[identity]})
    positives=sum(v>0 for v in judgments.values());hits=sum(r['relevance']>0 for r in ranked)
    first=next((r['rank'] for r in ranked if r['relevance']>0),None)
    dcg=sum((2**r['relevance']-1)/math.log2(r['rank']+1) for r in ranked)
    ideal=sum(1/math.log2(i+1) for i in range(1,min(5,positives)+1))
    return {'ranked':ranked,'hit_at_5':int(hits>0),'recall_at_5':hits/positives,'mrr_at_5':1/first if first else 0,'ndcg_at_5':dcg/ideal if ideal else 0}

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--host-config',type=Path,required=True,help='Private JSON for existing broker.Driver')
    parser.add_argument('--identity-files',type=Path,required=True,help='JSON map stable labels to exact files: bundle/store/index/model/runtime receipts/assets')
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    queries=read_queries();out=args.output
    if out.exists():raise ValueError('fresh_output_directory_required')
    config=json.loads(args.host_config.read_text())
    if config.get('mode')!='mcp' or Path(config.get('server_binary','')).resolve()!=SERVER.resolve():raise ValueError('fixed_real_mcp_binary_required')
    for key in ['root','catalog_store','catalog_trust','model_dir','index_store']:
        if not config.get(key):raise ValueError('configured_catalog_index_model_required')
    files=json.loads(args.identity_files.read_text())
    if not isinstance(files,dict) or not files:raise ValueError('identity_manifest_required')
    files.update(server_binary=str(SERVER),sdk_driver=str(BINARY),broker_source=str(ROOT/'target/m1-16-controller/broker.py'),benchmark_source=__file__,queries=str(QUERIES),projection=str(ROOT/'target/m1-16-catalog/records.json'),source_evidence=str(ROOT/'target/m1-16-catalog/source-evidence.json'))
    identities={label:{'sha256':sha(path),'bytes':Path(path).stat().st_size} for label,path in files.items()}
    out.mkdir(parents=True)
    cancel=threading.Event();prior={}
    for sig in [signal.SIGINT,signal.SIGTERM]:prior[sig]=signal.signal(sig,lambda *_:cancel.set())
    driver=None;sampler=None
    receipt={'status':'running','scope':'closed15crate16versionresearchprojection;notgeneralIRproof','plan':{'queries':8,'modes':list(MODES),'warm_repetitions':REPETITIONS,'top_k':5,'rss_sample_interval_seconds':SAMPLE_SECONDS,'cold_request':'rust.catalog.status; excludes driver startup','warmup':'one S01-en query per mode, excluded from warm measurements','warm_order':'repetition outer, query input order, modes lexical/semantic/hybrid serial'},'identities':identities,'measurements':[],'warmups':[]}
    calls=0
    def call(name,arguments,label):
        nonlocal calls
        if cancel.is_set():raise RuntimeError('cancelled')
        start=time.monotonic();response=driver.request({'op':'call','name':name,'arguments':arguments},cancel);end=time.monotonic()
        calls+=1
        (out/f'{calls:03d}-{label}.json').write_bytes(encoded(response)+b'\n')
        return response,{'elapsed_seconds':end-start,'response_bytes':len(encoded(response)),'response_sha256':hashlib.sha256(encoded(response)).hexdigest(),'rss':sampler.during(start,end)}
    try:
        started=time.monotonic();driver=Driver(config,startup_seconds=45,call_seconds=180);ready=time.monotonic()
        receipt['startup_ready_seconds']=ready-started;receipt['ready']=driver.ready;receipt['host_profile']=HOST_PROFILE
        pid=driver.ready.get('server_pid')
        if not isinstance(pid,int) or pid<=0:raise ValueError('server_pid_required')
        sampler=RssSampler(pid)
        started=time.monotonic();tools=driver.request({'op':'tools'},cancel)
        receipt['discovery_seconds']=time.monotonic()-started
        if len(tools.get('tools',[]))!=13:raise ValueError('expected13tools')
        cold,timing=call('rust.catalog.status',{},'cold-catalog-status')
        receipt['first_catalog_load']=timing;receipt['catalog_status']=payload(cold)
        expected_identity=None
        for mode in MODES:
            q=queries[0];response,timing=call('rust.crate.search',{'query':q['query'],'mode':mode,'limit':5,'filters':q['filters']},'warmup-'+mode)
            search=payload(response)['search']
            if search['effective_mode']!=mode or search['fallback'] is not None:raise ValueError('native_mode_fallback')
            expected_identity=expected_identity or search['snapshot_fingerprint']
            if search['snapshot_fingerprint']!=expected_identity:raise ValueError('snapshot_identity_drift')
            receipt['warmups'].append({'mode':mode,**timing})
        receipt['snapshot_fingerprint']=expected_identity
        for repetition in range(1,REPETITIONS+1):
            for query in queries:
                for mode in MODES:
                    response,timing=call('rust.crate.search',{'query':query['query'],'mode':mode,'limit':5,'filters':query['filters']},f'r{repetition}-{query["id"]}-{mode}')
                    search=payload(response)['search']
                    row={'query_id':query['id'],'language':query['language'],'mode':mode,'repetition':repetition,**timing,'effective_mode':search['effective_mode'],'fallback':search['fallback'],'window':search['window'],**relevance(search,query)}
                    receipt['measurements'].append(row)
                    if search['snapshot_fingerprint']!=expected_identity:raise ValueError('snapshot_identity_drift')
                    if search['effective_mode']!=mode or search['fallback'] is not None:raise ValueError('native_mode_fallback')
        receipt['hashes_unchanged_after']=all(sha(path)==identities[label]['sha256'] for label,path in files.items())
        if not receipt['hashes_unchanged_after']:raise ValueError('artifact_changed_during_run')
        receipt['summary']={mode:{'warm_count':len(rows:=[r for r in receipt['measurements'] if r['mode']==mode]),'median_elapsed_seconds':statistics.median(r['elapsed_seconds'] for r in rows),'min_elapsed_seconds':min(r['elapsed_seconds'] for r in rows),'max_elapsed_seconds':max(r['elapsed_seconds'] for r in rows),'mean_hit_at_5':statistics.mean(r['hit_at_5'] for r in rows),'mean_mrr_at_5':statistics.mean(r['mrr_at_5'] for r in rows),'mean_ndcg_at_5':statistics.mean(r['ndcg_at_5'] for r in rows)} for mode in MODES}
        receipt['status']='completed'
    except Exception as exc:
        receipt['status']='failed';receipt['error_kind']=type(exc).__name__
        if isinstance(exc,(ValueError,RuntimeError)):receipt['error_code']=str(exc)[:200]
    finally:
        if driver:
            try:
                receipt['cleanup']=driver.cancel_and_join() if cancel.is_set() or receipt['status']=='failed' else driver.close()
            except Exception as exc:
                receipt['cleanup']={'cleanup_failed':True,'error_kind':type(exc).__name__}
            if receipt['cleanup'].get('cleanup_failed'):receipt['status']='cleanup_failed'
        if sampler:
            sampler.close();receipt['rss']={'scope':'owned MCP server PID only; not driver/container; peak observed samples not exact OS peak','samples':len(sampler.samples),'sampling_errors':sampler.errors,'peak_observed_rss_kib':max((s['rss_kib'] for s in sampler.samples),default=None)}
            (out/'rss-samples.json').write_bytes(encoded(sampler.samples)+b'\n')
        for sig,handler in prior.items():signal.signal(sig,handler)
        receipt['calls']=calls
        (out/'receipt.json').write_bytes(encoded(receipt)+b'\n')
    print(json.dumps({'status':receipt['status'],'warm_measurements':len(receipt['measurements']),'calls':calls}))
    return 0 if receipt['status']=='completed' else 1

if __name__=='__main__':raise SystemExit(main())
