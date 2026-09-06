#!/usr/bin/env python3
"""Serial, explicit doctor gate. No provisioning, image pulls or user projects."""
import json
import os
import pathlib
import platform
import re
import selectors
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
DOCKER = '/Applications/Docker.app/Contents/Resources/bin/docker'
IMAGE = 'sha256:384a1742ecc53cdd3a9c0bf36c6f8b66db73ddd118aeeae6e55654ea998ae36a'
CALIBRATION = '/opt/rust/bin/cargo check --frozen --message-format=json --jobs=1'
FINGERPRINT = re.compile(r'sha256:[0-9a-f]{64}\Z')
JOB = re.compile(r'rust-mcp-cargo-([0-9a-f]{32})\Z')
JOIN_SECONDS = 300


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


class Capture:
    """Drain both pipes incrementally, retaining at most limit bytes per stream."""

    def __init__(self, command, env, cwd, limit, group=False):
        self.child = subprocess.Popen(command, cwd=cwd, env=env,
                                      stdin=subprocess.DEVNULL,
                                      stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                      start_new_session=group)
        self.group = group
        self.selector = selectors.DefaultSelector()
        self.data = {'stdout': bytearray(), 'stderr': bytearray()}
        self.total = {'stdout': 0, 'stderr': 0}
        self.limit = limit
        self.closed = False
        for name, pipe in [('stdout', self.child.stdout), ('stderr', self.child.stderr)]:
            os.set_blocking(pipe.fileno(), False)
            self.selector.register(pipe, selectors.EVENT_READ, name)

    def pump(self, wait=0.05):
        if not self.selector.get_map():
            time.sleep(min(wait, 0.05))
            return
        for key, _ in self.selector.select(wait):
            try:
                chunk = os.read(key.fileobj.fileno(), 65536)
            except BlockingIOError:
                continue
            if not chunk:
                self.selector.unregister(key.fileobj)
                key.fileobj.close()
                continue
            name = key.data
            self.total[name] += len(chunk)
            remaining = self.limit - len(self.data[name])
            if remaining > 0:
                self.data[name].extend(chunk[:remaining])

    def finish(self, deadline):
        while self.child.poll() is None or self.selector.get_map():
            if time.monotonic() >= deadline:
                raise RuntimeError('Child or its output pipes exceeded the harness deadline')
            self.pump(min(0.05, max(0, deadline - time.monotonic())))
        return self.child.wait()

    def send(self, sig):
        if self.child.poll() is None:
            try:
                # Doctor cancellation deliberately targets only its PID. Its gateway
                # owns Docker descendants and must join their cleanup itself.
                os.kill(self.child.pid, sig)
            except ProcessLookupError:
                pass

    def force_stop(self):
        if self.closed:
            return
        if self.child.poll() is None:
            try:
                if self.group:
                    os.killpg(self.child.pid, signal.SIGKILL)
                else:
                    self.child.kill()
            except ProcessLookupError:
                pass
        try:
            self.finish(time.monotonic() + 5)
        except RuntimeError:
            pass
        finally:
            for key in list(self.selector.get_map().values()):
                self.selector.unregister(key.fileobj)
                key.fileobj.close()
            self.selector.close()
            self.closed = True

    def save(self, output, label):
        for name, data in self.data.items():
            (output / f'{label}.{name}.log').write_bytes(data)

    def bounded(self):
        require(all(count <= self.limit for count in self.total.values()),
                f'Captured output exceeded {self.limit} bytes per stream; prefix retained')


def run(command, env, cwd, timeout, limit=65536, output=None, label=None, group=False):
    capture = Capture(command, env, cwd, limit, group)
    try:
        code = capture.finish(time.monotonic() + timeout)
        capture.bounded()
        require(code == 0, f'{label or "Control command"} failed with exit {code}')
        return bytes(capture.data['stdout'])
    finally:
        capture.force_stop()
        if output is not None:
            capture.save(output, label)


