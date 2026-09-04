import json
import hashlib
import subprocess
import sys
import os
from pathlib import Path
import queue
import tempfile
import threading
import time
import unittest
from unittest.mock import patch, Mock
import participant as p

TOOLS=[{'name':'echo','description':'Fixed test echo','inputSchema':{'type':'object','properties':{},'additionalProperties':False}}]

class FakeTransport:
    scenario='ok'
    def __init__(self,args,cwd,cancel):
        self.q=queue.Queue();self.cancel=cancel;self.closed=False;self.sent=[];self.failure=None
        FakeTransport.instance=self
    def send(self,message):
        self.sent.append(message);method=message.get('method');ident=message.get('id')
        def reply(value):self.q.put({'id':ident,'result':value})
        if method=='initialize':reply({})
        if method=='config/read':
            conf={'features':{k:False for k in p.DISABLED},'web_search':'disabled','project_doc_max_bytes':0,'project_doc_fallback_filenames':[],'mcp_servers':{}}
            conf['features'].update(code_mode_host=True,skip_host_skill_discovery=True,network_proxy=None)
            if self.scenario=='auth_elicitation':conf['features']['auth_elicitation']=True
            if self.scenario=='proxy_enabled':conf['features']['network_proxy']={'enabled':True}
            if self.scenario=='proxy_disabled':conf['features']['network_proxy']=False
            for key in p.GUARDS:
                row=conf;parts=key.split('.')
                for part in parts[:-1]:row=row.setdefault(part,{})
                row[parts[-1]]=False
            if self.scenario=='guard':conf['features']['shell_tool']=True
            if self.scenario in ['unknown_false','unknown_true']:conf['features']['unreviewed_native_feature']=self.scenario=='unknown_true'
            if self.scenario=='missing_feature':del conf['features']['apps']
            reply({'config':conf})
        if method=='thread/start':reply({'model':'gpt-5.6-sol','reasoningEffort':'medium','modelProvider':'other' if self.scenario=='provider' else 'openai','instructionSources':[],'runtimeWorkspaceRoots':[],'thread':{'id':'t'}})
        if method=='turn/start':
            reply({'turn':{'id':'u'}})
            if self.scenario=='dead':return
            self.q.put({'id':'call','method':'item/tool/call','params':{'tool':'bad' if self.scenario=='unknown' else 'echo','arguments':{'unexpected':1} if self.scenario=='extra' else {}}})
        if ident=='call' and 'result' in message:
            for output in [2,5]:self.q.put({'method':'thread/tokenUsage/updated','params':{'tokenUsage':{'total':{'outputTokens':output,'inputTokens':10}}}})
            if self.scenario=='many':
                self.q.put({'id':'call','method':'item/tool/call','params':{'tool':'echo','arguments':{}}})
            else:self.q.put({'method':'turn/completed','params':{'turn':{'status':'failed' if self.scenario in ['error','error_close'] else 'completed','error':{'message':'fixture failure'} if self.scenario in ['error','error_close'] else None}}})
        if method=='turn/interrupt':self.q.put({'method':'turn/completed','params':{'turn':{'status':'interrupted'}}})
    def is_alive(self):return self.scenario!='dead'
    def receive(self,timeout=.1):
        try:return self.q.get(timeout=timeout)
        except queue.Empty:return None
    def close(self):
        self.closed=True
        if self.scenario in ['close_error','error_close']:
            raise subprocess.CalledProcessError(1,['ps'],stderr='PRIVATE ERROR CANARY')
        return {'exit_code':0,'parent_joined':True,'inspection_complete':True,'cleanup_errors':[],'remaining_observed_pids':[],'reader_joined':True,'transport_failure':None,'forced_parent_stop':self.scenario=='forced'}

