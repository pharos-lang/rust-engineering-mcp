#!/usr/bin/env python3
"""Actual signed-import/restart/native-index CLI gate; no provisioning."""
import json, os, pathlib, socket, subprocess, sys, tempfile
if not __debug__:
    raise RuntimeError('Optimized Python is unsupported')
ROOT=pathlib.Path(__file__).resolve().parents[1]
os.chdir(ROOT)
if sys.platform!='darwin':
    sys.exit('Catalog CLI confinement gate requires macOS26+/APFS; no other platform claim')
# Asset identities are independently checked by the full semantic gate. The actual
# model loader also verifies exact E5 hashes before native parsing.
model=pathlib.Path(os.environ['RUST_MCP_E5_DIR']).resolve(strict=True)
native=pathlib.Path(os.environ['ORT_LIB_LOCATION']).resolve(strict=True)
env={k:v for k,v in os.environ.items() if k in ['HOME','PATH','SDKROOT','DEVELOPER_DIR','CARGO_HOME','RUSTUP_HOME']}
env.update(CARGO_INCREMENTAL='0',ORT_LIB_LOCATION=str(native),ORT_SKIP_DOWNLOAD='1',CARGO_TARGET_DIR=str(ROOT/'target/semantic-compat'))
result=subprocess.run(['cargo','test','-p','rust-engineering-mcp','--features','local','--test','catalog_cli','--locked','--offline','--no-run','--message-format=json'],env=env,check=True,stdout=subprocess.PIPE,text=True)
binaries=[]
for line in result.stdout.splitlines():
    row=json.loads(line)
    if row.get('reason')=='compiler-artifact' and row.get('executable') and row.get('profile',{}).get('test'):
        binaries.append(row['executable'])
if len(binaries)!=1: sys.exit('Exact catalog CLI test binary missing')
for family,address in [(socket.AF_INET,('127.0.0.1',0)),(socket.AF_INET6,('::1',0))]:
    with socket.socket(family,socket.SOCK_STREAM) as s: s.bind(address)
with tempfile.TemporaryDirectory(prefix='rust-mcp-catalog-gate-') as scratch:
    profile=pathlib.Path(scratch)/'deny-network.sb'
    profile.write_text('(version 1) (allow default) (deny network*)\n')
    runtime={'PATH':'/usr/bin:/bin','RUST_MCP_E5_DIR':str(model),'RUST_MCP_NETWORK_DENIED':'1'}
    base=['/usr/bin/sandbox-exec','-f',str(profile),binaries[0],'--test-threads=1','--nocapture']
    for args,expected in [([],5),(['--ignored'],1)]:
        result=subprocess.run(base+args,env=runtime,check=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
        print(result.stdout,flush=True)
        if f'test result: ok. {expected} passed;' not in result.stdout: sys.exit('Expected catalog tests did not execute')
print('PASS signed CLI import/status/restart/rollback and real E5 native index rebuild/reopen under network deny')
