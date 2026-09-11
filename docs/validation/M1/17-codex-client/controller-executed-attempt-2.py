#!/usr/bin/env python3
"""Closed stock-Codex native MCP qualifier. Never imports project code."""
from __future__ import annotations
import argparse, hashlib, json, os, queue, re, select, signal, subprocess, sys, tempfile, threading, time
from pathlib import Path

TOOLS = ("rust.project.open","rust.project.inspect","rust.toolchain.inspect","rust.check","rust.fmt.check","rust.clippy","rust.test","rust.dependencies.audit","rust.diagnostics.explain","rust.quality.gate","rust.catalog.status","rust.crate.search","rust.crate.inspect")
DISABLED_HOST_SERVERS = ("node_repl", "youtrack")
DISABLED = ("shell_tool","apps","plugins","hooks","memories","multi_agent","multi_agent_v2","browser_use","browser_use_external","computer_use","code_mode","code_mode_only","code_mode_host","image_generation","view_image","tool_suggest","remote_plugin","skill_search","skill_mcp_dependency_install","workspace_dependencies","goals","token_budget","sleep_tool","current_time_reminder","deferred_executor","standalone_web_search","in_app_browser","in_app_chat","in_app_local_automation","artifact","js_repl","mcp_2026_07_28")
GUARDS = ("agents.enabled","orchestrator.skills.enabled","orchestrator.mcp.enabled","skills.include_instructions","skills.bundled.enabled")
MAX_LINE=1024*1024; MAX_LOG=16*1024*1024; MAX_STDERR=65536; CLEANUP=260; NATURAL_SETTLE=30

def enc(v): return json.dumps(v,ensure_ascii=True,allow_nan=False,separators=(',',':')).encode()
def sha(v): return hashlib.sha256(v if isinstance(v,bytes) else enc(v)).hexdigest()
def toml(v):
    if isinstance(v,bool): return str(v).lower()
    if isinstance(v,str): return json.dumps(v)
    if isinstance(v,list): return '['+','.join(toml(x) for x in v)+']'
    if isinstance(v,dict): return '{'+','.join(f'{k}={toml(x)}' for k,x in v.items())+'}'
    if v is None: raise ValueError('TOML null forbidden')
    return str(v)
def at(v,path):
    for k in path.split('.'): v=v.get(k) if isinstance(v,dict) else None
    return v
def paths(v):
    if isinstance(v,dict):
        for x in v.values(): yield from paths(x)
    elif isinstance(v,list):
        for x in v: yield from paths(x)
    elif isinstance(v,str) and v.startswith('rust-artifact://'): yield v

def validate_plan(p):
    required={'codex','codex_sha256','model','effort','server_binary','server_binary_sha256','server_args','fixture_root','fixture_files_sha256','state_root','neutral_parent','prompt','wall_seconds','max_output_tokens','docker','docker_socket','expected_catalog_fingerprint'}
    if set(p)!=required: raise ValueError('plan_fields')
    for k in ('codex','server_binary','fixture_root','state_root','neutral_parent','docker','docker_socket'):
        q=Path(p[k]);
        if not q.is_absolute() or any(ord(c)<32 for c in str(q)): raise ValueError('absolute_path')
    if Path(p['neutral_parent']) != Path('/private/tmp'): raise ValueError('neutral_parent')
    if not isinstance(p['server_args'],list) or not all(isinstance(x,str) and not any(ord(c)<32 for c in x) for x in p['server_args']): raise ValueError('server_args')
    if p['model']!='gpt-5.6-sol' or p['effort']!='medium': raise ValueError('identity')
    for k in ('codex_sha256','server_binary_sha256'):
        if not re.fullmatch(r'[0-9a-f]{64}',p[k]):raise ValueError('binary_hash')
    if set(p['fixture_files_sha256'])!={'Cargo.toml','Cargo.lock','src/lib.rs','tests/behavior.rs'} or not all(re.fullmatch(r'[0-9a-f]{64}',x) for x in p['fixture_files_sha256'].values()):raise ValueError('fixture_hashes')
    if not re.fullmatch(r'sha256:[0-9a-f]{64}',p['expected_catalog_fingerprint']):raise ValueError('catalog_fingerprint')
    if not isinstance(p['prompt'],str) or not p['prompt'] or len(p['prompt'].encode())>MAX_LINE//2: raise ValueError('prompt')
    if not 1<=p['wall_seconds']<=900 or not 1<=p['max_output_tokens']<=30000: raise ValueError('budgets')

