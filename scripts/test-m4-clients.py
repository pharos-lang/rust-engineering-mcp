#!/usr/bin/env python3
"""Prepared, source-bound qualification harness for the five M4 tools.

The default preflight is client- and Docker-free.  ``--run`` is intentionally
closed until the five production advertisement switches and the externally
recorded stock-Codex synchronous latency gate are both enabled.
"""
from __future__ import annotations

import argparse
import importlib.util
import json
import os
import pathlib
import queue
import shutil
import sqlite3
import subprocess
import sys
import tarfile
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
M3_PATH = ROOT / "scripts/test-m3-clients.py"
SESSION = ROOT / "scripts/m4-inspector-session.mjs"
UNIT = ROOT / "scripts/test-m4-clients-unit.py"
ATTEMPTS = ROOT / "docs/validation/m4-clients"
CURRENT = ROOT / "docs/validation/M4-clients.json"
PREFLIGHT = ROOT / "docs/validation/M4-clients-pre-advertisement.json"
SERVER = ROOT / "target/release/rust-engineering-mcp"
NODE = pathlib.Path("/Users/cburgosro/.nvm/versions/node/v24.15.0/bin/node")
INSPECTOR = ROOT / "target/m1-17-inspector/node_modules/@modelcontextprotocol/inspector/clients/cli/build/index.js"
INSPECTOR_PACKAGE = ROOT / "target/m1-17-inspector/node_modules/@modelcontextprotocol/inspector/package.json"
DOCKER = pathlib.Path("/Applications/Docker.app/Contents/Resources/bin/docker")
ZSTD = pathlib.Path("/opt/homebrew/bin/zstd")
IMAGE = "sha256:25ed3626e710081a571a86a29521eaf2e890e796afd422ba5e409e0ce1891635"
M4_TOOLS = (
    "rust.deny", "rust.unsafe.scan", "rust.supply_chain.inspect",
    "rust.quality.gate.v2", "rust.miri",
)
M3_TOOLS = (
    "rust.project.open", "rust.project.inspect", "rust.toolchain.inspect",
    "rust.check", "rust.fmt.check", "rust.clippy", "rust.test",
    "rust.test.nextest", "rust.dependencies.audit", "rust.diagnostics.explain",
    "rust.quality.gate", "rust.catalog.status", "rust.crate.search",
    "rust.crate.inspect", "rust.manifest.patch", "rust.fmt.apply",
    "rust.fix.apply", "rust.dependency.add", "rust.dependency.remove",
    "rust.coverage", "rust.semver.check", "rust.mutation.test",
)
EXPECTED_TOOLS = M3_TOOLS + M4_TOOLS
READY_MARKERS = {
    "rust.deny": ("deny.rs", "RUST_MCP_TEST_SECURITY_READY"),
    "rust.unsafe.scan": ("unsafe_scan.rs", "RUST_MCP_TEST_SCANNER_READY"),
    "rust.supply_chain.inspect": ("supply_chain.rs", "RUST_MCP_TEST_SUPPLY_READY"),
    "rust.quality.gate.v2": ("quality_v2.rs", "RUST_MCP_TEST_GATE_V2_READY"),
    "rust.miri": ("miri.rs", "RUST_MCP_TEST_MIRI_READY"),
}
SAFE_PROTOCOL_KEYS = frozenset({
    "client", "direction", "session", "bytes", "sha256", "malformed",
    "method", "tasks_declared", "tasks_advertised", "tool", "resource_scheme",
})


