"""Synthetic schema/denominator tests only; no measured study results."""
import ast
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
import analyze as a


def put(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value)+'\n')


def fixture(root, run_id='R01-A', passed=True, infrastructure=False, usage=None):
    planned = next(r for r in a.schedule() if r['run_id']==run_id)
    candidate = {'candidate':1, 'sha256':'a'*64, 'kind':'patch' if run_id.startswith('R') else 'selection'}
    run = {**planned, 'freeze_sha256':'b'*64,
           'status':'cleanup_failed' if infrastructure else 'participant_completed',
           'post_run_freeze_verified':True, 'docker_before':{'absent':True}, 'docker_after':{'absent':not infrastructure},
           'driver_cleanup':{'cleanup_failed':infrastructure}, 'elapsed_seconds':12, 'cleanup_seconds':2,
           'participant':{'status':'completed', 'task_status':'completed', 'turn_status':'completed',
               'infrastructure_failed':infrastructure, 'cleanup_failed':infrastructure,
               'usage':usage, 'usage_coverage':'reported_total' if usage is not None else 'unknown'},
           'broker':{'candidates':[candidate], 'observed_requests':2, 'admitted_requests':2,
               'requests':[{'name':'read_project_file'}, {'name':'raw_validate'}],
               'validation_requests':[{'name':'raw_validate', 'elapsed_ms':100,
                   'result':{'termination':'exited','exit_code':0,'duration_ms':50,'total_duration_ms':70}}]}}
    evaluation = {'run_id':run_id, 'freeze_sha256':'b'*64, 'participant_status':run['status'], 'candidate_count':1,
                  'post_oracle_freeze_verified':True, 'candidates':[{**candidate,'status':'passed' if passed else 'failed_gate',
                    'final_success':passed,'stages':[{'stage':'check','passed':passed,'elapsed_seconds':1}],
                    'automated_gates_passed':passed}],
                  'first':{'evaluation_index':0,'candidate':1},'final':{'evaluation_index':0,'candidate':1,'deduplicated':True},
                  'first_success':passed,'final_success':passed}
    directory=root/run_id
    put(directory/'run.json',run);put(directory/'evaluation/evaluation.json',evaluation)
    put(directory/'evaluation/reviewed-evaluation.json',evaluation)
    return run,evaluation


