#!/usr/bin/env python3
"""Local CI entrypoint. No installs, downloads or remote integration.
core runs portable gates; full additionally requires calibrated macOS/Docker/model and M2 mutation fixtures.
"""
import argparse, datetime, hashlib, json, os, pathlib, platform, re, shutil, stat, subprocess, sys, time
ROOT=pathlib.Path(__file__).resolve().parents[1]

RUST_TEST_RESULT = re.compile(
    r"^test result: (?P<status>ok|FAILED)\. "
    r"(?P<passed>\d+) passed; (?P<failed>\d+) failed; "
    r"(?P<ignored>\d+) ignored; (?P<measured>\d+) measured; "
    r"(?P<filtered_out>\d+) filtered out"
)
PYTHON_UNITTEST_RESULT = re.compile(r"^Ran (?P<executed>\d+) tests? in ")


def utc_now():
    """Return a stable, explicit UTC wall-clock timestamp for evidence receipts."""
    return datetime.datetime.now(datetime.UTC).isoformat().replace("+00:00", "Z")


def parse_test_summary_line(line):
    """Parse summaries emitted by the test runners used by this gate."""
    stripped = line.strip()
    match = RUST_TEST_RESULT.match(stripped)
    if match:
        counts = {key: int(value) for key, value in match.groupdict().items()
                  if key != "status"}
        return {"runner": "rust-test-harness", "status": match.group("status"), **counts}
    match = PYTHON_UNITTEST_RESULT.match(stripped)
    if match:
        return {"runner": "python-unittest", "executed": int(match.group("executed"))}
    return None


def run_step(report, save, name, command, env, require_test_groups=False,
             output_stream=None):
    """Run one gate step, persist its evidence and reject missing test summaries."""
    if output_stream is None:
        output_stream=sys.stdout
    start=time.monotonic()
    print(f'GATE {name}',file=output_stream,flush=True)
    row={'name':name,'command':command,'started_at':utc_now(),'status':'running'}
    report['steps'].append(row);save()
    process=subprocess.Popen(command,env=env,stdout=subprocess.PIPE,
                             stderr=subprocess.STDOUT,text=True,errors='replace',bufsize=1)
    output_lines=0
    output_bytes=0
    test_groups=[]
    if process.stdout is None:
        raise RuntimeError(f'{name} output pipe unavailable')
    for line in process.stdout:
        print(line,end='',file=output_stream,flush=True)
        output_lines+=1
        output_bytes+=len(line.encode('utf-8'))
        parsed=parse_test_summary_line(line)
        if parsed:
            test_groups.append(parsed)
    process.stdout.close()
    returncode=process.wait()
    counts={'output_lines':output_lines,'output_bytes_utf8':output_bytes,
            'test_groups':test_groups,
            'rust_passed':sum(group.get('passed',0) for group in test_groups),
            'rust_failed':sum(group.get('failed',0) for group in test_groups),
            'python_unittest_executed':sum(
                group.get('executed',0) for group in test_groups)}
    evidence_error=None
    if require_test_groups and not test_groups:
        evidence_error='required test summaries were not observed'
    status='passed' if returncode==0 and evidence_error is None else 'failed'
    row.update(status=status,exit_code=returncode,finished_at=utc_now(),
               seconds=round(time.monotonic()-start,3),counts=counts)
    if evidence_error:
        row['evidence_error']=evidence_error
    save()
    if returncode:
        raise RuntimeError(f'{name} failed ({returncode})')
    if evidence_error:
        raise RuntimeError(f'{name} evidence failed: {evidence_error}')

