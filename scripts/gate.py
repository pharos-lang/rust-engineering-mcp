#!/usr/bin/env python3
"""Local CI entrypoint. No installs, downloads or remote integration.
core runs portable gates; full additionally requires calibrated macOS/Docker/model.
"""
import argparse, json, os, pathlib, platform, shutil, subprocess, sys, time
ROOT=pathlib.Path(__file__).resolve().parents[1]

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=['core','full'])
    parser.add_argument('--report', type=pathlib.Path, default=ROOT/'target/gate-report.json')
    args=parser.parse_args()
    os.chdir(ROOT)
    env={k:v for k,v in os.environ.items() if k in ['HOME','PATH','TMPDIR','CARGO_HOME','RUSTUP_HOME','SDKROOT','DEVELOPER_DIR','CARGO_TARGET_DIR','RUST_MCP_TEST_SOCKET','RUST_MCP_E5_DIR','ORT_LIB_LOCATION']}
    env.update(CARGO_INCREMENTAL='0',ORT_SKIP_DOWNLOAD='1',CARGO_TERM_COLOR='never')
    report={'mode':args.mode,'platform':platform.platform(),'machine':platform.machine(),'steps':[],'status':'running'}
    def save():
        args.report.parent.mkdir(parents=True,exist_ok=True)
        args.report.write_text(json.dumps(report,indent=2)+'\n')
    def run(name,command):
        start=time.monotonic()
        print(f'GATE {name}',flush=True)
        row={'name':name,'command':command,'status':'running'};report['steps'].append(row);save()
        result=subprocess.run(command,env=env)
        row.update(status='passed' if result.returncode==0 else 'failed',exit_code=result.returncode,seconds=round(time.monotonic()-start,3));save()
        if result.returncode: raise RuntimeError(f'{name} failed ({result.returncode})')
    try:
        if os.name=='nt': raise RuntimeError('Windows fixture harness not calibrated; matrix gate unavailable')
        if args.mode=='full':
            if sys.platform!='darwin' or platform.machine()!='arm64': raise RuntimeError('Full M0 gate calibrated only on macOS ARM64')
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
        run('fmt',['cargo','fmt','--all','--check'])
        for step in ['check','clippy','test']:
            command=['cargo',step,'--workspace','--all-targets','--locked','--offline']
            if step=='clippy': command+=['--','-D','warnings']
            run(step,command)
        run('doctests',['cargo','test','--workspace','--doc','--locked','--offline'])
        run('architecture',[sys.executable,'scripts/check-architecture.py'])
        run('vendor',[sys.executable,'scripts/verify-vendor.py'])
        run('cargo-fixtures',[sys.executable,'scripts/test-fixtures.py',str(ROOT),'--cargo',cargo])
        run('audit',['cargo','audit','--no-fetch'])
        run('deny',['cargo','deny','--all-features','--locked','--offline','check','--disable-fetch','--hide-inclusion-graph','advisories','bans','sources'])
        if args.mode=='full':
            run('docker-security',['sh','scripts/test-execution.sh'])
            run('rust-security',[sys.executable,'scripts/test-rust-execution.py'])
            run('audit-data',[sys.executable,'scripts/test-audit-data.py'])
            run('semantic',[sys.executable,'scripts/test-semantic.py'])
            run('catalog',[sys.executable,'scripts/test-catalog.py'])
            run('catalog-status',[sys.executable,'scripts/test-catalog-status.py'])
            run('crate-search',[sys.executable,'scripts/test-crate-search.py'])
            run('crate-inspect',[sys.executable,'scripts/test-crate-inspect.py'])
            run('doctor',[sys.executable,'scripts/test-doctor.py'])
        report['status']='passed'
    except Exception as error:
        report['status']='failed';report['error']=str(error);save();raise
    save()
    print(f"PASS {args.mode} gate: {args.report}",flush=True)

if __name__=='__main__':
    if not __debug__: raise RuntimeError('Optimized Python mode is rejected')
    main()
