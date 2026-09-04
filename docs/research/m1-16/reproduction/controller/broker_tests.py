"""No model, Docker or Rust execution. Real filesystem + fake driver boundaries."""
import copy
import json
import os
from pathlib import Path
import signal
import stat
import tempfile
import threading
import unittest
from unittest.mock import patch

import broker

INITIAL = {'Cargo.toml': '[package]\nname="fixture"\nversion="0.1.0"\nedition="2024"\n',
           'Cargo.lock': 'version = 4\n[[package]]\nname="fixture"\nversion="0.1.0"\n',
           'src/lib.rs': 'pub fn value() -> u32 { 1 }\n'}


def declarations():
    fields = {'project_ref': {'type': 'string'}, 'timeout': {'type': 'integer'},
              'lint_profile': {'type': 'string'}, 'path': {'type': 'string'},
              'profile': {'type': 'string'}}
    return [{'name': name, 'description': 'unchanged '+name,
             'inputSchema': {'type': 'object', 'additionalProperties': False,
                             'properties': copy.deepcopy(fields)}} for name in broker.TOOLS]


class FakeDriver:
    def __init__(self, root, arm='A'):
        self.config = {'root': str(root), 'mode': 'raw' if arm == 'A' else 'mcp'}
        self.calls = []
        self.cancelled = False
        self.result = {'content': [{'type': 'text', 'text': 'compiler evidence'}],
                       'structuredContent': {'outcome': 'failed'}, 'isError': False}
        self.discovered = declarations()
        self.hook = None

    def request(self, payload, cancel):
        self.calls.append(copy.deepcopy(payload))
        if self.hook:
            self.hook()
        if payload['op'] == 'tools':
            return {'tools': copy.deepcopy(self.discovered)}
        return copy.deepcopy(self.result)

    def cancel_and_join(self):
        self.cancelled = True