def source_inventory(root, env):
    """Bind tracked and untracked code/config/fixtures without following links."""
    names = subprocess.check_output(
        ['git', 'ls-files', '--cached', '--others', '--exclude-standard', '-z'],
        cwd=root, env=env).decode().split('\0')
    roots = {'AGENTS.md', 'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml',
             'deny.toml', 'rustfmt.toml', 'clippy.toml', '.gitignore'}
    prefixes = ('crates/', 'scripts/', 'fixtures/', 'vendor/', '.cargo/', '.github/')
    rows = []
    for name in sorted(set(names)):
        if name not in roots and not name.startswith(prefixes):
            continue
        path = root / name
        try:
            info = path.lstat()
        except FileNotFoundError:
            rows.append({'path': name, 'kind': 'absent'})
            continue
        if stat.S_ISLNK(info.st_mode):
            data = os.readlink(path).encode()
            kind = 'symlink-target'
            digest = hashlib.sha256(data).hexdigest()
            size = len(data)
        elif stat.S_ISREG(info.st_mode):
            with path.open('rb') as stream:
                digest = hashlib.file_digest(stream, 'sha256').hexdigest()
            size, kind = info.st_size, 'file'
        else:
            raise RuntimeError(f'unsupported gate source entry: {name}')
        rows.append({'path': name, 'kind': kind, 'mode': oct(stat.S_IMODE(info.st_mode)),
                     'bytes': size, 'sha256': digest})
    return rows

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=['core','full'])
    parser.add_argument('--report', type=pathlib.Path, default=ROOT/'target/gate-report.json')
    args=parser.parse_args()
    os.chdir(ROOT)
    env={k:v for k,v in os.environ.items() if k in ['HOME','PATH','TMPDIR','CARGO_HOME','RUSTUP_HOME','SDKROOT','DEVELOPER_DIR','CARGO_TARGET_DIR','RUST_MCP_TEST_SOCKET','RUST_MCP_E5_DIR','ORT_LIB_LOCATION']}
    env.update(CARGO_INCREMENTAL='0',ORT_SKIP_DOWNLOAD='1',CARGO_TERM_COLOR='never')
    report={'schema':'rust-mcp-gate-report-v2','mode':args.mode,
            'platform':platform.platform(),'machine':platform.machine(),
            'started_at':utc_now(),'steps':[],'status':'running'}
    def save():
        args.report.parent.mkdir(parents=True,exist_ok=True)
        args.report.write_text(json.dumps(report,indent=2)+'\n')
    def run(name,command,require_test_groups=False):
        run_step(report,save,name,command,env,require_test_groups)
    try:
        if os.name=='nt': raise RuntimeError('Windows fixture harness not calibrated; matrix gate unavailable')
        if args.mode=='full':
            if sys.platform!='darwin' or platform.machine()!='arm64': raise RuntimeError('Full native gate calibrated only on macOS ARM64')
            for key in ['RUST_MCP_TEST_SOCKET','RUST_MCP_E5_DIR','ORT_LIB_LOCATION']:
                if not env.get(key): raise RuntimeError(f'{key} required for full gate; no substitution')
        for name in ['rustup','cargo-audit','cargo-deny']:
            if not shutil.which(name,path=env.get('PATH')): raise RuntimeError(f'{name} missing; provision explicitly; no install attempted')
        # which queries an installed toolchain; it never installs the requested one.
        cargo=subprocess.check_output(['rustup','which','--toolchain','1.98.1','cargo'],env=env,text=True).strip()
        rustc=str(pathlib.Path(cargo).with_name('rustc'))
        env['PATH']=str(pathlib.Path(cargo).parent)+os.pathsep+env.get('PATH','')
        env['RUSTC']=rustc
        for binary, prefix in [(cargo,'cargo 1.98.1 '),(rustc,'rustc 1.98.1 ')]:
            actual=subprocess.check_output([binary,'--version'],env=env,text=True).strip()
            report[pathlib.Path(binary).name]=actual
            if not actual.startswith(prefix): raise RuntimeError('Rust/Cargo1.98.1 required; no automatic toolchain replacement')
        report['source_inputs'] = source_inventory(ROOT, env)
        save()
        run('fmt',['cargo','fmt','--all','--check'])
        for step in ['check','clippy','test']:
            command=['cargo',step,'--workspace','--all-targets','--locked','--offline']
            if step=='clippy': command+=['--','-D','warnings']
            run(step,command,require_test_groups=step=='test')
        run('doctests',['cargo','test','--workspace','--doc','--locked','--offline'],
            require_test_groups=True)
        run('architecture',[sys.executable,'scripts/check-architecture.py'])
        run('gate-reporting',[sys.executable,'scripts/test-gate-reporting.py'],
            require_test_groups=True)
        run('release-artifact-tests',[sys.executable,'-B','scripts/test-release-artifact.py'],
            require_test_groups=True)
        run('release-smoke-tests',[sys.executable,'-B','scripts/test-release-smoke.py'],
            require_test_groups=True)
        run('codex-qualifier-tests',[sys.executable,'-B','scripts/test-codex-model-qualifier.py'],
            require_test_groups=True)
        run('vendor',[sys.executable,'scripts/verify-vendor.py'])
        run('cargo-fixtures',[sys.executable,'scripts/test-fixtures.py',str(ROOT),'--cargo',cargo])
        run('audit',['cargo','audit','--no-fetch'])
        run('deny',['cargo','deny','--all-features','--locked','--offline','check','--disable-fetch','--hide-inclusion-graph','advisories','bans','sources'])
        if args.mode=='full':
            run('docker-security',['sh','scripts/test-execution.sh'])
            run('rust-security',[sys.executable,'scripts/test-rust-execution.py'])
            run('m2-runtime',[sys.executable,'scripts/test-m2-runtime.py'])
            run('m3-runtime',[sys.executable,'-B','scripts/test-m3-runtime.py'])
            run('audit-data',[sys.executable,'scripts/test-audit-data.py'])
            run('semantic',[sys.executable,'scripts/test-semantic.py'])
            run('catalog',[sys.executable,'scripts/test-catalog.py'])
            run('catalog-status',[sys.executable,'scripts/test-catalog-status.py'])
            run('crate-search',[sys.executable,'scripts/test-crate-search.py'])
            run('crate-inspect',[sys.executable,'scripts/test-crate-inspect.py'])
            run('doctor',[sys.executable,'scripts/test-doctor.py'])
        report['source_inputs_unchanged'] = source_inventory(ROOT, env) == report['source_inputs']
        if not report['source_inputs_unchanged']:
            raise RuntimeError('code/config/fixture inputs changed during gate; qualification rejected')
        report['status']='passed';report['finished_at']=utc_now()
    except Exception as error:
        report['status']='failed';report['error']=str(error);report['finished_at']=utc_now();save();raise
    save()
    print(f"PASS {args.mode} gate: {args.report}",flush=True)

if __name__=='__main__':
    if not __debug__: raise RuntimeError('Optimized Python mode is rejected')
    main()