def file_sha(path):
    h=hashlib.sha256()
    with Path(path).open('rb') as f:
        for block in iter(lambda:f.read(65536),b''):h.update(block)
    return h.hexdigest()
def state_snapshot(p,require_empty=True):
    root=Path(p['state_root']);st=root.stat()
    if not root.is_dir() or st.st_uid!=os.geteuid() or st.st_mode&0o777!=0o700:raise RuntimeError('state_root_policy')
    entries={}
    for x in sorted(root.rglob('*')):
        name=str(x.relative_to(root))
        if x.is_symlink():entries[name]={'type':'symlink'}
        elif x.is_file():entries[name]={'type':'file','sha256':file_sha(x),'bytes':x.stat().st_size}
        elif x.is_dir():entries[name]={'type':'directory'}
        else:entries[name]={'type':'other'}
    if require_empty and entries:raise RuntimeError('state_root_not_empty:'+json.dumps(entries,sort_keys=True,separators=(',',':')))
    return entries
def fixture_snapshot(p):
    root=Path(p['fixture_root']);actual={}
    for x in root.rglob('*'):
        if x.is_symlink():raise RuntimeError('fixture_symlink')
        if x.is_file():actual[str(x.relative_to(root))]=file_sha(x)
    if actual!=p['fixture_files_sha256']:raise RuntimeError('fixture_identity')
    return actual
def verify_identities(p,empty_state=True):
    if file_sha(p['codex'])!=p['codex_sha256'] or file_sha(p['server_binary'])!=p['server_binary_sha256']:raise RuntimeError('binary_identity')
    if p['server_args'][p['server_args'].index('--state-root')+1]!=p['state_root']:raise RuntimeError('state_root_argument')
    return {'codex':p['codex_sha256'],'server':p['server_binary_sha256'],'fixture':fixture_snapshot(p),'state':state_snapshot(p,empty_state)}

def cleanup_state(p):
    before=state_snapshot(p,False)
    if not before:return {'before':{},'removed':[],'after':{}}
    directories=[name for name,value in before.items() if value['type']=='directory']
    if len(directories)!=1 or not re.fullmatch(r'rust-mcp-control-[0-9a-f]{32}',directories[0]):raise RuntimeError('state_cleanup_directory')
    directory=directories[0];expected={directory,*[f'{directory}/{name}' for name in ('config.json','seccomp.json','seccomp-socket.json','seccomp-rust.json')]}
    if set(before)!=expected or any(before[name]['type']!='file' for name in expected-{directory}):raise RuntimeError('state_cleanup_inventory')
    root=Path(p['state_root'])
    for name in sorted(expected-{directory}):
        path=root/name;st=path.stat()
        if path.is_symlink() or st.st_uid!=os.geteuid() or st.st_mode&0o777!=0o600:raise RuntimeError('state_cleanup_policy')
    removed=[]
    for name in sorted(expected-{directory}):
        (root/name).unlink();removed.append(name)
    (root/directory).rmdir();removed.append(directory)
    return {'before':before,'removed':removed,'after':state_snapshot(p)}

def overrides(p):
    server={'command':p['server_binary'],'args':p['server_args'],'cwd':'/','env':{},'env_vars':[],
            'enabled':True,'required':True,'startup_timeout_sec':45,'tool_timeout_sec':300,
            'enabled_tools':list(TOOLS),'default_tools_approval_mode':'approve'}
    values={'mcp_servers':{'rust_engineering':server},'model':p['model'],'model_reasoning_effort':p['effort'],
            'web_search':'disabled','project_doc_max_bytes':0,'project_doc_fallback_filenames':[],
            'developer_instructions':'','instructions':'','notify':[]}
    values.update({f'mcp_servers.{name}.enabled':False for name in DISABLED_HOST_SERVERS})
    values.update({f'features.{k}':False for k in DISABLED}); values.update({k:False for k in GUARDS})
    return values
