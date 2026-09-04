"""Bounded participant client of the installed Codex app-server, not a server.

handler(name, arguments, cancellation_event) executes synchronously on the caller
thread and MUST honor cancellation and join its gateway cleanup before returning.
No detached handler execution is used. Runtime budgets are cooperative, not hard
native CPU/RAM limits. Configuration reflects pinned-source-reviewed V8 API guards.
"""
from __future__ import annotations
import hashlib
import json
import os
from pathlib import Path
from collections import deque
import select
import re
import subprocess
import signal
import threading
import tempfile
import time

CLI = '/opt/homebrew/bin/codex'
MAX_LINE = 1024 * 1024
MAX_LOG = 16 * 1024 * 1024
MAX_STDERR = 64 * 1024
MAX_CALLS = 64
WRITE_SECONDS = 5
DISABLED = ['shell_tool','apps','plugins','hooks','memories','multi_agent','multi_agent_v2','browser_use','browser_use_external','computer_use','code_mode','code_mode_only','image_generation','view_image','tool_suggest','remote_plugin','skill_search','skill_mcp_dependency_install','workspace_dependencies','goals','token_budget','sleep_tool','current_time_reminder','deferred_executor','deferred_tool_world_state','standalone_web_search','in_app_browser','in_app_chat','in_app_local_automation','in_app_dictation','realtime_conversation','artifact',
    # Pinned features.rs: connector auth prompts, background local migration,
    # removed JS/remote-control flags, native MCP protocol and TUI mention popup.
    'auth_elicitation','background_paginated_rollout_migration','js_repl',
    'mcp_2026_07_28','mentions_v2','remote_control']
# Exact unfiltered config/read map, requalified without thread/start or turn/start.
# The older preflight.py filtered effective_features and was NOT an inventory.
# network_proxy is an optional typed field (features.rs), observed null: never
# override/deactivate a host security setting; a non-null value fails this freeze.
# This map is serialized configuration, not default feature state/tool inventory.
EXPECTED_FEATURES = {**{key:False for key in DISABLED},
                     'code_mode_host':True,'skip_host_skill_discovery':True,
                     'network_proxy':None}
GUARDS = ['agents.enabled','orchestrator.skills.enabled','orchestrator.mcp.enabled','skills.include_instructions','skills.bundled.enabled']


def encoded(value):
    return json.dumps(value, ensure_ascii=True, separators=(',', ':'), allow_nan=False).encode()


def at(value, path):
    for key in path.split('.'):
        value = value.get(key) if isinstance(value, dict) else None
    return value


def validate_tools(tools):
    names = set()
    for tool in tools:
        if set(tool) != {'name','description','inputSchema'}:
            raise ValueError('closed dynamic tool declaration required')
        name = tool['name']
        if not isinstance(name,str) or not re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]{0,63}', name) or name in names:
            raise ValueError('invalid or duplicate tool name')
        schema = tool['inputSchema']
        if schema.get('type') != 'object' or schema.get('additionalProperties') is not False:
            raise ValueError('closed object input schema required')
        names.add(name)
    if not names or len(encoded(tools)) > MAX_LINE:
        raise ValueError('empty or oversized tools')
    return names


class ByteQueue:
    """FIFO retaining encoded bytes, with byte-based backpressure and bounded close."""
    def __init__(self, capacity=MAX_LOG):
        self.capacity = capacity
        self.bytes = 0
        self.items = deque()
        self.condition = threading.Condition()
        self.closed = False

    def put(self, data):
        if len(data) > self.capacity: raise ValueError('transport_queue_budget')
        with self.condition:
            while self.bytes + len(data) > self.capacity and not self.closed:
                self.condition.wait()
            if self.closed: return False
            self.items.append(data); self.bytes += len(data)
            self.condition.notify_all()
            return True

    def get(self, timeout):
        deadline = time.monotonic() + timeout
        with self.condition:
            while not self.items and not self.closed:
                remaining = deadline - time.monotonic()
                if remaining <= 0: return None
                self.condition.wait(remaining)
            if not self.items: return None
            data = self.items.popleft(); self.bytes -= len(data)
            self.condition.notify_all()
            return json.loads(data)

    def close(self):
        with self.condition:
            self.closed = True
            self.condition.notify_all()