def load_m3():
    """Reuse M3's bounded subprocess, proxy, digest and receipt primitives."""
    spec = importlib.util.spec_from_file_location("rust_mcp_m3_clients", M3_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError("M3 client harness is unavailable")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def source_hashes() -> dict[str, str]:
    m3 = load_m3()
    paths = (M3_PATH, ROOT / "scripts/m3-inspector-session.mjs",
             ROOT / "scripts/codex-model-qualifier.py", pathlib.Path(__file__).resolve(),
             SESSION, UNIT, ROOT / "docs/validation/M1/17-codex-client/controller.py")
    return {str(path.relative_to(ROOT)): m3.file_digest(path) for path in paths if path.is_file()}


def advertisement_state() -> dict[str, bool]:
    base = ROOT / "crates/mcp-server/src/stdio"
    shared = (base / "security_tool.rs").read_text()
    shared_ready = (
        "pub(super) fn advertised(_test_variable: &str) -> bool" in shared
        and "    true\n}" in shared
    )
    state = {}
    for tool, (name, marker) in READY_MARKERS.items():
        source = (base / name).read_text()
        if "const ADVERTISEMENT_READY: bool = true;" in source:
            state[tool] = True
        elif "const ADVERTISEMENT_READY: bool = false;" in source:
            state[tool] = False
        elif f'super::security_tool::advertised("{marker}")' in source and shared_ready:
            state[tool] = True
        else:
            raise RuntimeError(f"advertisement switch missing for {tool}")
    return state


def preflight() -> dict[str, object]:
    package_version = None
    if INSPECTOR_PACKAGE.is_file():
        package_version = json.loads(INSPECTOR_PACKAGE.read_text()).get("version")
    codex_name = shutil.which("codex")
    codex_version = None
    if codex_name:
        result = subprocess.run([codex_name, "--version"], capture_output=True,
                                text=True, timeout=10, check=False)
        codex_version = result.stdout.strip()
    switches = advertisement_state()
    ready = all(switches.values())
    receipt = {
        "schema": "rust-mcp-m4-clients-preflight-v1",
        "status": "prepared" if not ready else "ready",
        "execution_performed": False,
        "image_id": IMAGE,
        "expected_tools": list(EXPECTED_TOOLS),
        "advertisement": switches,
        "run_requires_all_advertised": True,
        "codex_sync_latency_gate": {
            "required": "30 cold and 30 warm observations, each <=60 seconds",
            "authorization_env": "RUST_MCP_M4_CODEX_SYNC_QUALIFIED=1",
            "authorized": os.environ.get("RUST_MCP_M4_CODEX_SYNC_QUALIFIED") == "1",
            "tasks_supported": False,
        },
        "clients": {
            "inspector": {"expected": "2.5.0", "observed": package_version,
                          "tasks": True, "resource": True, "cancel": True},
            "codex_app_server": {"expected": "codex-cli 0.153.0", "observed": codex_version,
                                 "tasks": False, "resource": True, "cancel": False},
        },
        "source_sha256": source_hashes(),
        "m3_reuse": ["proxy", "run_bounded", "next_attempt", "protocol_summary",
                     "assert_no_credentials", "controller.py transport"],
    }
    return receipt


def validate_protocol_metadata(path: pathlib.Path) -> dict[str, object]:
    m3 = load_m3()
    rows = [json.loads(line) for line in path.read_text().splitlines() if line]
    for row in rows:
        extra = set(row) - SAFE_PROTOCOL_KEYS
        if extra:
            raise RuntimeError("protocol metadata contains unapproved keys: " + ",".join(sorted(extra)))
        if not isinstance(row.get("sha256"), str) or len(row["sha256"]) != 64:
            raise RuntimeError("protocol metadata digest is invalid")
    encoded = path.read_bytes().lower()
    for forbidden in (b"authorization", b"access_token", b"refresh_token", b"auth.json"):
        if forbidden in encoded:
            raise RuntimeError("credential-shaped protocol evidence")
    summary = m3.protocol_summary(path, True)
    summary["metadata_only"] = True
    return summary


def tar_header(name: str, size: int) -> bytes:
    header = bytearray(512)
    header[:len(name)] = name.encode()
    for start, end, value in ((100,108,0o600),(108,116,0),(116,124,0),
                              (124,136,size),(136,148,0),(329,337,0),(337,345,0)):
        header[start:end] = f"{value:0{end-start-1}o}\0".encode()
    header[156] = ord("0")
    header[257:263] = b"ustar\0"
    header[263:265] = b"00"
    header[148:156] = b"        "
    checksum = sum(header)
    header[148:156] = f"{checksum:06o}\0 ".encode()
    return bytes(header)


def fresh_catalog_bundle(private_root: pathlib.Path) -> pathlib.Path:
    """Refresh the checked seed-42 fixture without embedding any production key."""
    m3 = load_m3()
    source = ROOT / "fixtures/catalog/fixture-1.tar.zst"
    raw = private_root / "catalog-source.tar"
    subprocess.run([str(ZSTD), "-q", "-d", "-o", str(raw), str(source)],
                   timeout=20, check=True)
    with tarfile.open(raw, "r:") as archive:
        database = bytearray(archive.extractfile("catalog.sqlite").read())
    database_path = private_root / "catalog.sqlite"
    database_path.write_bytes(database)
    now = int(time.time())
    provenance = {"source_kind":"registry_snapshot","source_id":"fixture-only-m4-client",
                  "created_at":now,"observed_at":now,"integrity":"verified","network_used":False}
    connection = sqlite3.connect(database_path)
    try:
        connection.execute("UPDATE snapshots SET sequence=7, provenance=? WHERE id=1",
                           (json.dumps(provenance, separators=(",", ":")),))
        connection.commit()
    finally:
        connection.close()
    database = database_path.read_bytes()
    manifest = {"snapshot_format_version":1,"catalog_schema_version":1,
                "semantic_index_version":None,"embedding_model_id":None,
                "publisher":"fixture-only","channel":"test","sequence":7,
                "catalog_provenance":provenance,
                "files":[{"path":"catalog.sqlite","byte_length":len(database),
                           "sha256":m3.digest(database)}]}
    manifest_bytes = json.dumps(manifest, separators=(",", ":"), ensure_ascii=False).encode()
    signer = ("const c=require('node:crypto'),fs=require('node:fs');"
              "const s=Buffer.alloc(32,42),p=Buffer.concat([Buffer.from('302e020100300506032b657004220420','hex'),s]);"
              "const k=c.createPrivateKey({key:p,format:'der',type:'pkcs8'});"
              "process.stdout.write(c.sign(null,fs.readFileSync(0),k));")
    signed = b"rust-engineering-catalog-bundle-v1\0" + manifest_bytes
    signature = subprocess.run([str(NODE), "-e", signer], input=signed,
                               capture_output=True, timeout=10, check=True).stdout
    if len(signature) != 64:
        raise RuntimeError("fixture signature length mismatch")
    archive = bytearray()
    for name, data in (("manifest.json", manifest_bytes),
                       ("signature.ed25519", signature), ("catalog.sqlite", database)):
        archive.extend(tar_header(name, len(data)))
        archive.extend(data)
        archive.extend(b"\0" * ((512 - len(data) % 512) % 512))
    archive.extend(b"\0" * 1024)
    raw_out = private_root / "catalog-fresh.tar"
    raw_out.write_bytes(archive)
    output = private_root / "catalog-fresh.tar.zst"
    subprocess.run([str(ZSTD), "-q", "-1", "-o", str(output), str(raw_out)],
                   timeout=20, check=True)
    os.chmod(output, 0o600)
    return output


def prepare_fixture(private_root: pathlib.Path) -> dict[str, pathlib.Path | str]:
    m3 = load_m3()
    project = private_root / "project"
    shutil.copytree(ROOT / "fixtures/nextest/passing", project)
    manifests = [project / "Cargo.toml", *project.glob("*/Cargo.toml")]
    for manifest in manifests:
        if manifest.is_file():
            text = manifest.read_text()
            if "license" not in text:
                text = text.replace("[package]\n", "[package]\nlicense = \"MIT\"\n", 1)
            manifest.write_text(text)
    license_bytes = (ROOT / "fixtures/m4-deny-native/LICENSE-MIT").read_bytes()
    for manifest in manifests:
        if manifest.is_file():
            (manifest.parent / "LICENSE-MIT").write_bytes(license_bytes)
    cancel_project = private_root / "cancel-project"
    (cancel_project / "src").mkdir(parents=True)
    (cancel_project / "Cargo.toml").write_text(
        "[package]\nname = \"m4-cancel-fixture\"\nversion = \"0.1.0\"\n"
        "edition = \"2024\"\nlicense = \"MIT\"\n"
    )
    (cancel_project / "Cargo.lock").write_text(
        "version = 4\n\n[[package]]\nname = \"m4-cancel-fixture\"\nversion = \"0.1.0\"\n"
    )
    (cancel_project / "src/lib.rs").write_text(
        "#[test]\nfn waits_for_cancellation() { loop { std::hint::spin_loop(); } }\n"
    )
    (cancel_project / "LICENSE-MIT").write_bytes(license_bytes)
    policy = private_root / "policy.json"
    policy.write_text(json.dumps({"schema_version":1,"rules":{"allowed_licenses":["MIT"],
        "banned_packages":[],"multiple_versions":"deny","wildcards":"deny"},"suppressions":[]},
        separators=(",", ":")))
    os.chmod(policy, 0o600)
    rustsec = private_root / "rustsec.json"
    advisory = (ROOT / "crates/catalog-adapter/tests/fixtures/rustsec/RUSTSEC-2023-0071.md").read_text()
    now = int(time.time())
    rustsec.write_text(json.dumps({"format_version":1,"sequence":1,
        "source_id":"fixture-m4-client","created_at":now,"observed_at":now,
        "records":[{"path":"crates/rsa/RUSTSEC-2023-0071.md","markdown":advisory}]},
        separators=(",", ":")))
    os.chmod(rustsec, 0o600)
    catalog = fresh_catalog_bundle(private_root)
    store = private_root / "catalog-store"; store.mkdir(mode=0o700)
    trust = private_root / "trust.json"
    shutil.copyfile(ROOT / "fixtures/catalog/fixture-trust.json", trust)
    os.chmod(trust, 0o600)
    import_result = subprocess.run([str(SERVER), "catalog", "import", str(catalog),
        "--store", str(store), "--trust", str(trust), "--json"], capture_output=True,
        timeout=30, check=False)
    if import_result.returncode != 0:
        raise RuntimeError("fresh fixture catalog import failed")
    vendor = (ROOT / "fixtures/cargo-vendor-data/vendor").resolve()
    vendor_result = subprocess.run([str(SERVER), "cargo-vendor", "inspect", "--directory",
        str(vendor), "--json"], capture_output=True, timeout=30, check=True)
    vendor_json = json.loads(vendor_result.stdout)
    fingerprint = next(v for v in m3.find_values(vendor_json, "tree_fingerprint") if isinstance(v, str))
    return {"project":project,"cancel_project":cancel_project,
            "policy":policy,"policy_hash":"sha256:"+m3.file_digest(policy),
            "rustsec":rustsec,"rustsec_hash":"sha256:"+m3.file_digest(rustsec),
            "vendor":vendor,"vendor_hash":fingerprint,"catalog_store":store,"catalog_trust":trust}


def server_argv(state: pathlib.Path, socket: str, fixture: dict) -> list[str]:
    return [str(SERVER),"serve","--stdio","--root",str(fixture["project"]),
        "--root",str(fixture["cancel_project"]),
        "--catalog-store",str(fixture["catalog_store"]),"--catalog-trust",str(fixture["catalog_trust"]),
        "--security-policy",str(fixture["policy"]),"--security-policy-sha256",str(fixture["policy_hash"]),
        "--cargo-vendor-dir",str(fixture["vendor"]),"--cargo-vendor-tree-sha256",str(fixture["vendor_hash"]),
        "--rustsec-snapshot",str(fixture["rustsec"]),"--rustsec-sha256",str(fixture["rustsec_hash"]),
        "--docker",str(DOCKER),"--docker-socket",socket,"--state-root",str(state),"--rust-image",IMAGE]


def inspector_gate(attempt: pathlib.Path, socket: str, fixture: dict) -> dict:
    m3 = load_m3(); observation = attempt / "protocol.jsonl"
    state = attempt / "state-inspector"; state.mkdir(mode=0o700)
    # Keep Node package resolution beside the installed Inspector dependencies.
    bridge = ROOT / "target/m1-17-inspector" / f"m4-{attempt.name}-bridge.mjs"
    suffix = b"\nexport { InspectorClient, createTransportNode };\n"
    with bridge.open("xb") as stream:
        stream.write(INSPECTOR.read_bytes() + suffix)
    proxy_argv = [sys.executable,str(pathlib.Path(__file__).resolve()),"proxy","--client","inspector",
                  "--observation",str(observation),"--server-argv-json",
                  json.dumps(server_argv(state,socket,fixture),separators=(",",":"))]
    try:
        result = m3.run_bounded([str(NODE),str(SESSION),str(bridge),
            json.dumps(proxy_argv,separators=(",",":"))],attempt,1800,attempt/"inspector-session.json")
    finally:
        bridge.unlink(missing_ok=True)
    if result["exit_code"] != 0:
        raise RuntimeError("Inspector M4 session failed")
    outcome = json.loads((attempt/"inspector-session.stdout").read_text())
    if not all(outcome.get(k) is True for k in ("discovery","positive","negative","cancel","resource","task_flow")):
        raise RuntimeError("Inspector M4 oracle incomplete")
    return {"version":"2.5.0","bundle_sha256":m3.file_digest(INSPECTOR),"session":result,"oracles":outcome}


def codex_gate(attempt: pathlib.Path, socket: str, fixture: dict, codex: pathlib.Path) -> dict:
    """Stock app-server conversion gate; deliberately no Tasks calls or auth copy."""
    m3 = load_m3(); observation=attempt/"protocol.jsonl"; state=attempt/"state-codex"; state.mkdir(mode=0o700)
    controller_path=ROOT/"docs/validation/M1/17-codex-client/controller.py"
    spec=importlib.util.spec_from_file_location("m4_codex_controller",controller_path)
    if spec is None or spec.loader is None: raise RuntimeError("Codex controller unavailable")
    controller=importlib.util.module_from_spec(spec); spec.loader.exec_module(controller)
    controller.TOOLS=EXPECTED_TOOLS; controller.DISABLED_HOST_SERVERS=()
    base=controller.overrides
    def overrides(plan):
        values=base(plan); values["features.code_mode_host"]=True; values["features.mcp_2026_07_28"]=True
        values["features.skip_host_skill_discovery"]=True; return values
    controller.overrides=overrides
    proxy=[sys.executable,str(pathlib.Path(__file__).resolve()),"proxy","--client","codex-app-server",
           "--observation",str(observation),"--server-argv-json",
           json.dumps(server_argv(state,socket,fixture),separators=(",",":"))]
    plan={"codex":str(codex),"server_binary":proxy[0],"server_args":proxy[1:],
          "model":"gpt-5.6-sol","effort":"medium"}
    source_home=pathlib.Path(os.environ.get("CODEX_HOME",pathlib.Path.home()/".codex"))
    auth=source_home/"auth.json"; private=pathlib.Path(tempfile.mkdtemp(prefix="rust-mcp-m4-codex-",dir="/private/tmp")); os.chmod(private,0o700)
    if not auth.is_file(): shutil.rmtree(private); raise RuntimeError("Codex auth unavailable")
    os.symlink(auth,private/"auth.json")
    previous=os.environ.get("CODEX_HOME"); os.environ["CODEX_HOME"]=str(private); transport=None
    try:
        transport=controller.Transport(controller.command(plan),attempt); controller.init(transport,attempt)
        started=controller.thread_start(transport,plan,attempt); thread=started.get("thread",{}).get("id")
        if not isinstance(thread,str): raise RuntimeError("Codex thread missing")
        def call(name,args,timeout=60):
            value=transport.rpc("mcpServer/tool/call",{"threadId":thread,"server":"rust_engineering","tool":name,"arguments":args},timeout)
            if not isinstance(value,dict): raise RuntimeError(f"Codex conversion failed for {name}")
            return value
        opened=call("rust.project.open",{"path":str(fixture["project"])})
        refs=[x for x in m3.find_values(opened,"project_ref") if isinstance(x,str)]
        if len(set(refs))!=1: raise RuntimeError("Codex ProjectRef ambiguous")
        results={}
        for tool in M4_TOOLS:
            args={"project_ref":refs[0],"execution_mode":"synchronous","timeout_seconds":60}
            if tool=="rust.quality.gate.v2": args["profile"]="strict"
            results[tool]=call(tool,args)
            if results[tool].get("structuredContent",{}).get("status")!="passed":
                raise RuntimeError(f"Codex positive {tool} did not pass")
        negative=call("rust.deny",{"project_ref":"prj_"+"0"*32,"execution_mode":"synchronous","timeout_seconds":60})
        if negative.get("structuredContent",{}).get("status") not in {"blocked","unavailable"}:
            raise RuntimeError("Codex negative conversion was not preserved")
        uris=[x for value in results.values() for x in m3.find_values(value,"uri") if isinstance(x,str)]
        for uri in uris:
            controller.validate_resource(transport.rpc("mcpServer/resource/read",{"threadId":thread,"server":"rust_engineering","uri":uri},60))
        prompt=("Use only the configured Rust Engineering MCP tools. Open " + str(fixture["project"]) +
                ", then call rust.deny, rust.unsafe.scan, rust.supply_chain.inspect, "
                "rust.quality.gate.v2 with profile strict, and rust.miri. Use synchronous execution "
                "and timeout_seconds 60 for every M4 tool. Report each structured status. Do not use "
                "any non-MCP capability.")
        turn=transport.rpc("turn/start",{"threadId":thread,"input":[{"type":"text","text":prompt}]},30).get("turn",{})
        turn_id=turn.get("id")
        if not isinstance(turn_id,str): raise RuntimeError("Codex model turn did not start")
        completed=False; observed=set(); passed=set(); deadline=time.monotonic()+900
        events=attempt/"codex-m4-model-events.jsonl"
        while time.monotonic()<deadline and not completed:
            try: event=transport.q.get(timeout=0.25)
            except queue.Empty:
                if transport.failure: raise RuntimeError(transport.failure)
                continue
            m3.append_observation(events,event)
            item=event.get("params",{}).get("item",{})
            if item.get("type")=="mcpToolCall" and isinstance(item.get("tool"),str):
                observed.add(item["tool"])
                if (item.get("status")=="completed" and item.get("error") is None
                        and (item.get("result") or {}).get("structuredContent",{}).get("status")=="passed"):
                    passed.add(item["tool"])
            if event.get("method")=="turn/completed" and event.get("params",{}).get("turn",{}).get("id")==turn_id: completed=True
        required={"rust.project.open",*M4_TOOLS}
        if not completed or not required.issubset(passed): raise RuntimeError("Codex model-directed M4 flow incomplete or not passed")
        return {"version":"codex-cli 0.153.0","positive_tools":list(results),"negative":True,
                "resources":len(uris),"tasks_declared":False,"auth_copy":False,
                "task_cancel":"not supported by stock client","model_turn_completed":True,
                "model_turn_tools":sorted(observed),"model_turn_passed_tools":sorted(passed),"model_events_sha256":m3.file_digest(events)}
    finally:
        try:
            if transport is not None:
                cleanup=transport.close()
                if not cleanup.get("cleanup_verified",False): raise RuntimeError("Codex cleanup unverified")
        finally:
            if previous is None: os.environ.pop("CODEX_HOME",None)
            else: os.environ["CODEX_HOME"]=previous
            shutil.rmtree(private,ignore_errors=True)


def run(socket: str) -> int:
    m3=load_m3(); check=preflight()
    if not all(check["advertisement"].values()): raise RuntimeError("five M4 advertisement switches are not enabled")
    if os.environ.get("RUST_MCP_M4_CODEX_SYNC_QUALIFIED")!="1":
        raise RuntimeError("Codex synchronous M4 gate awaits 30 cold and 30 warm <=60s qualification")
    codex_name=shutil.which("codex"); required=(SERVER,NODE,INSPECTOR,INSPECTOR_PACKAGE,DOCKER,ZSTD)
    missing=[str(p) for p in required if not p.is_file()]
    if missing or not codex_name: raise RuntimeError("missing prerequisites: "+", ".join(missing+([] if codex_name else ["codex"])))
    if check["clients"]["inspector"]["observed"]!="2.5.0" or check["clients"]["codex_app_server"]["observed"]!="codex-cli 0.153.0":
        raise RuntimeError("stock client version mismatch")
    # M4 owns a separate immutable attempt namespace.
    ATTEMPTS.mkdir(parents=True,exist_ok=True); numbers=[]
    for path in ATTEMPTS.glob("attempt-*"):
        try: numbers.append(int(path.name.removeprefix("attempt-")))
        except ValueError: pass
    attempt=ATTEMPTS/f"attempt-{max(numbers,default=0)+1}"; attempt.mkdir(mode=0o700)
    private=pathlib.Path(tempfile.mkdtemp(prefix="rust-mcp-m4-clients-",dir="/private/tmp")); os.chmod(private,0o700)
    gate_spec=importlib.util.spec_from_file_location("m4_gate_inventory",ROOT/"scripts/gate.py")
    if gate_spec is None or gate_spec.loader is None: raise RuntimeError("gate inventory unavailable")
    gate=importlib.util.module_from_spec(gate_spec);gate_spec.loader.exec_module(gate)
    candidate_sources=gate.source_inventory(ROOT,os.environ.copy())
    receipt={"schema":"rust-mcp-m4-clients-v1","status":"failed","image_id":IMAGE,
             "attempt":attempt.name,"source_sha256":source_hashes(),"candidate":{"server_sha256":m3.file_digest(SERVER)}}
    receipt["candidate"]["sources"]=candidate_sources
    try:
        fixture=prepare_fixture(private)
        receipt["fixture_inputs"]={
            "policy_sha256":m3.file_digest(fixture["policy"]),
            "rustsec_sha256":m3.file_digest(fixture["rustsec"]),
            "catalog_bundle_sha256":m3.file_digest(private/"catalog-fresh.tar.zst"),
            "catalog_trust_sha256":m3.file_digest(fixture["catalog_trust"]),
            "cargo_vendor_tree_sha256":fixture["vendor_hash"],
            "network_used":False,
        }
        receipt["inspector"]=inspector_gate(attempt,socket,fixture)
        receipt["codex_app_server"]=codex_gate(attempt,socket,fixture,pathlib.Path(codex_name))
        receipt["protocol"]=validate_protocol_metadata(attempt/"protocol.jsonl")
        if (candidate_sources != gate.source_inventory(ROOT,os.environ.copy())
                or receipt["candidate"]["server_sha256"] != m3.file_digest(SERVER)
                or receipt["source_sha256"] != source_hashes()):
            raise RuntimeError("client qualification inputs changed during execution")
        receipt["status"]="passed"
    except Exception as error:
        receipt["error"]={"type":type(error).__name__,"message":str(error)}; raise
    finally:
        shutil.rmtree(private,ignore_errors=True)
        receipt["private_fixture_removed"]=not private.exists()
        m3.assert_no_credentials(attempt); m3.save_json(attempt/"receipt.json",receipt,exclusive=True)
        if receipt["status"]=="passed":
            if CURRENT.exists(): raise RuntimeError("current M4 client receipt already exists")
            m3.save_json(CURRENT,receipt,exclusive=True)
    return 0


def main() -> int:
    parser=argparse.ArgumentParser(); subs=parser.add_subparsers(dest="command")
    proxy_parser=subs.add_parser("proxy"); proxy_parser.add_argument("--client",required=True)
    proxy_parser.add_argument("--observation",type=pathlib.Path,required=True)
    proxy_parser.add_argument("--server-argv-json",required=True)
    parser.add_argument("--run",action="store_true"); parser.add_argument("--docker-socket",default=os.environ.get("RUST_MCP_TEST_SOCKET"))
    parser.add_argument("--write-preflight",action="store_true")
    options=parser.parse_args()
    if options.command=="proxy":
        argv=json.loads(options.server_argv_json)
        if not isinstance(argv,list) or not argv or any(not isinstance(x,str) for x in argv): raise RuntimeError("invalid closed server argv")
        return load_m3().proxy(argv,options.observation,options.client)
    if not options.run:
        receipt=preflight()
        if options.write_preflight:
            if PREFLIGHT.exists(): raise RuntimeError("M4 preflight receipt already exists")
            load_m3().save_json(PREFLIGHT,receipt,exclusive=True)
        print(json.dumps(receipt,sort_keys=True)); return 0
    if not options.docker_socket or not pathlib.Path(options.docker_socket).is_absolute():
        raise RuntimeError("an absolute RUST_MCP_TEST_SOCKET is required")
    return run(options.docker_socket)


if __name__ == "__main__":
    raise SystemExit(main())