def report_from(capture):
    capture.bounded()
    require(len(capture.data['stdout']) <= 128 * 1024, 'Doctor report exceeds ADR045 bound')
    report = json.loads(capture.data['stdout'])
    require(isinstance(report, dict) and set(report) == {
        'format_version', 'operation', 'mode', 'status', 'duration_ms',
        'checks', 'catalog', 'runtime'}, 'Unexpected doctor report fields')
    require(type(report['format_version']) is int and report['format_version'] == 1
            and report['operation'] == 'doctor'
            and report['mode'] == 'active', 'Wrong doctor format, operation or mode')
    require(report['status'] in ('passed', 'warning', 'failed'), 'Unknown overall status')
    require(type(report['duration_ms']) is int and report['duration_ms'] >= 0,
            'Missing measured diagnostic duration')
    require(isinstance(report['checks'], list) and 0 < len(report['checks']) <= 32,
            'Missing or unbounded diagnostic checks')
    for check in report['checks']:
        require(isinstance(check, dict) and set(check) == {
            'id', 'scope', 'status', 'reason', 'component_reason', 'action', 'severity'},
            'Unexpected diagnostic check fields')
        require(check['scope'] in ('catalog_snapshot', 'local_model', 'host_filesystem',
                                  'approved_runtime', 'host', 'compiled_feature', 'diagnostic')
                and check['status'] in ('available', 'unavailable', 'not_configured',
                                        'not_checked', 'not_used', 'warning')
                and check['severity'] in ('passed', 'warning', 'failed'),
                'Unknown diagnostic scope or status')
        require(check['reason'] in ('verified', 'not_configured', 'active_required',
                                   'unavailable', 'unsupported_platform', 'unknown', 'fresh',
                                   'freshness_needs_review', 'owned_rustsec_engine', 'interrupted',
                                   'cleanup_uncertain', 'deadline', 'output_limit', 'internal'),
                'Unknown diagnostic reason')
        require(check['action'] in ('none', 'configure_optional', 'review_configured_files',
                                    'run_active', 'review_runtime', 'refresh_snapshot_explicitly',
                                    'use_supported_platform', 'review_diagnostic'),
                'Unknown diagnostic action')
    return report


def validate_success(report):
    require(report['status'] in ('passed', 'warning'), 'Active doctor failed')
    checks = report['checks']
    for name in ('rustc', 'cargo', 'rustfmt', 'clippy', 'sandbox'):
        found = [check for check in checks if check['id'] == name]
        require(len(found) == 1 and found[0]['scope'] == 'approved_runtime'
                and found[0]['status'] == 'available' and found[0]['reason'] == 'verified'
                and found[0]['severity'] == 'passed', f'Missing verified runtime check: {name}')
    require(not any(check['severity'] == 'failed' for check in checks),
            'Passing report contains failed checks')
    observation = report['runtime']
    require(isinstance(observation, dict), 'Missing actual runtime observation')
    expected_inventory = {
        'rustc_version': '1.98.1', 'cargo_version': '1.98.1', 'channel': 'stable',
        'host_triple': 'aarch64-unknown-linux-gnu',
        'installed_targets': ['aarch64-unknown-linux-gnu'],
        'installed_components': [
            {'component': 'cargo', 'target': None},
            {'component': 'clippy', 'target': None},
            {'component': 'rust_std', 'target': 'aarch64-unknown-linux-gnu'},
            {'component': 'rustc', 'target': None},
            {'component': 'rustfmt', 'target': None}],
    }
    require(observation.get('inventory') == expected_inventory, 'Runtime inventory drift')
    require(observation.get('declared_toolchain') is None,
            'Synthetic diagnostic source unexpectedly declares a toolchain')
    require(FINGERPRINT.fullmatch(observation.get('source_fingerprint', '')),
            'Missing diagnostic source fingerprint')
    runtime = observation['runtime']
    require(runtime['image_id'] == IMAGE and runtime['platform'] == 'linux/aarch64',
            'Unapproved runtime identity')
    require(FINGERPRINT.fullmatch(runtime['configuration_fingerprint']),
            'Missing configuration fingerprint')
    executions = runtime['executions']
    require([entry['command'] for entry in executions] == [
        'compiler_version', 'cargo_version', 'installed_components'],
        'Missing actual fixed inventory executions')
    fingerprints = [entry['execution_fingerprint'] for entry in executions]
    require(all(FINGERPRINT.fullmatch(value) for value in fingerprints)
            and len(set(fingerprints)) == 3, 'Invalid or reused execution fingerprints')


