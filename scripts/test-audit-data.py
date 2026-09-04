#!/usr/bin/env python3
"""Explicit owned RustSec/SQLite gate under actual macOS network deny; no downloads."""
import json
import os
import pathlib
import platform
import socket
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
TEST = 'audit::tests::owned_rustsec_sqlite_audit_runs_with_actual_network_deny'


def main():
    if sys.platform != 'darwin' or platform.machine() != 'arm64':
        raise RuntimeError('Only macOS ARM64 network-denied test process is calibrated')
    allowed = {'HOME', 'PATH', 'CARGO_HOME', 'RUSTUP_HOME', 'CARGO_TARGET_DIR',
               'SDKROOT', 'DEVELOPER_DIR', 'TMPDIR'}
    env = {k: v for k, v in os.environ.items() if k in allowed}
    env['CARGO_INCREMENTAL'] = '0'
    cargo = pathlib.Path(subprocess.check_output(
        ['rustup', 'which', '--toolchain', '1.98.1', 'cargo'], env=env, text=True).strip())
    env['PATH'] = str(cargo.parent) + os.pathsep + env.get('PATH', '')
    env['RUSTC'] = str(cargo.with_name('rustc'))
    for binary, prefix in [(cargo, 'cargo 1.98.1 '), (cargo.with_name('rustc'), 'rustc 1.98.1 ')]:
        if not subprocess.check_output([str(binary), '--version'], env=env, text=True).startswith(prefix):
            raise RuntimeError('Pinned installed toolchain required; never install from gate')
    result = subprocess.run([str(cargo), 'test', '-p', 'rust-engineering-catalog', '--lib',
                             '--locked', '--offline', '--no-run', '--message-format=json'],
                            cwd=ROOT, env=env, stdout=subprocess.PIPE, text=True, check=True)
    binaries = []
    for line in result.stdout.splitlines():
        row = json.loads(line)
        if (row.get('reason') == 'compiler-artifact' and row.get('executable')
                and row.get('profile', {}).get('test')
                and row.get('target', {}).get('name') == 'rust_engineering_catalog'):
            binaries.append(row['executable'])
    if len(binaries) != 1:
        raise RuntimeError('Expected exactly one actual catalog test binary')
    for family, address in [(socket.AF_INET, ('127.0.0.1', 0)), (socket.AF_INET6, ('::1', 0))]:
        for kind in [socket.SOCK_STREAM, socket.SOCK_DGRAM]:
            with socket.socket(family, kind) as probe:
                probe.bind(address)
    output = ROOT / 'target/audit-data'
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='rust-mcp-audit-data-') as scratch:
        profile = pathlib.Path(scratch) / 'deny-network.sb'
        profile.write_text('(version 1) (allow default) (deny network*)\n')
        absent = pathlib.Path(scratch) / 'no-runtime-temp'
        runtime = {'PATH': '/usr/bin:/bin', 'TMPDIR': str(absent), 'RUST_MCP_NETWORK_DENIED': '1'}
        result = subprocess.run(['/usr/bin/sandbox-exec', '-f', str(profile), binaries[0],
                                 TEST, '--exact', '--ignored', '--nocapture', '--test-threads=1'],
                                env=runtime, cwd=ROOT, stdout=subprocess.PIPE,
                                stderr=subprocess.STDOUT, text=True)
        (output / 'audit-data.log').write_text(result.stdout)
        if result.returncode or 'test result: ok. 1 passed;' not in result.stdout:
            raise RuntimeError('Actual network-denied audit did not pass: target/audit-data/audit-data.log')
        if absent.exists():
            raise RuntimeError('Unexpected temporary directory creation')
    marker = 'M1_AUDIT_DATA_RECEIPT '
    receipts = [json.loads(line.partition(marker)[2]) for line in result.stdout.splitlines() if marker in line]
    if len(receipts) != 1 or not receipts[0]['actual_rustsec_sqlite_finding']:
        raise RuntimeError('Missing actual RustSec/SQLite receipt')
    receipt = receipts[0]
    receipt.update(positive_tcp_udp_ipv4_ipv6_controls=True, runtime_temp_absent=True)
    (output / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print('PASS owned RustSec/SQLite under actual macOS test-process network deny; no temp writes')


if __name__ == '__main__':
    if not __debug__:
        raise RuntimeError('Optimized Python is rejected')
    main()