def command(p):
    result=[p['codex'],'app-server','--stdio','--strict-config']
    for k,v in overrides(p).items(): result += ['-c',f'{k}={toml(v)}']
    return result

class Transport:
    def __init__(self,args,cwd):
        env={k:os.environ[k] for k in ('HOME','CODEX_HOME','TMPDIR','LANG') if k in os.environ};env['PATH']='/usr/bin:/bin:/usr/sbin:/sbin'
        self.p=subprocess.Popen(args,cwd=cwd,env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,start_new_session=True)
        os.set_blocking(self.p.stdin.fileno(),False)
        self.q=queue.Queue();self.err=bytearray();self.total=0;self.failure=None;self.ids=0;self.seen={};self.stop=threading.Event()
        self.threads=[threading.Thread(target=self._out,daemon=True),threading.Thread(target=self._err,daemon=True),threading.Thread(target=self._monitor,daemon=True)]
        for t in self.threads:t.start()
    def _fail(self,e): self.failure=self.failure or e;self.stop.set()
    def _out(self):
        try:
            while True:
                line=self.p.stdout.readline()
                if not line:break
                self.total+=len(line)
                if len(line)>MAX_LINE or self.total>MAX_LOG:self._fail('stdout_budget');break
                self.q.put(json.loads(line,parse_constant=lambda _:(_ for _ in()).throw(ValueError())))
        except Exception:self._fail('stdout_invalid')
    def _err(self):
        try:
            while True:
                b=self.p.stderr.read(8192)
                if not b:break
                self.err.extend(b[:max(0,MAX_STDERR-len(self.err))]);self.total+=len(b)
                if self.total>MAX_LOG:self._fail('stderr_budget');break
        except Exception:self._fail('stderr_io')
    @staticmethod
    def procs():
        r=subprocess.run(['/bin/ps','-axo','pid=,ppid=,pgid=,lstart=,comm='],env={},capture_output=True,text=True,timeout=2,check=True);out={}
        for line in r.stdout.splitlines():
            f=line.split(None,8)
            if len(f)==9:out[int(f[0])]={'pid':int(f[0]),'ppid':int(f[1]),'pgid':int(f[2]),'started':' '.join(f[3:8]),'name':Path(f[8]).name}
        return out
    def _monitor(self):
        try:
            while not self.stop.wait(.1) and self.p.poll() is None:
                rows=self.procs(); owned={self.p.pid}
                for _ in range(12):owned|={pid for pid,r in rows.items() if r['ppid'] in owned}
                for pid in owned:
                    if pid in rows:self.seen[pid]=rows[pid]
        except Exception:self._fail('process_monitor')
    def send(self,method,params):
        self.ids+=1;data=enc({'id':self.ids,'method':method,'params':params})+b'\n'
        if len(data)>MAX_LINE:raise RuntimeError('request_budget')
        self._write(data);return self.ids
    def notify(self,method,params):self._write(enc({'method':method,'params':params})+b'\n')
    def _write(self,data):
        view=memoryview(data);deadline=time.monotonic()+5
        while view:
            remaining=deadline-time.monotonic()
            if remaining<=0:self._fail('stdin_timeout');raise RuntimeError('stdin_timeout')
            try:
                count=os.write(self.p.stdin.fileno(),view)
                if count<=0:raise RuntimeError('stdin_closed')
                view=view[count:]
            except BlockingIOError:select.select([],[self.p.stdin],[],min(.05,remaining))
    def rpc(self,method,params,timeout=60,log=None):
        ident=self.send(method,params);end=time.monotonic()+timeout
        while time.monotonic()<end:
            if self.failure:raise RuntimeError(self.failure)
            try:m=self.q.get(timeout=.1)
            except queue.Empty:continue
            if log is not None:log(m)
            if m.get('id')==ident:
                if 'error'in m:raise RuntimeError(f'rpc_{method}_error_{m["error"].get("code")}')
                return m.get('result')
            if 'id'in m: self._write(enc({'id':m['id'],'error':{'code':-32601,'message':'denied'}})+b'\n');raise RuntimeError('server_request')
        raise RuntimeError(f'rpc_{method}_timeout')
    def close(self):
        errors=[];forced=False
        try:self.p.stdin.close()
        except Exception:errors.append('stdin_close')
        try:self.p.wait(timeout=CLEANUP)
        except subprocess.TimeoutExpired:
            forced=True;errors.append('parent_timeout');self.p.terminate()
            try:self.p.wait(timeout=10)
            except subprocess.TimeoutExpired:self.p.kill();self.p.wait(timeout=10)
        self.stop.set()
        for t in self.threads:t.join(timeout=3)
        settle_start=time.monotonic();remaining=[]
        while True:
            rows=self.procs();remaining=[]
            for pid,old in self.seen.items():
                if pid in rows and rows[pid]['started']==old['started']:remaining.append(pid)
            if not remaining or time.monotonic()-settle_start>=NATURAL_SETTLE:break
            time.sleep(.1)
        joined=all(not t.is_alive() for t in self.threads)
        return {'exit_code':self.p.returncode,'forced':forced,'reader_monitor_joined':joined,'remaining_observed_pids':remaining,
                'observed_processes':list(self.seen.values()),'natural_settle_seconds':round(time.monotonic()-settle_start,3),'errors':errors,'transport_failure':self.failure,
                'stderr_bytes':len(self.err),'stderr_sha256':sha(bytes(self.err)),
                'cleanup_verified':self.p.returncode==0 and not forced and joined and not remaining and not errors and not self.failure}

