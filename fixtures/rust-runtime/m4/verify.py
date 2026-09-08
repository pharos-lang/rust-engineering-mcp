#!/usr/bin/env python3
"""Read-only native verifier for the provisioned M4 image; runs no project source."""
import argparse, datetime as dt, hashlib, json, pathlib, subprocess, uuid

def digest(value): return "sha256:"+hashlib.sha256(value).hexdigest()
def main():
    p=argparse.ArgumentParser(); p.add_argument("--docker",required=True); p.add_argument("--host",required=True); p.add_argument("--output",required=True,type=pathlib.Path); a=p.parse_args()
    if not pathlib.Path(a.docker).is_absolute() or not a.host.startswith("unix:///"): p.error("absolute Docker and local Unix socket required")
    a.output.mkdir(parents=True,exist_ok=True); docker=[a.docker,"--host",a.host]; image=(a.output/"image-id").read_text().strip()
    inspection=json.loads(subprocess.check_output(docker+["image","inspect",image],text=True))[0]
    base=json.loads(subprocess.check_output(docker+["image","inspect","rust-engineering-runtime:1.98.1-arm64-m3"],text=True))[0]
    config_digest=digest(json.dumps(inspection["Config"],sort_keys=True,separators=(",",":")).encode())
    expected_config={"User":"65534:65534","Env":["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"],"WorkingDir":"/work"}
    if inspection["Config"]!=expected_config or inspection["Os"]!="linux" or inspection["Architecture"]!="arm64": raise SystemExit("final image config/platform mismatch")
    if base["Id"]!="sha256:384a1742ecc53cdd3a9c0bf36c6f8b66db73ddd118aeeae6e55654ea998ae36a": raise SystemExit("M3 base changed")
    (a.output/"image-inspect.json").write_text(json.dumps(inspection,indent=2)+"\n")
    common=docker+["run","--rm","--pull","never","--platform","linux/arm64","--network","none","--read-only","--cap-drop","ALL","--security-opt","no-new-privileges","--user","65534:65534","--pids-limit","64","--memory","512m","--memory-swap","512m","--cpus","1","--tmpfs","/tmp:size=64m,noexec,nosuid,nodev","--workdir","/work","--entrypoint","/usr/bin/env",image]
    env=["-i","PATH=/opt/rust-nightly-2026-09-07/bin:/opt/security/bin:/usr/bin:/bin","CARGO=/opt/rust-nightly-2026-09-07/bin/cargo","RUSTC=/opt/rust-nightly-2026-09-07/bin/rustc","CARGO_HOME=/tmp/cargo","CARGO_NET_OFFLINE=true","MIRI_LIB_SRC=/opt/rust-nightly-2026-09-07/lib/rustlib/src/rust/library","MIRI_SYSROOT=/opt/miri-sysroot/2026-09-07/aarch64-unknown-linux-gnu"]
    results=[]
    def run(label,command,required=True):
        name="m4-verify-"+uuid.uuid4().hex; cmd=common.copy(); idx=cmd.index(image); cmd[idx:idx]=["--name",name]
        try: r=subprocess.run(cmd+env+command,capture_output=True,text=True,timeout=120)
        except subprocess.TimeoutExpired as e: r=subprocess.CompletedProcess(cmd,124,e.stdout or "",e.stderr or "timeout")
        finally:
            query=docker+["ps","-aq","--filter",f"name=^/{name}$"]
            if subprocess.check_output(query).strip(): subprocess.run(docker+["rm","-f",name],check=True,capture_output=True)
        results.append({"label":label,"command":command,"required":required,"exit_code":r.returncode,"stdout":r.stdout,"stderr":r.stderr}); return r
    run("cargo-deny-version",["/opt/security/bin/cargo-deny","--version"])
    run("nightly-rustc-version",["/opt/rust-nightly-2026-09-07/bin/rustc","--version","--verbose"])
    run("nightly-cargo-version",["/opt/rust-nightly-2026-09-07/bin/cargo","--version","--verbose"])
    run("miri-version",["/opt/rust-nightly-2026-09-07/bin/miri","--version"])
    run("cargo-miri-help",["/opt/rust-nightly-2026-09-07/bin/cargo","miri","--help"])
    run("cargo-deny-elf",["/usr/bin/readelf","-h","/opt/security/bin/cargo-deny"])
    run("cargo-deny-interpreter",["/usr/bin/readelf","-l","/opt/security/bin/cargo-deny"])
    run("cargo-deny-ldd",["/usr/bin/ldd","/opt/security/bin/cargo-deny"])
    run("executable-hashes",["/usr/bin/sha256sum","/opt/security/bin/cargo-deny","/opt/rust-nightly-2026-09-07/bin/rustc","/opt/rust-nightly-2026-09-07/bin/cargo","/opt/rust-nightly-2026-09-07/bin/miri","/opt/rust-nightly-2026-09-07/bin/cargo-miri"])
    run("executable-sizes",["/usr/bin/stat","-c","%s %n","/opt/security/bin/cargo-deny","/opt/rust-nightly-2026-09-07/bin/rustc","/opt/rust-nightly-2026-09-07/bin/cargo","/opt/rust-nightly-2026-09-07/bin/miri","/opt/rust-nightly-2026-09-07/bin/cargo-miri"])
    run("sysroot-tree",["/bin/sh","-c","find /opt/miri-sysroot/2026-09-07/aarch64-unknown-linux-gnu -type f -print0 | sort -z | xargs -0 sha256sum | sha256sum; du -sb /opt/miri-sysroot/2026-09-07/aarch64-unknown-linux-gnu"])
    run("notices",["/bin/sh","-c","find /usr/share/doc/rust-runtime/m4 -type f -print -exec sha256sum {} \\;"])
    forbidden="/usr/bin/apt /usr/bin/apt-get /usr/bin/dpkg /usr/bin/dpkg-query /usr/bin/curl /usr/bin/wget /usr/bin/git /usr/bin/ssh /usr/bin/nc /usr/bin/netcat /usr/bin/socat /opt/m4-input /opt/m4-build /opt/rust-nightly-2026-09-07/lib/rustlib/uninstall.sh /opt/rust-nightly-2026-09-07/lib/rustlib/install.log /root/.cargo /opt/cargo"
    run("forbidden-absent",["/bin/sh","-c",f"for p in {forbidden}; do test ! -e \"$p\" || exit 1; done"])
    deny_probe=run("cargo-deny-json-debug",["/bin/sh","-c","mkdir /tmp/deny-probe; printf '%s\\n' '[package]' 'name = \"m4-probe\"' 'version = \"0.0.0\"' 'edition = \"2024\"' > /tmp/deny-probe/Cargo.toml; mkdir /tmp/deny-probe/src; printf '%s\\n' 'fn main() {}' > /tmp/deny-probe/src/main.rs; /opt/rust/bin/cargo generate-lockfile --manifest-path /tmp/deny-probe/Cargo.toml --offline; /opt/security/bin/cargo-deny --format json --log-level debug --offline --locked --manifest-path /tmp/deny-probe/Cargo.toml check licenses bans sources"],required=False)
    probe=run("readonly-cargo-miri-print-sysroot",["/opt/rust-nightly-2026-09-07/bin/cargo","miri","setup","--print-sysroot","--target","aarch64-unknown-linux-gnu"],required=False)
    hard=[x for x in results if x["required"] and x["exit_code"]!=0]
    observed={x["label"]:x for x in results}
    try: deny_events=[json.loads(line) for line in deny_probe.stderr.splitlines() if line]
    except json.JSONDecodeError: deny_events=[]
    deny_json_ok=(deny_probe.returncode==4 and deny_probe.stdout=="" and bool(deny_events) and any(x.get("type")=="log" and x.get("fields",{}).get("level")=="DEBUG" for x in deny_events) and any(x.get("type")=="summary" for x in deny_events))
    semantic_ok=(observed["cargo-deny-version"]["stdout"].startswith("cargo-deny 0.19.7") and "commit-hash: 5a2be9f5f075d31e3ca5526b5b029881ce441253" in observed["nightly-rustc-version"]["stdout"] and "commit-hash: 3c0b534756e166d12eb9fd2e1abfe5b42ac6101e" in observed["nightly-cargo-version"]["stdout"] and "Machine:                           AArch64" in observed["cargo-deny-elf"]["stdout"] and "/lib/ld-linux-aarch64.so.1" in observed["cargo-deny-interpreter"]["stdout"] and "not found" not in observed["cargo-deny-ldd"]["stdout"]+observed["cargo-deny-ldd"]["stderr"] and deny_json_ok)
    status="passed" if not hard and semantic_ok and probe.returncode==0 and probe.stdout.strip()=="/opt/miri-sysroot/2026-09-07/aarch64-unknown-linux-gnu" else "blocked_upstream_readonly_probe" if not hard and semantic_ok and probe.returncode!=0 else "failed"
    receipt={"schema":"rust-engineering-mcp.m4-native-provisioning-verification.v1","status":status,"gateway_approved":False,"observed_at":dt.datetime.now(dt.timezone.utc).isoformat(),"image_id":inspection["Id"],"image_size":inspection["Size"],"config_digest":config_digest,"base_image_id":base["Id"],"m3_image_untouched":True,"run_security":{"network":"none","read_only":True,"cap_drop":"ALL","no_new_privileges":True,"user":"65534:65534","project_source_mounted":False},"cargo_deny_json_debug_oracle":{"passed":deny_json_ok,"expected_exit_code":4,"stdout_empty":deny_probe.stdout=="","stderr_json_event_count":len(deny_events)},"results":results,"readonly_probe_observation":"cargo-miri setup always attempts an atomic sysroot rebuild even when MIRI_SYSROOT names the prepared sysroot; it fails closed on the read-only filesystem before printing the path" if probe.returncode else "passed","claims":"provisioning evidence only; no gateway or hostile-project qualification"}
    (a.output/"verification.json").write_text(json.dumps(receipt,indent=2)+"\n"); print(status)
    if status=="failed": raise SystemExit("required provisioning verification failed")
if __name__=="__main__": main()