class Cases(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='m116-broker-')
        # macOS /var is symlinked; supply the host-selected physical parent.
        self.parent = Path(self.temp.name).resolve()
        os.chmod(self.parent, 0o700)
        self.workspace = broker.Workspace(self.parent, INITIAL)
        self.cancel = threading.Event()

    def tearDown(self):
        self.workspace.close()
        self.temp.cleanup()

    def build(self, arm='A', **kwargs):
        driver = FakeDriver(self.workspace.root, arm)
        instance = broker.Broker(arm, self.workspace, driver, {'records': ['same facts']},
                                 allow_project_code=True, **kwargs)
        return instance, driver

    def test_exact_closure_and_private_separate_artifacts(self):
        self.assertEqual(self.workspace.read('src/lib.rs'), INITIAL['src/lib.rs'])
        self.assertEqual(stat.S_IMODE(self.workspace.root.stat().st_mode), 0o700)
        self.assertNotIn(self.workspace.root, self.workspace.artifacts.parents)
        for path in ('../Cargo.toml', '/etc/passwd', 'tests/behavior.rs', 'README.md'):
            with self.assertRaises(broker.Denied):
                self.workspace.read(path)

    def test_patch_saved_exactly_and_immutable_files_untouched(self):
        instance, _ = self.build()
        answer = instance.handle('submit_patch', {'source': '//é\npub fn value()->u32{2}\n'}, self.cancel)
        self.assertEqual(answer['candidate'], 1)
        self.assertEqual((self.workspace.artifacts/'candidate-01.rs').read_text(), self.workspace.read('src/lib.rs'))
        self.assertEqual(self.workspace.read('Cargo.toml'), INITIAL['Cargo.toml'])
        self.assertEqual(self.workspace.read('Cargo.lock'), INITIAL['Cargo.lock'])

    def test_utf8_byte_bound_and_shared_six_candidates(self):
        instance, _ = self.build()
        invalid = instance.handle('submit_patch', {'source': 'é'*16385}, self.cancel)
        self.assertTrue(invalid['retryable'])
        for n in range(3):
            instance.handle('submit_patch', {'source': str(n)}, self.cancel)
            instance.handle('submit_selection', {'name': 'serde', 'version': '1.0.0', 'evidence': 'snapshot'}, self.cancel)
        self.assertIn('candidate_budget', instance.handle('submit_patch', {'source': '7'}, self.cancel)['broker_error'])
        self.assertEqual([r['candidate'] for r in instance.receipt()['candidates']], list(range(1, 7)))
        self.assertEqual(len(list(self.workspace.artifacts.glob('selection-*.json'))), 3)

    def test_symlink_hardlink_fifo_denied(self):
        source = self.workspace.root/'src/lib.rs'
        source.unlink()
        source.symlink_to('/etc/passwd')
        with self.assertRaises(broker.BrokerError): self.workspace.read('src/lib.rs')
        source.unlink()
        os.link(self.workspace.root/'Cargo.toml', source)
        with self.assertRaises(broker.BrokerError): self.workspace.read('src/lib.rs')
        source.unlink()
        os.mkfifo(source)
        with self.assertRaises(broker.BrokerError): self.workspace.read('src/lib.rs')

    def test_root_rename_and_src_replacement_fail_closed(self):
        self.workspace.root.rename(self.workspace.root.with_name('moved'))
        self.workspace.root.mkdir(mode=0o700)
        with self.assertRaises(broker.BrokerError): self.workspace.submit('replacement')

    def test_immutable_manifest_change_denied(self):
        (self.workspace.root/'Cargo.toml').write_text('replaced')
        with self.assertRaises(broker.BrokerError): self.workspace.snapshot()

    def test_raw_quality_uses_four_fixed_commands_and_one_budget(self):
        instance, driver = self.build()
        answer = instance.handle('raw_validate', {'stage': 'quality'}, self.cancel)
        self.assertEqual([r['command'] for r in driver.calls], ['fmt', 'check', 'clippy', 'test'])
        self.assertTrue(answer['lock_audit_fact']['std_only_locked_closure'])
        self.assertFalse(answer['lock_audit_fact']['cargo_audit_executed'])
        self.assertEqual(len(instance.receipt()['validation_requests']), 1)
        for request in driver.calls:
            self.assertEqual([f['path'] for f in request['files']], list(broker.FILES))
        for _ in range(5): instance.handle('raw_validate', {'stage': 'check'}, self.cancel)
        before = len(driver.calls)
        self.assertTrue(instance.handle('raw_validate', {'stage': 'test'}, self.cancel)['retryable'])
        self.assertEqual(before, len(driver.calls))

    def test_b_exact_discovery_mapping_and_unchanged_sdk_payload(self):
        instance, driver = self.build('B')
        tools = instance.tools(self.cancel)
        mapped = {t['name']: t for t in tools}
        self.assertEqual(len(tools), 17)
        self.assertNotIn('raw_validate', mapped)
        self.assertNotIn('read_catalog_facts', mapped)
        for original in driver.discovered:
            tool = mapped[original['name'].replace('.', '_')]
            self.assertEqual(tool['inputSchema'], original['inputSchema'])
        args = {'project_ref': 'prj_abc', 'lint_profile': 'strict'}
        result = instance.handle('rust_clippy', args, self.cancel)
        self.assertEqual(result, driver.result)
        self.assertEqual(driver.calls[-1], {'op': 'call', 'name': 'rust.clippy', 'arguments': args})

    def test_protocol_denials_retry_without_rewrite_or_validation_charge(self):
        instance, driver = self.build('B')
        instance.tools(self.cancel)
        for name, args in [('rust_test', {'timeout': 31}), ('rust_test', {'timeout': True}),
                           ('rust_clippy', {}), ('rust_clippy', {'lint_profile': 'default'}),
                           ('raw_validate', {'stage': 'check'}),
                           ('resource_read', {'uri': 'file:///etc/passwd'})]:
            self.assertTrue(instance.handle(name, args, self.cancel)['retryable'])
        self.assertEqual(len(driver.calls), 1)
        self.assertEqual(instance.receipt()['validation_requests'], [])
        instance.handle('rust_test', {'timeout': 30}, self.cancel)
        self.assertEqual(driver.calls[-1]['arguments'], {'timeout': 30})

    def test_catalog_projection_is_copied_and_only_parent_supplied(self):
        instance, _ = self.build()
        result = instance.handle('read_catalog_facts', {}, self.cancel)
        self.assertEqual(result, {'records': ['same facts']})
        result['records'].append('mutated')
        self.assertEqual(instance.handle('read_catalog_facts', {}, self.cancel), {'records': ['same facts']})

    def test_cancellation_before_and_after_driver_joins(self):
        instance, driver = self.build()
        self.cancel.set()
        with self.assertRaises(broker.BrokerError): instance.handle('read_catalog_facts', {}, self.cancel)
        self.assertTrue(driver.cancelled)
        self.cancel.clear()
        driver.cancelled = False
        driver.hook = self.cancel.set
        with self.assertRaises(broker.BrokerError): instance.handle('raw_validate', {'stage': 'quality'}, self.cancel)
        self.assertTrue(driver.cancelled)
        self.assertEqual(len(driver.calls), 1)

    def test_global_requests_and_explicit_host_consent(self):
        instance, driver = self.build()
        instance.allow_project_code = False
        self.assertIn('consent', instance.handle('raw_validate', {'stage': 'check'}, self.cancel)['broker_error'])
        self.assertEqual(driver.calls, [])
        for _ in range(63): instance.handle('read_catalog_facts', {}, self.cancel)
        self.assertFalse(instance.handle('read_catalog_facts', {}, self.cancel)['retryable'])
        self.assertEqual(instance.receipt()['admitted_requests'], 64)

    def test_committed_submission_survives_cancellation_before_delivery(self):
        instance, driver = self.build()
        original = self.workspace.submit
        def submit_then_cancel(source):
            result = original(source)
            self.cancel.set()
            return result
        with patch.object(self.workspace, 'submit', side_effect=submit_then_cancel):
            answer = instance.handle('submit_patch', {'source': INITIAL['src/lib.rs']}, self.cancel)
        self.assertEqual(answer['candidate'], 1)
        self.assertEqual(answer['sha256'], instance.receipt()['candidates'][0]['sha256'])
        self.assertTrue(driver.cancelled)

    def test_response_budget_is_infrastructure_failure_no_truncation(self):
        instance, driver = self.build()
        driver.result = {'stdout': 'x'*broker.MAX_REPLY}
        with self.assertRaisesRegex(broker.BrokerError, 'response_budget'):
            instance.handle('raw_validate', {'stage': 'check'}, self.cancel)
        self.assertEqual(len(instance.receipt()['validation_requests'][0]['result']['stdout']), broker.MAX_REPLY)

    def test_explain_fixed_command_and_code_only(self):
        instance, driver = self.build()
        instance.handle('raw_explain', {'code': 'E0502'}, self.cancel)
        self.assertEqual(driver.calls[-1]['command'], 'explain')
        self.assertEqual(driver.calls[-1]['code'], 'E0502')
        self.assertTrue(instance.handle('raw_explain', {'code': '--help'}, self.cancel)['retryable'])

    def test_missing_mcp_tool_fails_discovery(self):
        instance, driver = self.build('B')
        driver.discovered.pop()
        with self.assertRaises(broker.BrokerError): instance.tools(self.cancel)