class Tests(unittest.TestCase):
    def run_fake(self,scenario='ok',handler=None,**kwargs):
        FakeTransport.scenario=scenario
        if handler is None:handler=lambda name,args,cancel:{'canary':'test'}
        with tempfile.TemporaryDirectory() as out, patch.object(p,'Transport',FakeTransport),patch.object(p,'configuration',lambda:[]):
            result=p.run_participant('test',TOOLS,handler,out,**kwargs)
            self.assertTrue(FakeTransport.instance.closed)
            self.assertEqual(json.loads((Path(out)/'receipt.json').read_text()),result)
            neutral=Path(result['neutral_cwd'])
            self.assertEqual(neutral.parent,Path('/private/tmp').resolve())
            if neutral.exists():neutral.rmdir() # Empty fake fixture only; production preserves uncertain cleanup.
            return result
    def test_normal_handler_on_caller_thread_and_cumulative_usage_not_added(self):
        caller=threading.get_ident()
        def handler(name,args,cancel):
            self.assertEqual(threading.get_ident(),caller);return {'ok':True}
        result=self.run_fake(handler=handler)
        self.assertEqual(result['status'],'completed');self.assertEqual(len(result['tool_calls']),1)
        self.assertEqual(result['usage']['total']['outputTokens'],5)
        self.assertEqual(result['tool_calls'][0]['response_bytes'],11)
    def test_invalid_guards_stop_before_turn(self):
        result=self.run_fake('guard');self.assertEqual(result['model_turns_sent'],0)
        self.assertEqual(result['failure_code'],'effective_guard_mismatch')
    def test_unknown_tool_denied_without_handler(self):
        def handler(*args):self.fail('handler invoked')
        result=self.run_fake('unknown',handler)
        self.assertEqual(result['tool_calls'],[]);self.assertEqual(result['status'],'interrupted_or_failed')
    def test_wall_cancel_reaches_blocked_handler_and_cleanup_waits(self):
        observed=[]
        def handler(name,args,cancel):
            self.assertTrue(cancel.wait(2));time.sleep(.03);observed.append('joined');return {}
        result=self.run_fake(handler=handler,wall_seconds=.08)
        self.assertEqual(observed,['joined']);self.assertTrue(result['admission_stopped']);self.assertEqual(result['stop_reason'],'wall_deadline')
        self.assertNotEqual(result['status'],'completed')
    def test_max_64_admissions(self):
        result=self.run_fake('many');self.assertEqual(len(result['tool_calls']),64)
        self.assertEqual(result['status'],'interrupted_or_failed')
    def test_output_threshold_stops_and_missing_usage_unknown(self):
        result=self.run_fake(max_output_tokens=1);self.assertTrue(result['output_threshold_reached']);self.assertEqual(result['usage_coverage'],'partial_reported_before_interruption')
        result=self.run_fake('unknown');self.assertEqual(result['usage_coverage'],'unknown')
    def test_dead_transport_fails_promptly(self):
        result=self.run_fake('dead');self.assertEqual(result['failure_code'],'transport_stopped')
        self.assertLess(result['elapsed_seconds'],1)
    def test_turn_error_preserved(self):
        result=self.run_fake('error');self.assertEqual(result['turn_error'],{'message':'fixture failure'})
        self.assertEqual(result['usage_coverage'],'partial_reported_before_interruption')
    def test_forced_cleanup_cannot_pass(self):
        result=self.run_fake('forced');self.assertEqual(result['status'],'cleanup_failed')
    def test_provider_mismatch_rejected_before_turn(self):
        result=self.run_fake('provider');self.assertEqual(result['model_turns_sent'],0)
        self.assertEqual(result['failure_code'],'identity_or_instructions_mismatch')
    def test_duplicate_or_open_tools_rejected(self):
        with self.assertRaises(ValueError):p.validate_tools(TOOLS+TOOLS)
        with self.assertRaises(ValueError):p.validate_tools([{'name':'x','description':'x','inputSchema':{'type':'object'}}])
    def test_non_json_response_fails_without_raw_error(self):
        result=self.run_fake(handler=lambda *args:{'oops':float('nan')})
        self.assertNotEqual(result['status'],'completed');self.assertEqual(result['tool_calls'][0]['failure_kind'],'ValueError')

    def test_extra_arguments_reach_retryable_broker_without_aborting(self):
        seen=[]
        def handler(name,args,cancel):
            seen.append(args)
            return {'broker_error':'unexpected_or_missing_argument','retryable':True}
        result=self.run_fake('extra',handler)
        self.assertEqual(seen,[{'unexpected':1}])
        self.assertEqual(result['status'],'completed')

    def test_nonreading_pipe_has_bounded_write_and_unlocks(self):
        from types import SimpleNamespace
        read,write=os.pipe();os.set_blocking(write,False)
        stream=os.fdopen(write,'wb',buffering=0)
        transport=p.Transport.__new__(p.Transport)
        transport.process=SimpleNamespace(stdin=stream)
        transport.lock=threading.Lock();transport.cancel=threading.Event();transport.failure=None
        try:
            while True:
                try:os.write(write,b'x'*4096)
                except BlockingIOError:break
            start=time.monotonic()
            with patch.object(p,'WRITE_SECONDS',.1):
                with self.assertRaises(RuntimeError):transport.send({'data':'x'*65536})
            self.assertLess(time.monotonic()-start,1)
            self.assertTrue(transport.cancel.is_set())
            self.assertTrue(transport.lock.acquire(timeout=.1));transport.lock.release()
        finally:stream.close();os.close(read)

    def test_unknown_feature_false_or_true_and_missing_key_reject_before_turn(self):
        for scenario in ['unknown_false','unknown_true','missing_feature']:
            with self.subTest(scenario=scenario):
                result=self.run_fake(scenario)
                self.assertEqual(result['model_turns_sent'],0)
                self.assertEqual(result['failure_code'],'effective_guard_mismatch')
                self.assertFalse(result['effective_guard_checks']['exact_feature_config'])

    def test_close_exception_still_writes_receipt_and_preserves_turn_failure(self):
        result=self.run_fake('error_close')
        self.assertEqual(result['task_status'],'interrupted_or_failed')
        self.assertEqual(result['turn_status'],'failed')
        self.assertEqual(result['turn_error'],{'message':'fixture failure'})
        self.assertTrue(result['cleanup_failed'])
        self.assertTrue(result['infrastructure_failed'])
        self.assertIsNone(result['cleanup']['remaining_observed_pids'])
        self.assertNotIn('PRIVATE ERROR CANARY',json.dumps(result))

    def test_cleanup_failure_preserves_completed_task_and_stop_reason(self):
        result=self.run_fake('close_error')
        self.assertEqual(result['task_status'],'completed')
        self.assertEqual(result['turn_status'],'completed')
        self.assertEqual(result['status'],'cleanup_failed')
        self.assertEqual(result['stop_reason'],'turn_completed')

    def test_prompt_tool_hashes_and_rpc_sequence_are_retained(self):
        with tempfile.TemporaryDirectory() as out, patch.object(p,'Transport',FakeTransport),patch.object(p,'configuration',lambda:[]):
            FakeTransport.scenario='ok'
            result=p.run_participant('test',TOOLS,lambda *args:{},out)
            self.assertEqual(result['prompt_sha256'],hashlib.sha256(b'test').hexdigest())
            self.assertEqual(result['tool_declarations_sha256'],hashlib.sha256(p.encoded(TOOLS)).hexdigest())
            events=[json.loads(line) for line in (Path(out)/'events.jsonl').read_text().splitlines()]
            self.assertEqual([x['request_method'] for x in events if x.get('matched_response')],['initialize','config/read','thread/start','turn/start'])
            self.assertEqual(result['logs_bytes'],(Path(out)/'events.jsonl').stat().st_size)

    def test_abandoned_output_events_neutral_or_receipt_reject_before_launch(self):
        for name in ['events.jsonl','neutral','receipt.json']:
            with self.subTest(name=name),tempfile.TemporaryDirectory() as out,patch.object(p,'Transport') as launch:
                prior=Path(out)/name
                if name=='neutral':prior.mkdir();(prior/'canary').write_text('old run')
                else:prior.write_text('old run')
                with self.assertRaisesRegex(ValueError,'output already exists'):
                    p.run_participant('test',TOOLS,lambda *args:{},out)
                launch.assert_not_called()
                self.assertEqual((prior/'canary' if prior.is_dir() else prior).read_text(),'old run')

    def test_committed_handler_result_survives_cancellation_race(self):
        def handler(name,args,cancel):
            cancel.set();return {'candidate':1,'sha256':'fixture'}
        with tempfile.TemporaryDirectory() as out,patch.object(p,'Transport',FakeTransport),patch.object(p,'configuration',lambda:[]):
            FakeTransport.scenario='ok'
            result=p.run_participant('test',TOOLS,handler,out)
            call=result['tool_calls'][0]
            self.assertTrue(call['cancellation_after_handler'])
            self.assertNotIn('failure_kind',call)
            self.assertFalse(result['infrastructure_failed'])
            events=[json.loads(line) for line in (Path(out)/'events.jsonl').read_text().splitlines()]
            event=next(x['tool_call'] for x in events if 'tool_call' in x)
            self.assertEqual(event['response'],{'candidate':1,'sha256':'fixture'})
            self.assertNotEqual(result['status'],'completed')

    def test_stop_reasons_distinguish_budget_denial_and_handler_failure(self):
        self.assertEqual(self.run_fake('unknown')['stop_reason'],'unadmitted_request')
        self.assertEqual(self.run_fake('many')['stop_reason'],'tool_call_limit')
        self.assertEqual(self.run_fake(max_output_tokens=1)['stop_reason'],'output_token_threshold')
        def fail(*args):raise RuntimeError('PRIVATE HANDLER ERROR')
        result=self.run_fake(handler=fail)
        self.assertEqual(result['stop_reason'],'handler_failed_or_cancelled')
        self.assertNotIn('PRIVATE HANDLER ERROR',json.dumps(result))

    def test_byte_queue_backpressure_resumes_and_close_unblocks(self):
        q=p.ByteQueue(capacity=8)
        self.assertTrue(q.put(b'{"a":1}'))
        done=threading.Event()
        thread=threading.Thread(target=lambda:(q.put(b'{}'),done.set()))
        thread.start()
        self.assertFalse(done.wait(.05))
        self.assertEqual(q.get(.1),{'a':1})
        self.assertTrue(done.wait(1));thread.join()
        self.assertEqual(q.get(.1),{})
        q.put(b'{"a":1}')
        results=[]
        thread=threading.Thread(target=lambda:results.append(q.put(b'{}')))
        thread.start();q.close();thread.join(timeout=1)
        self.assertFalse(thread.is_alive());self.assertEqual(results,[False])
        self.assertLessEqual(q.bytes,8)

    def test_real_pipe_more_than_32_notifications_wait_for_consumer(self):
        with tempfile.TemporaryDirectory() as out,patch.object(p.Transport,'processes',return_value={}):
            transport=p.Transport([sys.executable,'-c','import sys; sys.stdout.write("{}\\n"*100); sys.stdout.flush(); sys.stdin.read()'],out,threading.Event())
            try:
                until=time.monotonic()+3
                while transport.messages.bytes<300 and time.monotonic()<until:time.sleep(.01)
                # Simulate a synchronous handler busy beyond the former .2s limit.
                time.sleep(.3)
                self.assertIsNone(transport.failure)
                self.assertEqual([transport.receive(.1) for _ in range(100)],[{}]*100)
            finally:cleanup=transport.close()
            self.assertTrue(cleanup['parent_joined']);self.assertTrue(cleanup['reader_joined'])
            self.assertEqual(cleanup['cleanup_errors'],[])

    def test_ps_failure_during_close_does_not_skip_owned_parent_join(self):
        def failed_ps():raise subprocess.CalledProcessError(1,['ps'],stderr='PRIVATE PS ERROR')
        with tempfile.TemporaryDirectory() as out,patch.object(p.Transport,'processes',side_effect=failed_ps):
            transport=p.Transport([sys.executable,'-c','import sys; sys.stdin.read()'],out,threading.Event())
            cleanup=transport.close()
            self.assertTrue(cleanup['parent_joined']);self.assertTrue(cleanup['reader_joined'])
            self.assertFalse(cleanup['inspection_complete'])
            self.assertIsNone(cleanup['remaining_observed_pids'])
            self.assertEqual(transport.process.poll(),0)
            self.assertNotIn('PRIVATE PS ERROR',json.dumps(cleanup))
            self.assertTrue(cleanup['cleanup_errors'])

    def test_monitor_snapshot_is_joined_before_reading_observed_processes(self):
        entered=threading.Event();release=threading.Event()
        def delayed_ps():
            if threading.current_thread() is not threading.main_thread():
                entered.set();self.assertTrue(release.wait(2))
            return {}
        with tempfile.TemporaryDirectory() as out,patch.object(p.Transport,'processes',side_effect=delayed_ps):
            transport=p.Transport([sys.executable,'-c','import sys; sys.stdin.read()'],out,threading.Event())
            self.assertTrue(entered.wait(1))
            timer=threading.Timer(.05,release.set);timer.start()
            cleanup=transport.close();timer.join()
            self.assertTrue(cleanup['reader_joined'])
            self.assertTrue(cleanup['inspection_complete'])
            self.assertEqual(cleanup['observed_processes'],[])

    def test_neutral_cwd_is_external_private_and_nonempty_evidence_is_preserved(self):
        with tempfile.TemporaryDirectory() as out,patch.object(p,'Transport',FakeTransport),patch.object(p,'configuration',lambda:[]):
            FakeTransport.scenario='ok'
            def handler(*args):
                neutral=Path(json.loads((Path(out)/'neutral').read_text())['cwd'])
                self.assertEqual(neutral.parent,Path('/private/tmp').resolve())
                self.assertEqual(neutral.stat().st_mode & 0o777,0o700)
                (neutral/'fixture').write_text('preserve')
                return {}
            result=p.run_participant('test',TOOLS,handler,out)
            neutral=Path(result['neutral_cwd'])
            try:
                self.assertEqual(result['neutral_directory_disposition'],'preserved_nonempty_or_unremovable')
                self.assertEqual((neutral/'fixture').read_text(),'preserve')
            finally:(neutral/'fixture').unlink();neutral.rmdir()

    def test_real_ps_failure_preserves_receipt_and_joins_actual_child(self):
        with tempfile.TemporaryDirectory() as out,patch.object(p,'configuration',return_value=[sys.executable,'-c','import sys; sys.stdin.read()']),patch.object(p.Transport,'processes',side_effect=OSError('PRIVATE PS ERROR')):
            result=p.run_participant('test',TOOLS,lambda *args:{},out)
            self.assertEqual(json.loads((Path(out)/'receipt.json').read_text()),result)
            self.assertTrue(result['cleanup']['parent_joined'])
            self.assertTrue(result['cleanup']['reader_joined'])
            self.assertTrue(result['cleanup_failed'])
            self.assertIn('failure_kind',result)
            self.assertNotIn('PRIVATE PS ERROR',json.dumps(result))
            Path(result['neutral_cwd']).rmdir() # Empty fixture only.

    def test_cooperative_cancellation_remains_distinct_from_infrastructure_error(self):
        def cancelled(name,args,cancel):
            cancel.set();raise RuntimeError('cancelled')
        result=self.run_fake(handler=cancelled)
        self.assertEqual(result['tool_calls'][0]['failure_class'],'cancelled')
        self.assertEqual(result['tool_calls'][0]['failure_code'],'cancelled')
        self.assertFalse(result['infrastructure_failed'])
        self.assertNotEqual(result['task_status'],'completed')

    def test_parent_terminate_error_still_attempts_kill_and_all_joins(self):
        import io
        from types import SimpleNamespace
        transport=p.Transport.__new__(p.Transport)
        process=SimpleNamespace(pid=123456,returncode=0,stdin=io.BytesIO(),stdout=io.BytesIO(),stderr=io.BytesIO(),
            wait=Mock(side_effect=[subprocess.TimeoutExpired('fixture',10),subprocess.TimeoutExpired('fixture',10),0]),
            terminate=Mock(side_effect=OSError('PRIVATE TERMINATE ERROR')),kill=Mock())
        transport.process=process;transport.lock=threading.Lock();transport.observed_lock=threading.Lock()
        transport.monitor_stop=threading.Event();transport.observed={};transport.messages=p.ByteQueue()
        transport.failure=None;transport.stderr_bytes=0;transport.stderr_retained=bytearray()
        transport.readers=[threading.Thread(target=lambda:None) for _ in range(3)]
        for reader in transport.readers:reader.start()
        with patch.object(p.Transport,'processes',side_effect=OSError('PRIVATE PS ERROR')):
            result=transport.close()
        process.terminate.assert_called_once();process.kill.assert_called_once()
        self.assertEqual(process.wait.call_count,3)
        self.assertTrue(result['parent_joined']);self.assertTrue(result['reader_joined'])
        self.assertTrue(result['forced_parent_stop']);self.assertFalse(result['inspection_complete'])
        self.assertNotIn('PRIVATE',json.dumps(result))
        self.assertTrue(process.stdout.closed);self.assertTrue(process.stderr.closed)

    def test_connector_elicitation_effect_enabled_rejects_before_turn(self):
        result=self.run_fake('auth_elicitation')
        self.assertEqual(result['model_turns_sent'],0)
        self.assertFalse(result['effective_guard_checks']['exact_feature_config'])

    def test_network_proxy_drift_is_rejected_without_disabling_security_setting(self):
        for scenario in ['proxy_enabled','proxy_disabled']:
            with self.subTest(scenario=scenario):
                result=self.run_fake(scenario)
                self.assertEqual(result['model_turns_sent'],0)
                self.assertFalse(result['effective_guard_checks']['exact_feature_config'])

    def test_config_explicitly_disables_source_reviewed_extras_and_does_not_override_proxy(self):
        with tempfile.TemporaryDirectory() as config_home,patch.dict(os.environ,{'CODEX_HOME':config_home}):
            args=p.configuration()
        settings=args[4::2]
        for key in ['auth_elicitation','background_paginated_rollout_migration','js_repl','mcp_2026_07_28','mentions_v2','remote_control']:
            self.assertIn('features.'+key+'=false',settings)
        self.assertFalse(any(setting.startswith('features.network_proxy=') for setting in settings))

    def test_unjoined_buffered_reader_retains_owned_descriptor_and_returns_bounded(self):
        import io
        from types import SimpleNamespace
        read_fd,write_fd=os.pipe()
        stdout=os.fdopen(read_fd,'rb')
        transport=p.Transport.__new__(p.Transport)
        transport.process=SimpleNamespace(pid=123456,returncode=0,stdin=io.BytesIO(),
            stdout=stdout,stderr=io.BytesIO(),wait=Mock(return_value=0))
        transport.lock=threading.Lock();transport.observed_lock=threading.Lock()
        transport.monitor_stop=threading.Event();transport.observed={};transport.messages=p.ByteQueue()
        transport.cancel=threading.Event();transport.failure=None
        transport.stderr_bytes=0;transport.stderr_retained=bytearray()
        transport.readers=[threading.Thread(target=transport.read_stdout,daemon=True),
                           threading.Thread(target=lambda:None,daemon=True),
                           threading.Thread(target=lambda:None,daemon=True)]
        for reader in transport.readers:reader.start()
        try:
            started=time.monotonic()
            with patch.object(p.Transport,'processes',return_value={}),patch.object(p.os,'close',wraps=os.close) as raw_close:
                result=transport.close()
                raw_close.assert_not_called()
            self.assertLess(time.monotonic()-started,2.5)
            self.assertTrue(result['parent_joined'])
            self.assertFalse(result['reader_joined']);self.assertFalse(result['inspection_complete'])
            self.assertIn('reader_join_timeout',[error['code'] for error in result['cleanup_errors']])
            self.assertTrue(transport.readers[0].is_alive())
            self.assertFalse(stdout.closed)
            self.assertEqual(stdout.fileno(),read_fd)
            os.fstat(read_fd) # The BufferedReader still owns an open descriptor.
        finally:
            os.close(write_fd) # Fixture releases EOF; production never borrows/closes reader FD.
            transport.readers[0].join(timeout=2)
            self.assertFalse(transport.readers[0].is_alive())
            stdout.close();transport.process.stderr.close()

if __name__=='__main__':unittest.main()
