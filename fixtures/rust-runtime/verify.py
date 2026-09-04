#!/usr/bin/env python3
"""Verify provisioned tool versions only; this is not sandbox calibration."""
import argparse
import datetime
import json
import pathlib
import subprocess
import uuid

parser = argparse.ArgumentParser()
parser.add_argument('--docker', required=True)
parser.add_argument('--host', required=True)
parser.add_argument('--output', required=True, type=pathlib.Path)
args = parser.parse_args()
if not pathlib.Path(args.docker).is_absolute() or not args.host.startswith('unix:///'):
    parser.error('Absolute Docker path and local Unix socket required')
image = (args.output / 'image-id').read_text().strip()
docker = [args.docker, '--host', args.host]
inspection = json.loads(subprocess.check_output(docker + ['image', 'inspect', image]))[0]
expected = {'User': '65534:65534', 'Env': ['PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin'], 'WorkingDir': '/work'}
if inspection['Config'] != expected or inspection['Architecture'] != 'arm64' or inspection['Os'] != 'linux':
    raise SystemExit('Unexpected image execution configuration or platform')
(args.output / 'image-inspect.json').write_text(json.dumps(inspection, indent=2) + '\n')
run = docker + ['run', '--rm', '--pull', 'never', '--platform', 'linux/arm64', '--network', 'none',
                '--read-only', '--cap-drop', 'ALL', '--security-opt', 'no-new-privileges',
                '--user', '65534:65534', '--pids-limit', '64', '--memory', '256m', '--memory-swap',
                '256m', '--cpus', '1', '--log-driver', 'none', '--no-healthcheck', '--workdir', '/work',
                '--tmpfs', '/tmp:size=64m,noexec,nosuid,nodev', '--entrypoint', '/usr/bin/env', image,
                '-i', expected['Env'][0]]
commands = [['/opt/rust/bin/rustc', '--version', '--verbose'],
            ['/opt/rust/bin/cargo', '--version', '--verbose'], ['/opt/rust/bin/rustfmt', '--version'],
            ['/opt/rust/bin/clippy-driver', '--version'], ['/opt/rust/bin/cargo-clippy', '--version'],
            ['/usr/bin/gcc', '--version'],
            ['/usr/bin/sha256sum', '/opt/rust/bin/rustc', '/opt/rust/bin/cargo', '/opt/rust/bin/rustfmt',
              '/opt/rust/bin/clippy-driver', '/opt/rust/bin/cargo-clippy'], ['/usr/bin/dpkg-query', '-W'],
            ['/usr/bin/cat', '--', '/opt/rust/lib/rustlib/components']]
results = []
for command in commands:
    name = 'rust-runtime-verify-' + uuid.uuid4().hex
    bounded_run = run.copy()
    bounded_run[4:4] = ['--name', name]
    try:
        result = subprocess.run(bounded_run + command, capture_output=True, text=True, timeout=60)
    finally:
        query = docker + ['ps', '--all', '--quiet', '--filter', 'name=^/' + name + '$']
        if subprocess.check_output(query, timeout=30).strip():
            subprocess.run(docker + ['rm', '--force', name], check=True, timeout=30)
        if subprocess.check_output(query, timeout=30).strip():
            raise SystemExit('Runtime verification container cleanup unconfirmed')
    results.append(dict(command=command, exit_code=result.returncode, stdout=result.stdout, stderr=result.stderr))
receipt = dict(observed_at=datetime.datetime.now(datetime.timezone.utc).isoformat(), image_id=image,
               run_prefix=run, results=results)
(args.output / 'verification.json').write_text(json.dumps(receipt, indent=2) + '\n')
if any(result['exit_code'] != 0 for result in results):
    raise SystemExit('A runtime verification command failed; see verification.json')
if not results[0]['stdout'].startswith('rustc 1.98.1 ') or not results[1]['stdout'].startswith('cargo 1.98.1 '):
    raise SystemExit('Unexpected Rust or Cargo version')
expected_components = {'cargo', 'clippy-preview', 'rust-std-aarch64-unknown-linux-gnu', 'rustc', 'rustfmt-preview'}
components = results[-1]['stdout'].splitlines()
if len(components) != len(expected_components) or set(components) != expected_components:
    raise SystemExit('Unexpected installed component inventory')
print('Exact Rust/Cargo 1.98.1 and installed components verified; no sandbox certification')
