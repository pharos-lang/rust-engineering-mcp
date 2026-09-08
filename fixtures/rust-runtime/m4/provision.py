#!/usr/bin/env python3
"""Host-only, manifest-closed M4 runtime acquisition and Docker build."""
from __future__ import annotations
import argparse, datetime as dt, hashlib, json, os, shutil, subprocess, tarfile
from pathlib import Path, PurePosixPath
import urllib.parse, urllib.request
import re

ROOT = Path(__file__).resolve().parents[3]
MANIFEST = ROOT / "docs/validation/m4-provisioning-proposal/manifest.json"
HERE = Path(__file__).resolve().parent

def sha256(path):
    with Path(path).open("rb") as f: return hashlib.file_digest(f, "sha256").hexdigest()

def regular(path):
    path = Path(path)
    if path.is_symlink() or not path.is_file(): raise ValueError(f"expected unlinked regular file: {path}")

def validate_archive(path):
    regular(path); seen=set(); names=[]
    with tarfile.open(path, "r:*") as tf:
        for m in tf.getmembers():
            p=PurePosixPath(m.name); normalized=p.as_posix().rstrip("/")
            if not m.name or p.is_absolute() or ".." in p.parts: raise ValueError(f"unsafe archive path: {m.name!r}")
            if not re.fullmatch(r"[A-Za-z0-9_./+@(), =:-]+", m.name): raise ValueError(f"archive path cannot be safely indexed: {m.name!r}")
            if normalized in seen: raise ValueError(f"duplicate archive member: {m.name!r}")
            seen.add(normalized)
            if m.issym() or m.islnk() or m.isdev() or m.isfifo(): raise ValueError(f"linked or special archive member: {m.name!r}")
            if not (m.isfile() or m.isdir()): raise ValueError(f"unsupported archive member: {m.name!r}")
            names.append(m.name)
    return names

def download(url, path, want_hash, want_size):
    path=Path(path); parsed=urllib.parse.urlsplit(url)
    if parsed.scheme != "https" or not parsed.hostname: raise ValueError(f"only HTTPS accepted: {url}")
    if path.exists(): regular(path)
    else:
        partial=path.with_suffix(path.suffix+".partial")
        if partial.exists(): regular(partial); partial.unlink()
        req=urllib.request.Request(url,headers={"User-Agent":"rust-engineering-mcp-m4-provision/1"})
        with urllib.request.urlopen(req,timeout=600) as src, partial.open("xb") as dst: shutil.copyfileobj(src,dst,1024*1024)
        os.replace(partial,path)
    got_hash=sha256(path); got_size=path.stat().st_size
    if (got_hash,got_size)!=(want_hash,want_size): raise ValueError(f"integrity mismatch {path.name}: {got_size} {got_hash}")
    return {"url":url,"filename":path.name,"sha256":got_hash,"size":got_size}

def config_digest(config):
    return "sha256:"+hashlib.sha256(json.dumps(config,sort_keys=True,separators=(",",":")).encode()).hexdigest()

def inputs(manifest):
    d=manifest["cargo_deny"]; n=manifest["nightly_miri"]
    result=[{**d["source_package"],"filename":"cargo-deny-0.19.7.crate","kind":"deny-source"},
            {**d["lock"],"filename":"cargo-deny-tag.Cargo.lock","kind":"deny-lock"},
            {**n["manifest"],"filename":"channel-rust-nightly.toml","kind":"nightly-manifest"},
            {**n["sysroot"]["lock"],"filename":"rust-library.Cargo.lock","kind":"library-lock"}]
    result += [{**c,"filename":urllib.parse.unquote(c["url"].rsplit("/",1)[1]),"kind":"rust-component"} for c in n["components"]]
    unique={(x["name"],x["version"],x["sha256"]):x for x in d["dependencies"]+n["sysroot"]["dependencies"]}
    if (len(d["dependencies"]),len(n["sysroot"]["dependencies"]),len(unique))!=(212,31,238): raise ValueError("dependency closure differs from 212/31/238")
    for x in sorted(unique.values(),key=lambda y:(y["name"],y["version"])):
        result.append({**x,"url":x["download_url"],"filename":f'{x["name"]}-{x["version"]}.crate',"kind":"registry-crate"})
    if len({x["filename"] for x in result})!=len(result): raise ValueError("context filename collision")
    return result

def validate_manifest(m):
    if m.get("schema")!="rust-engineering-mcp.m4-provisioning-proposal.v1": raise ValueError("unexpected manifest schema")
    b=m["base_image"]
    expected=("sha256:384a1742ecc53cdd3a9c0bf36c6f8b66db73ddd118aeeae6e55654ea998ae36a","sha256:7d4e58b9e29b2045c13d71542f7892ee071a6886a1b939c4cbfc3ff7ce40dc45","linux","arm64")
    if (b["required_image_id"],b["required_config_digest"],b["os"],b["architecture"])!=expected: raise ValueError("base differs from ADR-066")
    inputs(m)

def inspect(docker,ref): return json.loads(subprocess.check_output(docker+["image","inspect",ref],text=True))[0]
def verify_base(m,x):
    b=m["base_image"]
    if (x["Id"],config_digest(x["Config"]),x["Os"],x["Architecture"])!=(b["required_image_id"],b["required_config_digest"],b["os"],b["architecture"]): raise ValueError("M3 base identity/config/platform mismatch")
def validate_context(path,allowed):
    for p in Path(path).iterdir():
        if p.name not in allowed or p.is_symlink() or not p.is_file(): raise ValueError(f"unexpected or linked build-context entry: {p}")
