#!/usr/bin/env python3
"""Independent post-participant evaluation. Never returns hidden results to agents.

Selection evaluation is pure. Repair --execute-oracles starts fixed raw Gateway
stages and one real MCP audit per distinct first/final candidate SHA. Both arms use
the same path. No candidate is accepted just because it compiles. Final acceptance
requires a principal manual review bound to candidate SHA and all recorded gates.
Use --finalize EXISTING_EVALUATION --manual-review REVIEW for that separate step.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import sys
import threading
import time

import broker
import containment

spec = importlib.util.spec_from_file_location('study_runner', Path(__file__).with_name('run-study.py'))
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)

REPAIR_REVIEW = ('public_signature', 'required_behavior', 'no_unsafe', 'no_lint_suppression',
                 'no_source_tests', 'no_hardcoded_oracle_cases', 'authorized_edits')
SELECTION_REVIEW = ('api_feature_caveat', 'unknowns_not_promoted', 'no_integration_claim',
                    'no_safety_or_legal_claim', 'accurate_evidence_interpretation')


def corpus_date(projection):
    timestamp = projection.get('provenance', {}).get('observed_at')
    if type(timestamp) is not int:
        raise ValueError('corpus_date_missing')
    return datetime.fromtimestamp(timestamp, timezone.utc).date().isoformat()


def selection_candidate(candidate, label, projection, corpus_date):
    selection = candidate.get('selection', {})
    identity = str(selection.get('name', ''))+'@'+str(selection.get('version', ''))
    evidence = selection.get('evidence', '')
    if not isinstance(evidence, str):
        evidence = ''
    record = next((r for r in projection['records'] if r['name'] == selection.get('name')), None)
    version = next((v for v in record['versions'] if v['version'] == selection.get('version')), None) if record else None
    checks = {'accepted_exact_identity': identity in label['accepted_identities'],
              'recorded_exact_version': version is not None,
              'snapshot_fingerprint_cited': projection['snapshot_fingerprint'] in evidence,
              'provenance_source_id_cited': projection['provenance']['source_id'] in evidence,
              'corpus_date_cited': corpus_date in evidence}
    if version:
        checks.update(declared_msrv_cited=bool(version['rust_version'] and version['rust_version'] in evidence),
                      declared_license_cited=bool(version['license'] and version['license'] in evidence))
        def msrv(text):
            if not isinstance(text, str) or not re.fullmatch(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(\.(0|[1-9][0-9]*))?', text):
                return None
            values = tuple(map(int, text.split('.')))
            return values+(0,)*(3-len(values))
        declared, maximum = msrv(version['rust_version']), msrv(label['declared_msrv_lte'])
        checks['declared_msrv_within_constraint'] = declared is not None and maximum is not None and declared <= maximum
        # Labels preselect accepted licenses. Avoid pretending to implement SPDX
        # legal interpretation with substring checks.
        checks['accepted_identity_license_binding'] = checks['accepted_exact_identity'] and bool(version['license'])
    else:
        checks.update(declared_msrv_cited=False, declared_license_cited=False,
                      declared_msrv_within_constraint=False, accepted_identity_license_binding=False)
    passed = all(checks.values())
    return {'candidate': candidate.get('candidate'), 'sha256': candidate.get('sha256'),
            'kind': 'selection', 'selection': selection, 'identity': identity,
            'deterministic_checks': checks, 'deterministic_passed': passed,
            'predicate_version': 'v2_accessible_snapshot_and_provenance_not_v1_raw_source_hash',
            'manual_review_required': list(SELECTION_REVIEW),
            'final_success': None if passed else False,
            'status': 'pending_blinded_review' if passed else 'failed_deterministic_predicate'}


def sdk_data(result):
    if not isinstance(result, dict) or result.get('isError') is True or 'mcp_error' in result:
        raise ValueError('mcp_oracle_error')
    content = result.get('structuredContent')
    if not isinstance(content, dict):
        raise ValueError('mcp_structured_result_missing')
    return content


def raw_pass(result):
    return (result.get('termination') == 'exited' and type(result.get('exit_code')) is int
            and result['exit_code'] == 0 and result.get('stdout_truncated') is False
            and result.get('stderr_truncated') is False)


def repair_candidate(candidate, config, item, output, cancel):
    output.mkdir(mode=0o700)
    artifact = Path(candidate['artifact_path'])
    if runner.digest(artifact) != candidate['sha256']:
        raise ValueError('candidate_artifact_hash_mismatch')
    if artifact.stat().st_size > 32768:
        raise ValueError('candidate_artifact_budget')
    base = Path(config['corpus_root'])/'repair'/item
    files = {name: runner.read_bytes(base/'initial'/name).decode('utf-8') for name in broker.FILES}
    files['src/lib.rs'] = runner.read_bytes(artifact, 32768).decode('utf-8')
    hidden = runner.read_bytes(base/'hidden/behavior.rs').decode('utf-8')
    source = [{'path': name, 'text': text} for name, text in files.items()]
    source.append({'path': 'tests/behavior.rs', 'text': hidden})
    receipt = {'candidate': candidate['candidate'], 'sha256': candidate['sha256'], 'kind': 'patch',
               'stages': [], 'audit': None, 'manual_review_required': list(REPAIR_REVIEW),
               'final_success': None, 'status': 'infrastructure_failed'}
    workspace = driver = None
    started = time.monotonic()
    try:
        receipt['docker_before']=containment.observe(config['driver']['docker_socket'],output)
        if not receipt['docker_before']['absent']:raise ValueError('preexisting_execution_objects')
        workspace = broker.Workspace(output, files)
        state = output/'raw-state'
        state.mkdir(mode=0o700)
        driver = broker.Driver(runner.driver_config(config, workspace, 'raw', state, output/'raw.stderr'))
        for command in ('fmt', 'check', 'clippy', 'test'):
            beginning = time.monotonic()
            result = driver.request({'op': 'execute', 'files': source, 'command': command}, cancel)
            receipt['stages'].append({'stage': command, 'result': result,
                                      'passed': raw_pass(result),
                                      'elapsed_seconds': round(time.monotonic()-beginning, 3)})
        receipt['raw_cleanup'] = driver.close()
        if receipt['raw_cleanup'].get('cleanup_failed'):
            raise ValueError('raw_oracle_cleanup_failed')
        driver = None
        state = output/'mcp-state'
        state.mkdir(mode=0o700)
        driver = broker.Driver(runner.driver_config(config, workspace, 'mcp', state, output/'mcp.stderr'))
        discovered = driver.request({'op': 'tools'}, cancel)
        if {t['name'] for t in discovered['tools']} != set(broker.TOOLS):
            raise ValueError('oracle_discovery_mismatch')
        opened = sdk_data(driver.request({'op': 'call', 'name': 'rust.project.open',
                                        'arguments': {'path': str(workspace.root)}}, cancel))
        if opened.get('status') != 'passed':
            raise ValueError('oracle_project_open_failed')
        project = opened['data']['project_ref']
        # Metadata discovery belongs to the product and may be required before
        # audit. Preserve its result; it does not replace any requested raw stage.
        receipt['mcp_inspect'] = driver.request({'op': 'call', 'name': 'rust.project.inspect',
                                               'arguments': {'project_ref': project}}, cancel)
        result = driver.request({'op': 'call', 'name': 'rust.dependencies.audit',
                                 'arguments': {'project_ref': project}}, cancel)
        receipt['audit'] = {'result': result, 'passed': sdk_data(result).get('status') == 'passed'}
        receipt['mcp_cleanup'] = driver.close()
        if receipt['mcp_cleanup'].get('cleanup_failed'):
            raise ValueError('mcp_oracle_cleanup_failed')
        driver = None
        receipt['automated_gates_passed'] = all(s['passed'] for s in receipt['stages']) and receipt['audit']['passed']
        receipt['status'] = 'pending_principal_review' if receipt['automated_gates_passed'] else 'failed_gate'
        receipt['final_success'] = None if receipt['automated_gates_passed'] else False
    except BaseException as exc:
        receipt['failure_kind'] = type(exc).__name__
        if isinstance(exc, broker.BrokerError):
            receipt['failure_code'] = str(exc)
        if driver is not None:
            receipt['failure_cleanup'] = driver.cancel_and_join()
    finally:
        if driver is not None and driver.cleanup is None:
            receipt['failure_cleanup'] = driver.cancel_and_join()
        if workspace is not None:
            workspace.close()
        try:
            receipt['docker_after']=containment.observe(config['driver']['docker_socket'],output)
            if not receipt['docker_after']['absent']:
                receipt.update(status='cleanup_failed',final_success=False,automated_gates_passed=False)
        except Exception as exc:
            receipt['docker_after']={'absent':False,'observation_failure':type(exc).__name__}
            receipt.update(status='cleanup_failed',final_success=False,automated_gates_passed=False)
        receipt['elapsed_seconds'] = round(time.monotonic()-started, 3)
        runner.write_json(output/'result.json', receipt)
    return receipt


def evaluate_run(run_path, config, projection, execute_oracles, verify_inputs=None):
    run_path = Path(run_path)
    run = runner.read_json(run_path/'run.json')
    candidates = run.get('broker', {}).get('candidates', [])
    kind = 'patch' if run['item'].startswith('R') else 'selection'
    # The first submission is candidate1, including a wrong submission kind.
    # Do not silently skip a wrong-kind first attempt and award a later success.
    selected = [candidates[0], candidates[-1]] if candidates else []
    if kind == 'patch' and selected and not execute_oracles:
        raise ValueError('explicit_execute_oracles_required')
    labels = runner.read_json(Path(config['corpus_root'])/'selection/tasks-and-labels.json')
    label = next((item for item in labels if item['id'] == run['item'][:3]), None)
    if kind == 'selection' and label is None:
        raise ValueError('selection_label_not_found')
    date = corpus_date(projection) if kind == 'selection' else None
    output = run_path/'evaluation'
    output.mkdir(mode=0o700)
    result = {'run_id': run['run_id'], 'freeze_sha256': run['freeze_sha256'],
              'participant_status': run['status'], 'candidate_count': len(candidates),
              'first': None, 'final': None, 'candidates': [], 'final_success': False if not candidates else None}
    cache = {}
    result['corpus_date'] = date
    for role, candidate in zip(('first', 'final'), selected):
        key = (candidate.get('kind'), candidate.get('sha256'))
        if key not in cache:
            if candidate.get('kind') != kind:
                observation = {'candidate': candidate.get('candidate'), 'sha256': candidate.get('sha256'),
                               'kind': candidate.get('kind'), 'status': 'wrong_submission_kind', 'final_success': False}
            elif kind == 'selection':
                observation = selection_candidate(candidate, label, projection, date)
            else:
                observation = repair_candidate(candidate, config, run['item'], output/candidate['sha256'], threading.Event())
            cache[key] = len(result['candidates'])
            result['candidates'].append(observation)
        result[role] = {'candidate': candidate.get('candidate'), 'evaluation_index': cache[key],
                        'deduplicated': role == 'final' and selected[0].get('sha256') == candidate.get('sha256')}
    result['post_oracle_freeze_verified'] = None
    if verify_inputs is not None:
        try:
            verify_inputs()
            result['post_oracle_freeze_verified'] = True
        except Exception:
            result['post_oracle_freeze_verified'] = False
            result['infrastructure_failure'] = 'post_oracle_input_drift'
    runner.write_json(output/'evaluation.json', result)
    return result


def finalize(evaluation, reviews):
    result = json.loads(json.dumps(evaluation))
    if reviews.get('principal_approved') is not True:
        raise ValueError('principal_review_not_approved')
    review_map = {r['sha256']: r for r in reviews['candidates']}
    for candidate in result['candidates']:
        if candidate.get('final_success') is False:
            continue
        review = review_map.get(candidate['sha256'])
        required = candidate.get('manual_review_required', [])
        automated = candidate.get('automated_gates_passed', candidate.get('deterministic_passed')) is True
        candidate['final_success'] = bool(automated and result.get('post_oracle_freeze_verified') is True
                                           and review and required and
                                           all(review.get('checks', {}).get(k) is True for k in required))
        candidate['manual_review'] = review
        candidate['status'] = 'passed' if candidate['final_success'] else 'failed_or_review_incomplete'
    for role in ('first', 'final'):
        reference = result.get(role)
        result[role+'_success'] = (result['candidates'][reference['evaluation_index']]['final_success']
                                   if reference else False)
    # Infrastructure remains a distinct denominator, even if a captured patch
    # independently passes. Do not relabel a failed participant run as completed.
    result['infrastructure_status_preserved'] = result['participant_status']
    return result



def self_test():
    """Pure qualification; no subprocess, model, Docker or Rust invocation."""
    import unittest
    from unittest.mock import patch

    class Tests(unittest.TestCase):
        def sample(self):
            projection = {'snapshot_fingerprint': 'sha256:'+'a'*64,
                          'provenance': {'source_id': 'research:sample'},
                          'records': [{'name': 'sample', 'versions': [
                              {'version': '1.0.0', 'rust_version': '1.60', 'license': 'MIT OR Apache-2.0'}]}]}
            label = {'accepted_identities': ['sample@1.0.0'], 'declared_msrv_lte': '1.70',
                     'objective_oracle': {'source_hash_and_snapshot_date_required': True}}
            candidate = {'candidate': 1, 'sha256': 'b'*64, 'kind': 'selection', 'selection': {
                'name': 'sample', 'version': '1.0.0',
                'evidence': projection['snapshot_fingerprint']+' research:sample 2026-09-04 1.60 MIT OR Apache-2.0'}}
            return candidate, label, projection

        def test_schedule_24_exact_counterbalance(self):
            runs = runner.schedule()
            self.assertEqual(len(runs), 24)
            self.assertEqual(len({r['run_id'] for r in runs}), 24)
            self.assertEqual([runs[i]['run_id'] for i in range(0, 24, 2)],
                             ['R01-A', 'R02-B', 'R03-A', 'R04-B', 'S01-en-A', 'S01-es-B',
                              'S02-en-B', 'S02-es-A', 'S03-en-A', 'S03-es-B', 'S04-en-B', 'S04-es-A'])

        def test_initial_exact_three_files(self):
            self.assertEqual(set(runner.SELECTION_FILES), set(broker.FILES))
            self.assertNotIn('tests/behavior.rs', runner.SELECTION_FILES)

        def test_selection_still_requires_review(self):
            c, label, projection = self.sample()
            value = selection_candidate(c, label, projection, '2026-09-04')
            self.assertTrue(value['deterministic_passed'])
            self.assertIsNone(value['final_success'])
            self.assertEqual(value['selection'], c['selection'])

        def test_v1_source_hash_predicate_replaced(self):
            c, label, projection = self.sample()
            self.assertTrue(selection_candidate(c, label, projection, '2026-09-04')['deterministic_passed'])
            self.assertTrue(label['objective_oracle']['source_hash_and_snapshot_date_required'])

        def test_missing_snapshot_fails(self):
            c, label, projection = self.sample()
            c['selection']['evidence'] = 'raw README source hash'
            self.assertFalse(selection_candidate(c, label, projection, '2026-09-04')['final_success'])

        def test_corpus_date_comes_from_frozen_provenance(self):
            self.assertEqual(corpus_date({'provenance': {'observed_at': 1788539204}}), '2026-09-04')
            self.assertEqual(corpus_date({'provenance': {'observed_at': 0}}), '1970-01-01')
            for projection in [{}, {'provenance': {'observed_at': True}}]:
                with self.assertRaises(ValueError): corpus_date(projection)

        def test_catalog_setup_rejects_unavailable_and_wrong_identity(self):
            context = {'catalog': {'status':'available','value':{'fingerprint':'snapshot'}},
                       'model': {'status':'available','value':{'identity':{'model':'fixed'}}},
                       'semantic_index': {'status':'available','value':{'metadata':{'snapshot':'snapshot'},'documents':15}}}
            result = {'structuredContent': {'status':'passed','data':{'context':context}}}
            expected = {'catalog_fingerprint':'snapshot','model_identity':{'model':'fixed'},
                        'index_metadata':{'snapshot':'snapshot'},'documents':15}
            self.assertEqual(runner.check_catalog(result, expected), expected)
            with self.assertRaises(broker.BrokerError): runner.check_catalog(result, dict(expected, documents=14))
            context['semantic_index']['status'] = 'unavailable'
            with self.assertRaises(broker.BrokerError): runner.check_catalog(result, expected)

        def test_wrong_version_fails(self):
            c, label, projection = self.sample()
            c['selection']['version'] = '2.0.0'
            self.assertFalse(selection_candidate(c, label, projection, '2026-09-04')['final_success'])

        def test_raw_gate_exit_and_truncation(self):
            good = {'termination': 'exited', 'exit_code': 0, 'stdout_truncated': False, 'stderr_truncated': False}
            self.assertTrue(raw_pass(good))
            for key, value in [('termination', 'cancelled'), ('exit_code', True), ('stderr_truncated', True)]:
                self.assertFalse(raw_pass(dict(good, **{key: value})))

        def test_mcp_error_cannot_pass_audit(self):
            for value in [{'mcp_error': {}}, {'isError': True, 'structuredContent': {'status': 'passed'}}, {'content': []}]:
                with self.assertRaises(ValueError):
                    sdk_data(value)

        def test_review_requires_matching_hash_and_preserves_infra(self):
            c, label, projection = self.sample()
            observation = selection_candidate(c, label, projection, '2026-09-04')
            base = {'participant_status': 'cleanup_failed', 'post_oracle_freeze_verified': True, 'candidates': [observation],
                    'first': {'evaluation_index': 0}, 'final': {'evaluation_index': 0}}
            review = {'principal_approved': True, 'candidates': [{'sha256': 'b'*64,
                      'checks': {key: True for key in SELECTION_REVIEW}}]}
            self.assertTrue(finalize(base, review)['final_success'])
            self.assertEqual(finalize(base, review)['infrastructure_status_preserved'], 'cleanup_failed')
            review['candidates'][0]['sha256'] = 'c'*64
            self.assertFalse(finalize(base, review)['final_success'])

        def test_mock_run_persists_candidate_and_cleanup_without_process(self):
            import tempfile
            from unittest.mock import Mock
            with tempfile.TemporaryDirectory(prefix='m116-runner-test-') as temp:
                parent = Path(temp).resolve()
                os.chmod(parent, 0o700)
                config = {'results_parent': str(parent), 'expected_catalog': {}, 'driver': {'server_binary': '/frozen/server', 'docker_socket': '/frozen/socket'}}
                fake = Mock()
                fake.ready = {'server_pid': None}
                fake.config = {}
                fake.close.return_value = {'cleanup_failed': False, 'execution_joined': True}
                def create_driver(host):
                    if host['mode'] == 'mcp':
                        probe = Mock()
                        probe.close.return_value = {'cleanup_failed': False}
                        probe.request.return_value = {'qualification_stub': True}
                        return probe
                    fake.config = host
                    return fake
                def participant_stub(prompt, tools, handler, output_dir, **budgets):
                    self.assertEqual(budgets, {'wall_seconds': 900, 'max_output_tokens': 30000})
                    handler('submit_patch', {'source': 'pub fn test_value() {}\n'}, threading.Event())
                    return {'status': 'completed'}
                with patch.object(runner, 'verify', return_value=(config, {})), \
                     patch.object(runner, 'prompt_and_files', return_value=('Repair task', runner.SELECTION_FILES)), \
                     patch.object(broker, 'Driver', side_effect=create_driver), \
                     patch.object(containment, 'observe', return_value={'absent':True,'objects':{}}), \
                     patch.object(runner, 'check_catalog', return_value={}), \
                     patch.object(runner.participant, 'run_participant', side_effect=participant_stub):
                    result = runner.run_one(runner.schedule()[0], '/config', '/freeze', 'a'*64)
                self.assertEqual(result['status'], 'participant_completed')
                self.assertIsNone(result['final_success'])
                self.assertEqual(len(result['broker']['candidates']), 1)
                self.assertTrue((parent/'R01-A/run.json').is_file())
                artifact = result['broker']['candidates'][0]['artifact_path']
                self.assertEqual(runner.digest(artifact), result['broker']['candidates'][0]['sha256'])
                self.assertEqual(fake.config['mode'], 'raw')
                self.assertEqual(((parent/'R01-A').stat().st_mode & 0o777), 0o700)

        def test_first_final_identical_oracle_deduplicated(self):
            import tempfile
            with tempfile.TemporaryDirectory(prefix='m116-evaluate-test-') as temp:
                directory = Path(temp).resolve()
                candidate = {'candidate': 1, 'kind': 'patch', 'sha256': 'a'*64, 'artifact_path': '/not-read-by-mock'}
                runner.write_json(directory/'run.json', {'run_id': 'R01-A', 'item': 'R01', 'freeze_sha256': 'b'*64,
                                  'status': 'participant_completed', 'broker': {'candidates': [candidate]}})
                original = runner.read_json
                def read(path, *args):
                    return [] if str(path).endswith('tasks-and-labels.json') else original(path, *args)
                with patch.object(runner, 'read_json', side_effect=read), \
                     patch.object(sys.modules[__name__], 'repair_candidate', return_value={'sha256': 'a'*64, 'final_success': None}) as oracle:
                    value = evaluate_run(directory, {'corpus_root': '/frozen/corpus'}, {}, True)
                self.assertEqual(oracle.call_count, 1)
                self.assertTrue(value['final']['deduplicated'])
                self.assertEqual(value['first']['evaluation_index'], value['final']['evaluation_index'])

        def test_host_reads_reject_symlink_ancestors_and_leaf(self):
            import tempfile
            with tempfile.TemporaryDirectory(prefix='m116-read-test-') as temp:
                root = Path(temp).resolve()
                (root/'real').mkdir()
                runner.write_json(root/'real/value.json', {'ok': True})
                (root/'alias').symlink_to(root/'real', target_is_directory=True)
                (root/'leaf.json').symlink_to(root/'real/value.json')
                for path in (root/'alias/value.json', root/'leaf.json'):
                    with self.assertRaises(OSError):
                        runner.read_json(path)
                self.assertEqual(runner.read_json(root/'real/value.json'), {'ok': True})
                with self.assertRaises(FileExistsError):
                    runner.write_json(root/'real/value.json', {'overwrite': True})

        def test_bad_freeze_rejected_before_process(self):
            with patch.object(broker, 'Driver', side_effect=AssertionError('must not start')):
                with self.assertRaises(ValueError):
                    runner.verify('/absent', '/absent', 'bad')

    outcome = unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(Tests))
    return 0 if outcome.wasSuccessful() else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--self-test', action='store_true')
    parser.add_argument('--config')
    parser.add_argument('--freeze')
    parser.add_argument('--freeze-sha256')
    parser.add_argument('--run')
    parser.add_argument('--execute-oracles', action='store_true')
    parser.add_argument('--finalize')
    parser.add_argument('--manual-review')
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    for key in ('config', 'freeze', 'run', 'finalize', 'manual_review'):
        value = getattr(args, key)
        if value:
            setattr(args, key, os.path.abspath(value))
    if args.finalize:
        if not args.manual_review:
            parser.error('--manual-review required')
        output = Path(args.finalize).with_name('reviewed-evaluation.json')
        value = finalize(runner.read_json(args.finalize), runner.read_json(args.manual_review))
        value['manual_review_sha256'] = runner.digest(args.manual_review)
        runner.write_json(output, value)
        print(json.dumps({'output': str(output), 'final_success': value['final_success']}))
        return 0
    if not all((args.config, args.freeze, args.freeze_sha256, args.run)):
        parser.error('--config --freeze --freeze-sha256 --run required')
    config, projection = runner.verify(args.config, args.freeze, args.freeze_sha256)
    run = runner.read_json(Path(args.run)/'run.json')
    if run['freeze_sha256'] != args.freeze_sha256:
        raise ValueError('run_freeze_mismatch')
    value = evaluate_run(args.run, config, projection, args.execute_oracles,
                         lambda: runner.verify(args.config, args.freeze, args.freeze_sha256))
    print(json.dumps({'run_id': value['run_id'], 'evaluations': len(value['candidates']),
                      'final_success': value['final_success']}))
    return 0


if __name__ == '__main__':
    sys.exit(main())