class Transport(unittest.TestCase):
    def make_driver(self):
        driver = broker.Driver.__new__(broker.Driver)
        driver.pending = bytearray()
        driver.lines = []
        driver.total = driver.stderr_total = 0
        driver.stderr_prefix = bytearray()
        driver.selector = broker.selectors.DefaultSelector()
        read, write = os.pipe()
        os.set_blocking(read, False)
        stream = os.fdopen(read, 'rb', buffering=0)
        driver.selector.register(stream, broker.selectors.EVENT_READ, 'out')
        self.addCleanup(stream.close)
        self.addCleanup(os.close, write)
        self.addCleanup(driver.selector.close)
        return driver, write

    def test_incremental_line_decode_and_line_cap(self):
        driver, write = self.make_driver()
        os.write(write, b'{"ok":')
        driver._pump(.01)
        self.assertEqual(driver.lines, [])
        os.write(write, b'true}\n')
        driver._pump(.01)
        self.assertEqual(json.loads(driver.lines[0]), {'ok': True})
        driver.pending = bytearray(b'x'*(broker.MAX_LINE-2))
        os.write(write, b'xx')
        with self.assertRaises(broker.BrokerError): driver._pump(.01)

    def test_response_total_budget(self):
        driver, write = self.make_driver()
        driver.total = broker.MAX_TOTAL
        os.write(write, b'x')
        with self.assertRaisesRegex(broker.BrokerError, 'total_budget'): driver._pump(.01)


    def test_spurious_readiness_preserves_later_response(self):
        driver, write = self.make_driver()
        os.write(write, b'{"ready":true}\n')
        with patch('broker.os.read', side_effect=BlockingIOError):
            driver._pump(.01)
        self.assertEqual(driver.lines, [])
        driver._pump(.01)
        self.assertEqual(json.loads(driver.lines[0]), {'ready':True})


