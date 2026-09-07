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
parser.add_argument('--plugins', action='store_true', help='Verify the separate M3 plugin image')
args = parser.parse_args()
if not pathlib.Path(args.docker).is_absolute() or not args.host.startswith('unix:///'):
    parser.error('Absolute Docker path and local Unix socket required')

here = pathlib.Path(__file__).resolve().parent
sources = json.loads((here / 'sources.json').read_text())
image = (args.output / 'image-id').read_text().strip()
docker = [args.docker, '--host', args.host]
inspection = json.loads(subprocess.check_output(docker + ['image', 'inspect', image]))[0]
expected = {
    'User': '65534:65534',
    'Env': ['PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin'],
    'WorkingDir': '/work',
}
if inspection['Config'] != expected or inspection['Architecture'] != 'arm64' or inspection['Os'] != 'linux':
    raise SystemExit('Unexpected image execution configuration or platform')
(args.output / 'image-inspect.json').write_text(json.dumps(inspection, indent=2) + '\n')

gateway_env = sources['provisioning']['gateway_environment']
standard_env = {'PATH': expected['Env'][0].split('=', 1)[1]}
plugin_env = dict(gateway_env)
run = docker + ['run', '--rm', '--pull', 'never', '--platform', 'linux/arm64', '--network', 'none',
                '--read-only', '--cap-drop', 'ALL', '--security-opt', 'no-new-privileges',
                '--user', '65534:65534', '--pids-limit', '64', '--memory', '256m', '--memory-swap',
                '256m', '--cpus', '1', '--log-driver', 'none', '--no-healthcheck', '--workdir', '/work',
                '--tmpfs', '/tmp:size=64m,noexec,nosuid,nodev', '--entrypoint', '/usr/bin/env', image]

results = []


def execute(label, command, environment, required=True):
    name = 'rust-runtime-verify-' + uuid.uuid4().hex
    bounded_run = run.copy()
    image_index = bounded_run.index(image)
    bounded_run[image_index:image_index] = ['--name', name]
    env_args = ['-i'] + [f'{key}={value}' for key, value in environment.items()]
    result = None
    try:
        result = subprocess.run(bounded_run + env_args + command, capture_output=True, text=True, timeout=60)
    except subprocess.TimeoutExpired as error:
        result = subprocess.CompletedProcess(bounded_run, 124,
                                             error.stdout or '', error.stderr or 'timeout')
    finally:
        query = docker + ['ps', '--all', '--quiet', '--filter', 'name=^/' + name + '$']
        if subprocess.check_output(query, timeout=30).strip():
            subprocess.run(docker + ['rm', '--force', name], check=True, timeout=30)
        if subprocess.check_output(query, timeout=30).strip():
            raise SystemExit('Runtime verification container cleanup unconfirmed')
    results.append(dict(label=label, command=command, environment=environment,
                        required=required, exit_code=result.returncode,
                        stdout=result.stdout, stderr=result.stderr))
    return result


base_commands = [
    ('rustc', ['/opt/rust/bin/rustc', '--version', '--verbose']),
    ('cargo', ['/opt/rust/bin/cargo', '--version', '--verbose']),
    ('rustfmt', ['/opt/rust/bin/rustfmt', '--version']),
    ('clippy-driver', ['/opt/rust/bin/clippy-driver', '--version']),
    ('cargo-clippy', ['/opt/rust/bin/cargo-clippy', '--version']),
    ('gcc', ['/usr/bin/gcc', '--version']),
]
for label, command in base_commands:
    execute(label, command, standard_env)

if args.plugins:
    plugin_commands = [
        ('cargo-nextest', ['/opt/rust/bin/cargo-nextest', '--version'], plugin_env),
        ('cargo-llvm-cov', ['/opt/rust/bin/cargo', 'llvm-cov', '--version'], plugin_env),
        ('cargo-semver-checks', ['/opt/rust/bin/cargo-semver-checks', '--version'], plugin_env),
        ('cargo-mutants', ['/opt/rust/bin/cargo-mutants', 'mutants', '--version'], plugin_env),
        ('llvm-profdata', [gateway_env['LLVM_PROFDATA'], '--version'], plugin_env),
        ('llvm-cov', [gateway_env['LLVM_COV'], '--version'], plugin_env),
    ]
    for label, command, environment in plugin_commands:
        execute(label, command, environment)
    cargo_home_only_env = dict(plugin_env)
    cargo_home_only_env['PATH'] = '/usr/bin:/bin'
    execute('cargo-llvm-cov-cargo-home-only',
            ['/opt/rust/bin/cargo', 'llvm-cov', '--version'], cargo_home_only_env)

executable_paths = [command[0] for _, command in base_commands]
if args.plugins:
    executable_paths += [
        '/opt/rust/bin/cargo-nextest', '/opt/rust/bin/cargo-llvm-cov',
        '/opt/rust/bin/cargo-semver-checks', '/opt/rust/bin/cargo-mutants',
        gateway_env['LLVM_PROFDATA'], gateway_env['LLVM_COV'],
    ]
hash_result = execute('executable-sha256', ['/usr/bin/sha256sum', *executable_paths], standard_env)
executable_hashes = {}
for line in hash_result.stdout.splitlines():
    fields = line.split()
    if len(fields) >= 2 and len(fields[0]) == 64:
        executable_hashes[' '.join(fields[1:])] = fields[0]

