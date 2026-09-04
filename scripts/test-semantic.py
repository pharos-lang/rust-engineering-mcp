#!/usr/bin/env python3
"""Explicit development gate: verified local assets, no provisioning or downloads.
RUST_MCP_E5_DIR points to the five files; ORT_LIB_LOCATION to native static libs.
"""
import hashlib, json, os, pathlib, socket, subprocess, sys, tempfile
ROOT = pathlib.Path(__file__).resolve().parents[1]
os.chdir(ROOT)
if sys.platform != 'darwin':
    sys.exit('This calibrated network profile currently requires macOS; no semantic gate claimed on this OS.')
model = pathlib.Path(os.environ['RUST_MCP_E5_DIR']).resolve(strict=True)
native = pathlib.Path(os.environ['ORT_LIB_LOCATION']).resolve(strict=True)
if {str(p.relative_to(native)) for p in native.rglob('*')} != {'libonnxruntime.a'}:
    sys.exit('Native directory contents differ from calibrated distribution')
subprocess.run([sys.executable,'scripts/verify-vendor.py'],check=True)
expected = '4d53c916ea95f09203324f9aad7b76f75c16d8a4bc98f8a949ea0ac73c07604d'
with (native / 'libonnxruntime.a').open('rb') as f:
    actual = hashlib.file_digest(f, 'sha256').hexdigest()
if actual != expected:
    sys.exit('Native ORT identity differs from calibrated artifact; review before changing the gate.')
for family, address in [(socket.AF_INET, ('127.0.0.1',0)), (socket.AF_INET6, ('::1',0))]:
    with socket.socket(family, socket.SOCK_STREAM) as s:
        s.bind(address)
print('PASS network positive controls IPv4/IPv6 bind; ORT SHA256', actual, flush=True)
env = {k:v for k,v in os.environ.items() if k in ['HOME','PATH','SDKROOT','DEVELOPER_DIR','CARGO_HOME','RUSTUP_HOME','CARGO_TARGET_DIR']}
env.update(CARGO_INCREMENTAL='0', ORT_LIB_LOCATION=str(native), ORT_SKIP_DOWNLOAD='1')
env.setdefault('CARGO_TARGET_DIR',str(ROOT/'target/semantic-compat'))
base = ['cargo']
for args in [
    ['check','--workspace','--all-features','--all-targets'],
    ['clippy','--workspace','--all-features','--all-targets','--','-D','warnings'],
]:
    at = args.index('--') if '--' in args else len(args)
    args[at:at]=['--locked','--offline']
    subprocess.run(base+args,env=env,check=True)
result=subprocess.run(base+['test','-p','rust-engineering-semantic','--features','local','--all-targets','--locked','--offline','--no-run','--message-format=json'],env=env,check=True,stdout=subprocess.PIPE,text=True)
binaries=[]
for line in result.stdout.splitlines():
    row=json.loads(line)
    if row.get('reason')=='compiler-artifact' and row.get('executable') and row.get('profile',{}).get('test'):
        binaries.append((row['target']['name'],row['executable']))
if not any(name=='local' for name,_ in binaries):
    sys.exit('Real semantic integration test binary missing')
with tempfile.TemporaryDirectory(prefix='rust-mcp-semantic-gate-') as scratch:
    nonexistent=pathlib.Path(scratch)/'no-temp-directory'
    profile=pathlib.Path(scratch)/'deny-network.sb'
    profile.write_text('(version 1) (allow default) (deny network*)\n')
    runtime={'PATH':'/usr/bin:/bin','TMPDIR':str(nonexistent),'RUST_MCP_E5_DIR':str(model),'RUST_MCP_NETWORK_DENIED':'1'}
    for name,binary in binaries:
        command=['/usr/bin/sandbox-exec','-f',str(profile),binary,'--test-threads=1','--nocapture']
        subprocess.run(command,env=runtime,check=True)
        subprocess.run(command+['--ignored'],env=runtime,check=True)
    if nonexistent.exists():
        sys.exit('Unexpected runtime temporary directory creation')
print('PASS semantic gate: real model, index, SQLite, calibrated network deny, configured TMPDIR remains absent')
