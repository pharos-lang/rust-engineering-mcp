#!/usr/bin/env python3
"""Read-only descriptive M1-16 analysis. Never starts processes or evaluates code.

--results is the parent of the 24 planned run directories. Current evaluator paths
are evaluation/evaluation.json and evaluation/reviewed-evaluation.json; final.json
is not an evaluator artifact. --output must be a new directory outside --results.
"""
import argparse
from collections import Counter, defaultdict
import hashlib
import json
import math
import os
import re
from pathlib import Path
import stat
import statistics

MAX_FILE = 64 * 1024 * 1024
MAX_JSON = 16 * 1024 * 1024
TOKENS = ('inputTokens', 'cachedInputTokens', 'outputTokens', 'reasoningOutputTokens')
READS = {'file_reads': 'read_project_file', 'catalog_projection_reads': 'read_catalog_facts',
         'resource_reads': 'resource_read', 'crate_search_calls': 'rust_crate_search',
         'crate_inspect_calls': 'rust_crate_inspect'}


def schedule():
    items = ['R01', 'R02', 'R03', 'R04'] + [f'S{i:02d}-{lang}' for i in range(1, 5) for lang in ('en', 'es')]
    rows = []
    for pair, item in enumerate(items, 1):
        odd = int(item[1:3]) % 2 == 1
        if item.endswith('-es'): odd = not odd
        for order, arm in enumerate(('A', 'B') if odd else ('B', 'A'), 1):
            rows.append({'run_id': f'{item}-{arm}', 'item': item, 'arm': arm,
                         'pair': pair, 'within_pair_order': order})
    return rows


def number(value):
    return value if type(value) in (int, float) and math.isfinite(value) and value >= 0 else None


def boolean(value):
    return value if type(value) is bool else None


def open_file(path):
    """No-follow each component; no process or network access."""
    path = Path(os.path.abspath(path))
    parent = os.open('/', os.O_RDONLY | os.O_DIRECTORY)
    try:
        for component in path.parts[1:-1]:
            child = os.open(component, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent)
            os.close(parent); parent = child
        return os.open(path.name, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW, dir_fd=parent)
    finally:
        os.close(parent)


def read_stable(path, retain=False):
    fd = open_file(path)
    try:
        before = os.fstat(fd)
        if not stat.S_ISREG(before.st_mode) or before.st_size > (MAX_JSON if retain else MAX_FILE):
            raise ValueError('artifact_type_or_budget')
        digest = hashlib.sha256(); data = bytearray(); count = 0
        while chunk := os.read(fd, 65536):
            count += len(chunk)
            if count > (MAX_JSON if retain else MAX_FILE): raise ValueError('artifact_budget')
            digest.update(chunk)
            if retain: data.extend(chunk)
        after = os.fstat(fd)
        stamp = lambda st: (st.st_dev, st.st_ino, st.st_size, st.st_mtime_ns, st.st_ctime_ns)
        if stamp(before) != stamp(after): raise ValueError('artifact_changed')
        return {'path': str(Path(os.path.abspath(path))), 'sha256': digest.hexdigest(), 'bytes': count}, bytes(data)
    finally:
        os.close(fd)


def load(path, issues, manifest, jsonl=False):
    try:
        evidence, data = read_stable(path, True)
        manifest[str(path)] = evidence
        if jsonl:
            value = [json.loads(line) for line in data.splitlines() if line.strip()]
            if not all(isinstance(v, dict) for v in value): raise ValueError('event_object_required')
        else:
            value = json.loads(data)
            if not isinstance(value, dict): raise ValueError('object_required')
        return value
    except FileNotFoundError:
        return None
    except (OSError, ValueError, RecursionError):
        issues.append({'path': str(path), 'code': 'unreadable_or_invalid_artifact'})
        return None


def walk_objects(value, path=''):
    if isinstance(value, dict):
        yield path, value
        for key, child in value.items():
            yield from walk_objects(child, path+'/'+str(key).replace('~', '~0').replace('/', '~1'))
    elif isinstance(value, list):
        for index, child in enumerate(value): yield from walk_objects(child, path+'/'+str(index))