class AnalysisTests(unittest.TestCase):
    def test_schedule_matches_current_runner_without_importing_process_adapters(self):
        source=Path(__file__).with_name('run-study.py').read_text()
        tree=ast.parse(source)
        subset=ast.Module(body=[node for node in tree.body if
            isinstance(node,ast.Assign) and any(isinstance(t,ast.Name) and t.id=='ITEMS' for t in node.targets)
            or isinstance(node,ast.FunctionDef) and node.name=='schedule'],type_ignores=[])
        namespace={};exec(compile(subset,'runner_schedule','exec'),namespace)
        self.assertEqual(a.schedule(),namespace['schedule']())
        self.assertEqual(len(a.schedule()),24)

    def test_missing_run_and_unknown_usage_keep_planned_denominators(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp).resolve();fixture(root)
            result=a.analyze(root)
            self.assertEqual(result['overall']['run_states'],{'recorded':1,'missing':23})
            self.assertEqual(result['overall']['final_success']['unknown'],23)
            self.assertEqual(result['overall']['final_success']['evaluated'],1)
            self.assertIsNone(result['overall']['tokens']['inputTokens']['sum_observed'])
            self.assertEqual(result['overall']['tokens']['inputTokens']['unknown'],24)
            self.assertEqual(result['paired_counts']['final'],{'unknown':12})

    def test_passing_candidate_does_not_erase_failed_infrastructure(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp).resolve();fixture(root,infrastructure=True)
            result=a.analyze(root);row=result['runs'][0]
            self.assertTrue(row['final_success']);self.assertEqual(row['infrastructure_state'],'failed')
            self.assertEqual(row['participant_task_status'],'completed')
            self.assertEqual(row['run_status'],'cleanup_failed')
            self.assertEqual(result['overall']['passing_candidate_with_failed_infrastructure'],1)

    def test_token_subsets_partial_coverage_and_discordant_pair(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp).resolve()
            fixture(root,usage={'total':{'inputTokens':100,'cachedInputTokens':30,'outputTokens':20,'reasoningOutputTokens':12}})
            run,_=fixture(root,'R01-B',passed=False,usage={'total':{'outputTokens':7}})
            run['participant']['usage_coverage']='partial_reported_before_interruption';put(root/'R01-B/run.json',run)
            result=a.analyze(root)
            self.assertEqual(result['overall']['tokens']['inputTokens']['sum_observed'],100)
            self.assertEqual(result['overall']['tokens']['outputTokens']['sum_observed'],27)
            self.assertEqual(result['overall']['tokens']['cachedInputTokens']['sum_observed'],30)
            self.assertEqual(result['overall']['tokens']['reasoningOutputTokens']['known'],1)
            self.assertEqual(result['paired_counts']['final']['A_only'],1)
            self.assertEqual(result['paired_counts']['final']['unknown'],11)
            self.assertEqual(result['overall']['usage_coverage']['partial_reported_before_interruption'],1)

    def test_hash_binding_and_current_evaluator_path_fail_closed(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp).resolve();_,evaluation=fixture(root)
            path=root/'R01-A/evaluation/reviewed-evaluation.json'
            evaluation['freeze_sha256']='c'*64;put(path,evaluation)
            result=a.analyze(root)
            self.assertIsNone(result['runs'][0]['first_success'])
            self.assertIn('evaluation_binding_mismatch',[i['code'] for i in result['runs'][0]['issues']])
            path.unlink();(root/'R01-A/evaluation/evaluation.json').unlink()
            evaluation['freeze_sha256']='b'*64;put(root/'R01-A/evaluation/final.json',evaluation)
            self.assertIsNone(a.analyze(root)['runs'][0]['final_success'])

    def test_read_oracle_search_and_timings_extract_without_double_count(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp).resolve();run,_=fixture(root)
            directory=root/'R01-A/participant';directory.mkdir()
            events=[{'tool_call':{'name':'raw_validate','response':run['broker']['validation_requests'][0]['result']}},
                    {'tool_call':{'name':'rust_crate_search','response':{'structuredContent':{'duration_ms':4,'data':{
                        'requested_mode':'hybrid','effective_mode':'lexical','fallback':{'reason':'missing_index'},
                        'snapshot_fingerprint':'sha256:'+('d'*64),'window':{'returned':2}}}}}}]
            (directory/'events.jsonl').write_text(''.join(json.dumps(e)+'\n' for e in events))
            row=a.analyze(root)['runs'][0]
            self.assertEqual(row['raw_gateway_duration_ms'],50)
            self.assertEqual(len(row['result_extracts']['executions']),1)
            self.assertEqual(row['result_extracts']['searches'][0]['effective_mode'],'lexical')
            self.assertEqual(row['read_counts']['file_reads'],1)
            self.assertEqual(row['oracle_candidates'][0]['stages'][0]['stage'],'check')

    def test_fresh_output_preserves_all_raw_outcomes_and_hashes(self):
        with tempfile.TemporaryDirectory() as temp:
            base=Path(temp).resolve();root=base/'results';root.mkdir();fixture(root)
            raw=root/'R01-A/raw-extra.json';raw.write_text('{"unrecognized_outcome":"retained"}\n')
            original=raw.read_bytes();result=a.write_report(root,base/'analysis')
            source=next(s for s in result['source_artifacts'] if s['path']==str(raw))
            self.assertEqual(source['sha256'],hashlib.sha256(original).hexdigest())
            self.assertEqual(raw.read_bytes(),original)
            self.assertTrue((base/'analysis/summary.md').is_file())
            self.assertTrue((base/'analysis/sources.md').is_file())
            with self.assertRaises(FileExistsError):a.write_report(root,base/'analysis')
            with self.assertRaises(ValueError):a.write_report(root,root/'analysis')

    def test_symlink_artifact_is_reported_without_reading_target(self):
        with tempfile.TemporaryDirectory() as temp:
            base=Path(temp).resolve();root=base/'results';root.mkdir();fixture(root)
            secret=base/'synthetic-secret';secret.write_text('fixture-canary')
            (root/'R01-A/alias').symlink_to(secret)
            result=a.analyze(root)
            self.assertTrue(result['artifact_issues'])
            self.assertFalse(any(s['path'].endswith('/alias') for s in result['source_artifacts']))


if __name__=='__main__':unittest.main()