ldd_result = execute('ldd-available', ['/usr/bin/test', '-x', '/usr/bin/ldd'], standard_env,
                     required=False)
ldd = {'available': ldd_result.returncode == 0, 'results': []}
if ldd['available']:
    for path in executable_paths:
        result = execute('ldd:' + path, ['/usr/bin/ldd', path], standard_env)
        ldd['results'].append({'path': path, 'exit_code': result.returncode,
                               'stdout': result.stdout, 'stderr': result.stderr,
                               'missing_library': 'not found' in result.stdout or 'not found' in result.stderr})

if args.plugins:
    components_result = execute('rust-components',
                                ['/usr/bin/cat', '--', '/opt/rust/lib/rustlib/components'], standard_env)
    package_result = execute('dpkg-list-captured',
                             ['/usr/bin/cat', '--', '/usr/share/doc/rust-runtime/dpkg-query.txt'],
                             standard_env)
    notice_result = execute(
        'license-notices',
        ['/usr/bin/find', '/usr/share/doc/rust-runtime', '-type', 'f', '-print', '-exec',
         '/usr/bin/sha256sum', '{}', ';'], standard_env)
    package_list = package_result.stdout
    notice_files = notice_result.stdout
    installer_paths = [
        '/opt/rust/lib/rustlib/uninstall.sh', '/opt/rust/lib/rustlib/install.sh',
        '/opt/install', '/opt/plugin-install', '/opt/plugins', '/opt/cargo', '/opt/cargo-target',
    ]
    installer_results = [
        execute('absent-build-file:' + path, ['/usr/bin/test', '!', '-e', path], standard_env)
        for path in installer_paths
    ]
    forbidden_paths = [
        '/usr/bin/apt', '/usr/bin/apt-cache', '/usr/bin/apt-get', '/usr/bin/apt-mark',
        '/usr/bin/dpkg', '/usr/bin/dpkg-query', '/usr/lib/apt/methods/http',
        '/usr/lib/apt/methods/https', '/usr/bin/curl', '/usr/bin/wget',
    ]
    for path in forbidden_paths:
        execute('absent:' + path, ['/usr/bin/test', '!', '-e', path], standard_env)
else:
    package_result = execute('dpkg-list', ['/usr/bin/dpkg-query', '-W'], standard_env)
    components_result = execute('rust-components',
                                ['/usr/bin/cat', '--', '/opt/rust/lib/rustlib/components'], standard_env)
    package_list = package_result.stdout
    notice_files = ''
    components = components_result.stdout.splitlines()

receipt = dict(observed_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),
               profile='m3' if args.plugins else 'm1', image_id=image, run_prefix=run,
               gateway_environment=plugin_env if args.plugins else standard_env,
               results=results, executable_sha256=executable_hashes, ldd=ldd,
               dpkg_list=package_list, license_notice_files=notice_files,
               sbom={'base_image': sources['base'], 'rust_version': sources['rust_version'],
                     'target': sources['target'],
                     'rust_components': sorted(components_result.stdout.splitlines()),
                     'plugins': [item['id'] for item in sources.get('plugins', [])] if args.plugins else []})
(args.output / 'verification.json').write_text(json.dumps(receipt, indent=2) + '\n')

if any(result['required'] and result['exit_code'] != 0 for result in results):
    raise SystemExit('A runtime verification command failed; see verification.json')
if not results[0]['stdout'].startswith('rustc 1.98.1 ') or not results[1]['stdout'].startswith('cargo 1.98.1 '):
    raise SystemExit('Unexpected Rust or Cargo version')
expected_components = {'cargo', 'clippy-preview', 'rust-std-aarch64-unknown-linux-gnu', 'rustc', 'rustfmt-preview'}
if args.plugins:
    expected_components.add('llvm-tools-preview')
    expected_plugins = {
        'cargo-nextest', 'cargo-llvm-cov', 'cargo-semver-checks', 'cargo-mutants',
        'llvm-profdata', 'llvm-cov',
    }
    observed_plugins = {result['label'] for result in results if result['label'] in expected_plugins
                        and result['exit_code'] == 0}
    if observed_plugins != expected_plugins:
        raise SystemExit('Unexpected M3 plugin component inventory')
    expected_versions = {
        'cargo-nextest': 'cargo-nextest 0.9.143',
        'cargo-llvm-cov': 'cargo-llvm-cov 0.9.0',
        'cargo-semver-checks': 'cargo-semver-checks 0.50.0',
        'cargo-mutants': 'cargo-mutants 27.1.0',
    }
    for result in results:
        prefix = expected_versions.get(result['label'])
        if prefix is not None and not result['stdout'].startswith(prefix):
            raise SystemExit(f"Unexpected {result['label']} version")
    if not notice_files or 'plugin-license-sources.txt' not in notice_files:
        raise SystemExit('License/notice capture is missing')
    if any(result.returncode != 0 for result in installer_results):
        raise SystemExit('An installer or build input remains in the M3 filesystem')
    if any('missing_library' in entry and entry['missing_library'] for entry in ldd['results']):
        raise SystemExit('A plugin has a missing shared-library dependency')
else:
    components = components_result.stdout.splitlines()
if args.plugins:
    components = components_result.stdout.splitlines()
if len(components) != len(expected_components) or set(components) != expected_components:
    raise SystemExit('Unexpected installed component inventory')
print('Exact Rust/Cargo 1.98.1 and runtime component inventory verified; no sandbox certification')