def extract_results(value, source):
    """Raw and normalized timings are separate; no duplicate summing of envelopes."""
    executions, timings, searches = [], [], []
    for pointer, obj in walk_objects(value):
        if 'termination' in obj and 'exit_code' in obj:
            executions.append({'source': source, 'pointer': pointer,
                               **{k: obj.get(k) for k in ('termination', 'exit_code', 'duration_ms',
                                  'total_duration_ms', 'stdout_truncated', 'stderr_truncated', 'validation_complete')}})
        if 'duration_ms' in obj:
            timings.append({'source': source, 'pointer': pointer, 'duration_ms': number(obj['duration_ms']),
                            'stage': obj.get('stage'), 'status': obj.get('status')})
        if 'requested_mode' in obj and 'effective_mode' in obj and 'fallback' in obj:
            searches.append({'source': source, 'pointer': pointer,
                             **{k: obj.get(k) for k in ('requested_mode', 'effective_mode', 'fallback',
                                'snapshot_fingerprint', 'window')}})
    return {'executions': executions, 'duration_observations': timings, 'searches': searches}


def infrastructure(run, participant):
    if run is None: return 'unknown'
    status = run.get('status', '')
    status = status if isinstance(status,str) else ''
    failed = (status.startswith(('infrastructure_failed', 'cleanup_failed'))
              or participant.get('infrastructure_failed') is True or participant.get('cleanup_failed') is True
              or run.get('driver_cleanup', {}).get('cleanup_failed') is True
              or run.get('post_run_freeze_verified') is False
              or any(run.get(k, {}).get('absent') is False for k in ('docker_before', 'docker_after')))
    if failed: return 'failed'
    # Absence of a failure flag is not proof of successful cleanup on old/partial receipts.
    verified = (run.get('post_run_freeze_verified') is True
                and all(run.get(k, {}).get('absent') is True for k in ('docker_before', 'docker_after'))
                and run.get('driver_cleanup', {}).get('cleanup_failed') is False
                and participant.get('infrastructure_failed') is False
                and participant.get('cleanup_failed') is False)
    return 'verified' if verified else 'unknown'


def success_role(evaluation, role, candidates, reviewed):
    if evaluation is None: return None
    if not candidates:
        return False if evaluation.get('candidate_count') == 0 else None
    reference = evaluation.get(role)
    if not isinstance(reference, dict): return None
    index = reference.get('evaluation_index')
    if type(index) is not int or not 0 <= index < len(evaluation.get('candidates', [])): return None
    observed = evaluation['candidates'][index]
    expected = candidates[0] if role == 'first' else candidates[-1]
    if (not re.fullmatch('[0-9a-f]{64}',str(expected.get('sha256','')))
            or reference.get('candidate') != expected.get('candidate')
            or observed.get('sha256') != expected.get('sha256') or observed.get('kind') != expected.get('kind')):
        return None
    outcome = boolean(observed.get('final_success'))
    # A passing candidate requires the reviewed artifact. A deterministic rejection
    # is already evidence of failure; pending review never manufactures success.
    if outcome is True and not reviewed: return None
    if reviewed and boolean(evaluation.get(role+'_success')) != outcome: return None
    return outcome