def packaged_lock(path):
    expected="cargo-deny-0.19.7/Cargo.lock"; names=validate_archive(path)
    if [n for n in names if n==expected] != [expected]: raise ValueError("package lacks exact Cargo.lock member")
    with tarfile.open(path,"r:gz") as tf:
        f=tf.extractfile(expected)
        if f is None: raise ValueError("unreadable packaged Cargo.lock")
        return f.read()

def generated(context,m,all_inputs):
    (context/"SHA256SUMS").write_text("".join(f'{x["sha256"]}  {x["filename"]}\n' for x in all_inputs))
    deps=[x for x in all_inputs if x["kind"]=="registry-crate"]
    (context/"dependency-map.tsv").write_text("".join(f'{x["filename"]}\t{x["name"]}-{x["version"]}\t{x["sha256"]}\n' for x in deps))
    sbom={"schema":"rust-engineering-mcp.m4-build-inputs.v1","manifest_sha256":sha256(MANIFEST),"inputs":[{k:x[k] for k in ("kind","filename","url","size","sha256")} for x in all_inputs],"licenses":sorted({(x.get("name") or x.get("id") or x["kind"],x.get("version",""),x.get("license","unknown")) for x in all_inputs})}
    (context/"build-inputs.json").write_text(json.dumps(sbom,indent=2)+"\n")
    installed={"schema":"rust-engineering-mcp.m4-guest-sbom.v1","base_image_id":m["base_image"]["required_image_id"],"target":m["constraints"]["target"],"components":[{"name":"cargo-deny","version":m["cargo_deny"]["version"],"license":m["cargo_deny"]["source_package"]["license"]}]+[{"name":x["id"],"version":x["version"],"license":x["license"]} for x in m["nightly_miri"]["components"]],"registry_packages":[{"name":x["name"],"version":x["version"],"sha256":x["sha256"],"license":x["license"]} for x in deps]}
    (context/"m4-sbom.json").write_text(json.dumps(installed,indent=2)+"\n")
    shutil.copyfile(HERE/"Dockerfile",context/"Dockerfile"); shutil.copyfile(HERE/"build.sh",context/"build.sh")

def main():
    p=argparse.ArgumentParser(); p.add_argument("--docker",required=True); p.add_argument("--host",required=True); p.add_argument("--output",required=True,type=Path); p.add_argument("--manifest",type=Path,default=MANIFEST); a=p.parse_args()
    if not Path(a.docker).is_absolute() or not a.host.startswith("unix:///"): p.error("absolute Docker executable and local Unix socket required")
    if a.manifest.resolve()!=MANIFEST.resolve(): raise SystemExit("only repository ADR-066 manifest accepted")
    m=json.loads(a.manifest.read_text()); validate_manifest(m); all_inputs=inputs(m)
    a.output.mkdir(parents=True,exist_ok=True); context=a.output/"build-context"; context.mkdir(parents=True,exist_ok=True)
    allowed={x["filename"] for x in all_inputs}|{"Dockerfile","build.sh","SHA256SUMS","dependency-map.tsv","build-inputs.json","m4-sbom.json"}
    for filename in {x["filename"] for x in all_inputs}:
        partial=context/(filename+".partial")
        if partial.exists(): regular(partial); partial.unlink()
    validate_context(context,allowed)
    docker=[a.docker,"--host",a.host]; before=inspect(docker,m["base_image"]["tag"]); verify_base(m,before)
    (a.output/"base-inspect-before.json").write_text(json.dumps(before,indent=2)+"\n")
    receipts=[]
    for i,x in enumerate(all_inputs,1):
        r=download(x["url"],context/x["filename"],x["sha256"],x["size"]); r["kind"]=x["kind"]
        if x["filename"].endswith((".crate",".tar.xz")): r["archive_member_count"]=len(validate_archive(context/x["filename"]))
        receipts.append(r); print(f'verified {i}/{len(all_inputs)} {x["filename"]}',flush=True)
    if packaged_lock(context/"cargo-deny-0.19.7.crate")!=(context/"cargo-deny-tag.Cargo.lock").read_bytes(): raise SystemExit("BLOCKER: packaged Cargo.lock differs byte-for-byte from pinned official tag lock")
    generated(context,m,all_inputs); validate_context(context,allowed)
    log=a.output/"build.log"; iid=a.output/"image-id"
    command=docker+["build","--platform","linux/arm64","--network","none","--pull=false","--progress","plain","--build-arg",f'BASE_IMAGE={m["base_image"]["tag"]}',"--tag","rust-engineering-runtime:1.98.1-arm64-m4","--iidfile",str(iid),str(context)]
    with log.open("w") as out: subprocess.run(command,stdout=out,stderr=subprocess.STDOUT,check=True)
    after=inspect(docker,m["base_image"]["tag"]); verify_base(m,after)
    image=inspect(docker,iid.read_text().strip())
    receipt={"schema":"rust-engineering-mcp.m4-provisioning.v1","status":"built_not_gateway_approved","observed_at":dt.datetime.now(dt.timezone.utc).isoformat(),"manifest_sha256":sha256(a.manifest),"base_image_id":before["Id"],"base_config_digest":config_digest(before["Config"]),"image_id":image["Id"],"tag":"rust-engineering-runtime:1.98.1-arm64-m4","docker_build":{"network":"none","pull":False,"platform":"linux/arm64","command":command},"archives":receipts,"packaged_cargo_lock_byte_identical":True,"build_log":{"path":str(log),"size":log.stat().st_size,"sha256":sha256(log)}}
    (a.output/"provisioning-receipt.json").write_text(json.dumps(receipt,indent=2)+"\n"); print(image["Id"])
if __name__=="__main__": main()