def docker_objects(p):
    # Called only in explicitly approved preflight/model modes.
    cfg=Path(tempfile.mkdtemp(prefix='m1-17-docker-',dir='/private/tmp'));cfg.chmod(0o700);(cfg/'config.json').write_text('{}\n')
    try:
        out={}
        for kind in ('container','volume'):
            args=[p['docker'],'--config',str(cfg),'--host','unix://'+p['docker_socket'],kind,'ls']
            if kind=='container':args+=['--all','--no-trunc']
            args+=['--filter','label=org.rust-mcp.execution=true','--format','{{json .}}']
            r=subprocess.run(args,env={},capture_output=True,text=True,timeout=15,check=True);out[kind]=r.stdout.splitlines()
        return out
    finally:
        try:(cfg/'config.json').unlink();cfg.rmdir()
        except OSError:pass

def thread_start(t,p,neutral):
    return t.rpc('thread/start',{'model':p['model'],'allowProviderModelFallback':False,'ephemeral':True,'environments':[],
      'runtimeWorkspaceRoots':[],'selectedCapabilityRoots':[],'cwd':str(neutral),'approvalPolicy':'never','sandbox':'read-only',
      'experimentalRawEvents':True,
      'baseInstructions':'Use only the configured Rust Engineering MCP tools for the bounded qualification.','developerInstructions':'',
      'config':{'model_reasoning_effort':p['effort']}},60)

def redacted_config(e):
    servers=e.get('mcp_servers',{})
    if set(servers)!={'rust_engineering',*DISABLED_HOST_SERVERS}:raise RuntimeError('effective_server_inventory')
    s=servers['rust_engineering']
    safe={'enabled':s.get('enabled'),'required':s.get('required'),'command_sha256':sha(str(s.get('command')).encode()),
          'args_sha256':sha(enc(s.get('args'))),'enabled_tools':s.get('enabled_tools'),'default_tools_approval_mode':s.get('default_tools_approval_mode')}
    if safe['enabled'] is not True or safe['required'] is not True or safe['enabled_tools']!=list(TOOLS) or safe['default_tools_approval_mode']!='approve':raise RuntimeError('effective_server_config')
    disabled_servers={name:servers[name].get('enabled') for name in DISABLED_HOST_SERVERS}
    if any(value is not False for value in disabled_servers.values()):raise RuntimeError('effective_host_server')
    disabled={k:e.get('features',{}).get(k) for k in DISABLED}
    if any(v is not False for v in disabled.values()) or any(at(e,k) is not False for k in GUARDS):raise RuntimeError('effective_guard')
    return {'mcp_servers':{'rust_engineering':safe},'disabled_host_servers':disabled_servers,'disabled_features':disabled,'guards':{k:at(e,k) for k in GUARDS},'web_search':e.get('web_search')}