def analyze_run(root, planned, manifest):
    directory = root/planned['run_id']; issues = []
    row = dict(planned, family='repair' if planned['item'].startswith('R') else 'selection',
               task_family=planned['item'][:3], language='es' if planned['item'].endswith('-es') else 'en')
    run = load(directory/'run.json', issues, manifest)
    started = load(directory/'started.json', issues, manifest)
    if run is not None and any(run.get(k) != v for k, v in planned.items()):
        issues.append({'path': str(directory/'run.json'), 'code': 'run_identity_mismatch'})
        run = None
    participant = (run or {}).get('participant', {})
    if not isinstance(participant, dict): participant = {}
    broker = (run or {}).get('broker')
    candidates = broker.get('candidates') if isinstance(broker, dict) else None
    candidates_known = isinstance(candidates, list) and all(isinstance(c, dict) for c in candidates)
    candidates = candidates if candidates_known else []
    pending = load(directory/'evaluation/evaluation.json', issues, manifest)
    reviewed = load(directory/'evaluation/reviewed-evaluation.json', issues, manifest)
    evaluation = reviewed if reviewed is not None else pending
    evaluation_valid = (evaluation is not None and run is not None and candidates_known
                        and evaluation.get('run_id') == planned['run_id']
                        and re.fullmatch('[0-9a-f]{64}',str(run.get('freeze_sha256','')))
                        and evaluation.get('freeze_sha256') == run.get('freeze_sha256')
                        and evaluation.get('candidate_count') == len(candidates)
                        and isinstance(evaluation.get('candidates'), list))
    if evaluation is not None and not evaluation_valid:
        issues.append({'path': str(directory/'evaluation'), 'code': 'evaluation_binding_mismatch'})
    selected = evaluation if evaluation_valid else None
    events = load(directory/'participant/events.jsonl', issues, manifest, jsonl=True)
    requests = broker.get('requests') if isinstance(broker, dict) else None
    request_known = isinstance(requests, list) and all(isinstance(r, dict) for r in requests)
    names = Counter(r.get('name') for r in requests) if request_known else Counter()
    validations = broker.get('validation_requests') if isinstance(broker, dict) else None
    validations_known = isinstance(validations, list) and all(isinstance(r, dict) for r in validations)
    if not validations_known: validations=None
    usage = participant.get('usage'); totals = usage.get('total', {}) if isinstance(usage, dict) else {}
    totals = totals if isinstance(totals, dict) else {}
    tokens = {key: value if type(value := totals.get(key)) is int and value >= 0 else None for key in TOKENS}
    row.update(run_state='recorded' if run is not None else 'invalid_receipt' if any(i['path']==str(directory/'run.json') for i in issues) else 'started_without_receipt' if started is not None else 'missing',
        run_status=(run or {}).get('status'), participant_task_status=participant.get('task_status'),
        participant_status=participant.get('status'), turn_status=participant.get('turn_status'),
        stop_reason=participant.get('stop_reason'), infrastructure_state=infrastructure(run, participant),
        evaluation_state='reviewed' if reviewed is not None else 'pending_review' if pending is not None else 'missing',
        first_candidate_reference=(selected or {}).get('first'), final_candidate_reference=(selected or {}).get('final'),
        first_success=success_role(selected, 'first', candidates, reviewed is not None),
        final_success=success_role(selected, 'final', candidates, reviewed is not None),
        evaluation_infrastructure_failure=(evaluation or {}).get('infrastructure_failure'),
        oracle_freeze_verified=(evaluation or {}).get('post_oracle_freeze_verified'),
        oracle_infrastructure_state=('failed' if (evaluation or {}).get('infrastructure_failure') or (evaluation or {}).get('post_oracle_freeze_verified') is False else 'verified' if evaluation_valid and evaluation.get('post_oracle_freeze_verified') is True else 'unknown'),
        candidate_count=len(candidates) if candidates_known else None,
        submission_revisions=max(0, len(candidates)-1) if candidates_known else None,
        distinct_candidate_hashes=len({c.get('sha256') for c in candidates}) if candidates_known else None,
        validation_requests=len(validations) if validations_known else None,
        broker_observed_requests=number(broker.get('observed_requests')) if isinstance(broker, dict) else None,
        broker_admitted_requests=number(broker.get('admitted_requests')) if isinstance(broker, dict) else None,
        tool_request_count=len(requests) if request_known else None,
        tool_counts=dict(names), read_counts={key: names[name] if request_known else None for key, name in READS.items()},
        usage_coverage=participant.get('usage_coverage', 'unknown'), tokens=tokens,
        elapsed_seconds=number((run or {}).get('elapsed_seconds')),
        participant_elapsed_seconds=number(participant.get('elapsed_seconds')),
        cleanup_seconds=number((run or {}).get('cleanup_seconds')),
        candidate_window_and_setup_seconds=number((run or {}).get('candidate_window_and_setup_seconds')),
        catalog_setup_seconds=number((run or {}).get('catalog_setup_seconds')),
        validation_elapsed_ms=sum(r['elapsed_ms'] for r in validations) if validations_known and all(number(r.get('elapsed_ms')) is not None for r in validations) else None,
        issues=issues)
    row['token_subset_consistency'] = {
        'cached_within_input': tokens['cachedInputTokens'] <= tokens['inputTokens'] if tokens['cachedInputTokens'] is not None and tokens['inputTokens'] is not None else None,
        'reasoning_within_output': tokens['reasoningOutputTokens'] <= tokens['outputTokens'] if tokens['reasoningOutputTokens'] is not None and tokens['outputTokens'] is not None else None}
    extracts = {'executions': [], 'duration_observations': [], 'searches': []}
    # Broker validation results are authoritative for validation calls. Events also
    # contain those results: use them only for calls outside that recorded set.
    validation_names = {r.get('name') for r in validations} if validations_known else set()
    for i, result in enumerate(validations or []):
        found = extract_results(result.get('result'), f'run.json#/broker/validation_requests/{i}/result')
        for key in extracts: extracts[key].extend(found[key])
    event_calls = [event['tool_call'] for event in events or [] if isinstance(event.get('tool_call'), dict)]
    for i, call in enumerate(event_calls):
        name = call.get('name', '')
        if name in validation_names or name.replace('_', '.') in validation_names: continue
        found = extract_results(call.get('response'), f'participant/events.jsonl#tool_call/{i}/response')
        for key in extracts: extracts[key].extend(found[key])
    row['result_extracts'] = extracts
    row['events_state'] = 'recorded' if events is not None else 'missing_or_invalid'
    row['unadmitted_denials'] = sum('denied_tool' in e for e in events) if events is not None else None
    row['retryable_broker_denials'] = sum(isinstance(c.get('response'), dict) and 'broker_error' in c['response'] for c in event_calls) if events is not None else None
    gateway = [number(x.get('duration_ms')) for x in extracts['executions']]
    row['raw_gateway_duration_ms'] = sum(gateway) if gateway and all(v is not None for v in gateway) else None
    row['raw_gateway_duration_observations'] = sum(v is not None for v in gateway)
    row['oracle_candidates'] = []
    for observation in (selected or {}).get('candidates', []):
        row['oracle_candidates'].append({k: observation.get(k) for k in ('candidate', 'sha256', 'kind', 'status',
            'final_success', 'automated_gates_passed', 'deterministic_passed', 'checks', 'elapsed_seconds',
            'stages', 'audit', 'raw_cleanup', 'mcp_cleanup', 'failure_cleanup', 'docker_before', 'docker_after')})
    # No raw outcome is rewritten or discarded: these exact input documents remain
    # linked/hashed in source_artifacts, including unknown fields and failed runs.
    row['raw_outcome_paths'] = [str(path) for path in (directory/'run.json', directory/'started.json',
        directory/'evaluation/evaluation.json', directory/'evaluation/reviewed-evaluation.json') if str(path) in manifest]
    return row


