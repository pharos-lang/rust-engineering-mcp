#!/usr/bin/env python3
"""Local review artifacts only. Never download, install globally or publish."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]
OUT = Path(__file__).resolve().parent
TARGET = ROOT / 'target/semantic-compat'
ORT = Path('<LOCAL_HOME>/Library/Caches/ort.pyke.io/dfbin/aarch64-apple-darwin/612739f75438dc0a075461e1fb454226b4a1eb175e60a7271ba966bbbb972cd4')
ORT_HASH = '4d53c916ea95f09203324f9aad7b76f75c16d8a4bc98f8a949ea0ac73c07604d'
LIMIT = 16 * 1024 * 1024


def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def query(command, env=None):
    return subprocess.check_output(command, cwd=ROOT, env=env, timeout=60).decode().strip()


def sources():
    paths = subprocess.check_output(['git', 'ls-files', '-z', '--cached', '--others',
                                     '--exclude-standard'], cwd=ROOT).decode().split('\0')
    rows = []
    for path in sorted(set(paths)):
        if not path:
            continue
        if (path in ('Cargo.lock', 'Cargo.toml', 'rust-toolchain.toml')
                or path.startswith(('crates/', 'vendor/'))
                and ('/src/' in path or path.endswith(('/Cargo.toml', '/build.rs')))):
            item = ROOT / path
            if item.is_symlink() or not item.is_file():
                raise RuntimeError('Unexpected nonregular source: ' + path)
            rows.append({'path': path, 'bytes': item.stat().st_size, 'sha256': sha(item)})
    return rows


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    os.chmod(OUT, 0o700)
    if {p.name for p in ORT.iterdir()} != {'libonnxruntime.a'} or sha(ORT / 'libonnxruntime.a') != ORT_HASH:
        raise RuntimeError('Approved cached ORT identity mismatch')
    env = {k: v for k, v in os.environ.items() if k in {
        'HOME', 'PATH', 'TMPDIR', 'SDKROOT', 'DEVELOPER_DIR', 'CARGO_HOME', 'RUSTUP_HOME'}}
    cargo = Path(query(['rustup', 'which', '--toolchain', '1.98.1', 'cargo'], env))
    env.update(CARGO_INCREMENTAL='0', CARGO_TARGET_DIR=str(TARGET),
               CARGO_TERM_COLOR='never', ORT_LIB_LOCATION=str(ORT), ORT_SKIP_DOWNLOAD='1',
               RUSTC=str(cargo.with_name('rustc')))
    env['PATH'] = str(cargo.parent) + os.pathsep + env.get('PATH', '')
    cargo_version = query([str(cargo), '--version'], env)
    rust_version = query([env['RUSTC'], '--version', '--verbose'], env)
    if not cargo_version.startswith('cargo 1.98.1 ') or not rust_version.startswith('rustc 1.98.1 '):
        raise RuntimeError('Exact installed Rust/Cargo 1.98.1 required')
    frozen = sources()
    (OUT / 'source-files.json').write_text(json.dumps(frozen, indent=2) + '\n')
    receipt = {'scope': 'local-review-only-not-distribution', 'status': 'building',
               'source_commit': query(['git', 'rev-parse', 'HEAD']),
               'source_status': query(['git', 'status', '--short']),
               'source_manifest_sha256': sha(OUT / 'source-files.json'),
               'cargo_lock_sha256': sha(ROOT / 'Cargo.lock'),
               'cargo': cargo_version, 'rustc': rust_version,
               'native_ort_sha256': ORT_HASH, 'native_ort_bytes': (ORT / 'libonnxruntime.a').stat().st_size,
               'builds': []}

    def save():
        (OUT / 'build-receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')

    save()
    try:
        for feature in ('core', 'local'):
            if sources() != frozen:
                raise RuntimeError('Source changed before build; refreeze required')
            command = [str(cargo), 'build', '--release', '--locked', '--offline',
                       '--no-default-features', '-p', 'rust-engineering-mcp', '--bin',
                       'rust-engineering-mcp', '--jobs', '2']
            if feature == 'local':
                command += ['--features', 'local']
            started = time.monotonic()
            row = {'feature': feature, 'command': command, 'status': 'building'}
            receipt['builds'].append(row)
            save()
            print('BUILD ' + feature + ' release; bounded log: ' + str(OUT / (feature + '-build.log')), flush=True)
            total = 0
            with (OUT / (feature + '-build.log')).open('wb') as log:
                child = subprocess.Popen(command, cwd=ROOT, env=env, stdin=subprocess.DEVNULL,
                                         stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
                for chunk in iter(lambda: child.stdout.read1(65536), b''):
                    if total < LIMIT:
                        log.write(chunk[:LIMIT - total])
                        log.flush()
                    total += len(chunk)
                code = child.wait()
            row.update(exit_code=code, seconds=round(time.monotonic() - started, 3),
                       log_total_bytes=total, log_truncated=total > LIMIT)
            if code or sources() != frozen:
                row['status'] = 'failed'
                raise RuntimeError(feature + ' build failed or source changed')
            destination = OUT / feature / 'bin/rust-engineering-mcp'
            destination.parent.mkdir(parents=True, mode=0o700, exist_ok=True)
            shutil.copyfile(TARGET / 'release/rust-engineering-mcp', destination)
            os.chmod(destination, 0o700)
            row.update(status='passed', binary=str(destination.relative_to(OUT)),
                       bytes=destination.stat().st_size, sha256=sha(destination))
            save()
            print('PASS ' + feature + ' release: ' + str(row['bytes']) + ' bytes', flush=True)
        receipt['status'] = 'passed'
    except BaseException as error:
        receipt.update(status='failed', error=str(error))
        raise
    finally:
        save()


if __name__ == '__main__':
    main()