def stalled_stdout(binary, root, output, label, interrupt):
    """Do not drain stdout: only product signal/deadline handling can release it."""
    read_fd, write_fd = os.pipe()
    child = None
    receipt = {'case': label, 'status': 'running', 'stdout_drained': False,
               'forced_stop': False, 'signal': 'SIGINT' if interrupt else None}
    try:
        os.set_blocking(write_fd, False)
        filled = 0
        # Finish with single bytes to guarantee no space remains for even a short
        # diagnostic. A bounded prefill also protects against unusual pipe behavior.
        for chunk in (b'x' * 4096, b'x'):
            while True:
                try:
                    filled += os.write(write_fd, chunk)
                    require(filled <= 16 * 1024 * 1024, 'Pipe prefill exceeded bound')
                except BlockingIOError:
                    break
        require(filled > 0, 'Pipe was not prefilled')
        os.set_blocking(write_fd, True)
        receipt['prefill_bytes'] = filled
        stderr_path = output / f'{label}.stderr.log'
        with stderr_path.open('wb') as stderr:
            started = time.monotonic()
            child = subprocess.Popen([binary, 'doctor', '--json'], cwd=root, env={},
                                     stdin=subprocess.DEVNULL, stdout=write_fd,
                                     stderr=stderr, start_new_session=True)
            os.close(write_fd)
            write_fd = None
            # With no configured inputs, passive observation completes quickly;
            # the completely full pipe prevents any report from being accepted.
            settle = started + 0.5
            while time.monotonic() < settle:
                require(child.poll() is None, 'Doctor exited before stalled-output observation')
                require(stderr_path.stat().st_size <= 128 * 1024, 'Stderr exceeded bound')
                time.sleep(0.01)
            signal_time = None
            if interrupt:
                signal_time = time.monotonic()
                os.kill(child.pid, signal.SIGINT)
            deadline = (signal_time + 3) if interrupt else (started + 8)
            while child.poll() is None:
                require(time.monotonic() < deadline, 'Doctor did not exit with stalled stdout')
                require(stderr_path.stat().st_size <= 128 * 1024, 'Stderr exceeded bound')
                time.sleep(0.01)
            code = child.wait()
            finished = time.monotonic()
            elapsed = finished - (signal_time if interrupt else started)
            require(code == 1, f'Stalled-output case must exit 1, got {code}')
            require(elapsed <= (3 if interrupt else 8), 'Output completion exceeded bound')
            # Reject immediate failure as evidence of the five-second output timer.
            if not interrupt:
                require(finished - started >= 5, 'Doctor failed before its output deadline')
            receipt.update(status='passed', exit_code=code,
                           elapsed_seconds=round(finished - started, 3),
                           signal_join_seconds=round(elapsed, 3) if interrupt else None)
            return receipt
    except BaseException as error:
        receipt.update(status='failed', error=str(error))
        raise
    finally:
        try:
            if child is not None and child.poll() is None:
                # Failure-only recovery; a forced stop can never contribute a pass.
                receipt['forced_stop'] = True
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                child.wait(timeout=5)
        finally:
            os.close(read_fd)
            if write_fd is not None:
                os.close(write_fd)
            (output / f'{label}.json').write_text(json.dumps(receipt, indent=2) + '\n')