def observations(values):
    known = [value for value in values if number(value) is not None]
    return {'known': len(known), 'unknown': len(values)-len(known),
            'sum_observed': sum(known) if known else None,
            'mean_observed': statistics.mean(known) if known else None,
            'median_observed': statistics.median(known) if known else None,
            'min_observed': min(known) if known else None, 'max_observed': max(known) if known else None}


def success_counts(rows, key):
    passed = sum(r[key] is True for r in rows); failed = sum(r[key] is False for r in rows)
    return {'planned': len(rows), 'passed': passed, 'failed': failed, 'unknown': len(rows)-passed-failed,
            'evaluated': passed+failed, 'rate_among_evaluated': passed/(passed+failed) if passed+failed else None,
            'observed_passes_over_planned': passed/len(rows) if rows else None}


def summarize(rows):
    result = {'planned': len(rows), 'run_states': dict(Counter(r['run_state'] for r in rows)),
              'infrastructure_states': dict(Counter(r['infrastructure_state'] for r in rows)),
              'evaluation_states': dict(Counter(r['evaluation_state'] for r in rows)),
              'oracle_infrastructure_states': dict(Counter(r['oracle_infrastructure_state'] for r in rows)),
              'first_success': success_counts(rows, 'first_success'), 'final_success': success_counts(rows, 'final_success'),
              'usage_coverage': dict(Counter(r['usage_coverage'] for r in rows)),
              'tokens': {key: observations([r['tokens'][key] for r in rows]) for key in TOKENS},
              'metrics': {key: observations([r[key] for r in rows]) for key in ('candidate_count', 'submission_revisions',
                  'validation_requests', 'tool_request_count', 'elapsed_seconds', 'participant_elapsed_seconds',
                  'cleanup_seconds', 'catalog_setup_seconds', 'validation_elapsed_ms', 'raw_gateway_duration_ms')}}
    result['reads'] = {key: observations([r['read_counts'][key] for r in rows]) for key in READS}
    result['passing_candidate_with_failed_infrastructure'] = sum(r['final_success'] is True and r['infrastructure_state']=='failed' for r in rows)
    return result


