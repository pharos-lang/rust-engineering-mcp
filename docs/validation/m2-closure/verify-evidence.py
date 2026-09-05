#!/usr/bin/env python3
"""Read-only verification of the final M2 receipts and current qualified inputs.

Run from any directory; emits JSON to stdout. It never runs Cargo or Docker.
The release binary must already exist from the recorded locked/offline build.
"""
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import runpy
import subprocess
import urllib.parse

ROOT = Path(__file__).resolve().parents[3]


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def sha(path):
    return hashlib.sha256((ROOT / path).read_bytes()).hexdigest()


def read(path):
    return json.loads((ROOT / path).read_text())


def main():
    full_path = 'docs/validation/M2-full-gate.json'
    full = read(full_path)
    require(full['status'] == 'passed', 'full gate not passed')
    require(len(full['steps']) == 24 and all(s['status'] == 'passed' for s in full['steps']),
            'full stage evidence incomplete')
    require(full['source_inputs_unchanged'], 'gate observed source drift')
    gate = runpy.run_path(str(ROOT / 'scripts/gate.py'))
    current = gate['source_inventory'](ROOT, os.environ.copy())
    require(current == full['source_inputs'], 'current qualified source inventory differs')

    runtime = read('docs/validation/M2-final-runtime.json')
    require(runtime['status'] == 'passed' and len(runtime['steps']) == 10,
            'runtime selections incomplete')
    require(sum(s['expected_executed'] for s in runtime['steps']) == 17,
            'runtime cases incomplete')
    for row in runtime['sources'] + runtime['configuration_inputs']:
        require(sha(row['path']) == row['sha256'], 'runtime input mismatch: ' + row['path'])
    for step in runtime['steps']:
        require(step['status'] == 'passed' and step['exit_code'] == 0, 'runtime failed')
        require(sha(step['log']) == step['log_sha256'], 'runtime log mismatch')
        text = (ROOT / step['log']).read_text()
        expected = f"test result: ok. {step['expected_executed']} passed; 0 failed; 0 ignored;"
        require(expected in text, 'runtime did not execute required cases')

    logs = read('docs/validation/M2-final-log-inventory.json')
    require(logs['gate_sha256'] == sha(full_path), 'log inventory bound to different gate')
    for row in logs['logs']:
        require(sha(row['path']) == row['sha256'], 'log mismatch: ' + row['path'])
        require((ROOT / row['path']).stat().st_size == row['bytes'], 'log size mismatch')

    m1 = read('docs/validation/M2-m1-contract-preservation.json')
    require(len(m1['exact_unchanged_snapshots']) == 13, 'M1 snapshot count')
    for row in m1['exact_unchanged_snapshots']:
        baseline = subprocess.check_output(['git', 'show', m1['public_baseline'] + ':' + row['path']], cwd=ROOT)
        require(baseline == (ROOT / row['path']).read_bytes(), 'M1 changed: ' + row['path'])
        require(sha(row['path']) == row['sha256'], 'M1 snapshot receipt mismatch')

    client_path = 'docs/validation/M2-clients.json'
    client = read(client_path)
    binary = read('docs/validation/M2-client-binary.json')
    require(client['status'] == 'passed', 'client qualification not passed')
    require(len(client['claude']['stream']['calls']) == 17, 'client call count')
    require(len(client['claude']['stream']['results']) == 17 and
            all(r['status'] == 'passed' for r in client['claude']['stream']['results']),
            'client results incomplete')
    require(sha(binary['binary']) == binary['binary_sha256'] == client['identities']['server_sha256'],
            'binary/client mismatch')
    require(binary['full_gate_sha256'] == sha(full_path), 'binary not bound to current full')
    require(binary['client_receipt_sha256'] == sha(client_path), 'binary/client receipt binding')
    require(sha(binary['build_log']) == binary['build_log_sha256'], 'build log mismatch')

    docs = ['README.md', 'CHANGELOG.md', 'SECURITY.md', 'docs/architecture.md',
            'docs/tools.md', 'docs/security-model.md', 'docs/compatibility.md',
            'docs/client-configuration.md', 'docs/implementation-status.md',
            'docs/roadmap/m2-safe-mutation.md', 'docs/roadmap/m2-m8.md',
            'docs/validation/M2-07.md', 'docs/validation/M2-matrix.md',
            'docs/validation/M2-traceability.md']
    docs += [str(p.relative_to(ROOT)) for p in (ROOT / 'docs/adr').glob('ADR-05*.md')]
    docs += [str(p.relative_to(ROOT)) for p in (ROOT / 'docs/reviews').glob('M2*.md')]
    local_links = 0
    for name in docs:
        fence = None
        for line in (ROOT / name).read_text().splitlines():
            match = re.match(r'^\s*(`{3,}|~{3,})', line)
            if match:
                mark = match[1][0]
                fence = None if fence == mark else mark
                continue
            if fence:
                continue
            for target in re.findall(r'\]\(([^)]+)\)', line):
                url = urllib.parse.urlsplit(target.strip('<>'))
                if url.scheme or not url.path:
                    continue
                local_links += 1
                require((ROOT / name).parent.joinpath(urllib.parse.unquote(url.path)).exists(),
                        'missing document target: ' + name + ' -> ' + target)
        require(fence is None, 'unclosed fence: ' + name)
    subprocess.run(['git', 'diff', '--check'], cwd=ROOT, check=True)
    print(json.dumps({
        'status': 'passed', 'recorded_at': datetime.datetime.now(datetime.UTC).isoformat(),
        'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
        'full_gate_sha256': sha(full_path), 'source_inputs': len(current),
        'runtime_cases': 17, 'runtime_selections': 10,
        'verified_logs': len(logs['logs']), 'm1_identical_snapshots': 13,
        'binary_sha256': binary['binary_sha256'],
        'client_receipt_sha256': sha(client_path), 'checked_documents': len(docs),
        'local_link_targets': local_links, 'fences_and_diff_check': 'passed',
        'script_sha256': sha(str(Path(__file__).resolve().relative_to(ROOT))),
        'limits': 'Local link targets and fences; no remote URL or fragment-anchor validation. No Cargo/Docker execution.'
    }, indent=2))


if __name__ == '__main__':
    main()