def main():
    require(sys.platform == 'darwin' and platform.machine() == 'arm64',
            'Doctor active gate requires macOS ARM64 and the approved Linux ARM64 runtime')
    socket = os.environ.get('RUST_MCP_TEST_SOCKET', '')
    require(socket.startswith('/') and not any(ord(char) < 32 or ord(char) == 127
                                               for char in socket),
            'RUST_MCP_TEST_SOCKET must be an explicit absolute socket path')
    require(stat.S_ISSOCK(os.stat(socket).st_mode), 'Explicit Docker socket is not a socket')
    require(pathlib.Path(DOCKER).is_file(), 'Approved Docker executable is unavailable')
    allowed = {'HOME', 'PATH', 'TMPDIR', 'CARGO_HOME', 'RUSTUP_HOME',
               'SDKROOT', 'DEVELOPER_DIR', 'CARGO_TARGET_DIR'}
    env = {key: value for key, value in os.environ.items() if key in allowed}
    env.update(CARGO_INCREMENTAL='0', CARGO_TERM_COLOR='never', ORT_SKIP_DOWNLOAD='1')
    output = ROOT / 'target/doctor-security'
    output.mkdir(parents=True, exist_ok=True)
    receipt = {'status': 'running', 'active_cases': 0, 'output_cases': 0,
               'output_results': [], 'cleanup': False,
               'image_id': IMAGE, 'observed_calibration_jobs': []}
    root = pathlib.Path(tempfile.mkdtemp(prefix='rust-mcp-doctor-gate-', dir='/private/tmp'))
    os.chmod(root, 0o700)
    clean = False
    try:
        docker_config = root / 'docker-config'
        docker_config.mkdir(mode=0o700)
        (docker_config / 'config.json').write_text('{}\n')
        docker = [DOCKER, '--config', str(docker_config), '--host', f'unix://{socket}']

        def control(args, deadline=None):
            timeout = 15 if deadline is None else min(15, deadline - time.monotonic())
            require(timeout > 0, 'Docker observation exceeded the shared deadline')
            return run(docker + args, {}, root, timeout)

        def objects(kind, nonce=None, running=False, deadline=None):
            args = [kind, 'ls']
            if kind == 'container':
                if not running:
                    args.append('--all')
                args.append('--no-trunc')
            args += ['--filter', 'label=org.rust-mcp.execution=true', '--filter',
                     'label=org.rust-mcp.rust-job' + (f'={nonce}' if nonce else ''),
                     '--format', '{{.Names}}\t{{.Command}}' if kind == 'container'
                     else '{{.Name}}']
            return [line for line in control(args, deadline).decode('utf-8').splitlines() if line]

        def assert_clean(nonce=None, deadline=None):
            for kind in ('container', 'volume'):
                require(not objects(kind, nonce, deadline=deadline),
                        f'Owned {kind} objects remain; no cleanup/removal attempted by harness')

        # Refuse to borrow another gate's objects or erase earlier failures. Parent
        # runs this gate serially; these listings never delete Docker objects.
        assert_clean()
        identity = json.loads(control(['image', 'inspect', IMAGE, '--format',
                                     '{{json .Id}}']))
        target = control(['image', 'inspect', IMAGE, '--format',
                          '{{.Os}}/{{.Architecture}}']).decode().strip()
        require(identity == IMAGE and target == 'linux/arm64', 'Approved image missing or changed')
        cargo = pathlib.Path(run(['rustup', 'which', '--toolchain', '1.98.1', 'cargo'],
                                 env, ROOT, 30).decode().strip())
        require(cargo.is_absolute() and cargo.is_file(), 'Installed Cargo path unavailable')
        env['PATH'] = str(cargo.parent) + os.pathsep + env.get('PATH', '')
        env['RUSTC'] = str(cargo.with_name('rustc'))
        for binary, prefix in [(str(cargo), 'cargo 1.98.1 '), (env['RUSTC'], 'rustc 1.98.1 ')]:
            require(run([binary, '--version'], env, ROOT, 30).decode().startswith(prefix),
                    'Rust/Cargo 1.98.1 required; no automatic substitution')
        print('DOCTOR build core CLI and run ordinary contract tests', flush=True)
        build = run([str(cargo), 'build', '--locked', '--offline', '--no-default-features',
                     '-p', 'rust-engineering-mcp', '--bin', 'rust-engineering-mcp',
                     '--message-format=json'], env, ROOT, 900, 8 * 1024 * 1024,
                    output, 'build', group=True)
        artifacts = [json.loads(line) for line in build.splitlines() if line]
        binaries = {item['executable'] for item in artifacts
                    if item.get('reason') == 'compiler-artifact'
                    and item.get('target', {}).get('name') == 'rust-engineering-mcp'
                    and item.get('executable')}
        require(len(binaries) == 1, 'Build did not identify exactly one core CLI binary')
        binary = binaries.pop()
        ordinary = run([str(cargo), 'test', '--locked', '--offline', '--no-default-features',
                        '-p', 'rust-engineering-mcp', '--test', 'doctor', '--',
                        '--test-threads=1'], env, ROOT, 900, 8 * 1024 * 1024,
                       output, 'ordinary', group=True)
        require(re.search(rb'test result: ok\. [1-9][0-9]* passed; 0 failed;', ordinary),
                'Ordinary doctor tests did not execute successfully')

        def start(label):
            state = root / label
            state.mkdir(mode=0o700)
            command = [binary, 'doctor', '--active', '--json', '--docker', DOCKER,
                       '--docker-socket', socket, '--rust-image', IMAGE, '--state-root', str(state)]
            return Capture(command, {}, root, 256 * 1024)

        def join_failure(capture, deadline=None):
            if capture.child.poll() is None:
                capture.send(signal.SIGINT)
                try:
                    capture.finish(deadline if deadline is not None
                                   else time.monotonic() + JOIN_SECONDS)
                except RuntimeError:
                    # A forced stop is a failed gate with retained state, never a
                    # successful cleanup claim. Do not remove Docker objects here.
                    capture.force_stop()

        print('DOCTOR actual successful calibration and runtime inventory', flush=True)
        successful = start('success-state')
        success_deadline = time.monotonic() + 900 + JOIN_SECONDS
        try:
            code = successful.finish(success_deadline)
            report = report_from(successful)
            require(code == 0, f'Active doctor failed with exit {code}')
            validate_success(report)
            (output / 'active.json').write_text(json.dumps(report, indent=2) + '\n')
            assert_clean()
            receipt['active_cases'] += 1
        finally:
            join_failure(successful, success_deadline)
            successful.force_stop()
            successful.save(output, 'active')

        for sig, label in [(signal.SIGINT, 'sigint'), (signal.SIGTERM, 'sigterm'),
                           (signal.SIGHUP, 'sighup')]:
            print(f'DOCTOR {label.upper()} during observed calibration and joined cleanup',
                  flush=True)
            assert_clean()
            cancelled = start(f'{label}-state')
            join_deadline = None
            try:
                deadline = time.monotonic() + 90
                nonce = None
                while time.monotonic() < deadline:
                    cancelled.pump(0)
                    require(cancelled.child.poll() is None,
                            'Doctor exited before a running calibration container was observed')
                    matches = []
                    for line in objects('container', running=True, deadline=deadline):
                        name, separator, command = line.partition('\t')
                        match = JOB.fullmatch(name)
                        if separator and match and command.strip('"') == CALIBRATION:
                            matches.append(match.group(1))
                    require(len(matches) <= 1, 'Concurrent calibration jobs violate serial gate')
                    if matches:
                        nonce = matches[0]
                        break
                    cancelled.pump(0.025)
                require(nonce is not None, 'No running calibration cargo check observed within 90s')
                require(nonce not in receipt['observed_calibration_jobs'],
                        'Fresh signal case reused a previous calibration nonce')
                receipt['observed_calibration_jobs'].append(nonce)
                interrupted = time.monotonic()
                join_deadline = interrupted + JOIN_SECONDS
                cancelled.send(sig)
                code = cancelled.finish(join_deadline)
                report = report_from(cancelled)
                require(code == 1 and report['status'] == 'failed',
                        f'{label} must produce a failed report and exit 1')
                require(any(check['reason'] == 'interrupted' and check['severity'] == 'failed'
                            for check in report['checks']),
                        'Missing explicit interruption diagnosis')
                require(not any(check['reason'] == 'cleanup_uncertain'
                                for check in report['checks']),
                        'Doctor reported uncertain cleanup')
                assert_clean(nonce, deadline=join_deadline)
                assert_clean(deadline=join_deadline)
                elapsed = round(time.monotonic() - interrupted, 3)
                receipt[f'{label}_join_seconds'] = elapsed
                require(elapsed <= JOIN_SECONDS, f'{label} cleanup verification exceeded 300s')
                (output / f'cancelled-{label}.json').write_text(json.dumps(report, indent=2) + '\n')
                receipt['active_cases'] += 1
            finally:
                join_failure(cancelled, join_deadline)
                cancelled.force_stop()
                cancelled.save(output, f'cancelled-{label}')

        for interrupt in (True, False):
            label = 'stdout-sigint' if interrupt else 'stdout-deadline'
            print(f'DOCTOR passive {label} with a prefilled undrained stdout pipe', flush=True)
            result = stalled_stdout(binary, root, output, label, interrupt)
            receipt['output_results'].append(result)
            receipt['output_cases'] += 1
        assert_clean()
        receipt.update(status='passed', cleanup=True)
        clean = True
    except BaseException as error:
        receipt.update(status='failed', error=str(error), retained_state=str(root))
        raise
    finally:
        (output / 'report.json').write_text(json.dumps(receipt, indent=2) + '\n')
        if clean:
            shutil.rmtree(root)
        else:
            print(f'DOCTOR failed; private state retained: {root}', file=sys.stderr)
    print(f'PASS doctor calibration, SIGINT/SIGTERM/SIGHUP cleanup and stalled output: {output}', flush=True)


if __name__ == '__main__':
    require(__debug__, 'Optimized Python mode is rejected')
    main()