def status_inventory(v):
    if not isinstance(v,dict) or v.get('nextCursor') is not None or not isinstance(v.get('data'),list):raise RuntimeError('status_shape')
    rows=v['data']
    if len(rows)!=3 or any(not isinstance(row,dict) for row in rows):raise RuntimeError('status_rows')
    by_name={row.get('name'):row for row in rows}
    if len(by_name)!=3 or set(by_name)!={'rust_engineering',*DISABLED_HOST_SERVERS}:raise RuntimeError('status_server_shape')
    target=by_name['rust_engineering'];tools=target.get('tools')
    if not isinstance(tools,dict) or set(tools)!=set(TOOLS) or len(tools)!=13:raise RuntimeError('status_tools')
    if any(not isinstance(value,dict) or value.get('name')!=name for name,value in tools.items()):raise RuntimeError('status_tool_identity')
    if any(by_name[name].get('tools')!={} or by_name[name].get('serverInfo') is not None for name in DISABLED_HOST_SERVERS):raise RuntimeError('status_disabled_server')
    info=target.get('serverInfo')
    if not isinstance(info,dict) or info.get('name')!='rust-engineering-mcp':raise RuntimeError('status_server_info')
    return {'server':'rust_engineering','server_version':info.get('version'),'tools':sorted(tools),'tools_sha256':sha(enc(tools)),'disabled_host_servers':list(DISABLED_HOST_SERVERS)}

def init(t,neutral):
    t.rpc('initialize',{'clientInfo':{'name':'m1-17-stock-codex-qualifier','version':'0.1'},'capabilities':{'experimentalApi':True}},30)
    t.notify('initialized',{})

def validate_tool_result(result,p,require_fingerprint=False):
    if not isinstance(result,dict) or result.get('isError') is True or result.get('error') is not None:raise RuntimeError('mcp_tool_error')
    structured=result.get('structuredContent')
    if not isinstance(structured,dict) or structured.get('status')!='passed':raise RuntimeError('mcp_tool_status')
    if require_fingerprint:
        found=[]
        def walk(x):
            if isinstance(x,dict):
                for k,v in x.items():
                    if k in ('fingerprint','snapshot_fingerprint') and isinstance(v,str):found.append(v)
                    walk(v)
            elif isinstance(x,list):
                for v in x:walk(v)
        walk(structured)
        if p['expected_catalog_fingerprint'] not in found:raise RuntimeError('catalog_fingerprint')
    return structured
def validate_resource(result):
    if not isinstance(result,dict) or result.get('isError') is True or result.get('error') is not None:raise RuntimeError('resource_error')
    contents=result.get('contents',result.get('content'))
    if not isinstance(contents,list) or not contents:raise RuntimeError('resource_empty')
    if not any(isinstance(x,dict) and any(isinstance(x.get(k),str) and x[k] for k in ('text','blob','uri')) for x in contents):raise RuntimeError('resource_empty')
    return contents
def validate_model_items(items,p):
    if len(items)!=1:raise RuntimeError('native_mcp_evidence')
    item=items[0]
    if item.get('server')!='rust_engineering' or item.get('tool')!='rust.catalog.status' or item.get('arguments')!={} or item.get('status')!='completed' or item.get('error') is not None:raise RuntimeError('native_mcp_evidence')
    validate_tool_result(item.get('result'),p,True)
    return item

