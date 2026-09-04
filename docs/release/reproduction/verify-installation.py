#!/usr/bin/env python3
"""Verify/copy local-review candidates; no production trust or global install."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
OUT = Path(__file__).resolve().parent
MODEL = Path('/private/tmp/rust-mcp-e5-m009/onnx')
NETWORK_DENY = '(version 1) (allow default) (deny network*)'
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location('doctor_gate', ROOT / 'scripts/test-doctor.py')
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)


def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def record(path):
    return {'path': str(path.relative_to(OUT)), 'bytes': path.stat().st_size,
            'sha256': sha(path), 'mode': oct(stat.S_IMODE(path.stat().st_mode))}


def copy(source, destination, mode=0o600):
    if source.is_symlink() or not source.is_file():
        raise RuntimeError('Candidate input is not a regular file: ' + str(source))
    destination.parent.mkdir(parents=True, mode=0o700, exist_ok=True)
    shutil.copyfile(source, destination)
    os.chmod(destination, mode)
    if sha(source) != sha(destination):
        raise RuntimeError('Copy hash mismatch: ' + str(destination))


def invoke(binary, args, label, expected=0):
    capture = gate.Capture(['/usr/bin/sandbox-exec', '-p', NETWORK_DENY,
                            str(binary), *args], {}, OUT, 128 * 1024)
    try:
        code = capture.finish(time.monotonic() + 150)
        capture.bounded()
        if code != expected or capture.data['stderr']:
            raise RuntimeError(label + ' unexpected exit/stderr: ' + str(code))
        result = json.loads(capture.data['stdout'])
        (OUT / (label + '.json')).write_text(json.dumps(result, indent=2) + '\n')
        return result
    finally:
        capture.force_stop()
        capture.save(OUT, label)


def inspect_binary(binary, feature):
    linkage = gate.run(['/usr/bin/otool', '-L', str(binary)], {}, OUT, 30).decode()
    load_commands = gate.run(['/usr/bin/otool', '-l', str(binary)], {}, OUT, 30,
                             1024 * 1024).decode()
    architecture = gate.run(['/usr/bin/file', str(binary)], {}, OUT, 30).decode().strip()
    if 'Mach-O 64-bit executable arm64' not in architecture:
        raise RuntimeError('Unexpected candidate architecture: ' + architecture)
    dependencies = []
    for line in linkage.splitlines()[1:]:
        name = line.strip().partition(' (compatibility version')[0]
        if name:
            dependencies.append(name)
            if not name.startswith(('/usr/lib/', '/System/Library/Frameworks/')):
                raise RuntimeError('Non-system dynamic dependency: ' + name)
    (OUT / (feature + '-otool-L.txt')).write_text(linkage)
    (OUT / (feature + '-otool-l.txt')).write_text(load_commands)
    gate.run(['/usr/bin/codesign', '--verify', '--strict', str(binary)], {}, OUT, 30)
    signature = gate.Capture(['/usr/bin/codesign', '-dv', '--verbose=4', str(binary)],
                             {}, OUT, 64 * 1024)
    try:
        if signature.finish(time.monotonic() + 30):
            raise RuntimeError('Cannot inspect local Mach-O signature metadata')
        signature.bounded()
        signature_text = bytes(signature.data['stderr']).decode()
        signature.save(OUT, feature + '-codesign')
    finally:
        signature.force_stop()
    # nm output is drained incrementally; retain only the requested global symbols.
    wanted = {'_OrtGetApiBase', '_sqlite3_libversion', '_ZSTD_versionString'}
    found = []
    process = subprocess.Popen(['/usr/bin/nm', '-g', '-U', str(binary)], env={},
                               stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                               stderr=subprocess.DEVNULL)
    total = 0
    try:
        for line in process.stdout:
            total += len(line)
            if total > 64 * 1024 * 1024 or len(line) > 64 * 1024:
                raise RuntimeError('Symbol inspection exceeded bound')
            text = line.decode('utf-8')
            if text.split() and text.split()[-1] in wanted:
                found.append(text.strip())
        if process.wait(timeout=30):
            raise RuntimeError('Symbol inspection failed')
    finally:
        if process.poll() is None:
            process.kill()
            process.wait(timeout=5)
    has_ort = any(line.split()[-1] == '_OrtGetApiBase' for line in found)
    if has_ort != (feature == 'local'):
        raise RuntimeError('Native ORT symbol does not match compiled feature')
    return {'architecture': architecture, 'dynamic_dependencies': dependencies,
            'local_codesign_verification': 'passed',
            'codesign_identity': [line for line in signature_text.splitlines()
                                 if line.startswith(('Signature=', 'TeamIdentifier=', 'Authority='))],
            'codesign_scope': 'local integrity only; no publisher authorization or notarization claim',
            'selected_static_symbols': found, 'symbol_output_bytes_scanned': total,
            'native_attribution_scope': 'selected symbols plus recorded static build input; not complete object/license audit'}


def main():
    build = json.loads((OUT / 'build-receipt.json').read_text())
    if build['status'] != 'passed':
        raise RuntimeError('Both accepted source-matched release candidates are required')
    # Harmless loopback bind controls discriminate sandbox denial from missing
    # network facilities; no remote address is contacted.
    import socket
    for family, address in ((socket.AF_INET, ('127.0.0.1', 0)),
                            (socket.AF_INET6, ('::1', 0))):
        with socket.socket(family, socket.SOCK_STREAM) as control:
            control.bind(address)
    negative = 'import socket\nfor f,a in ((socket.AF_INET,("127.0.0.1",0)),(socket.AF_INET6,("::1",0))):\n with socket.socket(f,socket.SOCK_STREAM) as s:\n  try: s.bind(a)\n  except PermissionError: pass\n  else: raise SystemExit("network deny was not enforced")\n'
    gate.run(['/usr/bin/sandbox-exec', '-p', NETWORK_DENY, sys.executable, '-c', negative],
             {}, OUT, 30)
    assets = OUT / 'assets'
    assets.mkdir(mode=0o700, exist_ok=True)
    copy(ROOT / 'LICENSE', assets / 'LICENSE.product-pending.txt')
    for name in ('inventory.json', 'THIRD_PARTY_NOTICES.candidate.txt'):
        copy(ROOT / 'docs/release' / name, assets / 'notices' / name)
    for source in sorted((ROOT / 'docs/release/upstream-licenses').rglob('*')):
        if source.is_file():
            copy(source, assets / 'notices/upstream-licenses' /
                 source.relative_to(ROOT / 'docs/release/upstream-licenses'))
    copy(ROOT / 'fixtures/catalog/fixture-trust.json', assets / 'fixture-trust.json')
    copy(ROOT / 'fixtures/catalog/fixture-1.tar.zst', assets / 'fixture-1.tar.zst')
    copy(ROOT / 'fixtures/catalog/README.md', assets / 'FIXTURE-ONLY.md')
    model_receipt = json.loads((ROOT / 'fixtures/semantic/model-receipt.json').read_text())
    for item in model_receipt['files']:
        name = Path(item['path']).name
        source = MODEL / name
        if source.stat().st_size != item['bytes'] or sha(source) != item['sha256']:
            raise RuntimeError('Existing E5 identity mismatch: ' + name)
        copy(source, assets / 'model' / name)
    copy(ROOT / 'fixtures/semantic/model-receipt.json', assets / 'model-receipt.json')
    installation = OUT / 'installation'
    installation.mkdir(mode=0o700)  # Never reuse or replace an existing install.
    results = {'scope': 'local-review-only-not-distribution', 'status': 'running',
               'product_license': 'owner_decision_pending',
               'trust_identity': 'public-forgeable-fixture-only/test; no publisher authorization',
               'network_controls': 'IPv4/IPv6 loopback bind positive outside and denied inside profile',
               'runtime_network_profile': NETWORK_DENY,
               'source_manifest_sha256': build['source_manifest_sha256'],
               'verification_script_sha256': sha(Path(__file__)),
               'capture_helper_sha256': sha(ROOT / 'scripts/test-doctor.py'), 'features': []}
    try:
        for feature in ('core', 'local'):
            supplied = OUT / feature / 'bin/rust-engineering-mcp'
            row = next(item for item in build['builds'] if item['feature'] == feature)
            if sha(supplied) != row['sha256']:
                raise RuntimeError('Candidate binary changed since build')
            root = installation / feature
            root.mkdir(mode=0o700)
            binary = root / 'bin/rust-engineering-mcp'
            copy(supplied, binary, 0o700)
            evidence = {'feature': feature, 'binary': record(binary),
                        'linkage': inspect_binary(binary, feature)}
            version = invoke(binary, ['version', '--json'], feature + '-version')
            if (version['version'] != '0.1.0-dev.1' or version['format_version'] != 1
                    or version['compiled_local'] != (feature == 'local')
                    or version['target_os'] != 'macos' or version['target_arch'] != 'aarch64'):
                raise RuntimeError('Installed version/build identity mismatch')
            passive = invoke(binary, ['doctor', '--json'], feature + '-passive')
            if passive['mode'] != 'passive' or passive['status'] != 'warning' or passive['runtime'] is not None:
                raise RuntimeError('Unconfigured doctor overclaimed readiness')
            trust_dir = root / 'trust'
            trust_dir.mkdir(mode=0o700)
            trust = trust_dir / 'fixture-trust.json'
            copy(assets / 'fixture-trust.json', trust)
            store = root / 'catalog'
            store.mkdir(mode=0o700)
            imported = invoke(binary, ['catalog', 'import', str(assets / 'fixture-1.tar.zst'),
                                      '--store', str(store), '--trust', str(trust), '--json'],
                              feature + '-import')
            if (imported['status'] != 'passed' or imported['catalog']['publisher'] != 'fixture-only'
                    or imported['catalog']['channel'] != 'test' or imported['catalog']['sequence'] != 1
                    or imported['network_used']):
                raise RuntimeError('Fixture import identity or network observation mismatch')
            stable = {name: sha(store / name) for name in ('active.bundle', 'floor.record')}
            sentinel = store / 'staging.bundle'
            sentinel.write_bytes(b'owned-installation-orphan-sentinel')
            os.chmod(sentinel, 0o600)
            flags = ['--catalog-store', str(store), '--catalog-trust', str(trust)]
            observed = invoke(binary, ['doctor', '--json', *flags], feature + '-catalog-doctor')
            checks = {item['id']: item for item in observed['checks']}
            if (checks['catalog']['status'] != 'available'
                    or checks['catalog_freshness']['reason'] != 'freshness_needs_review'
                    or observed['catalog']['catalog']['value']['sequence'] != 1):
                raise RuntimeError('Installed fixture catalog/freshness not observed')
            if stable != {name: sha(store / name) for name in stable} or sentinel.read_bytes() != b'owned-installation-orphan-sentinel':
                raise RuntimeError('Passive doctor mutated catalog administration state')
            if feature == 'local':
                installed_model = root / 'model'
                for item in model_receipt['files']:
                    name = Path(item['path']).name
                    copy(assets / 'model' / name, installed_model / name)
                model_observed = invoke(binary, ['doctor', '--json', *flags, '--catalog-model-dir',
                                                str(installed_model)], 'local-model-doctor')
                model_checks = {item['id']: item for item in model_observed['checks']}
                if model_checks['model']['status'] != 'available':
                    raise RuntimeError('Installed local model failed observation')
                evidence['model'] = 'available_from_exact_five_installed_files'
            failed = invoke(binary, ['doctor', '--json', '--catalog-store', str(root / 'absent'),
                                     '--catalog-trust', str(trust)], feature + '-missing-asset', 1)
            if failed['status'] != 'failed' or (root / 'absent').exists():
                raise RuntimeError('Missing asset was repaired or not rejected')
            evidence['offline_installation_checks'] = ['version', 'passive-doctor', 'fixture-import',
                'catalog-doctor-stale-evidence', 'passive-state-preservation', 'missing-asset-denial']
            results['features'].append(evidence)
        results['status'] = 'passed'
    except BaseException as error:
        results.update(status='failed', error=str(error))
        raise
    finally:
        (OUT / 'installation-receipt.json').write_text(json.dumps(results, indent=2) + '\n')
        rows = [record(p) for base in ('core', 'local', 'assets')
                for p in sorted((OUT / base).rglob('*')) if p.is_file()]
        (OUT / 'manifest.json').write_text(json.dumps({'scope': 'local-review-only-not-distribution',
            'product_license': 'pending', 'files': rows,
            'total_bytes': sum(row['bytes'] for row in rows)}, indent=2) + '\n')
        installed = [record(p) for p in sorted(installation.rglob('*')) if p.is_file()]
        (OUT / 'installation-manifest.json').write_text(json.dumps({'scope': 'owned-private-installation',
            'root': str(installation), 'files': installed,
            'total_bytes': sum(row['bytes'] for row in installed)}, indent=2) + '\n')
    print('PASS private core/local installation, linkage, fixture trust and passive doctor', flush=True)


if __name__ == '__main__':
    main()
