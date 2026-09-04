#!/usr/bin/env python3
"""Prospective M1-16 runner. --plan never starts a process; --execute is explicit.

Host config (all paths absolute): corpus_root, projection, results_parent, driver
(host Init fields except mode/root/state_root/stderr_path), qualification_receipts
(list of paths), qualification_approved=true. Freeze: protocol_version=2,
approved=true, config_sha256, files=[{path:absolute,sha256}]. Freeze must include
this script, evaluate.py, participant.py, broker.py, driver and server binaries,
all corpus files, projection, qualification receipts and all configured asset files.
The principal freezes these inputs; this program never approves its own freeze.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import stat
import sys
import threading
import time

import broker
import participant
import containment

HERE = Path(__file__).resolve().parent
PINNED_CLI = Path('/opt/homebrew/Caskroom/codex/0.153.0/bin/codex')
PINNED_CODE_HOST = PINNED_CLI.with_name('codex-code-mode-host')
ITEMS = ['R01', 'R02', 'R03', 'R04'] + [f'S{i:02d}-{lang}' for i in range(1, 5) for lang in ('en', 'es')]
SELECTION_FILES = {
    'Cargo.toml': '[package]\nname = "selection_workspace"\nversion = "0.1.0"\nedition = "2024"\n',
    'Cargo.lock': 'version = 4\n\n[[package]]\nname = "selection_workspace"\nversion = "0.1.0"\n',
    'src/lib.rs': '// Catalog selection task: no integration claim is requested.\n'}


def schedule():
    runs = []
    for pair, item in enumerate(ITEMS, 1):
        odd = int(item[1:3]) % 2 == 1
        if item.endswith('-es'):
            odd = not odd
        for ordinal, arm in enumerate(('A', 'B') if odd else ('B', 'A'), 1):
            runs.append({'run_id': f'{item}-{arm}', 'item': item, 'arm': arm,
                         'pair': pair, 'within_pair_order': ordinal})
    return runs



def open_relative(path, flags, mode=0o600):
    """Open each host-selected component without following symlinks."""
    path = absolute(path)
    parent = os.open('/', os.O_RDONLY | os.O_DIRECTORY)
    try:
        for component in path.parts[1:-1]:
            next_parent = os.open(component, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                                  dir_fd=parent)
            os.close(parent)
            parent = next_parent
        return os.open(path.name, flags | os.O_NOFOLLOW, mode, dir_fd=parent)
    finally:
        os.close(parent)


def read_bytes(path, cap=65536):
    fd = open_relative(path, os.O_RDONLY | os.O_NONBLOCK)
    try:
        before = os.fstat(fd)
        if not stat.S_ISREG(before.st_mode) or before.st_size > cap:
            raise ValueError('input_file_type_or_budget')
        data = bytearray()
        while len(data) <= cap:
            chunk = os.read(fd, min(65536, cap+1-len(data)))
            if not chunk:
                break
            data.extend(chunk)
        after = os.fstat(fd)
        stamp = lambda info: (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns, info.st_ctime_ns)
        if len(data) > cap or stamp(before) != stamp(after):
            raise ValueError('input_file_changed_or_budget')
        return bytes(data)
    finally:
        os.close(fd)


def digest(path):
    path = Path(path)
    fd = open_relative(path, os.O_RDONLY | os.O_NONBLOCK)
    try:
        before = os.fstat(fd)
        if not stat.S_ISREG(before.st_mode):
            raise ValueError('non_regular_frozen_file')
        result = hashlib.sha256()
        while chunk := os.read(fd, 1024*1024):
            result.update(chunk)
        after = os.fstat(fd)
        if (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns):
            raise ValueError('frozen_file_changed_during_read')
        return result.hexdigest()
    finally:
        os.close(fd)


def read_json(path, cap=16*1024*1024):
    return json.loads(read_bytes(path, cap))


def write_json(path, value):
    data = broker.encoded(value)+b'\n'
    fd = open_relative(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL)
    try:
        view = memoryview(data)
        while view:
            view = view[os.write(fd, view):]
        os.fsync(fd)
    finally:
        os.close(fd)


def absolute(value):
    path = Path(value)
    if not path.is_absolute() or '..' in path.parts or any(ord(c) < 32 for c in str(path)):
        raise ValueError('invalid_host_path')
    return path


def tree_files(root):
    root = absolute(root)
    if root.is_symlink():
        raise ValueError('symlink_asset_directory')
    for base, dirs, files in os.walk(root, followlinks=False):
        for name in dirs:
            if (Path(base)/name).is_symlink():
                raise ValueError('symlink_asset_directory')
        for name in files:
            path = Path(base)/name
            if path.is_symlink():
                raise ValueError('symlink_asset_file')
            yield path


def verify(config_path, freeze_path, freeze_sha256):
    if not re.fullmatch('[0-9a-f]{64}', freeze_sha256) or digest(freeze_path) != freeze_sha256:
        raise ValueError('freeze_digest_mismatch')
    freeze = read_json(freeze_path)
    config = read_json(config_path)
    if freeze.get('protocol_version') != 2 or freeze.get('approved') is not True:
        raise ValueError('freeze_not_approved_v2')
    if digest(config_path) != freeze.get('config_sha256'):
        raise ValueError('host_config_not_frozen')
    if config.get('qualification_approved') is not True or not config.get('qualification_receipts'):
        raise ValueError('qualification_not_approved')
    frozen = {}
    for entry in freeze['files']:
        path = absolute(entry['path'])
        if str(path) in frozen or not re.fullmatch('[0-9a-f]{64}', entry['sha256']):
            raise ValueError('invalid_freeze_entries')
        frozen[str(path)] = entry['sha256']
    required = {HERE/name for name in ('run-study.py', 'evaluate.py', 'broker.py', 'participant.py', 'containment.py')}
    if Path(participant.CLI).resolve() != PINNED_CLI:
        raise ValueError('participant_cli_target_changed')
    required.update((PINNED_CLI, PINNED_CODE_HOST, containment.DOCKER, broker.BINARY, absolute(config['projection']), absolute(config['driver']['server_binary'])))
    required.update(absolute(p) for p in config['qualification_receipts'])
    required.update(tree_files(config['corpus_root']))
    host = config['driver']
    forbidden = {'mode', 'root', 'state_root', 'stderr_path'}
    allowed = {'server_binary', 'docker_socket', 'catalog_store', 'catalog_trust', 'model_dir',
               'index_store', 'rustsec_path', 'rustsec_sha256'}
    if set(host)-allowed or set(host)&forbidden:
        raise ValueError('invalid_driver_host_configuration')
    for key in ('catalog_store', 'catalog_trust', 'model_dir', 'index_store', 'rustsec_path'):
        if key not in host:
            raise ValueError('incomplete_driver_host_configuration')
        path = absolute(host[key])
        required.update(tree_files(path) if path.is_dir() else [path])
    if not required <= {Path(p) for p in frozen}:
        raise ValueError('required_input_missing_from_freeze')
    for path, expected in frozen.items():
        if digest(path) != expected:
            raise ValueError('frozen_input_drift')
    projection = read_json(config['projection'])
    if not re.fullmatch(r'sha256:[0-9a-f]{64}', projection.get('snapshot_fingerprint', '')):
        raise ValueError('projection_snapshot_identity_missing')
    if not projection.get('provenance', {}).get('source_id') or not isinstance(projection.get('records'), list):
        raise ValueError('projection_provenance_or_records_missing')
    expected = config.get('expected_catalog')
    if not isinstance(expected, dict) or set(expected) != {'catalog_fingerprint','model_identity','index_metadata','documents'}:
        raise ValueError('expected_catalog_identity_missing')
    if expected['catalog_fingerprint'] != projection['snapshot_fingerprint'] or expected['index_metadata'].get('snapshot_fingerprint') != projection['snapshot_fingerprint'] or expected['index_metadata'].get('model') != expected['model_identity']:
        raise ValueError('expected_catalog_identity_inconsistent')
    if host['rustsec_sha256'].removeprefix('sha256:') != digest(host['rustsec_path']):
        raise ValueError('rustsec_binding_mismatch')
    # Same protected host parent convention as the workspace broker.
    fd = broker._open_dir(config['results_parent'])
    os.close(fd)
    return config, projection


def driver_config(config, workspace, mode, state_root, stderr_path):
    value = dict(config['driver'])
    value.update(mode=mode, root=str(workspace.root), state_root=str(state_root),
                 stderr_path=str(stderr_path))
    return value


def prompt_and_files(config, item):
    corpus = Path(config['corpus_root'])
    if item.startswith('R'):
        base = corpus/'repair'/item
        prompt = read_bytes(base/'prompt.txt').decode('utf-8')
        files = {name: read_bytes(base/'initial'/name).decode('utf-8') for name in broker.FILES}
    else:
        prompt = read_bytes(corpus/'selection/prompts'/f'{item}.txt').decode('utf-8')
        files = dict(SELECTION_FILES)
    return prompt, files


def check_catalog(result, expected):
    """Validate treatment availability, not just bytes present on disk."""
    body = result.get('structuredContent', {})
    context = body.get('data', {}).get('context', {})
    if result.get('isError') is True or body.get('status') != 'passed':
        raise broker.BrokerError('catalog_identity_unavailable')
    if any(context.get(key, {}).get('status') != 'available' for key in ('catalog', 'model', 'semantic_index')):
        raise broker.BrokerError('catalog_identity_unavailable')
    actual = {'catalog_fingerprint': context['catalog']['value']['fingerprint'],
              'model_identity': context['model']['value']['identity'],
              'index_metadata': context['semantic_index']['value']['metadata'],
              'documents': context['semantic_index']['value']['documents']}
    if actual != expected:
        raise broker.BrokerError('catalog_identity_mismatch')
    return actual


def run_one(run, config_path, freeze_path, freeze_sha256):
    config, projection = verify(config_path, freeze_path, freeze_sha256)
    parent = absolute(config['results_parent'])
    directory = parent/run['run_id']
    directory.mkdir(mode=0o700)  # No overwrite, resumption or silent replacement.
    prior = [r['run_id'] for r in schedule() if (parent/r['run_id']/'started.json').is_file()]
    position = next(i for i, r in enumerate(schedule()) if r['run_id'] == run['run_id'])
    receipt = dict(run, started_at=time.time(), observed_prior_run_ids=prior,
                   planned_schedule_prefix_preserved=prior == [r['run_id'] for r in schedule()[:position]],
                   freeze_sha256=freeze_sha256, status='infrastructure_failed',
                   first_attempt_success=None, final_success=None, oracle_status='not_evaluated')
    write_json(directory/'started.json', receipt)
    workspace = driver = instance = None
    started = time.monotonic()
    try:
        receipt['docker_before'] = containment.observe(config['driver']['docker_socket'], directory)
        if not receipt['docker_before']['absent']:
            raise broker.BrokerError('preexisting_execution_objects')
        prompt, files = prompt_and_files(config, run['item'])
        workspace = broker.Workspace(directory, files)
        state = directory/'driver-state'
        state.mkdir(mode=0o700)
        participant_output = directory/'participant'
        participant_output.mkdir(mode=0o700)
        driver = broker.Driver(driver_config(config, workspace, 'raw' if run['arm'] == 'A' else 'mcp', state, directory/'server.stderr'))
        # Both arms prove the same treatment assets available before their model
        # window. A uses a temporary SDK session; B keeps its measured session.
        probe = driver
        probe_start = time.monotonic()
        if run['arm'] == 'A':
            probe_state = directory/'catalog-setup-state'
            probe_state.mkdir(mode=0o700)
            probe = broker.Driver(driver_config(config, workspace, 'mcp', probe_state, directory/'catalog-setup.stderr'))
        try:
            receipt['setup_catalog_status'] = probe.request({'op':'call', 'name':'rust.catalog.status', 'arguments':{}}, threading.Event())
            receipt['setup_catalog_identities'] = check_catalog(receipt['setup_catalog_status'], config['expected_catalog'])
        finally:
            if probe is not driver:
                receipt['setup_catalog_cleanup'] = probe.close()
                if receipt['setup_catalog_cleanup'].get('cleanup_failed'):
                    receipt['status'] = 'cleanup_failed'
                    raise broker.BrokerError('catalog_setup_cleanup_failed')
            receipt['catalog_setup_seconds'] = round(time.monotonic()-probe_start,3)
        instance = broker.Broker(run['arm'], workspace, driver, projection,
                                 strict_clippy=True, allow_project_code=True)
        tools = instance.tools(threading.Event())
        instructions = ('\nUse only the supplied tools. The editable closure is src/lib.rs; Cargo.toml and Cargo.lock are immutable. '
                        'Submit each complete repair with submit_patch, or each choice with submit_selection. '
                        'Use object arguments for admitted tools; missing or extra object fields receive retryable denials. Unknown tools or non-object arguments terminate the run. '
                        'Use submit_selection evidence for the requested MSRV, license, caveat, snapshot_fingerprint, provenance source_id and corpus date. '
                        'You have at most 6 submitted candidates, 6 validation requests, 64 total tool calls, 900 seconds and 30000 observed output tokens. '
                        'Quality is one bundled validation request. Strict Clippy is required, and test timeout must be <=30 seconds. '
                        'Do not claim tests, safety or integration without recorded evidence. Final hidden evaluation occurs after your run; it is not available through tools.')
        if run['arm'] == 'B':
            instructions += '\nThe sole host-authorized project root for rust_project_open is: '+str(workspace.root)
        final_prompt = prompt+instructions
        write_json(directory/'prompt.json', {'text': final_prompt, 'sha256': hashlib.sha256(final_prompt.encode()).hexdigest()})
        receipt['driver_ready'] = driver.ready
        receipt['participant'] = participant.run_participant(final_prompt, tools, instance.handle,
                                                           participant_output, wall_seconds=900,
                                                           max_output_tokens=30000)
        receipt['status'] = ('participant_completed' if receipt['participant']['status'] == 'completed'
                             else 'participant_failed_or_interrupted')
        if receipt['participant'].get('cleanup_failed'):
            receipt['status'] = 'cleanup_failed'
    except BaseException as exc:
        receipt['failure_kind'] = type(exc).__name__
        if isinstance(exc, broker.BrokerError):
            receipt['failure_code'] = str(exc)
        if driver is not None:
            driver.cancel_and_join()
    finally:
        receipt['candidate_window_and_setup_seconds'] = round(time.monotonic()-started, 3)
        cleanup_started = time.monotonic()
        if instance is not None:
            receipt['broker'] = instance.receipt()
            patch_number = 0
            for entry in receipt['broker']['candidates']:
                if entry['kind'] == 'patch':
                    patch_number += 1
                    entry['artifact_path'] = str(workspace.artifacts/f'candidate-{patch_number:02d}.rs')
            write_json(directory/'broker.json', receipt['broker'])
        if driver is not None:
            try:
                receipt['driver_cleanup'] = driver.close()
            except BaseException as exc:
                receipt['driver_cleanup'] = driver.cleanup or {'cleanup_failed': True, 'failure_kind': type(exc).__name__}
            if receipt['driver_cleanup'].get('cleanup_failed'):
                receipt['status'] = 'cleanup_failed'
        if workspace is not None:
            receipt['workspace'] = str(workspace.root)
            receipt['candidate_artifacts'] = str(workspace.artifacts)
            workspace.close()
        try:
            receipt['docker_after'] = containment.observe(config['driver']['docker_socket'], directory)
            if not receipt['docker_after']['absent']: receipt['status']='cleanup_failed'
        except Exception as exc:
            receipt['docker_after']={'absent':False,'observation_failure':type(exc).__name__}
            receipt['status']='cleanup_failed'
        receipt['cleanup_seconds'] = round(time.monotonic()-cleanup_started, 3)
        receipt['elapsed_seconds'] = round(time.monotonic()-started, 3)
        try:
            verify(config_path, freeze_path, freeze_sha256)
            receipt['post_run_freeze_verified'] = True
        except Exception:
            receipt['post_run_freeze_verified'] = False
            receipt['status'] = 'infrastructure_failed_input_drift'
        write_json(directory/'run.json', receipt)
    return receipt


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config', required=True)
    parser.add_argument('--freeze', required=True)
    parser.add_argument('--freeze-sha256', required=True)
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument('--plan', action='store_true')
    action.add_argument('--run-id', choices=[r['run_id'] for r in schedule()])
    action.add_argument('--all', action='store_true')
    parser.add_argument('--execute', action='store_true')
    args = parser.parse_args()
    args.config = os.path.abspath(args.config)
    args.freeze = os.path.abspath(args.freeze)
    verify(args.config, args.freeze, args.freeze_sha256)
    runs = schedule() if args.all or args.plan else [r for r in schedule() if r['run_id'] == args.run_id]
    if args.plan:
        print(json.dumps({'runs': runs, 'count': len(runs), 'processes_started': 0}))
        return 0
    if not args.execute:
        parser.error('explicit --execute required; no process started')
    signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(KeyboardInterrupt()))
    failed = False
    for run in runs:
        result = run_one(run, args.config, args.freeze, args.freeze_sha256)
        print(json.dumps({'run_id': run['run_id'], 'status': result['status']}), flush=True)
        failed |= result['status'] != 'participant_completed'
        # Cleanup uncertainty can affect subsequent runs; keep failed records and
        # stop the series rather than contaminate the next pair with lingering work.
        if result['status'] == 'cleanup_failed' or result.get('participant',{}).get('cleanup_failed') or result.get('failure_kind') == 'KeyboardInterrupt' or result.get('docker_before',{}).get('absent') is False or result.get('post_run_freeze_verified') is not True:
            break
    return int(failed)


if __name__ == '__main__':
    sys.exit(main())