class TurnObserver:
    """Byte/token/time-bounded native turn evidence collector."""
    def __init__(self,p,thread,turn,interrupt):
        self.p=p;self.thread=thread;self.turn=turn;self.interrupt=interrupt;self.started=time.monotonic();self.bytes=0;self.items={};self.search_calls=[];self.search_outputs=[];self.usage=None;self.stop=None
    def add(self,m):
        data=enc(m);self.bytes+=len(data)+1
        if self.bytes>MAX_LOG:self.cancel('event_budget')
        params=m.get('params',{});item=params.get('item') if isinstance(params,dict) else None
        if isinstance(item,dict):
            typ=item.get('type')
            if typ in ('commandExecution','fileChange','dynamicToolCall','webSearch','imageView','collabAgentToolCall'):raise RuntimeError('forbidden_native_item')
            if typ=='mcpToolCall':
                if item.get('server')!='rust_engineering' or item.get('tool')!='rust.catalog.status' or not isinstance(item.get('id'),str):raise RuntimeError('unexpected_mcp_call')
                self.items[item['id']]=item
            if m.get('method')=='rawResponseItem/completed' and typ=='tool_search_call':
                if not isinstance(item.get('arguments'),str) or 'rust.catalog.status' not in item['arguments']:raise RuntimeError('unexpected_tool_search')
                self.search_calls.append({'arguments':item['arguments'],'execution':item.get('execution'),'status':item.get('status')})
            if m.get('method')=='rawResponseItem/completed' and typ=='tool_search_output':
                if item.get('status')!='completed' or not isinstance(item.get('tools'),list):raise RuntimeError('unexpected_tool_search_output')
                self.search_outputs.append({'status':item['status'],'tools_sha256':sha(enc(item['tools'])),'tool_count':len(item['tools'])})
        def usage(x):
            if isinstance(x,dict):
                if isinstance(x.get('total'),dict) and ('outputTokens' in x['total'] or 'reasoningOutputTokens' in x['total']):self.usage=x['total']
                for y in x.values():usage(y)
            elif isinstance(x,list):
                for y in x:usage(y)
        usage(m)
        if self.usage and int(self.usage.get('outputTokens',0))+int(self.usage.get('reasoningOutputTokens',0))>=self.p['max_output_tokens']:self.cancel('output_token_budget')
        if time.monotonic()-self.started>=self.p['wall_seconds']:self.cancel('wall_budget')
    def cancel(self,why):
        if self.stop is None:self.stop=why;self.interrupt(self.thread,self.turn)

