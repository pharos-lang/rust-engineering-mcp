#!/usr/bin/env python3
"""Verify the published LanceDB archive, reviewed patch and sourced license."""
import hashlib, io, os, pathlib, tarfile, difflib, tomllib
if not __debug__:
    raise RuntimeError("Verification requires Python assertions; optimized mode is rejected")
root=pathlib.Path(__file__).resolve().parents[1]
cache=pathlib.Path(os.environ.get('CARGO_HOME',str(pathlib.Path.home()/'.cargo')))/'registry/cache'
archives=list(cache.glob('*/lancedb-0.31.0.crate'))
assert archives, 'Published cached crate required; this check does not download'
data=archives[0].read_bytes()
assert hashlib.sha256(data).hexdigest()=='2bd0b54bb1cdd075efa5a8827ec16dcf5c0781253cd88e63988c174915c53fe2'
original={}
with tarfile.open(fileobj=io.BytesIO(data),mode='r:gz') as tar:
    for item in tar.getmembers():
        if item.isfile():
            original[str(pathlib.PurePosixPath(item.name).relative_to('lancedb-0.31.0'))]=tar.extractfile(item).read()
actual={str(p.relative_to(root/'vendor/lancedb')):p.read_bytes() for p in (root/'vendor/lancedb').rglob('*') if p.is_file()}
license_name='LICENSE'
assert actual.keys()==original.keys()|{license_name}, 'Vendor files added or removed'
# Exact upstream bytes for Cargo.lock's lancedb 0.31.0 VCS revision. The request,
# redirect, Git blob and SHA-256 receipt is retained in
# docs/release/upstream-licenses/receipt.json; this file was not in the .crate.
assert hashlib.sha256(actual[license_name]).hexdigest()=='58d1e17ffe5109a7ae296caafcadfdbe6a7d176f0bc4ab01e12a689b0499d8bd', 'Reviewed upstream license differs'
changed={name for name in original if original[name]!=actual[name]}
assert changed=={'Cargo.toml','Cargo.toml.orig'}, changed
assert actual['Cargo.toml']==original['Cargo.toml'].replace(b'[dependencies.lance-testing]',b'[dev-dependencies.lance-testing]')
assert actual['Cargo.toml.orig']==original['Cargo.toml.orig'].replace(b'lance-testing = { workspace = true }\n',b'').replace(b'[dev-dependencies]\n',b'[dev-dependencies]\nlance-testing = { workspace = true }\n')
patch=''.join(''.join(difflib.unified_diff(original[n].decode().splitlines(True),actual[n].decode().splitlines(True),fromfile='published/'+n,tofile='vendor/lancedb/'+n)) for n in sorted(changed))
expected=(root/'vendor/lancedb-manifest-only.patch').read_text()
assert patch==expected, 'Manifest delta differs from reviewed patch'
packages=tomllib.loads((root/'Cargo.lock').read_text())['package']
names={p['name'] for p in packages}
assert not (names & {'lance-testing','pprof','inferno','hf-hub'}), names & {'lance-testing','pprof','inferno','hf-hub'}
assert [(p['version']) for p in packages if p['name']=='tinyvec']==['1.12.0']
assert not any(p['name']=='quick-xml' and p['version']=='0.26.0' for p in packages)
print(f'PASS published SHA256; {len(original)} files; only two exact manifest edits; Rust sources unchanged')