class Cleanup(unittest.TestCase):
    def driver(self, running=False, receipt=None):
        from io import BytesIO
        from unittest.mock import Mock
        driver = broker.Driver.__new__(broker.Driver)
        driver.cleanup = None
        driver.config = {'mode':'mcp'}
        driver.cleanup_seconds = 0
        driver.pending = bytearray()
        driver.lines = []
        driver.total = driver.stderr_total = 0
        driver.stderr_prefix = bytearray()
        driver.selector = broker.selectors.DefaultSelector()
        out, writer = os.pipe()
        if receipt:
            os.write(writer, broker.encoded(receipt)+b'\n')
        os.close(writer)
        err, writer = os.pipe()
        os.close(writer)
        process = Mock()
        process.pid = 43210
        process.stdin = BytesIO()
        process.stdout = os.fdopen(out, 'rb', buffering=0)
        process.stderr = os.fdopen(err, 'rb', buffering=0)
        for stream, name in ((process.stdout, 'out'), (process.stderr, 'err')):
            os.set_blocking(stream.fileno(), False)
            driver.selector.register(stream, broker.selectors.EVENT_READ, name)
        state = [None if running else 0]
        process.poll.side_effect = lambda: state[0]
        process.kill.side_effect = lambda: state.__setitem__(0, -9)
        process.wait.side_effect = lambda **_: state[0]
        driver.process = process
        return driver

    def test_normal_close_requires_ack_and_exit(self):
        driver = self.driver(receipt={'closed': True, 'execution_joined': True, 'server_joined': True, 'cleanup_uncertain': False})
        result = driver._join(False)
        self.assertFalse(result['cleanup_failed'])
        self.assertTrue(result['server_cleanup_verified'])

    def test_forced_stop_never_passes_even_with_join_claim(self):
        driver = self.driver(running=True, receipt={'closed': True, 'execution_joined': True, 'server_joined': True, 'cleanup_uncertain': False})
        with patch('broker.os.kill') as kill:
            result = driver.cancel_and_join()
        kill.assert_called_once_with(43210, signal.SIGINT)
        driver.process.kill.assert_called_once()
        self.assertTrue(result['cleanup_failed'])
        self.assertTrue(result['forced_driver_stop'])
        self.assertFalse(result['execution_joined'])

    def test_exit_without_cleanup_receipt_is_unknown_failure(self):
        result = self.driver()._join(True)
        self.assertTrue(result['cleanup_failed'])
        self.assertFalse(result['server_cleanup_verified'])

    def test_server_wait_failure_cannot_pass_from_execution_ack(self):
        driver=self.driver(receipt={'closed':True,'execution_joined':True,'server_joined':False,'cleanup_uncertain':False})
        result=driver._join(False)
        self.assertTrue(result['cleanup_failed'])
        self.assertFalse(result['server_cleanup_verified'])

    def test_raw_has_no_server_cleanup_claim(self):
        driver=self.driver(receipt={'closed':True,'execution_joined':True,'server_joined':False,'cleanup_uncertain':False})
        driver.config={'mode':'raw'}
        result=driver._join(False)
        self.assertFalse(result['cleanup_failed'])
        self.assertIsNone(result['server_cleanup_verified'])

    def test_cleanup_uncertainty_or_missing_flag_cannot_pass_join(self):
        for flag in [True,None]:
            ack={'closed':True,'execution_joined':True,'server_joined':True}
            if flag is not None:ack['cleanup_uncertain']=flag
            result=self.driver(receipt=ack)._join(False)
            self.assertTrue(result['cleanup_failed'])

    def test_cancellation_preserves_other_driver_errors_as_failure(self):
        for error, uncertain, expected in [('cancelled',False,False),('gateway_cleanup_uncertain',True,True),('gateway_execution_or_cleanup',False,True)]:
            driver=self.driver(receipt={'driver_error':error,'execution_joined':True,'server_joined':True,'cleanup_uncertain':uncertain})
            driver.process.poll.side_effect=lambda:1
            driver.process.wait.side_effect=lambda **_:1
            result=driver._join(True)
            self.assertEqual(result['cleanup_failed'],expected)
            self.assertEqual(result['terminal_error'],error)


if __name__ == '__main__':
    unittest.main()
