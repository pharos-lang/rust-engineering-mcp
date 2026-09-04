#!/usr/bin/env python3
"""Small first-party invariants, not a proof of absence of hidden dependency IO."""
import json, pathlib, re, subprocess, tomllib
if not __debug__: raise RuntimeError("Optimized Python mode is rejected")
root=pathlib.Path(__file__).resolve().parents[1]
meta=json.loads(subprocess.check_output(['cargo','metadata','--no-deps','--format-version','1','--locked','--offline'],cwd=root))
allowed={'rust-engineering-domain':{'serde'},'rust-engineering-application':{'rust-engineering-domain'}}
for package in meta['packages']:
    if package['name'] in allowed:
        normal={d['name'] for d in package['dependencies'] if d['kind'] is None}
        assert normal<=allowed[package['name']], (package['name'],normal)
for path in (root/'crates').glob('*/src/**/*.rs'):
    source=path.read_text()
    if 'execution-adapter' not in path.parts:
        assert not re.search(r'(?:Command::new|std::process::Command|tokio::process::Command)',source), path
    if any(part in path.parts for part in ['domain','application']):
        assert not re.search(r'(?:std::fs|std::process|rmcp::|rusqlite::|lancedb::|serde_json::Value)',source), path
manifest=tomllib.loads((root/'Cargo.toml').read_text())
assert manifest['workspace']['package']['rust-version']=='1.98.1'
assert tomllib.loads((root/'rust-toolchain.toml').read_text())['toolchain']['channel']=='1.98.1'
for dependency in ['lancedb','fastembed','ort','rustsec']:
    assert manifest['workspace']['dependencies'][dependency]['default-features'] is False
for path in list((root/'crates/semantic-adapter/src').glob('*.rs'))+list((root/'crates/artifact-adapter/src').glob('*.rs')):
    assert not re.search(r'(?:std::fs|File::open|read_to_end|shared-memory://)',path.read_text()), path

assert manifest['workspace']['dependencies']['rustsec']['version']=='=0.32.0'
assert manifest['workspace']['dependencies']['cargo-lock']['version']=='=11.0.1'
assert not manifest['workspace']['dependencies']['rustsec'].get('features')
for path in [root/'crates/catalog-adapter/src/audit.rs', *(root/'crates/catalog-adapter/src/audit').glob('*.rs')]:
    assert not re.search(r'(?:Database::open|Database::fetch|Lockfile::load|Advisory::load_file|std::fs|File::open)',path.read_text()), path
print('PASS domain/application dependency and IO boundaries, sole process gateway, offline engine defaults, memory-only model adapter')