def pairs(rows):
    grouped = defaultdict(dict)
    for row in rows: grouped[row['item']][row['arm']] = row
    result = []
    for item, arms in grouped.items():
        pair = {'item': item, 'task_family': item[:3], 'family': arms['A']['family'], 'language': arms['A']['language']}
        for role in ('first', 'final'):
            a, b = (arms[arm][role+'_success'] for arm in ('A', 'B'))
            state = 'unknown' if a is None or b is None else 'both_pass' if a and b else 'both_fail' if not a and not b else 'A_only' if a else 'B_only'
            pair[role] = {'A': a, 'B': b, 'outcome': state}
        pair['infrastructure'] = {arm: arms[arm]['infrastructure_state'] for arm in ('A', 'B')}
        result.append(pair)
    return result


def analyze(results):
    root = Path(os.path.abspath(results)); manifest = {}; rows = []
    if not root.is_dir() or root.is_symlink(): raise ValueError('results_directory_required')
    for planned in schedule(): rows.append(analyze_run(root, planned, manifest))
    # Bind every retained raw artifact, not only fields understood by this version.
    artifact_issues = []
    for planned in schedule():
        directory = root/planned['run_id']
        if directory.is_symlink():
            artifact_issues.append({'path': str(directory), 'code': 'symlink_directory'}); continue
        for base, dirs, files in os.walk(directory, followlinks=False):
            for name in list(dirs):
                path = Path(base)/name
                if path.is_symlink(): dirs.remove(name); artifact_issues.append({'path': str(path), 'code': 'symlink_directory'})
            for name in files:
                path = Path(base)/name
                if str(path) in manifest: continue
                try: manifest[str(path)] = read_stable(path)[0]
                except (OSError, ValueError): artifact_issues.append({'path': str(path), 'code': 'unreadable_artifact'})
    groups = {}
    for dimensions in [('arm',), ('family',), ('task_family',), ('language',), ('arm', 'family'), ('arm', 'family', 'language')]:
        collection = defaultdict(list)
        for row in rows: collection[tuple(row[d] for d in dimensions)].append(row)
        groups['by_'+'_'.join(dimensions)] = [{'group': dict(zip(dimensions, key)), **summarize(value)} for key, value in sorted(collection.items())]
    paired = pairs(rows)
    return {'schema_version': 1, 'results_root': str(root), 'planned_runs': 24, 'runs': rows,
            'overall': summarize(rows), 'groups': groups, 'pairs': paired,
            'paired_counts': {role: dict(Counter(p[role]['outcome'] for p in paired)) for role in ('first', 'final')},
            'source_artifacts': sorted(manifest.values(), key=lambda x: x['path']), 'artifact_issues': artifact_issues,
            'unplanned_entries': sorted(p.name for p in root.iterdir() if p.name not in {r['run_id'] for r in schedule()}),
            'definitions': {
                'success': 'Candidate outcome, separate from participant and infrastructure status. Passing requires reviewed-evaluation.json; unknown is never failure or zero.',
                'denominators': 'All24 planned runs retained. Evaluated denominators count explicit booleans only; observed passes/planned is not an imputed success rate.',
                'submission_revisions': 'max(candidate_count-1,0), an operational submission count; validation_requests are the separately recorded validation cycles.',
                'tokens': 'Cumulative usage.total fields, never summed across updates. Cached input is a subset of input; reasoning output is a subset of output. Subsets are not added again.',
                'timing': 'Elapsed includes setup/candidate/cleanup per runner; catalog setup is recorded separately and participant_elapsed excludes pre-model catalog setup. Validation elapsed includes broker/driver overhead. Raw gateway duration is only explicit execution duration_ms; missing MCP gateway-only durations remain unknown. Nested timing observations are not added.',
                'language': 'Repair prompts are English; selection language follows item suffix. Family is repair/selection; task_family clusters paired ES/EN selection intents.',
                'interpretation': 'Descriptive pilot observations only. No causal claims, population inference or independence claims. Pair order/cache may affect timing.',
                'sources': 'All retained files under planned run directories are linked and hashed; originals remain unchanged. Symlinks and unstable/oversized artifacts are reported, never followed.'}}