def run_model(p,preflight_path,out):
    preflight=json.loads(Path(preflight_path).read_text())
    if preflight.get('status')!='passed' or preflight.get('plan_sha256')!=sha(enc(p)) or not preflight.get('cleanup',{}).get('cleanup_verified') or preflight.get('state_cleanup',{}).get('after')!={} or preflight.get('identities_after',{}).get('state')!={}:raise RuntimeError('preflight_identity')
    neutral=Path(tempfile.mkdtemp(prefix='m1-17-codex-',dir=p['neutral_parent']));neutral.chmod(0o700)
    events=out.with_suffix('.events.jsonl');
    if out.exists() or events.exists():raise RuntimeError('output_exists')
    receipt={'phase':'model','status':'failed','plan_sha256':sha(enc(p)),'preflight_sha256':sha(Path(preflight_path).read_bytes()),'prompt_sha256':sha(p['prompt'].encode()),'neutral':str(neutral),'model_turns':0}
    receipt['identities_before']=verify_identities(p);t=Transport(command(p),neutral);event_file=events.open('xb');observer=None
    try:
        init(t,neutral);cfg=t.rpc('config/read',{'includeLayers':False,'cwd':str(neutral)},30);receipt['config']=redacted_config(cfg.get('config',{}))
        thread=thread_start(t,p,neutral);tid=thread['thread']['id']
        if thread.get('instructionSources') or thread.get('runtimeWorkspaceRoots') or thread.get('modelProvider')!='openai' or thread.get('model')!=p['model'] or thread.get('reasoningEffort')!=p['effort']:raise RuntimeError('thread_identity')
        receipt['inventory']=status_inventory(t.rpc('mcpServerStatus/list',{'detail':'toolsAndAuthOnly'},60))
        if docker_objects(p)['container'] or docker_objects(p)['volume']:raise RuntimeError('preexisting_docker_objects')
        started=t.rpc('turn/start',{'threadId':tid,'input':[{'type':'text','text':p['prompt']}]},30)
        turn=started['turn']['id'];receipt['model_turns']=1
        def interrupt(thread_id,turn_id):
            try:t.send('turn/interrupt',{'threadId':thread_id,'turnId':turn_id})
            except Exception:pass
        observer=TurnObserver(p,tid,turn,interrupt);deadline=time.monotonic()+p['wall_seconds']+30
        completed=False
        while time.monotonic()<deadline:
            if observer.stop and time.monotonic()>observer.started+p['wall_seconds']+30:break
            try:m=t.q.get(timeout=.1)
            except queue.Empty:
                if t.failure:raise RuntimeError(t.failure)
                observer.add({'method':'qualifier/tick','params':{}});continue
            observer.add(m);event_file.write(enc(m)+b'\n');event_file.flush()
            if m.get('method')=='turn/completed' and m.get('params',{}).get('turn',{}).get('id')==turn:completed=True;break
            if 'id' in m:raise RuntimeError('unexpected_server_request')
        receipt['turn_completed']=completed;receipt['stop_reason']=observer.stop;receipt['usage']=observer.usage
        receipt['mcp_items']=[{'server':x.get('server'),'tool':x.get('tool'),'arguments':x.get('arguments'),'status':x.get('status'),'durationMs':x.get('durationMs'),'result':x.get('result'),'error':x.get('error')} for x in observer.items.values()]
        receipt['tool_search']={'calls':observer.search_calls,'outputs':observer.search_outputs}
        if not completed or observer.stop or len(observer.search_calls)!=1 or len(observer.search_outputs)!=1:raise RuntimeError('native_mcp_evidence')
        validate_model_items(receipt['mcp_items'],p)
        receipt['status']='passed'
    finally:
        event_file.close();receipt['cleanup']=t.close();receipt['docker_after']=docker_objects(p)
        if receipt['cleanup']['cleanup_verified']:
            try:receipt['state_cleanup']=cleanup_state(p)
            except Exception as e:receipt['state_cleanup_error']=str(e);receipt['status']='failed'
        try:receipt['identities_after']=verify_identities(p)
        except Exception as e:receipt['identity_after_error']=str(e);receipt['status']='failed'
        if receipt['docker_after']['container'] or receipt['docker_after']['volume'] or not receipt['cleanup']['cleanup_verified']:receipt['status']='failed'
        try:neutral.rmdir()
        except OSError:receipt['neutral_remove_failed']=True;receipt['status']='failed'
        out.write_bytes(enc(receipt)+b'\n')
    if receipt['status']!='passed':raise RuntimeError('model_qualification_failed')