class Transport:
    """Bounded stdout reader, independent cancellation, and owned process cleanup."""
    def __init__(self, args, cwd, cancel):
        env = {k:os.environ[k] for k in ['HOME','TMPDIR','LANG','CODEX_HOME'] if k in os.environ}
        env['PATH'] = '/usr/bin:/bin:/usr/sbin:/sbin'
        self.cancel = cancel
        self.messages = ByteQueue()
        self.lock = threading.Lock()
        self.failure = None
        self.stderr_bytes = 0
        self.stderr_retained = bytearray()
        self.observed = {}
        self.observed_lock = threading.Lock()
        self.monitor_stop = threading.Event()
        self.process = subprocess.Popen(args,cwd=cwd,env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        os.set_blocking(self.process.stdin.fileno(), False)
        self.readers = [threading.Thread(target=self.read_stdout,daemon=True), threading.Thread(target=self.read_stderr,daemon=True), threading.Thread(target=self.monitor,daemon=True)]
        for thread in self.readers: thread.start()

    def fail(self, reason):
        if self.failure is None: self.failure = reason
        self.cancel.set()

    def read_stdout(self):
        total = 0
        try:
            while True:
                line = self.process.stdout.readline(MAX_LINE + 1)
                if not line: break
                total += len(line)
                if len(line) > MAX_LINE or total > MAX_LOG:
                    self.fail('transport_output_budget'); break
                if not isinstance(json.loads(line), dict):
                    self.fail('invalid_transport_output'); break
                if not self.messages.put(line): break
        except Exception:
            self.fail('invalid_transport_output')

    def read_stderr(self):
        while True:
            chunk = self.process.stderr.read(4096)
            if not chunk: break
            self.stderr_bytes += len(chunk)
            self.stderr_retained.extend(chunk[:max(0,MAX_STDERR-len(self.stderr_retained))])
            if self.stderr_bytes > MAX_LOG: self.fail('stderr_budget')

    @staticmethod
    def processes():
        rows = {}
        result = subprocess.run(['/bin/ps','-axo','pid=,ppid=,pgid=,lstart=,comm='],capture_output=True,text=True,timeout=2,check=True)
        for line in result.stdout.splitlines():
            fields = line.split(None,8)
            if len(fields)==9:
                rows[int(fields[0])] = {'pid':int(fields[0]),'ppid':int(fields[1]),'pgid':int(fields[2]),'name':Path(fields[8]).name,'started':' '.join(fields[3:8])}
        return rows

    def monitor(self):
        try:
            while not self.monitor_stop.is_set() and self.process.poll() is None:
                rows = self.processes(); descendants = {self.process.pid}
                for _ in range(8): descendants |= {pid for pid,row in rows.items() if row['ppid'] in descendants}
                with self.observed_lock:
                    for pid in descendants:
                        if pid in rows: self.observed[pid] = rows[pid]
                self.monitor_stop.wait(.1)
        except Exception: self.fail('process_monitor_failed')

    def send(self, value):
        data = encoded(value) + b'\n'
        if len(data)>MAX_LINE: raise ValueError('outgoing_line_budget')
        deadline = time.monotonic() + WRITE_SECONDS
        if not self.lock.acquire(timeout=WRITE_SECONDS):
            self.fail('transport_write_lock_timeout')
            raise RuntimeError('transport_write_lock_timeout')
        try:
            view = memoryview(data)
            while view:
                remaining = deadline-time.monotonic()
                if remaining<=0:
                    self.fail('transport_write_timeout')
                    raise RuntimeError('transport_write_timeout')
                try:
                    count = os.write(self.process.stdin.fileno(), view)
                    if count<=0: raise RuntimeError('transport_write_closed')
                    view=view[count:]
                except BlockingIOError:
                    select.select([], [self.process.stdin], [], min(.05, remaining))
        except (OSError, ValueError):
            self.fail('transport_write_failed')
            raise RuntimeError('transport_write_failed') from None
        finally:
            self.lock.release()

    def is_alive(self):
        return self.process.poll() is None

    def receive(self, timeout=.1):
        return self.messages.get(timeout)

    def close(self):
        # Cleanup failures are orthogonal evidence. A failed inspection must not
        # skip the parent wait or readers, and must never prove child absence.
        errors = []
        inspection_complete = True
        forced = False
        forced_host = False
        def attempt(code, action):
            try: return action()
            except Exception as exc:
                errors.append({'code':code,'kind':type(exc).__name__})
                return None
        def inspect():
            nonlocal inspection_complete
            rows = attempt('process_inspection_failed', self.processes)
            if rows is None: inspection_complete = False
            return rows
        def wait_parent(seconds):
            try: self.process.wait(timeout=seconds); return True
            except subprocess.TimeoutExpired: return False
            except Exception as exc:
                errors.append({'code':'parent_wait_failed','kind':type(exc).__name__})
                return False

        if self.lock.acquire(timeout=WRITE_SECONDS):
            try: attempt('stdin_close_failed', self.process.stdin.close)
            finally: self.lock.release()
        else: errors.append({'code':'stdin_lock_timeout','kind':'TimeoutError'})
        parent_joined = wait_parent(10)
        if not parent_joined:
            forced = True
            attempt('parent_terminate_failed', self.process.terminate)
            parent_joined = wait_parent(10)
            if not parent_joined:
                attempt('parent_kill_failed', self.process.kill)
                parent_joined = wait_parent(10)
        self.monitor_stop.set()
        # Stop the monitor before snapshotting; the lock also protects a monitor
        # still completing its bounded ps call when this join expires.
        attempt('monitor_join_failed', lambda:self.readers[2].join(timeout=3))
        with self.observed_lock:
            observed = dict(self.observed)
        for pid, original in observed.items():
            def same_owned_host():
                current_rows = inspect()
                current = current_rows.get(pid) if current_rows is not None else None
                return (original['name']=='codex-code-mode-host' and original['ppid']==self.process.pid
                        and original['pgid']==pid and current is not None
                        and current['name']==original['name'] and current['started']==original['started']
                        and current['pgid']==original['pgid'] and current['ppid'] in [self.process.pid,1])
            if same_owned_host():
                forced_host = True
                attempt('owned_host_terminate_failed', lambda:os.kill(pid,signal.SIGTERM))
                until = time.monotonic()+2
                while time.monotonic()<until and same_owned_host(): time.sleep(.05)
                if same_owned_host():
                    attempt('owned_host_kill_failed', lambda:os.kill(pid,signal.SIGKILL))
        self.messages.close()
        for reader in self.readers:
            attempt('reader_join_failed', lambda:reader.join(timeout=1))
        if any(reader.is_alive() for reader in self.readers):
            errors.append({'code':'reader_join_timeout','kind':'TimeoutError'})
            # A BufferedReader owns its descriptor. Never os.close its borrowed
            # fileno: a later close/destructor could close an unrelated reused FD.
            # Bound-method daemon readers retain this Transport and its streams
            # while alive. Keep ownership intact, report unjoined, stop the series.
        if all(not r.is_alive() for r in self.readers):
            for stream in [self.process.stdout,self.process.stderr]:
                attempt('reader_stream_close_failed', stream.close)
        rows = inspect()
        with self.observed_lock:
            observed = dict(self.observed)
        # PID reuse is not a surviving observed process; compare birth identities.
        lingering = ([pid for pid,old in observed.items() if pid in rows and
                      rows[pid]['started']==old['started']] if rows is not None else None)
        readers_joined = all(not r.is_alive() for r in self.readers)
        if not readers_joined: inspection_complete = False
        return {'exit_code':self.process.returncode,'parent_joined':parent_joined,
                'forced_parent_stop':forced,'forced_owned_host_stop':forced_host,
                'observed_processes':list(observed.values()),'remaining_observed_pids':lingering,
                'reader_joined':readers_joined,'inspection_complete':inspection_complete,
                'cleanup_errors':errors,
                'stderr_bytes':self.stderr_bytes,'stderr_prefix_sha256':hashlib.sha256(self.stderr_retained).hexdigest(),
                'transport_failure':self.failure}



def configuration():
    overrides = {'web_search':'disabled','project_doc_max_bytes':0,'project_doc_fallback_filenames':[],
                 'model':'gpt-5.6-sol','model_reasoning_effort':'medium','developer_instructions':'','instructions':'','notify':[]}
    overrides.update({key:False for key in GUARDS})
    overrides.update({'features.'+key:False for key in DISABLED})
    overrides.update({'features.code_mode_host':True,'features.skip_host_skill_discovery':True})
    names = []
    config = Path(os.environ.get('CODEX_HOME',str(Path.home()/'.codex')))/'config.toml'
    if config.exists():
        for line in config.read_text().splitlines():
            match = re.fullmatch(r'\s*\[mcp_servers\.([A-Za-z0-9_-]+)\]\s*',line)
            if match: names.append(match.group(1))
    for name in names: overrides['mcp_servers.'+name+'.enabled'] = False
    args = [CLI,'app-server','--stdio']
    for key,value in overrides.items(): args += ['-c',key+'='+json.dumps(value)]
    return args


def run_participant(prompt, dynamic_tools, handler, output_dir, wall_seconds=900, max_output_tokens=30000):
    names = validate_tools(dynamic_tools)
    schemas = {tool['name']:tool['inputSchema'] for tool in dynamic_tools}
    if not isinstance(prompt,str) or len(prompt.encode())>MAX_LINE//2 or wall_seconds<=0 or max_output_tokens<=0:
        raise ValueError('invalid participant budget/input')
    out = Path(output_dir); out.mkdir(parents=True,exist_ok=True)
    if any(os.path.lexists(out/name) for name in ['receipt.json','events.jsonl','neutral']):
        raise ValueError('participant output already exists')
    # The app-server may inspect cwd ancestors for config. Keep its fresh private
    # cwd outside the product, corpus and evidence trees; never recursively delete it.
    neutral = Path(tempfile.mkdtemp(prefix='m1-16-neutral-',dir='/private/tmp')).resolve()
    (out/'neutral').write_bytes(encoded({'cwd':str(neutral)})+b'\n')
    (out/'events.jsonl').touch(exist_ok=False)
    cancel = threading.Event(); finished = threading.Event(); started = time.monotonic(); deadline=started+wall_seconds
    report = {'model_turns_sent':0,'requested_model':'gpt-5.6-sol','requested_effort':'medium','tool_calls':[],
              'usage':None,'usage_coverage':'unknown','text':'','status':'failed','admission_stopped':False,
              'stop_reason':None,'stop_reasons':[],'neutral_cwd':str(neutral),
              'prompt_sha256':hashlib.sha256(prompt.encode()).hexdigest(),
              'tool_declarations_sha256':hashlib.sha256(encoded(dynamic_tools)).hexdigest()}
    transport = Transport(configuration(),neutral.resolve(),cancel)
    ids = 0; state = {'thread':None,'turn':None}; log_bytes=0
    def send(method,params):
        nonlocal ids
        ids += 1; transport.send({'id':ids,'method':method,'params':params}); return ids
    stop_lock = threading.Lock()
    def interrupt(reason='cancelled'):
        with stop_lock:
            if report['stop_reason'] is None: report['stop_reason']=reason
            if reason not in report['stop_reasons']: report['stop_reasons'].append(reason)
        cancel.set(); report['admission_stopped']=True
        if state['thread'] and state['turn']:
            try: transport.send({'id':'interrupt','method':'turn/interrupt','params':{'threadId':state['thread'],'turnId':state['turn']}})
            except Exception: pass
    def watchdog():
        while not finished.wait(.05):
            if time.monotonic()>=deadline: interrupt('wall_deadline'); return
            if cancel.is_set(): interrupt(transport.failure or report['stop_reason'] or 'cancelled'); return
    watcher=threading.Thread(target=watchdog); watcher.start()
    def record(value):
        nonlocal log_bytes
        data=encoded(value)
        if log_bytes+len(data)+1>MAX_LOG-2*MAX_LINE:
            interrupt('event_log_budget'); raise RuntimeError('event_log_budget')
        with (out/'events.jsonl').open('ab') as f:f.write(data+b'\n')
        log_bytes+=len(data)+1
    def rpc(method,params):
        ident=send(method,params)
        while not cancel.is_set():
            message=transport.receive()
            if message is None:
                if transport.failure or not getattr(transport,'is_alive',lambda:True)():raise RuntimeError('transport_stopped')
                continue
            # Record preflight sequencing without config/auth response bodies.
            record({'phase':'rpc','request_method':method,'method':message.get('method'),
                    'matched_response':message.get('id')==ident,
                    'elapsed_seconds':round(time.monotonic()-started,3)})
            if message.get('id')==ident:
                if 'result' not in message: raise RuntimeError('rpc_failed_'+method.replace('/','_'))
                return message['result']
            if 'id' in message and 'method' in message:
                transport.send({'id':message['id'],'error':{'code':-32601,'message':'Preflight request denied'}})
                raise RuntimeError('unexpected_preflight_request')
        raise RuntimeError('preflight_cancelled')
    try:
        rpc('initialize',{'clientInfo':{'name':'m1_16_participant','version':'0.1'},'capabilities':{'experimentalApi':True}})
        transport.send({'method':'initialized','params':{}})
        conf=rpc('config/read',{'includeLayers':False,'cwd':str(neutral.resolve())})['config']
        checks={key:at(conf,key) is False for key in GUARDS}
        checks.update({key:at(conf,'features.'+key) is False for key in DISABLED})
        features=conf.get('features',{})
        checks['exact_feature_config']=isinstance(features,dict) and set(features)==set(EXPECTED_FEATURES) and all(features.get(k) is v for k,v in EXPECTED_FEATURES.items())
        report['effective_features_sha256']=hashlib.sha256(encoded(features)).hexdigest()
        report['effective_feature_keys']=sorted(features) if isinstance(features,dict) else []
        report['feature_guard_scope']='exact_config_keys_and_values_not_native_tool_inventory'
        checks['code_host']=at(conf,'features.code_mode_host') is True
        checks['skip_host_skills']=at(conf,'features.skip_host_skill_discovery') is True
        checks['web_disabled']=conf.get('web_search')=='disabled'
        checks['project_docs_disabled']=conf.get('project_doc_max_bytes')==0 and conf.get('project_doc_fallback_filenames')==[]
        checks['mcp_disabled']=all(v.get('enabled') is False for v in conf.get('mcp_servers',{}).values())
        checks['instruction_config_empty']=all(conf.get(k) in [None,''] for k in ['instructions','developer_instructions'])
        report['effective_guard_checks']=checks
        del conf
        if not all(checks.values()): raise RuntimeError('effective_guard_mismatch')
        thread=rpc('thread/start',{'model':'gpt-5.6-sol','allowProviderModelFallback':False,'ephemeral':True,
             'environments':[],'runtimeWorkspaceRoots':[],'selectedCapabilityRoots':[],'cwd':str(neutral.resolve()),
             'approvalPolicy':'never','sandbox':'read-only','config':{'model_reasoning_effort':'medium'},
             'baseInstructions':'Complete the supplied bounded task using the admitted tools.','developerInstructions':'',
             'dynamicTools':dynamic_tools})
        report['identity']={k:thread.get(k) for k in ['model','reasoningEffort','modelProvider','runtimeWorkspaceRoots']}
        report['instruction_sources_count']=len(thread.get('instructionSources',[]))
        if thread.get('model')!='gpt-5.6-sol' or thread.get('reasoningEffort')!='medium' or thread.get('modelProvider')!='openai' or thread.get('instructionSources') or thread.get('runtimeWorkspaceRoots'):
            raise RuntimeError('identity_or_instructions_mismatch')
        state['thread']=thread['thread']['id']; report['model_turns_sent']=1
        turn=rpc('turn/start',{'threadId':state['thread'],'input':[{'type':'text','text':prompt}]})
        state['turn']=turn['turn']['id']
        if cancel.is_set(): interrupt(report['stop_reason'] or transport.failure or 'cancelled')
        cleanup_deadline=None
        while True:
            if cancel.is_set() and cleanup_deadline is None: interrupt(report['stop_reason'] or transport.failure or 'cancelled');cleanup_deadline=time.monotonic()+30
            if cleanup_deadline and time.monotonic()>cleanup_deadline: raise RuntimeError('turn_cleanup_timeout')
            message=transport.receive()
            if message is None:
                if transport.failure or not getattr(transport,'is_alive',lambda:True)():raise RuntimeError('transport_stopped')
                continue
            method=message.get('method'); params=message.get('params',{})
            entry={'method':method,'elapsed_seconds':round(time.monotonic()-started,3)}
            if 'id' in message and method:
                args=params.get('arguments');name=params.get('tool')
                shape_ok=(name in schemas and isinstance(args,dict))
                admitted=(shape_ok and not cancel.is_set() and method=='item/tool/call' and name in names and isinstance(args,dict) and len(report['tool_calls'])<MAX_CALLS)
                if not admitted:
                    if method=='item/tool/call':
                        raw=encoded(args)
                        record({'denied_tool':{'name':name,'request':args,'request_bytes':len(raw),'request_sha256':hashlib.sha256(raw).hexdigest(),'elapsed_seconds':entry['elapsed_seconds'],'response':{'code':-32601,'message':'Tool request denied'}}})
                    transport.send({'id':message['id'],'error':{'code':-32601,'message':'Tool request denied'}})
                    report['denied_request']=method;interrupt('tool_call_limit' if len(report['tool_calls'])>=MAX_CALLS else 'unadmitted_request')
                else:
                    request_bytes=encoded(args)
                    call={'name':name,'request':args,'request_sha256':hashlib.sha256(request_bytes).hexdigest(),'request_bytes':len(request_bytes),'started_seconds':entry['elapsed_seconds']}
                    report['tool_calls'].append(call)
                    try:
                        # Synchronous caller-thread handler; watchdog independently sets cancel.
                        answer=handler(name,args,cancel); response_bytes=encoded(answer)
                        if len(response_bytes)>MAX_LINE//2: raise ValueError('handler_response_budget')
                        call.update(response=answer,response_sha256=hashlib.sha256(response_bytes).hexdigest(),response_bytes=len(response_bytes))
                        # The handler can commit just before cancellation. Retain its
                        # returned result; delivery after cancellation is orthogonal.
                        call['cancellation_after_handler']=cancel.is_set()
                        response={'contentItems':[{'type':'inputText','text':response_bytes.decode()}],'success':not (isinstance(answer,dict) and 'broker_error' in answer)}
                    except Exception as exc:
                        call['failure_kind']=type(exc).__name__
                        call['cancellation_before_failure']=cancel.is_set()
                        # Only fixed protocol cancellation codes are retained. Do
                        # not serialize arbitrary exception text from handlers.
                        cooperative_cancel=(cancel.is_set() and isinstance(exc,RuntimeError)
                                            and str(exc) in ['cancelled','driver_cancelled_or_deadline'])
                        call['failure_class']='cancelled' if cooperative_cancel else 'infrastructure'
                        if cooperative_cancel: call['failure_code']=str(exc)
                        interrupt('handler_cancelled' if cooperative_cancel else 'handler_failed_or_cancelled')
                        response={'contentItems':[{'type':'inputText','text':'Admitted operation failed or cancelled'}],'success':False}
                    call['finished_seconds']=round(time.monotonic()-started,3)
                    wire=encoded(response)
                    call['wire_response_bytes']=len(wire);call['wire_response_sha256']=hashlib.sha256(wire).hexdigest()
                    record({'tool_call':call})
                    transport.send({'id':message['id'],'result':response})
            if method=='thread/tokenUsage/updated':
                report['usage']=params.get('tokenUsage') # replacement, never sum cumulative updates
                total=(report['usage'] or {}).get('total',{})
                if total.get('outputTokens',0)>=max_output_tokens:report['output_threshold_reached']=True;interrupt('output_token_threshold')
            if method=='item/completed':
                item=params.get('item',{}); entry['item_type']=item.get('type')
                if item.get('type')=='agentMessage':
                    text=item.get('text','')
                    if len(text.encode())>MAX_LINE:interrupt('final_text_budget');raise RuntimeError('final_text_budget')
                    report['text']=text
            record(entry)
            if method=='turn/completed':
                report['turn_status']=params.get('turn',{}).get('status')
                report['turn_error']=params.get('turn',{}).get('error')
                report['usage_coverage']=('reported_total' if report['turn_status']=='completed' and not cancel.is_set() else 'partial_reported_before_interruption') if report['usage'] is not None else 'unknown'
                report['status']='completed' if report['turn_status']=='completed' and not cancel.is_set() else 'interrupted_or_failed'
                break
    except Exception as exc:
        interrupt('participant_failure');report['failure_kind']=type(exc).__name__
        if isinstance(exc,RuntimeError):report['failure_code']=str(exc)
    finally:
        # The handler has returned on this thread; do not detach gateway work.
        finished.set();watcher.join(timeout=WRITE_SECONDS+1)
        report['watchdog_joined']=not watcher.is_alive()
        if report['usage'] is not None and report['status']!='completed':report['usage_coverage']='partial_reported_before_interruption'
        report['task_status']=report['status']
        try: report['cleanup']=transport.close()
        except Exception as exc:
            # Last defensive boundary: close() itself should retain phase errors.
            report['cleanup']={'cleanup_errors':[{'code':'transport_close_failed','kind':type(exc).__name__}],
                               'parent_joined':False,'inspection_complete':False,
                               'remaining_observed_pids':None,'reader_joined':False}
        cleanup=report['cleanup']
        if cleanup.get('parent_joined') is True and cleanup.get('reader_joined') is True and cleanup.get('remaining_observed_pids')==[] and cleanup.get('inspection_complete') is True:
            try:
                neutral.rmdir()
                report['neutral_directory_disposition']='removed_empty_after_join'
            except OSError:
                report['neutral_directory_disposition']='preserved_nonempty_or_unremovable'
        else:
            report['neutral_directory_disposition']='preserved_cleanup_uncertain'
        report['cleanup_failed']=bool(not report['watchdog_joined'] or cleanup.get('remaining_observed_pids') or
            not cleanup.get('reader_joined') or cleanup.get('transport_failure') or cleanup.get('forced_parent_stop') or
            cleanup.get('forced_owned_host_stop') or cleanup.get('cleanup_errors') or
            cleanup.get('inspection_complete') is not True or cleanup.get('parent_joined') is not True or cleanup.get('remaining_observed_pids') is None)
        report['infrastructure_failed']=report['cleanup_failed'] or 'failure_kind' in report or any(call.get('failure_class')=='infrastructure' for call in report['tool_calls'])
        if report['cleanup_failed']: report['status']='cleanup_failed'
        if report['stop_reason'] is None:
            report['stop_reason']='turn_completed' if report['task_status']=='completed' else 'turn_failed'
        for call in report['tool_calls']:
            call.pop('request',None);call.pop('response',None) # Bodies live once in bounded events.jsonl.
        report['elapsed_seconds']=round(time.monotonic()-started,3)
        report['logs_bytes']=log_bytes
        receipt_bytes=encoded(report)
        if len(receipt_bytes)+log_bytes>MAX_LOG:
            report['text']='';report['receipt_budget_exceeded']=True;report['infrastructure_failed']=True
            report['status']='receipt_budget_exceeded';receipt_bytes=encoded(report)
        (out/'receipt.json').write_bytes(receipt_bytes+b'\n')
    return report