def markdown(result):
    lines = ['# M1-16 descriptive pilot analysis', '',
             'Candidate success, participant completion and infrastructure status remain separate. '
             'Unknown outcomes and missing usage are not imputed. This report makes no causal or population claims.', '',
             '| Arm / family / language | Planned | Recorded | Infra failed | First pass / evaluated | Final pass / evaluated | Final unknown |',
             '| --- | ---: | ---: | ---: | ---: | ---: | ---: |']
    for row in result['groups']['by_arm_family_language']:
        group = row['group']; first = row['first_success']; final = row['final_success']
        lines.append(f"| {group['arm']} / {group['family']} / {group['language']} | {row['planned']} | {row['run_states'].get('recorded',0)} | {row['infrastructure_states'].get('failed',0)} | {first['passed']} / {first['evaluated']} | {final['passed']} / {final['evaluated']} | {final['unknown']} |")
    lines += ['', '| Pair outcome | First | Final |', '| --- | ---: | ---: |']
    for key in ('both_pass', 'both_fail', 'A_only', 'B_only', 'unknown'):
        lines.append(f"| {key} | {result['paired_counts']['first'].get(key,0)} | {result['paired_counts']['final'].get(key,0)} |")
    lines += ['', '| Arm | Candidates observed | Validation cycles observed | Tool requests observed | Elapsed seconds observed | Input tokens observed | Cached input subset | Output tokens observed | Reasoning output subset |',
              '| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |']
    def cell(metric):
        value = metric['sum_observed']
        return f"{round(value,3) if value is not None else 'unknown'} (n={metric['known']})"
    for row in result['groups']['by_arm']:
        values = [row['metrics'][key] for key in ('candidate_count', 'validation_requests', 'tool_request_count', 'elapsed_seconds')]+[row['tokens'][key] for key in TOKENS]
        lines.append('| '+row['group']['arm']+' | '+' | '.join(cell(v) for v in values)+' |')
    lines += ['', 'Counts use all24 planned runs; success fractions use only explicitly evaluated outcomes. '
              'Token/timing totals show their observed-run coverage; cached/reasoning subsets are not additional tokens. '
              'Validation cycles and extra submissions are different operational counts. Pair order and warm caches can affect time.', '',
              f"Passing final candidates with failed infrastructure: {result['overall']['passing_candidate_with_failed_infrastructure']}. "
              f"Artifact read issues: {len(result['artifact_issues'])}; per-run issues: {sum(len(r['issues']) for r in result['runs'])}.", '',
              'Per-run task/turn/infra states, read counts, usage coverage, first/final references, oracle stages, '
              'raw timing observations and search fallback extracts are retained in [analysis.json](analysis.json). '
              'Every original artifact is linked with SHA-256 in [sources.md](sources.md).']
    return '\n'.join(lines)+'\n'


def write_report(results, output):
    root = Path(os.path.abspath(results)); out = Path(os.path.abspath(output))
    if out == root or root in out.parents: raise ValueError('output_must_be_outside_results')
    if out.exists() or out.is_symlink(): raise FileExistsError('fresh_output_required')
    result = analyze(root)
    documents={'analysis.json':json.dumps(result, ensure_ascii=True, allow_nan=False, indent=2)+'\n',
               'summary.md':markdown(result)}
    lines = ['# Raw evidence', '', 'Original files are retained in place, never rewritten.', '', '| File | Bytes | SHA-256 |', '| --- | ---: | --- |']
    for source in result['source_artifacts']:
        path = source['path']; label = str(Path(path).relative_to(root)).replace('|','&#124;').replace('[','&#91;').replace(']','&#93;')
        lines.append(f"| [{label}](<{path}>) | {source['bytes']} | {source['sha256']} |")
    documents['sources.md']='\n'.join(lines)+'\n'
    # Create and write through directory handles; reject symlink ancestors and
    # existing output instead of redirecting or overwriting evidence.
    parent=os.open('/',os.O_RDONLY|os.O_DIRECTORY)
    try:
        for part in out.parts[1:-1]:
            child=os.open(part,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW,dir_fd=parent)
            os.close(parent);parent=child
        os.mkdir(out.name,mode=0o700,dir_fd=parent)
        directory=os.open(out.name,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW,dir_fd=parent)
        try:
            for name,text in documents.items():
                fd=os.open(name,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600,dir_fd=directory)
                try:
                    data=memoryview(text.encode())
                    while data:data=data[os.write(fd,data):]
                finally:os.close(fd)
        finally:os.close(directory)
    finally:os.close(parent)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--results', required=True); parser.add_argument('--output', required=True)
    args = parser.parse_args()
    result = write_report(args.results, args.output)
    print(json.dumps({'planned': 24, 'recorded': result['overall']['run_states'].get('recorded', 0), 'output': os.path.abspath(args.output)}))


if __name__ == '__main__': main()