def run_preflight(p,out):
    neutral=Path(tempfile.mkdtemp(prefix='m1-17-codex-',dir=p['neutral_parent']));neutral.chmod(0o700)
    receipt={'phase':'preflight','status':'failed','plan_sha256':sha(enc(p)),'neutral':str(neutral),'model_turns':0}
    receipt['identities_before']=verify_identities(p);t=Transport(command(p),neutral)
    try:
        init(t,neutral);cfg=t.rpc('config/read',{'includeLayers':False,'cwd':str(neutral)},30);receipt['config']=redacted_config(cfg.get('config',{}))
        thread=thread_start(t,p,neutral);tid=thread['thread']['id'];receipt['thread']={k:thread.get(k) for k in ('model','reasoningEffort','modelProvider','runtimeWorkspaceRoots')}
        if thread.get('instructionSources') or thread.get('runtimeWorkspaceRoots') or thread.get('modelProvider')!='openai' or thread.get('model')!=p['model'] or thread.get('reasoningEffort')!=p['effort']:raise RuntimeError('thread_identity')
        receipt['inventory']=status_inventory(t.rpc('mcpServerStatus/list',{'detail':'full'},60))
        def call(tool,args,timeout=300):return t.rpc('mcpServer/tool/call',{'threadId':tid,'server':'rust_engineering','tool':tool,'arguments':args},timeout)
        opened=call('rust.project.open',{'path':p['fixture_root']},60);validate_tool_result(opened,p);refs=[]
        def findref(x):
            if isinstance(x,dict):
                if isinstance(x.get('project_ref'),str):refs.append(x['project_ref'])
                for y in x.values():findref(y)
            elif isinstance(x,list):
                for y in x:findref(y)
        findref(opened)
        if len(set(refs))!=1:raise RuntimeError('project_ref')
        ref=refs[0];status=call('rust.catalog.status',{},60);validate_tool_result(status,p,True);inspection=call('rust.project.inspect',{'project_ref':ref},60);validate_tool_result(inspection,p)
        before=docker_objects(p)
        if before['container'] or before['volume']:raise RuntimeError('preexisting_docker_objects')
        execution=call('rust.check',{'project_ref':ref},300);validate_tool_result(execution,p);after=docker_objects(p)
        if after['container'] or after['volume']:raise RuntimeError('remaining_docker_objects')
        uris=sorted(set(paths(execution)))
        if not uris:raise RuntimeError('no_resource')
        resource=t.rpc('mcpServer/resource/read',{'threadId':tid,'server':'rust_engineering','uri':uris[0]},60);validate_resource(resource)
        receipt['calls']={'project_open':sha(enc(opened)),'catalog_status':sha(enc(status)),'project_inspect':sha(enc(inspection)),'rust_check':sha(enc(execution)),'resource_read':sha(enc(resource)),'resource_uri':uris[0]}
        receipt['docker']={'before':before,'after':after};receipt['status']='passed'
    finally:
        receipt['cleanup']=t.close()
        if receipt['cleanup']['cleanup_verified']:
            try:receipt['state_cleanup']=cleanup_state(p)
            except Exception as e:receipt['state_cleanup_error']=str(e);receipt['status']='failed'
        try:receipt['identities_after']=verify_identities(p)
        except Exception as e:receipt['identity_after_error']=str(e);receipt['status']='failed'
        try:neutral.rmdir()
        except OSError:receipt['neutral_remove_failed']=True
        if not receipt['cleanup']['cleanup_verified']:receipt['status']='failed'
        out.write_bytes(enc(receipt)+b'\n')
    if receipt['status']!='passed':raise RuntimeError('preflight_failed')

def self_tests():
    import unittest
    suite=unittest.defaultTestLoader.discover(str(Path(__file__).parent),pattern='test_*.py')
    result=unittest.TextTestRunner(verbosity=2).run(suite)
    if not result.wasSuccessful():raise SystemExit(1)

def main():
    ap=argparse.ArgumentParser();sp=ap.add_subparsers(dest='mode',required=True)
    sp.add_parser('self-test'); q=sp.add_parser('show-config');q.add_argument('plan');r=sp.add_parser('preflight');r.add_argument('plan');r.add_argument('receipt');r.add_argument('--approved-plan-sha256',required=True)
    m=sp.add_parser('model');m.add_argument('plan');m.add_argument('preflight');m.add_argument('receipt');m.add_argument('--approved-plan-sha256',required=True);m.add_argument('--approved-prompt-sha256',required=True)
    a=ap.parse_args()
    if a.mode=='self-test':self_tests();return
    p=json.loads(Path(a.plan).read_text());validate_plan(p)
    if a.mode=='show-config':print(json.dumps({'plan_sha256':sha(enc(p)),'command':command(p),'prompt_sha256':sha(p['prompt'].encode()),'enabled_tools':TOOLS},indent=2));return
    if sha(enc(p))!=a.approved_plan_sha256:raise SystemExit('approved plan hash mismatch')
    out=Path(a.receipt)
    if out.exists():raise SystemExit('receipt exists')
    if a.mode=='preflight':run_preflight(p,out)
    elif sha(p['prompt'].encode())!=a.approved_prompt_sha256:raise SystemExit('approved prompt hash mismatch')
    else:run_model(p,a.preflight,out)
if __name__=='__main__':main()
