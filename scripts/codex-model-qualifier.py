#!/usr/bin/env python3
"""Candidate-bound two-session stock Codex M1 qualification harness."""
from __future__ import annotations
import argparse,base64,binascii,ctypes,datetime,difflib,hashlib,json,os,queue,re,signal,stat,subprocess,sys,threading,time
from pathlib import Path
from typing import Any

TOOLS=("rust.project.open","rust.project.inspect","rust.toolchain.inspect","rust.check","rust.fmt.check","rust.clippy","rust.test","rust.dependencies.audit","rust.diagnostics.explain","rust.quality.gate","rust.catalog.status","rust.crate.search","rust.crate.inspect")
REPAIR_TOOLS=("rust.project.open","rust.project.inspect","rust.check","rust.quality.gate")
MISSING_TOOLS=("rust.project.open","rust.check")
OUTPUT_KEYS={"data","diagnostics","duration_ms","error_code","error_message","evidence","status","summary","truncation"}
REPAIR_FLAGS=("--root","--docker","--docker-socket","--state-root","--rust-image")
MISSING_FLAGS=("--root",)
DISABLED=("shell_tool","apps","plugins","hooks","memories","multi_agent","multi_agent_v2","browser_use","browser_use_external","computer_use","code_mode","code_mode_only","image_generation","view_image","tool_suggest","remote_plugin","skill_search","skill_mcp_dependency_install","workspace_dependencies","goals","token_budget","sleep_tool","deferred_executor","standalone_web_search","in_app_browser","in_app_chat","in_app_local_automation","artifact","js_repl")
ENABLED=("mcp_2026_07_28","code_mode_host","skip_host_skill_discovery")
SAFE_METHODS={"thread/started","thread/status/changed","thread/settings/updated","turn/started","turn/completed","turn/diff/updated","item/started","item/completed","item/plan/delta","item/agentMessage/delta","item/reasoning/summaryTextDelta","item/reasoning/summaryPartAdded","item/reasoning/textDelta","rawResponseItem/completed","thread/tokenUsage/updated","mcpServer/toolCallProgress","remoteControl/status/changed","deprecationNotice","warning","mcpServer/startupStatus/updated","account/rateLimits/updated"}
SAFE_ITEMS={"userMessage","agentMessage","reasoning","plan","mcpToolCall","fileChange"}
SAFE_RAW={"reasoning","message","mcp_call","mcp_call_output","mcp_list_tools","mcp_list_tools_output","custom_tool_call","custom_tool_call_output"}
HEX=re.compile(r"^[0-9a-f]{64}$");FP=re.compile(r"^sha256:[0-9a-f]{64}$")
PLAN_KEYS={"schema_version","codex_source","codex_sha256","codex_version","code_host_source","code_host_sha256","server_source","server_sha256","docker_cli","docker_sha256","docker_socket","docker_label","auth_source","private_root","output_root","fixture_root","fixture_snapshot","model","effort","budgets","schema_bundle_sha256","feature_world","allowed_config_layer_types","runtime","missing_error_message","phases"}
MAX_FILES=512;MAX_FILE_BYTES=8<<20;MAX_TREE_BYTES=32<<20;MAX_BUNDLE_FILES=4096;MAX_BUNDLE_FILE_BYTES=16<<20;MAX_BUNDLE_BYTES=256<<20;MAX_PRIVATE_FILES=16384;MAX_PRIVATE_FILE_BYTES=512<<20;MAX_PRIVATE_TREE_BYTES=2<<30;MAX_CLEANUP_ENTRIES=32768;MAX_LINE_BYTES=4<<20
DOCKER_SERVER_VERSION="29.7.2"
DOCKER_SERVER_ARCH="arm64"
DOCKER_SERVER_OS="linux"
PRODUCT_RUNTIME_PLATFORM="linux/aarch64"
PRODUCT_DOCKER_LABEL="org.rust-mcp.execution=true"
MISSING_RUNTIME_MESSAGE="Host runtime policy, failed calibration or current capacity denied check"

def enc(v:Any)->bytes:return json.dumps(v,sort_keys=True,separators=(",",":"),ensure_ascii=True,allow_nan=False).encode()
def digest(v:bytes)->str:return hashlib.sha256(v).hexdigest()
def loads_strict(v):
 def reject(value):raise ValueError(f"non-finite JSON number:{value}")
 return json.loads(v,parse_constant=reject)
def write_all(fd:int,v:bytes):
 view=memoryview(v)
 while view:
  n=os.write(fd,view)
  if n<=0:raise RuntimeError("short write")
  view=view[n:]
def file_digest(p:Path)->str:
 h=hashlib.sha256()
 with p.open("rb") as f:  # NOSONAR -- p is plan-authenticated or an owned output.
  for b in iter(lambda:f.read(1<<20),b""):h.update(b)
 return h.hexdigest()
def abs_path(v:Any,label:str,physical=True)->Path:
 if not isinstance(v,str) or not Path(v).is_absolute() or any(ord(c)<32 for c in v):raise ValueError(f"{label}:absolute")
 p=Path(v)
 if physical and p.resolve(strict=False)!=p:raise ValueError(f"{label}:symlink")  # NOSONAR -- absolute, control-free path checked immediately above.
 return p
def private_dir(p:Path,label:str,empty=False):
 s=p.lstat()  # NOSONAR -- callers pass an absolute plan-authenticated path.
 if p.is_symlink() or not stat.S_ISDIR(s.st_mode) or stat.S_IMODE(s.st_mode)!=0o700 or s.st_uid!=os.geteuid():raise ValueError(f"{label}:owned 0700")
 if empty and any(p.iterdir()):raise ValueError(f"{label}:not empty")
def safe_source(p:Path,want:str,executable:bool):
 s=p.lstat()
 if p.is_symlink() or not stat.S_ISREG(s.st_mode) or s.st_uid!=os.geteuid() or stat.S_IMODE(s.st_mode)&0o022:raise ValueError(f"unsafe source:{p}")
 if executable and not stat.S_IMODE(s.st_mode)&0o100:raise ValueError(f"not executable:{p}")
 if file_digest(p)!=want:raise ValueError(f"hash mismatch:{p}")
def secure_create(p:Path,mode=0o600)->int:
 fd=os.open(p,os.O_WRONLY|os.O_CREAT|os.O_EXCL|getattr(os,"O_NOFOLLOW",0),mode)  # NOSONAR -- exclusive no-follow create below an owned private directory.
 if stat.S_IMODE(os.fstat(fd).st_mode)!=mode:os.close(fd);raise RuntimeError("create mode")
 return fd
def copy_exact(src:Path,dst:Path,mode:int):
 i=os.open(src,os.O_RDONLY|getattr(os,"O_NOFOLLOW",0))
 try:o=secure_create(dst,mode)
 except Exception:os.close(i);raise
 try:
  while True:
   b=os.read(i,1<<20)
   if not b:break
   write_all(o,b)
  os.fsync(o)
 finally:os.close(i);os.close(o)
def fs_snapshot(root:Path)->dict[str,dict[str,Any]]:
 private_dir(root,"snapshot root");out={".":{"type":"directory","mode":stat.S_IMODE(root.lstat().st_mode)}};pending=[root];count=total=0
 while pending:
  parent=pending.pop()
  for e in sorted(os.scandir(parent),key=lambda x:x.name):
   count+=1
   if count>MAX_FILES:raise RuntimeError("snapshot entry budget")
   p=Path(e.path);s=e.stat(follow_symlinks=False);rel=p.relative_to(root).as_posix()
   if stat.S_ISDIR(s.st_mode):kind="directory";pending.append(p)
   elif stat.S_ISREG(s.st_mode):kind="file"
   elif stat.S_ISLNK(s.st_mode):kind="symlink"
   else:kind="other"
   row={"type":kind,"mode":stat.S_IMODE(s.st_mode)}
   if kind=="file":
    if s.st_nlink!=1 or s.st_size>MAX_FILE_BYTES:raise RuntimeError(f"unsafe fixture file:{rel}")
    total+=s.st_size
    if total>MAX_TREE_BYTES:raise RuntimeError("snapshot byte budget")
    row.update(sha256=file_digest(p),bytes=s.st_size)
   out[rel]=row
 return dict(sorted(out.items()))
def copy_fixture(src:Path,dst:Path):
 snap=fs_snapshot(src)
 if any(x["type"] not in {"directory","file"} for x in snap.values()):raise RuntimeError("fixture contains non-regular entry")
 dst.mkdir(mode=0o700)
 for rel,row in snap.items():
  if rel==".":continue
  target=dst/rel
  if row["type"]=="directory":target.mkdir(mode=0o700)
  else:copy_exact(src/rel,target,0o600)
 return snap
def sanitized_snapshot(snapshot):
 return {k:({"type":v["type"],"mode":v["mode"],"redacted":True} if Path(k).name=="auth.json" else v) for k,v in snapshot.items()}
def private_inventory(root:Path)->dict[str,dict[str,Any]]:
 private_dir(root,"private inventory root");out={".":{"type":"directory","mode":stat.S_IMODE(root.lstat().st_mode)}};pending=[root];count=total=0
 while pending:
  parent=pending.pop()
  for entry in sorted(os.scandir(parent),key=lambda x:x.name):
   count+=1
   if count>MAX_PRIVATE_FILES:raise RuntimeError("private inventory entry budget")
   path=Path(entry.path);mode=entry.stat(follow_symlinks=False).st_mode;rel=path.relative_to(root).as_posix()
   if stat.S_ISDIR(mode):
    row={"type":"directory","mode":stat.S_IMODE(mode)}
    if rel=="bin":row["contents_excluded"]="staged identities recorded separately"
    else:pending.append(path)
   elif stat.S_ISREG(mode):
    size=entry.stat(follow_symlinks=False).st_size
    if size>MAX_PRIVATE_FILE_BYTES:raise RuntimeError("private inventory file budget")
    total+=size
    if total>MAX_PRIVATE_TREE_BYTES:raise RuntimeError("private inventory byte budget")
    row={"type":"file","mode":stat.S_IMODE(mode),"bytes":size,"sha256":file_digest(path)}
   elif stat.S_ISLNK(mode):row={"type":"symlink","mode":stat.S_IMODE(mode)}
   else:row={"type":"other","mode":stat.S_IMODE(mode)}
   out[rel]=row
 return dict(sorted(out.items()))
def closed_bundle_digest(root:Path)->str:
 private_dir(root,"schema bundle root");contents={};pending=[root];count=total=0
 while pending:
  parent=pending.pop()
  for entry in sorted(os.scandir(parent),key=lambda x:x.name):
   count+=1
   if count>MAX_BUNDLE_FILES:raise RuntimeError("schema bundle entry budget")
   path=Path(entry.path);info=entry.stat(follow_symlinks=False);rel=path.relative_to(root).as_posix()
   if stat.S_ISDIR(info.st_mode):pending.append(path);continue
   if not stat.S_ISREG(info.st_mode) or info.st_nlink!=1 or info.st_size>MAX_BUNDLE_FILE_BYTES:raise RuntimeError(f"unsafe schema bundle entry:{rel}")
   total+=info.st_size
   if total>MAX_BUNDLE_BYTES:raise RuntimeError("schema bundle byte budget")
   contents[rel]=file_digest(path)
 return digest(enc(contents))
def schema_resolve(schema,root):
 seen=0
 while isinstance(schema,dict) and "$ref" in schema:
  ref=schema["$ref"]
  if not isinstance(ref,str) or not ref.startswith("#/"):raise RuntimeError("external schema ref")
  node=root
  for part in ref[2:].split("/"):node=node[part.replace("~1","/").replace("~0","~")]
  schema=node;seen+=1
  if seen>32:raise RuntimeError("schema ref depth")
 return schema
def schema_valid(value,schema,root,depth=0):
 if depth>64:raise RuntimeError("schema validation depth")
 if schema is True:return True
 if schema is False or not isinstance(schema,dict):return False
 schema=schema_resolve(schema,root)
 if "allOf" in schema and not all(schema_valid(value,x,root,depth+1) for x in schema["allOf"]):return False
 if "anyOf" in schema and not any(schema_valid(value,x,root,depth+1) for x in schema["anyOf"]):return False
 if "oneOf" in schema and sum(schema_valid(value,x,root,depth+1) for x in schema["oneOf"])!=1:return False
 if "enum" in schema and value not in schema["enum"]:return False
 if "const" in schema and value!=schema["const"]:return False
 types=schema.get("type");types=[types] if isinstance(types,str) else types
 if types:
  matches={"null":value is None,"object":isinstance(value,dict),"array":isinstance(value,list),"string":isinstance(value,str),"boolean":isinstance(value,bool),"integer":isinstance(value,int) and not isinstance(value,bool),"number":isinstance(value,(int,float)) and not isinstance(value,bool)}
  if not any(matches.get(x,False) for x in types):return False
 if isinstance(value,dict):
  required=schema.get("required",[])
  if any(k not in value for k in required):return False
  props=schema.get("properties",{})
  if schema.get("additionalProperties") is False and any(k not in props for k in value):return False
  if any(k in props and not schema_valid(v,props[k],root,depth+1) for k,v in value.items()):return False
 if isinstance(value,list) and "items" in schema and any(not schema_valid(x,schema["items"],root,depth+1) for x in value):return False
 return True
def schema_variant(schema,root,type_value,depth=0):
 if depth>32:return None
 schema=schema_resolve(schema,root)
 enums=find(schema.get("properties",{}).get("type",{}),"enum")
 if any(isinstance(x,list) and type_value in x for x in enums):return schema
 for key in ("oneOf","anyOf","allOf"):
  for child in schema.get(key,[]):
   found=schema_variant(child,root,type_value,depth+1)
   if found is not None:return found
 return None
def protocol_schemas(root:Path,want:str):
 actual=closed_bundle_digest(root)
 if actual!=want:raise RuntimeError("schema bundle digest")
 docs={name:loads_strict((root/"v2"/f"{name}.json").read_text(encoding="utf-8")) for name in ("ThreadStartParams","TurnStartParams")}
 turn=docs["TurnStartParams"];sandbox=schema_resolve(turn["properties"]["sandboxPolicy"]["anyOf"][0],turn);variants=sandbox.get("oneOf",[]);workspace=[x for x in variants if any(isinstance(v,list) and "workspaceWrite" in v for v in find(x,"enum"))]
 if len(workspace)!=1:raise RuntimeError("workspaceWrite subschema")
 return {"bundle_sha256":actual,"documents":docs,"workspace":workspace[0],"subschema_sha256":{k:digest(enc(v)) for k,v in docs.items()},"workspace_sha256":digest(enc(workspace[0]))}
def build_protocol_params(schemas,fixture:Path,prompt:str,p,thread_id=None):
 thread_schema=schemas["documents"]["ThreadStartParams"];turn_schema=schemas["documents"]["TurnStartParams"]
 thread={"model":p["model"],"ephemeral":True,"cwd":str(fixture),"approvalPolicy":"never","serviceName":"rust_m1_qualifier","baseInstructions":"Use only the configured MCP server; do not use shell, web, dynamic tools, or paths outside the workspace.","developerInstructions":"Follow the user task exactly.","config":{"model_reasoning_effort":p["effort"]}}
 sandbox_field=thread_schema.get("properties",{}).get("sandbox")
 if sandbox_field:
  selected=False
  for candidate in ("workspace-write","workspaceWrite"):
   if schema_valid(candidate,sandbox_field,thread_schema):thread["sandbox"]=candidate;selected=True;break
  if not selected:raise RuntimeError("generated thread sandbox has no workspace candidate")
 if not schema_valid(thread,thread_schema,thread_schema):raise RuntimeError("thread/start params violate generated schema")
 ww=schema_resolve(schemas["workspace"],turn_schema);props=ww.get("properties",{});policy={"type":"workspaceWrite"}
 if "writableRoots" in props:policy["writableRoots"]=[str(fixture)]
 if "networkAccess" in props:policy["networkAccess"]=False
 if "excludeTmpdirEnvVar" in props:policy["excludeTmpdirEnvVar"]=True
 if "excludeSlashTmp" in props:policy["excludeSlashTmp"]=True
 if "readOnlyAccess" in props:
  read_schema=schema_variant(props["readOnlyAccess"],turn_schema,"restricted")
  if read_schema is None:raise RuntimeError("restricted readOnlyAccess schema absent")
  read={"type":"restricted"};read_props=read_schema.get("properties",{})
  if "includePlatformDefaults" in read_props:read["includePlatformDefaults"]=False
  if "readableRoots" in read_props:read["readableRoots"]=[str(fixture)]
  if "include" in read_props:read["include"]=[str(fixture)]
  if "exclude" in read_props:read["exclude"]=[]
  policy["readOnlyAccess"]=read
 if not schema_valid(policy,ww,turn_schema):raise RuntimeError("workspaceWrite policy violates generated schema")
 turn={"threadId":thread_id or "THREAD_ID","input":[{"type":"text","text":prompt}],"cwd":str(fixture),"approvalPolicy":"never","sandboxPolicy":policy,"model":p["model"],"effort":p["effort"]}
 if not schema_valid(turn,turn_schema,turn_schema):raise RuntimeError("turn/start params violate generated schema")
 return thread,turn
def validate_schema_bundle(root:Path,want:str):
 schemas=protocol_schemas(root,want);actual=schemas["bundle_sha256"]
 thread,turn=schemas["documents"]["ThreadStartParams"],schemas["documents"]["TurnStartParams"];ww=schemas["workspace"];props=ww.get("properties",{})
 if "approvalPolicy" not in thread.get("properties",{}) or "approvalPolicy" not in turn.get("properties",{}) or "sandboxPolicy" not in turn.get("properties",{}) or not all(k in props for k in ("type","writableRoots","networkAccess","excludeTmpdirEnvVar","excludeSlashTmp")):raise RuntimeError("experimental schema lacks typed sandbox fields")
 return {"schema_bundle_sha256":actual,"validation":"protocol-schema/request evidence; app-server does not echo sandbox policy","required_fields_present":True,"subschema_sha256":schemas["subschema_sha256"],"workspace_sha256":schemas["workspace_sha256"]},schemas
def parse_server_args(args:Any,phase:str,fixture:Path,state:Path,plan=None,docker:Path|None=None)->list[str]:
 flags=REPAIR_FLAGS if phase=="repair" else MISSING_FLAGS
 if not isinstance(args,list) or args[:2]!=["serve","--stdio"] or not all(isinstance(x,str) and x for x in args) or len(args)!=2+2*len(flags) or tuple(args[2::2])!=flags:raise ValueError(f"{phase}:arg allowlist")
 vals=dict(zip(args[2::2],args[3::2]))
 if vals["--root"]!="$FIXTURE":raise ValueError(f"{phase}:fixture placeholder")
 if phase=="repair":
  if vals["--state-root"]!="$STATE" or not FP.fullmatch(vals["--rust-image"]):raise ValueError("repair:state/runtime")
  if vals["--docker"]!="$DOCKER" or plan and (vals["--docker-socket"]!=plan["docker_socket"] or vals["--rust-image"]!=plan["runtime"]["image_id"]):raise ValueError("repair:docker/runtime identity")
 return [str(fixture) if x=="$FIXTURE" else str(state) if x=="$STATE" else str(docker) if x=="$DOCKER" and docker else x for x in args]
def validate_numbers(p):
 budgets=p.get("budgets")
 if not isinstance(budgets,dict) or set(budgets)!={"missing_runtime","repair","cleanup_seconds"}:raise ValueError("budgets")
 for phase in ("missing_runtime","repair"):
  row=budgets.get(phase)
  if not isinstance(row,dict) or set(row)!={"wall_seconds","max_output_tokens","rpc_timeout_seconds"}:raise ValueError(f"budgets:{phase}")
  for k,lo,hi in (("wall_seconds",30,900),("max_output_tokens",1,16000),("rpc_timeout_seconds",1,60)):
   v=row.get(k)
   if isinstance(v,bool) or not isinstance(v,int) or not lo<=v<=hi:raise ValueError(f"{phase}:{k}")
 v=budgets.get("cleanup_seconds")
 if isinstance(v,bool) or not isinstance(v,int) or not 10<=v<=120:raise ValueError("cleanup_seconds")
def validate_plan(p:dict[str,Any],live=True):
 if set(p)!=PLAN_KEYS or p.get("schema_version")!=4:raise ValueError("closed plan v4")
 validate_numbers(p)
 if (p.get("model"),p.get("effort"),p.get("codex_version"))!=("gpt-5.6-sol","medium","codex-cli 0.153.0"):raise ValueError("identity")
 for k in ("codex_sha256","code_host_sha256","server_sha256","docker_sha256","schema_bundle_sha256"):
  if not HEX.fullmatch(str(p.get(k,""))):raise ValueError(k)
 paths={k:abs_path(p[k],k,k not in {"codex_source","code_host_source","server_source","docker_cli","docker_socket","auth_source"}) for k in ("codex_source","code_host_source","server_source","docker_cli","docker_socket","auth_source","private_root","output_root","fixture_root")}
 if p.get("docker_label")!=PRODUCT_DOCKER_LABEL:raise ValueError("docker_label")
 rt=p.get("runtime")
 if not isinstance(rt,dict) or set(rt)!={"image_id","rust_version","cargo_version","platform"} or not FP.fullmatch(str(rt.get("image_id",""))):raise ValueError("runtime")
 if p.get("missing_error_message")!=MISSING_RUNTIME_MESSAGE:raise ValueError("missing_error_message")
 layers=p.get("allowed_config_layer_types")
 if layers!=["builtIn","commandLine"]:raise ValueError("allowed_config_layer_types")
 if set(p.get("phases",{}))!={"repair","missing_runtime"}:raise ValueError("two phases")
 for n,tools in (("repair",REPAIR_TOOLS),("missing_runtime",MISSING_TOOLS)):
  x=p["phases"][n]
  if set(x)!={"prompt","prompt_sha256","server_args","enabled_tools","effective_config_sha256","descriptor_sha256"} or x["enabled_tools"]!=list(tools) or digest(x["prompt"].encode())!=x["prompt_sha256"] or not HEX.fullmatch(str(x["effective_config_sha256"])) or not HEX.fullmatch(str(x["descriptor_sha256"])):raise ValueError(n)
  prompt=x["prompt"].casefold();banned=(*TOOLS,"project_ref","E0502","apply_patch","rust_engineering","SANDBOX_DENIED",MISSING_RUNTIME_MESSAGE)
  if any(token.casefold() in prompt for token in banned):raise ValueError(f"{n}:prompt leaks expected evidence")
  parse_server_args(x["server_args"],n,paths["fixture_root"],paths["private_root"]/"placeholder",p)
 if "src/lib.rs" not in p.get("fixture_snapshot",{}) or any(k=="target" or k.startswith("target/") for k in p["fixture_snapshot"]):raise ValueError("fixture")
 world=p.get("feature_world")
 if not isinstance(world,dict) or not set(ENABLED).issubset(world) or not all(isinstance(k,str) and isinstance(v,bool) for k,v in world.items()) or not set(DISABLED).issubset(world):raise ValueError("feature_world")
 for name in ("repair","missing_runtime"):
  if p["phases"][name]["effective_config_sha256"]!=expected_effective_hash(p,name):raise ValueError(f"{name}:effective config approval")
 if live:
  private_dir(paths["private_root"],"private",True);private_dir(paths["output_root"],"output",True);private_dir(paths["fixture_root"],"fixture")
  safe_source(paths["codex_source"],p["codex_sha256"],True);safe_source(paths["code_host_source"],p["code_host_sha256"],True);safe_source(paths["server_source"],p["server_sha256"],True);safe_source(paths["docker_cli"],p["docker_sha256"],True)
  socket_info=paths["docker_socket"].lstat()
  if paths["docker_socket"].is_symlink() or not stat.S_ISSOCK(socket_info.st_mode) or socket_info.st_uid!=os.geteuid():raise ValueError("docker socket identity")
  s=paths["auth_source"].lstat()
  if paths["auth_source"].is_symlink() or not stat.S_ISREG(s.st_mode) or s.st_uid!=os.geteuid() or stat.S_IMODE(s.st_mode)!=0o600 or paths["auth_source"].name!="auth.json":raise ValueError("auth material")
  if fs_snapshot(paths["fixture_root"])!=p["fixture_snapshot"]:raise ValueError("fixture identity")
  r=subprocess.run([str(paths["codex_source"]),"--version"],capture_output=True,text=True,check=True,timeout=10,env={"PATH":"/usr/bin:/bin","LC_ALL":"C","LANG":"C"})
  if r.stdout.strip()!=p["codex_version"]:raise ValueError("version")
 return paths
def toml(v):
 if isinstance(v,bool):return str(v).lower()
 if isinstance(v,str):return json.dumps(v)
 if isinstance(v,list):return "["+",".join(toml(x) for x in v)+"]"
 if isinstance(v,dict):return "{"+",".join(f"{k}={toml(x)}" for k,x in v.items())+"}"
 if isinstance(v,int):return str(v)
 raise TypeError(type(v).__name__)
def configured_features(p):
 return {name:name in ENABLED for name in sorted(p["feature_world"])}
def overrides(server,args,enabled,p):
 m={"command":str(server),"args":args,"cwd":"/","env":{},"env_vars":[],"enabled":True,"required":True,"enabled_tools":list(enabled),"disabled_tools":[x for x in TOOLS if x not in enabled],"startup_timeout_sec":45,"tool_timeout_sec":300,"default_tools_approval_mode":"approve"}
 o={"mcp_servers":{"rust_engineering":m},"model":p["model"],"model_reasoning_effort":p["effort"],"model_provider":"openai","web_search":"disabled","sandbox_mode":"workspace-write","approval_policy":"never","project_doc_max_bytes":0,"project_doc_fallback_filenames":[],"developer_instructions":"","instructions":"","notify":[]}
 for name,value in configured_features(p).items():o[f"features.{name}"]=value
 o.update({"agents.enabled":False,"orchestrator.skills.enabled":False,"orchestrator.mcp.enabled":False,"skills.include_instructions":False,"skills.bundled.enabled":False});return o
def normalized_effective(p,phase):
 enabled=REPAIR_TOOLS if phase=="repair" else MISSING_TOOLS;m={"command":"$SERVER","args":p["phases"][phase]["server_args"],"cwd":"/","env":{},"env_vars":[],"enabled":True,"required":True,"enabled_tools":list(enabled),"disabled_tools":[x for x in TOOLS if x not in enabled],"startup_timeout_sec":45,"tool_timeout_sec":300,"default_tools_approval_mode":"approve"}
 return {"server":m,"model":p["model"],"effort":p["effort"],"provider":"openai","web":"disabled","sandbox_mode":"workspace-write","approval_policy":"never","notify":[],"project_doc_max_bytes":0,"project_doc_fallback_filenames":[],"developer_instructions":"","instructions":"","agents":{"enabled":False},"orchestrator":{"skills":{"enabled":False},"mcp":{"enabled":False}},"skills":{"include_instructions":False,"bundled":{"enabled":False}},"allowed_config_layer_types":p["allowed_config_layer_types"],"features":configured_features(p)}
def expected_effective_hash(p,phase):return digest(enc(normalized_effective(p,phase)))
def discover_features(codex,home,temp):
 r=subprocess.run([str(codex),"features","list"],capture_output=True,text=True,timeout=10,env={"HOME":str(home),"CODEX_HOME":str(home),"TMPDIR":str(temp),"PATH":"/usr/bin:/bin","LC_ALL":"C","LANG":"C"})
 if r.returncode:raise RuntimeError("feature discovery")
 out={}
 for line in r.stdout.splitlines():
  parts=line.split()
  if len(parts)<3 or parts[-1] not in {"true","false"}:raise RuntimeError("feature discovery format")
  out[parts[0]]=parts[-1]=="true"
 if not out:raise RuntimeError("feature world empty")
 return out
def find(v,key,budget=None):
 budget=budget or [10000];budget[0]-=1
 if budget[0]<0:raise RuntimeError("nested data budget")
 out=[]
 if isinstance(v,dict):
  for k,x in v.items():
   if k==key:out.append(x)
   out+=find(x,key,budget)
 elif isinstance(v,list):
  for x in v:out+=find(x,key,budget)
 return out
def output_payload(item):
 r=item.get("result")
 if not isinstance(r,dict) or not isinstance(r.get("structuredContent"),dict) or set(r["structuredContent"])!=OUTPUT_KEYS:raise RuntimeError("structured output keys")
 return r["structuredContent"]

class Transcript:
 def __init__(self,path):self.fd=secure_create(path);self.start=time.monotonic();self.total=0;self.lock=threading.Lock();self.closed=False;self.needles=[]
 def set_needles(self,needles):
  with self.lock:self.needles=needle_variants(needles)
 def add(self,phase,kind,data):
  b=enc({"elapsed_seconds":round(time.monotonic()-self.start,3),"phase":phase,"kind":kind,"data":data})+b"\n"
  with self.lock:
   if self.closed:return
   if any(n and n in b for n in self.needles):raise RuntimeError("secret rejected before transcript write")
   self.total+=len(b)
   if self.total>32<<20:raise RuntimeError("transcript budget")
   write_all(self.fd,b)
 def close(self):
  with self.lock:
   if not self.closed:os.fsync(self.fd);os.close(self.fd);self.closed=True

class TransportCloseError(RuntimeError):pass
def require_transport_closed(name,result):
 if not isinstance(result,dict) or result.get("exit_code") or result.get("forced") or not result.get("threads_joined") or result.get("remaining_pids") or result.get("failure"):raise TransportCloseError(f"{name}:transport cleanup invalid")
def process_executable(pid):
 if sys.platform=="darwin":
  libproc=ctypes.CDLL("/usr/lib/libproc.dylib",use_errno=True);buf=ctypes.create_string_buffer(4096);size=libproc.proc_pidpath(pid,buf,len(buf))
  if size<=0:
   error=ctypes.get_errno()
   if error==3:raise ProcessLookupError(pid)
   raise OSError(error or 5,"proc_pidpath",pid)
  return str(Path(os.fsdecode(buf.value)))
 if sys.platform.startswith("linux"):return os.readlink(f"/proc/{pid}/exe")
 raise RuntimeError("process executable identity unsupported")
def process_is_live(pid):
 try:os.kill(pid,0);return True
 except ProcessLookupError:return False
 except PermissionError:return True
class Transport:
 def __init__(self,phase,cmd,cwd,home,temp,log,deadline,rpc_timeout=30,allowed_executables=None,required_role=None):
  self.phase=phase;self.log=log;self.deadline=deadline;self.rpc_timeout=rpc_timeout;self.q=queue.Queue();self.events=queue.Queue();self.n=0;self.requests={};self.failure=None;self.stderr=bytearray();self.observed=set();self.process_identities={};self.allowed_executables={str(Path(k)):v for k,v in (allowed_executables or {}).items()};self.required_role=required_role;self.stop=threading.Event();self.closed=False;self.close_result=None;self.close_lock=threading.Lock()
  env={"HOME":str(home),"CODEX_HOME":str(home),"TMPDIR":str(temp),"PATH":str(Path(cmd[0]).parent)+":/usr/bin:/bin:/usr/sbin:/sbin","LC_ALL":"C","LANG":"C","PYTHONDONTWRITEBYTECODE":"1"}
  self.p=subprocess.Popen(cmd,cwd=cwd,env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,start_new_session=True,bufsize=0)
  try:
   os.set_blocking(self.p.stdin.fileno(),False);self.pgid=os.getpgid(self.p.pid)
   if self.pgid!=self.p.pid:raise RuntimeError("process group ownership")
   self.threads=[threading.Thread(target=self._out,daemon=True),threading.Thread(target=self._err,daemon=True),threading.Thread(target=self._monitor,daemon=True)]
   for x in self.threads:x.start()
  except Exception:
   try:os.killpg(self.p.pid,signal.SIGKILL)
   except ProcessLookupError:pass
   self.p.wait(timeout=5)
   for stream in (self.p.stdin,self.p.stdout,self.p.stderr):
    if stream and not stream.closed:stream.close()
   raise
  log.add(phase,"process",{"pid":self.p.pid,"argv_sha256":digest(enc(cmd)),"env_keys":sorted(env)})
 def remaining(self,ceiling=30):
  v=min(ceiling,self.deadline-time.monotonic())
  if v<=0:raise TimeoutError("global wall timeout")
  return v
 def _out(self):
  buf=bytearray()
  try:
   fd=self.p.stdout.fileno()
   while True:
    b=os.read(fd,65536)
    if not b:break
    buf.extend(b)
    if len(buf)>MAX_LINE_BYTES:raise RuntimeError("line budget")
    while b"\n" in buf:
     raw,_,tail=buf.partition(b"\n");buf=bytearray(tail);m=loads_strict(raw);method=self.requests.get(m.get("id"));data={"id":m.get("id"),"response_sha256":digest(enc(m))} if method in {"config/read","mcpServerStatus/list"} else m;self.log.add(self.phase,"app_server",data);(self.q if "id" in m and ("result" in m or "error" in m) else self.events).put(m)
   if buf:raise RuntimeError("unterminated JSON line")
  except Exception as e:self.failure=f"stdout:{e}";self.events.put(None)
 def _err(self):
  try:
   while True:
    b=os.read(self.p.stderr.fileno(),65536)
    if not b:break
    self.stderr+=b
    if len(self.stderr)>2<<20:raise RuntimeError("stderr budget")
  except Exception as e:self.failure=self.failure or f"stderr:{e}"
 @staticmethod
 def _rows():
  r=subprocess.run(["/bin/ps","-ww","-axo","pid=,ppid=,pgid=,args="],capture_output=True,text=True,check=True,timeout=5);out=[]
  for line in r.stdout.splitlines():
   parts=line.strip().split(None,3)
   if len(parts)==4:out.append((int(parts[0]),int(parts[1]),int(parts[2]),parts[3]))
  return out
 def _monitor(self):
  try:
   while not self.stop.wait(.2):
    rows=self._rows();owned={pid for pid,_,pgid,_ in rows if pgid==self.pgid};owned.add(self.p.pid);changed=True
    while changed:
     changed=False
     for pid,ppid,_,_ in rows:
      if ppid in owned and pid not in owned:owned.add(pid);changed=True
    if self.allowed_executables:
     commands={pid:command for pid,_,_,command in rows}
     for pid in owned-{self.p.pid}:
      command=commands.get(pid)
      if not command:continue
      try:executable=process_executable(pid)
      except OSError as e:
       if process_is_live(pid):raise RuntimeError(f"live descendant executable unresolved:{pid}:{type(e).__name__}") from e
       continue
      approval=self.allowed_executables.get(executable)
      if approval is None:raise RuntimeError(f"unexpected descendant executable:{Path(executable).name}:{digest(executable.encode())}")
      role,expected_sha256=approval;actual_sha256=file_digest(Path(executable))
      if actual_sha256!=expected_sha256:raise RuntimeError(f"descendant executable identity:{role}")
      self.process_identities[pid]={"role":role,"executable_sha256":actual_sha256,"argv_sha256":digest(command.encode())}
    self.observed|=owned
  except Exception as e:self.failure=self.failure or f"monitor:{e}"
 def send(self,obj):
  self.log.add(self.phase,"client_request",{"method":obj.get("method"),"id":obj.get("id"),"params_sha256":digest(enc(obj.get("params",{})))})
  v=memoryview(enc(obj)+b"\n")
  while v:
   self.remaining(5)
   try:v=v[os.write(self.p.stdin.fileno(),v):]
   except BlockingIOError:time.sleep(.01)
 def rpc(self,method,params,ceiling=None):
  ceiling=self.rpc_timeout if ceiling is None else min(ceiling,self.rpc_timeout)
  self.n+=1;ident=self.n;self.requests[ident]=method;self.send({"method":method,"id":ident,"params":params});end=time.monotonic()+self.remaining(ceiling);park=[]
  try:
   while time.monotonic()<end:
    try:m=self.q.get(timeout=min(.5,end-time.monotonic()))
    except queue.Empty:continue
    if m.get("id")==ident:
     if "error" in m:raise RuntimeError(f"{method}:{m['error']}")
     return m.get("result")
    park.append(m)
   raise TimeoutError(f"RPC timeout:{method}")
  finally:
   for m in park:self.q.put(m)
 def close(self,forced=False):
  with self.close_lock:
   if self.closed:return self.close_result
   if self.p.poll() is None:
    try:self.p.stdin.close();self.p.wait(timeout=min(10,max(.1,self.deadline-time.monotonic())))
    except Exception:
     forced=True
     try:os.killpg(self.p.pid,signal.SIGKILL)
     except ProcessLookupError:pass
     self.p.wait(timeout=5)
   self.stop.set()
   for x in self.threads:x.join(3)
   live={x for x,_,_,_ in self._rows()};before_kill=sorted((self.observed-{self.p.pid})&live)
   for pid in before_kill:
    try:
     if os.getpgid(pid)!=self.pgid:self.failure=self.failure or f"foreign or reused pid:{pid}";continue
     os.kill(pid,signal.SIGKILL)
    except ProcessLookupError:pass
    except PermissionError:self.failure=self.failure or f"foreign or reused pid:{pid}"
   time.sleep(.05);live={x for x,_,_,_ in self._rows()};remaining=sorted((self.observed-{self.p.pid})&live)
   roles=sorted(x["role"] for x in self.process_identities.values())
   if self.required_role and self.required_role not in roles:self.failure=self.failure or f"required descendant not observed:{self.required_role}"
   for s in (self.p.stdout,self.p.stderr):
    if s and not s.closed:s.close()
   identities=sorted(self.process_identities.values(),key=lambda x:(x["role"],x["argv_sha256"]));result={"exit_code":self.p.returncode,"forced":forced,"pgid":self.pgid,"threads_joined":all(not x.is_alive() for x in self.threads),"remaining_pids_observed_before_kill":before_kill,"remaining_pids":remaining,"stderr_bytes":len(self.stderr),"stderr_sha256":digest(bytes(self.stderr)),"failure":self.failure,"observed_pids":sorted(self.observed),"descendant_identities":identities,"code_host_observed":self.required_role in roles if self.required_role else None};self.close_result=result;self.closed=True;self.log.add(self.phase,"cleanup",result);return result

def validate_model(t,p):
 r=t.rpc("model/list",{"limit":100,"includeHidden":True});m=[x for x in r.get("data",[]) if x.get("id")==p["model"]]
 if len(m)!=1 or p["effort"] not in [x.get("reasoningEffort") for x in m[0].get("supportedReasoningEfforts",[])]:raise RuntimeError("model identity")
 return {"model":p["model"],"effort":p["effort"],"entry_sha256":digest(enc(m[0]))}
def validate_effective(t,p,server,args,enabled,fixture,phase):
 r=t.rpc("config/read",{"includeLayers":True,"cwd":str(fixture)});cfg=r.get("config",{});layers=r.get("layers",[]);expected=overrides(server,args,enabled,p);actual=cfg.get("mcp_servers",{})
 if set(actual)!={"rust_engineering"} or not isinstance(actual["rust_engineering"],dict):raise RuntimeError("effective MCP drift")
 observed_server=dict(actual["rust_engineering"])
 if observed_server.pop("environment_id",None) not in (None,"local"):raise RuntimeError("effective MCP environment drift")
 observed_server.setdefault("env_vars",[])
 if observed_server!=expected["mcp_servers"]["rust_engineering"]:raise RuntimeError("effective MCP drift")
 for k in ("model","model_reasoning_effort","model_provider","web_search","sandbox_mode","approval_policy","notify","project_doc_max_bytes","project_doc_fallback_filenames","developer_instructions","instructions"):
  if cfg.get(k)!=expected[k]:raise RuntimeError(f"effective config drift:{k}")
 features=cfg.get("features",{})
 feature_config=configured_features(p)
 if not isinstance(features,dict) or set(features)-set(feature_config) or any(features.get(k)!=v for k,v in feature_config.items() if k in features) or any(v and k not in features for k,v in feature_config.items()):raise RuntimeError("effective feature world drift")
 if cfg.get("agents",{}).get("enabled") is not False or cfg.get("orchestrator",{}).get("skills",{}).get("enabled") is not False or cfg.get("orchestrator",{}).get("mcp",{}).get("enabled") is not False or cfg.get("skills",{}).get("include_instructions") is not False or cfg.get("skills",{}).get("bundled",{}).get("enabled") is not False:raise RuntimeError("effective auxiliary capability drift")
 normalized_layers=[]
 for layer in layers:
  if not isinstance(layer,dict):raise RuntimeError("unexpected config layer")
  if "type" in layer:
   typ=layer.get("type")
   if typ not in {"builtIn","commandLine"}:raise RuntimeError("unexpected config layer")
  else:
   name=layer.get("name")
   raw=name.get("type") if isinstance(name,dict) else None
   if raw=="sessionFlags":typ="commandLine"
   elif raw in {"project","user","system"}:typ="builtIn"
   else:raise RuntimeError("unexpected config layer")
   layer_config=layer.get("config")
   if raw=="sessionFlags" and not isinstance(layer_config,dict):raise RuntimeError("session config layer")
   if raw in {"user","system"} and layer_config!={}:raise RuntimeError("ambient config layer")
   if raw=="project" and (not isinstance(layer.get("disabledReason"),str) or not layer["disabledReason"]):raise RuntimeError("active project config layer")
  normalized_layers.append(typ)
 if not set(normalized_layers).issubset(set(p["allowed_config_layer_types"])) or "commandLine" not in normalized_layers:raise RuntimeError("unexpected config layer")
 server_safe={k:observed_server[k] for k in ("command","args","cwd","env","env_vars","enabled","required","enabled_tools","disabled_tools","default_tools_approval_mode","startup_timeout_sec","tool_timeout_sec")}
 for key in ("startup_timeout_sec","tool_timeout_sec"):
  if isinstance(server_safe[key],bool) or not isinstance(server_safe[key],(int,float)) or int(server_safe[key])!=server_safe[key]:raise RuntimeError("effective MCP timeout drift")
  server_safe[key]=int(server_safe[key])
 safe={**server_safe,"model":cfg["model"],"effort":cfg["model_reasoning_effort"],"provider":cfg["model_provider"],"web":cfg["web_search"],"sandbox_mode":cfg["sandbox_mode"],"approval_policy":cfg["approval_policy"],"notify":cfg["notify"],"project_doc_max_bytes":cfg["project_doc_max_bytes"],"project_doc_fallback_filenames":cfg["project_doc_fallback_filenames"],"developer_instructions":cfg["developer_instructions"],"instructions":cfg["instructions"],"agents":{"enabled":cfg["agents"]["enabled"]},"orchestrator":cfg["orchestrator"],"skills":cfg["skills"],"allowed_config_layer_types":p["allowed_config_layer_types"],"features":feature_config,"observed_feature_world_sha256":digest(enc(features))}
 normalized=normalized_effective(p,phase);actual_normalized={k:safe[k] for k in ("model","effort","provider","web","sandbox_mode","approval_policy","notify","project_doc_max_bytes","project_doc_fallback_filenames","developer_instructions","instructions","agents","orchestrator","skills","allowed_config_layer_types","features")};actual_normalized["server"]={**server_safe,"command":"$SERVER","args":p["phases"][phase]["server_args"]};approved=digest(enc(actual_normalized))
 if actual_normalized!=normalized:raise RuntimeError("approved effective config structure")
 if approved!=p["phases"][phase]["effective_config_sha256"]:raise RuntimeError(f"approved effective config hash:{approved}")
 return {"sanitized":safe,"approved_effective_sha256":approved,"layers_sha256":digest(enc(layers)),"layer_count":len(layers)}
def validate_inventory(t,p,phase,expected_tools):
 r=t.rpc("mcpServerStatus/list",{"cursor":None,"limit":100});rows=r.get("data",[])
 if r.get("nextCursor") is not None or len(rows)!=1 or rows[0].get("name")!="rust_engineering" or set(rows[0].get("tools",{}))!=set(expected_tools):raise RuntimeError("inventory")
 status=rows[0].get("status",rows[0].get("runtimeStatus"));state=status.get("state") if isinstance(status,dict) else status
 if state not in {None,"connected","ready","healthy"}:raise RuntimeError("MCP server not healthy")
 d=digest(enc(rows[0]["tools"]))
 if d!=p["phases"][phase]["descriptor_sha256"]:raise RuntimeError("phase descriptor digest")
 return {"tools":sorted(expected_tools),"descriptor_sha256":d,"descriptor_sha256_approved":p["phases"][phase]["descriptor_sha256"],"server_status":state,"server_info":rows[0].get("serverInfo")}
def common(item,status):
 expected_lifecycle="failed" if status=="blocked" else "completed"
 if item.get("status")!=expected_lifecycle or item.get("error") is not None:raise RuntimeError("MCP lifecycle")
 p=output_payload(item)
 if p["status"]!=status:raise RuntimeError("status")
 return p
def safe_call(item):
 a=item.get("arguments");selected={}
 if isinstance(a,dict):
  if isinstance(a.get("path"),str):selected["path_basename"]=Path(a["path"]).name
  if "project_ref" in a:selected["project_ref_sha256"]=digest(str(a["project_ref"]).encode())
 return {"tool":item.get("tool"),"arguments_sha256":digest(enc(a)),"safe_arguments":selected,"result_sha256":digest(enc(item.get("result")))}
def native_discovery(item):return item.get("server") in {"codex","rust_engineering"} and item.get("tool") in {"list_mcp_resources","list_mcp_resource_templates"}
def native_resource_read(item):return item.get("server")=="rust_engineering" and item.get("tool")=="read_mcp_resource"
def validate_native_discovery(item):
 expected_args={} if item.get("server")=="codex" else {"server":"rust_engineering"}
 if item.get("status")!="completed" or item.get("error") is not None or item.get("arguments")!=expected_args:raise RuntimeError("native discovery lifecycle")
 result=item.get("result",{});content=result.get("content") if isinstance(result,dict) else None
 if result.get("structuredContent") is not None or not isinstance(content,list) or len(content)!=1 or content[0].get("type")!="text":raise RuntimeError("native discovery result")
 parsed=loads_strict(content[0].get("text", ""))
 key="resources" if item.get("tool")=="list_mcp_resources" else "resourceTemplates";expected={key:[]}
 if item.get("server")=="rust_engineering":expected={"server":"rust_engineering",**expected}
 if parsed!=expected:raise RuntimeError("native discovery scope")
def validate_native_resource_read(item,approved_uris):
 args=item.get("arguments")
 if not isinstance(args,dict) or set(args)!={"server","uri"} or args.get("server")!="rust_engineering" or args.get("uri") not in approved_uris:raise RuntimeError("native resource read scope")
 if item.get("status")=="inProgress":return
 if item.get("status")!="completed" or item.get("error") is not None:raise RuntimeError("native resource read lifecycle")
 result=item.get("result")
 if not isinstance(result,dict) or result.get("structuredContent") is not None:raise RuntimeError("native resource read result")
 content=result.get("content")
 if not isinstance(content,list) or len(content)!=1 or content[0].get("type") not in {"text","resource"}:raise RuntimeError("native resource read content")
 if content[0]["type"]=="text":
  parsed=loads_strict(content[0].get("text", ""))
  if not isinstance(parsed,dict) or set(parsed)!={"server","uri","ttlMs","cacheScope","contents"} or parsed.get("server")!="rust_engineering" or parsed.get("uri")!=args["uri"] or parsed.get("ttlMs")!=0 or parsed.get("cacheScope")!="private":raise RuntimeError("native resource read text envelope")
  contents=parsed.get("contents")
  if not isinstance(contents,list) or len(contents)!=1:raise RuntimeError("native resource read text contents")
  resource=contents[0]
  if not isinstance(resource,dict) or set(resource)!={"uri","mimeType","blob","_meta"} or resource.get("uri")!=args["uri"] or resource.get("mimeType")!="application/octet-stream":raise RuntimeError("native resource read blob")
  meta=resource.get("_meta")
  if not isinstance(meta,dict) or set(meta)!={"retention_remaining_seconds","sha256","size_bytes","truncated"} or not isinstance(meta.get("retention_remaining_seconds"),int) or meta["retention_remaining_seconds"]<0 or meta.get("truncated") is not False or not HEX.fullmatch(str(meta.get("sha256",""))) or not isinstance(meta.get("size_bytes"),int) or meta["size_bytes"]<1:raise RuntimeError("native resource read metadata")
  try:decoded=base64.b64decode(resource.get("blob",""),validate=True)
  except (binascii.Error,ValueError) as e:raise RuntimeError("native resource read encoding") from e
  if len(decoded)!=meta["size_bytes"] or digest(decoded)!=meta["sha256"]:raise RuntimeError("native resource read integrity")
 else:
  resource=content[0].get("resource")
  if not isinstance(resource,dict) or resource.get("uri")!=args["uri"] or not isinstance(resource.get("text"),str) or not resource["text"]:raise RuntimeError("native resource read resource")
def validate_runtime(payload,expected):
 data=payload.get("data");runtime=data.get("runtime") if isinstance(data,dict) else None
 if not isinstance(runtime,dict):raise RuntimeError("runtime evidence absent")
 for k in ("image_id","rust_version","cargo_version","platform"):
  if runtime.get(k)!=expected[k]:raise RuntimeError(f"runtime evidence:{k}")
 for k in ("configuration_fingerprint","execution_fingerprint"):
  if not FP.fullmatch(str(runtime.get(k,""))):raise RuntimeError(f"runtime evidence:{k}")
 return {k:runtime[k] for k in ("image_id","rust_version","cargo_version","platform","configuration_fingerprint","execution_fingerprint")}
def parse_patch_input(raw):
 for k in ("id","call_id","input"):
  if not isinstance(raw.get(k),str) or not raw[k]:raise RuntimeError(f"raw patch {k}")
 if raw.get("status")!="completed" or raw.get("name")!="apply_patch":raise RuntimeError("raw patch lifecycle")
 try:decoded=loads_strict(raw["input"])
 except json.JSONDecodeError as e:raise RuntimeError("raw patch input JSON") from e
 patch=decoded.get("patch") if isinstance(decoded,dict) else None
 if re.search(r"^\*\*\* Move to:",patch or "",re.MULTILINE):raise RuntimeError("raw patch move rejected")
 paths=re.findall(r"^\*\*\* Update File: (.+)$",patch or "",re.MULTILINE)
 if paths!=["src/lib.rs"] or any(Path(x).is_absolute() or ".." in Path(x).parts for x in paths):raise RuntimeError("raw patch confinement")
 return {"id":raw["id"],"call_id":raw["call_id"],"input_sha256":digest(raw["input"].encode()),"patch":patch}
def apply_update_patch(source:str,patch:str)->str:
 lines=patch.splitlines()
 if len(lines)<5 or lines[0]!="*** Begin Patch" or lines[-1]!="*** End Patch" or lines[1]!="*** Update File: src/lib.rs":raise RuntimeError("raw patch envelope")
 body=lines[2:-1];hunks=[];current=None
 for line in body:
  if line.startswith("@@"):
   current=[];hunks.append(current);continue
  if current is None:raise RuntimeError("raw patch hunk absent")
  if line=="\\ No newline at end of file":continue
  if not line or line[0] not in " +-":raise RuntimeError("raw patch line")
  current.append((line[0],line[1:]))
 if not hunks or any(not h for h in hunks):raise RuntimeError("raw patch empty hunk")
 source_lines=source.splitlines();trailing=source.endswith("\n");cursor=0
 for hunk in hunks:
  old=[text for op,text in hunk if op in " -"];new=[text for op,text in hunk if op in " +"]
  if not old:raise RuntimeError("raw patch insertion unsupported")
  matches=[i for i in range(cursor,len(source_lines)-len(old)+1) if source_lines[i:i+len(old)]==old]
  if len(matches)!=1:raise RuntimeError("raw patch does not bind uniquely to source")
  pos=matches[0];source_lines[pos:pos+len(old)]=new;cursor=pos+len(new)
 return "\n".join(source_lines)+("\n" if trailing else "")
def patch_from_file_change(item,fixture):
 changes=item.get("changes")
 if item.get("status")!="completed" or not item.get("id") or not isinstance(changes,list) or len(changes)!=1:raise RuntimeError("fileChange sequence")
 change=changes[0];kind=change.get("kind");path=change.get("path");diff=change.get("diff")
 if path not in {"src/lib.rs",str(fixture/"src/lib.rs")} or not isinstance(kind,dict) or set(kind)-{"type","move_path"} or kind.get("type")!="update" or kind.get("move_path") is not None or not isinstance(diff,str) or not diff.startswith("@@") or not diff.endswith("\n"):raise RuntimeError("fileChange confinement")
 return "*** Begin Patch\n*** Update File: src/lib.rs\n"+diff+"*** End Patch"
def phase_state(name,items,actions,p,fixture):
 stage=0;first=current=None;calls=[];changes=plans=probes=post_inspects=0;runtime=None;file_patch=None
 for item in items:
  if item["type"]=="plan":
   if name!="repair" or item.get("status") not in (None,"completed") or not isinstance(item.get("text"),str) or not item["text"]:raise RuntimeError("model plan lifecycle")
   plans+=1;continue
  if item["type"]=="fileChange":
   if name!="repair" or stage!=3:raise RuntimeError("fileChange sequence")
   file_patch=patch_from_file_change(item,fixture)
   changes+=1;stage=4;continue
  if item["type"]!="mcpToolCall":continue
  if item.get("server")!="rust_engineering":raise RuntimeError("server")
  tool=item.get("tool");calls.append(safe_call(item))
  if tool=="rust.project.open" and stage==0 and item.get("arguments")=={"path":"."}:
   x=common(item,"blocked")
   if probes or x.get("error_code")!="INVALID_PROJECT" or x.get("error_message")!="Project structure is invalid or unsupported" or x.get("data") is not None or x.get("evidence")!={"kind":"local"}:raise RuntimeError("relative project probe contract")
   probes+=1;continue
  if tool=="rust.project.open" and stage in ({0,4} if name=="repair" else {0}):
   if item.get("arguments",{}).get("path")!=str(fixture):raise RuntimeError("project.open path")
   refs=find(common(item,"passed"),"project_ref")
   if len(refs)!=1:raise RuntimeError("project_ref")
   if stage==0:first=refs[0]
   elif refs[0]==first:raise RuntimeError("snapshot rotation")
   current=refs[0];stage=1 if name=="missing_runtime" or current==first else 5
  elif name=="repair" and tool=="rust.project.inspect" and stage==1:
   if item.get("arguments",{}).get("project_ref")!=current:raise RuntimeError("inspect project_ref")
   common(item,"passed");stage=2
  elif name=="repair" and tool=="rust.check" and stage==2:
   if item.get("arguments",{}).get("project_ref")!=current or "E0502" not in {str(x) for x in find(common(item,"failed"),"code")}:raise RuntimeError("E0502")
   stage=3
  elif name=="repair" and tool=="rust.project.inspect" and stage==5:
   if post_inspects or item.get("arguments",{}).get("project_ref")!=current:raise RuntimeError("post-repair inspect project_ref")
   common(item,"passed");post_inspects+=1
  elif name=="repair" and tool in {"rust.check","rust.quality.gate"} and stage==5:
   if item.get("arguments",{}).get("project_ref")!=current:raise RuntimeError("repair project_ref")
   runtime=validate_runtime(common(item,"passed"),p["runtime"]);stage=6
  elif name=="missing_runtime" and tool=="rust.check" and stage==1:
   if item.get("arguments",{}).get("project_ref")!=current:raise RuntimeError("missing project_ref")
   x=common(item,"blocked")
   if x.get("error_code")!="SANDBOX_DENIED" or x.get("error_message")!=p["missing_error_message"] or x.get("data") is not None or x.get("evidence")!={"kind":"local"}:raise RuntimeError("product runtime unconfigured contract")
   stage=2
  else:raise RuntimeError(f"sequence:{tool}:{stage}")
 if stage!=(6 if name=="repair" else 2):raise RuntimeError("phase incomplete")
 patches=[x for x in actions if x["kind"]=="raw_patch_call"]
 if name=="missing_runtime" and patches:raise RuntimeError("missing phase raw patch")
 if name=="repair":
  kinds=[x["kind"] for x in actions if x["kind"] not in {"raw:reasoning","raw:message"}]
  try:eidx=kinds.index("e0502");fidx=kinds.index("fileChange")
  except ValueError as e:raise RuntimeError("raw patch order/count") from e
  raw_ok=False
  if patches:
   try:cidx=kinds.index("raw_patch_call");oidx=kinds.index("raw_patch_output")
   except ValueError as e:raise RuntimeError("raw patch order/count") from e
   raw_ok=len(patches)==1 and kinds.count("raw_patch_output")==1 and eidx<cidx and oidx==cidx+1 and fidx==oidx+1 and patches[0].get("patch")==file_patch
  elif kinds.count("raw_patch_output")==0:raw_ok=eidx<fidx
  if kinds.count("raw:mcp_list_tools")>1 or ("raw:mcp_list_tools" in kinds and kinds.index("raw:mcp_list_tools")>kinds.index("item:mcpToolCall")) or changes!=1 or not raw_ok or kinds.count("fileChange")!=1:raise RuntimeError("raw discovery/patch order/count")
 if name=="missing_runtime":
  kinds=[x["kind"] for x in actions]
  if kinds.count("raw:mcp_list_tools")>1 or ("raw:mcp_list_tools" in kinds and kinds.index("raw:mcp_list_tools")>kinds.index("item:mcpToolCall")):raise RuntimeError("raw discovery duplicate or late")
 selected_patch=patches[0].get("patch") if patches else file_patch
 return {"claim":"product_runtime_unconfigured" if name=="missing_runtime" else "candidate_runtime_repair","calls":calls,"model_plan_item_count":plans,"relative_project_probe_count":probes,"post_repair_inspect_count":post_inspects,"first_ref_sha256":digest(str(first).encode()),"final_ref_sha256":digest(str(current).encode()),"file_changes":changes,"patch_evidence":"raw_apply_patch" if patches else ("fileChange" if file_patch else None),"raw_apply_patch":len(patches),"raw_patch_input_sha256":patches[0].get("input_sha256") if patches else (digest(selected_patch.encode()) if selected_patch else None),"_raw_patch":selected_patch,"runtime":runtime}

def run_phase(name,p,codex,server,docker,home,temp,fixture,state,log,deadline,registry,schemas):
 if name=="repair" and not state.is_relative_to(fixture.parent):raise ValueError("state-root outside owned work root")
 x=p["phases"][name];enabled=REPAIR_TOOLS if name=="repair" else MISSING_TOOLS;args=parse_server_args(x["server_args"],name,fixture,state,p,docker);cmd=[str(codex),"app-server","--stdio","--strict-config"]
 for k,v in overrides(server,args,enabled,p).items():cmd += ["-c",f"{k}={toml(v)}"]
 normalized=[]
 for value in cmd:
  item=value.replace(str(codex),"$CODEX").replace(str(server),"$SERVER").replace(str(docker),"$DOCKER").replace(str(fixture),"$FIXTURE").replace(str(state),"$STATE")
  normalized.append(item)
 log.add(name,"normalized_argv",normalized)
 executable_allowlist={str(codex):("codex",p["codex_sha256"]),str(server):("rust-engineering-mcp",p["server_sha256"]),str(codex.with_name("codex-code-mode-host")):("codex-code-mode-host",p["code_host_sha256"]),str(docker):("docker",p["docker_sha256"])}
 t=Transport(name,cmd,fixture,home,temp,log,deadline,p["budgets"][name]["rpc_timeout_seconds"],executable_allowlist,"codex-code-mode-host");registry.append(t)
 init=t.rpc("initialize",{"clientInfo":{"name":"rust_m1_qualifier","title":"Rust M1 Qualifier","version":"3"},"capabilities":{"experimentalApi":True}});t.send({"method":"initialized","params":{}})
 identity={"platformFamily":init.get("platformFamily"),"platformOs":init.get("platformOs"),"userAgent_sha256":digest(str(init.get("userAgent","")).encode())};model=validate_model(t,p);effective=validate_effective(t,p,server,args,enabled,fixture,name);inventory=validate_inventory(t,p,name,enabled)
 thread_params,turn_template=build_protocol_params(schemas,fixture,x["prompt"],p);r=t.rpc("thread/start",thread_params);thread=r.get("thread",{});thread_id=thread.get("id");provider=r.get("threadProvider",r.get("modelProvider",thread.get("modelProvider")))
 if not thread_id or provider!="openai" or r.get("instructionSources") not in ([],None):raise RuntimeError("thread/provider/instructions")
 turn_template["threadId"]=thread_id
 if not schema_valid(turn_template,schemas["documents"]["TurnStartParams"],schemas["documents"]["TurnStartParams"]):raise RuntimeError("bound turn params violate schema")
 r=t.rpc("turn/start",turn_template);turn=r.get("turn",{});turn_id=turn.get("id")
 if not turn_id or turn.get("status")!="inProgress":raise RuntimeError("turn start")
 items=[];seen=set();raw_seen=set();actions=[];patch=None;usage=None;finished=False;drain_until=None;approved_resource_uris=set()
 while time.monotonic()<deadline and (not finished or time.monotonic()<(drain_until or 0)):  # NOSONAR -- deadline is derived from bounded, hash-approved plan budgets.
  wait_until=min(deadline,drain_until) if finished else deadline
  try:m=t.events.get(timeout=min(.5,max(.01,wait_until-time.monotonic())))
  except queue.Empty:continue
  if m is None:raise RuntimeError(t.failure or "stream")
  if "id" in m:raise RuntimeError("server request")
  method=m.get("method");params=m.get("params",{})
  if finished and method not in {"thread/tokenUsage/updated","thread/status/changed"}:raise RuntimeError("event after turn completion")
  if method=="error":raise RuntimeError("error event")
  if method not in SAFE_METHODS:raise RuntimeError(f"notification:{method}")
  if method=="remoteControl/status/changed" and (params.get("status")!="disabled" or params.get("environmentId") is not None):raise RuntimeError("remote control notification")
  if method=="deprecationNotice" and (not isinstance(params.get("summary"),str) or not params["summary"]):raise RuntimeError("deprecation notification")
  if method=="warning" and (params.get("threadId")!=thread_id or not isinstance(params.get("message"),str) or not params["message"]):raise RuntimeError("warning notification")
  if method=="mcpServer/startupStatus/updated" and (params.get("threadId")!=thread_id or params.get("name")!="rust_engineering" or params.get("status") not in {"starting","ready"} or params.get("error") is not None or params.get("failureReason") is not None):raise RuntimeError("MCP startup notification")
  if method=="thread/started" and params.get("thread",{}).get("id")!=thread_id:raise RuntimeError("thread start notification")
  if method=="account/rateLimits/updated" and not isinstance(params.get("rateLimits"),dict):raise RuntimeError("rate limit notification")
  if method=="thread/settings/updated":
   settings=params.get("threadSettings",{});sandbox=settings.get("sandboxPolicy",{});collab=settings.get("collaborationMode",{});collab_settings=collab.get("settings",{}) if isinstance(collab,dict) else {}
   if params.get("threadId")!=thread_id or settings.get("approvalPolicy")!="never" or settings.get("cwd")!=str(fixture) or settings.get("model")!=p["model"] or settings.get("effort")!=p["effort"] or settings.get("modelProvider")!="openai" or settings.get("multiAgentMode")!="explicitRequestOnly" or sandbox.get("type")!="workspaceWrite" or sandbox.get("networkAccess") is not False or sandbox.get("excludeTmpdirEnvVar") is not True or sandbox.get("excludeSlashTmp") is not True or sandbox.get("writableRoots") not in ([],[str(fixture)]) or collab.get("mode")!="default" or collab_settings.get("model")!=p["model"] or collab_settings.get("reasoning_effort")!=p["effort"]:raise RuntimeError("thread settings notification")
  if "threadId" in params and params["threadId"]!=thread_id:raise RuntimeError("foreign thread")
  if "turnId" in params and params["turnId"]!=turn_id:raise RuntimeError("foreign turn")
  if method in {"item/started","item/completed"}:
   item=params.get("item",{})
   if params.get("threadId")!=thread_id or params.get("turnId")!=turn_id or item.get("type") not in SAFE_ITEMS:raise RuntimeError("item lifecycle binding")
   if item.get("type")=="mcpToolCall" and not (item.get("server")=="rust_engineering" and item.get("tool") in enabled) and not native_discovery(item) and not native_resource_read(item):raise RuntimeError("unexpected started MCP tool")
   if item.get("type")=="mcpToolCall" and native_resource_read(item):validate_native_resource_read(item,approved_resource_uris)
   if name=="missing_runtime" and item.get("type")=="fileChange":raise RuntimeError("missing phase fileChange")
  if method=="rawResponseItem/completed":
   raw=params.get("item",{});typ=raw.get("type")
   if params.get("threadId")!=thread_id or params.get("turnId")!=turn_id or typ not in SAFE_RAW:raise RuntimeError(f"raw:{typ}")
   raw_id=raw.get("id")
   if not isinstance(raw_id,str) or not raw_id or raw_id in raw_seen:raise RuntimeError("raw item bind/dedup")
   raw_seen.add(raw_id)
   if typ=="custom_tool_call":
    if name!="repair" or patch is not None:raise RuntimeError("raw custom")
    patch=parse_patch_input(raw);actions.append({"kind":"raw_patch_call","call_id":patch["call_id"],"input_sha256":patch["input_sha256"],"patch":patch["patch"]})
   elif typ=="custom_tool_call_output":
    if patch is None or raw.get("status")!="completed" or not isinstance(raw.get("id"),str) or raw.get("call_id")!=patch["call_id"]:raise RuntimeError("raw patch output binding")
    actions.append({"kind":"raw_patch_output","call_id":raw["call_id"]})
   elif typ=="mcp_list_tools":
    names={item.get("name") for item in raw.get("tools",[]) if isinstance(item,dict)}
    if not isinstance(raw.get("id"),str) or names!=set(enabled):raise RuntimeError("raw MCP discovery inventory")
    actions.append({"kind":"raw:mcp_list_tools"})
   else:actions.append({"kind":f"raw:{typ}"})
  if method=="item/completed":
   item=params.get("item",{});ident=item.get("id")
   if not isinstance(ident,str) or not ident or ident in seen:raise RuntimeError("item bind/dedup")
   seen.add(ident)
   if item["type"] in {"mcpToolCall","fileChange","plan"}:
    if item["type"]=="mcpToolCall" and native_discovery(item):validate_native_discovery(item);actions.append({"kind":"native_resource_discovery"});continue
    if item["type"]=="mcpToolCall" and native_resource_read(item):validate_native_resource_read(item,approved_resource_uris);actions.append({"kind":"native_resource_read","uri_sha256":digest(item["arguments"]["uri"].encode())});continue
    if item["type"]=="mcpToolCall" and item.get("tool") not in enabled:raise RuntimeError("disallowed MCP tool")
    items.append(item)
    if item["type"]=="mcpToolCall" and item.get("tool")=="rust.check" and "E0502" in {str(z) for z in find(output_payload(item),"code")}:
     actions.append({"kind":"e0502"})
     uris={str(z) for z in find(output_payload(item),"uri") if str(z).startswith("rust-artifact://")}
     if len(uris)!=1:raise RuntimeError("diagnostic artifact URI")
     approved_resource_uris.update(uris)
    elif item["type"]=="fileChange":actions.append({"kind":"fileChange","diff_sha256":digest(item["changes"][0]["diff"].encode())})
    else:actions.append({"kind":f"item:{item['type']}"})
  if method=="thread/tokenUsage/updated":
   vals=find(params,"outputTokens")
   if any(isinstance(z,bool) or not isinstance(z,int) or z<0 for z in vals):raise RuntimeError("invalid usage")
   if vals:usage=max(vals)
   if usage is not None and usage>p["budgets"][name]["max_output_tokens"]:raise RuntimeError("token budget")
  if method=="turn/completed":
   tr=params.get("turn",{})
   if finished or tr.get("id")!=turn_id or tr.get("status")!="completed" or tr.get("error") is not None:raise RuntimeError("turn completion")
   finished=True;drain_until=min(deadline,time.monotonic()+.1)
 if not finished:raise TimeoutError("wall timeout")
 if usage is None or usage<=0:raise RuntimeError("usage absent")
 safe_turn={**turn_template,"threadId":"sha256:"+digest(thread_id.encode()),"input":[{"type":"text","text_sha256":digest(x["prompt"].encode())}]};state_result=phase_state(name,items,actions,p,fixture);raw_patch=state_result.pop("_raw_patch")
 return {"status":"passed","identity":identity,"model":model,"effective_config":effective,"inventory":inventory,"thread_id_sha256":digest(thread_id.encode()),"turn_id_sha256":digest(turn_id.encode()),"usage_output_tokens":usage,"zero_error_events":True,"state":state_result,"protocol_params":{"thread":thread_params,"turn":safe_turn,"thread_sha256":digest(enc(thread_params)),"turn_sha256":digest(enc(safe_turn)),"hash_form":"normalized-redacted"},"approval_policy":"never"},t,raw_patch

def docker_command(cli,socket,args,timeout=15,deadline=None):
 if deadline is not None:
  timeout=min(timeout,deadline-time.monotonic())
  if timeout<=0:raise TimeoutError("cleanup budget")
 return subprocess.run([str(cli),"--host",f"unix://{socket}",*args],capture_output=True,text=True,timeout=timeout,env={"PATH":"/usr/bin:/bin","LC_ALL":"C","LANG":"C"})  # NOSONAR -- cli is hash-pinned; args come from closed internal command builders; no shell.
def docker_daemon_evidence(cli,socket,image_id,platform):
 version=docker_command(cli,socket,["version","--format","{{.Server.Version}} {{.Server.Arch}} {{.Server.Os}}"]);parts=version.stdout.strip().split()
 if platform!=PRODUCT_RUNTIME_PLATFORM or version.returncode or parts!=[DOCKER_SERVER_VERSION,DOCKER_SERVER_ARCH,DOCKER_SERVER_OS]:raise RuntimeError("docker server identity")
 image=docker_command(cli,socket,["image","inspect","--format","{{.Id}}",image_id])
 if image.returncode or image.stdout.strip()!=image_id:raise RuntimeError("docker image digest")
 clock=docker_command(cli,socket,["info","--format","{{.SystemTime}}"])
 try:
  observed=datetime.datetime.fromisoformat(clock.stdout.strip().replace("Z","+00:00"))
  if observed.tzinfo is None:raise ValueError("timezone absent")
  skew=observed.timestamp()-time.time()
 except Exception as e:raise RuntimeError("docker daemon clock") from e
 if clock.returncode or abs(skew)>60:raise RuntimeError("docker daemon clock skew")
 margin=min(65,max(2,abs(skew)+2))
 return {"server_version":parts[0],"arch":parts[1],"os":parts[2],"image_id":image.stdout.strip(),"clock_skew_seconds":round(skew,3),"event_margin_seconds":round(margin,3),"system_time_sha256":digest(clock.stdout.strip().encode())}
def docker_events(cli,socket,label,since,until,timeout=15):
 filters=["--filter",f"label={label}"] if label else []
 r=docker_command(cli,socket,["events","--since",f"{since:.9f}","--until",f"{until:.9f}",*filters,"--format","{{json .}}"],timeout=timeout)
 if r.returncode:raise RuntimeError("docker events")
 out=[]
 for line in r.stdout.splitlines():
  if not line:continue
  event=loads_strict(line);action=event.get("Action",event.get("status"));attrs=event.get("Actor",{}).get("Attributes",{});out.append({"action":action,"image":attrs.get("image",attrs.get("from")),"id":event.get("id",event.get("ID"))})
 return out
def validate_phase_docker(name,events,inventory,image_id):
 lifecycle=[x for x in events if x["action"] in {"create","start"}]
 if name=="missing_runtime":
  if events or any(inventory.values()):raise RuntimeError("missing phase touched Docker")
 else:
  actions={x["action"] for x in lifecycle}
  if actions!={"create","start"} or any(x["image"]!=image_id for x in lifecycle):raise RuntimeError("repair Docker lifecycle/image")
 return {"events":events,"inventory":inventory,"asserted_zero":name=="missing_runtime","asserted_create_start":name=="repair"}
def docker_inventory(cli,socket,label=None,deadline=None):
 out={}
 suffix=["--filter",f"label={label}"] if label else []
 for kind,args in {"containers":["ps","-aq",*suffix],"volumes":["volume","ls","-q",*suffix],"networks":["network","ls","-q",*suffix]}.items():
  r=docker_command(cli,socket,args,deadline=deadline)
  if r.returncode:raise RuntimeError(f"docker inventory:{kind}")
  out[kind]=sorted({x for x in r.stdout.splitlines() if x})
 return out
def docker_cleanup(cli,socket,label,baseline,deadline=None):
 current=docker_inventory(cli,socket,label,deadline)
 if any(baseline.values()):raise RuntimeError("docker baseline not empty")
 key,value=label.split("=",1);removed={"containers":[],"volumes":[],"networks":[]}
 for kind in removed:
  for ident in current[kind]:
   inspect=["inspect","--format",f"{{{{ index .Config.Labels {json.dumps(key)} }}}}",ident] if kind=="containers" else [kind[:-1],"inspect","--format",f"{{{{ index .Labels {json.dumps(key)} }}}}",ident];r=docker_command(cli,socket,inspect,deadline=deadline)
   if r.returncode or r.stdout.strip()!=value:raise RuntimeError(f"docker label verification:{kind}")
   args=["rm","-f",ident] if kind=="containers" else [kind[:-1],"rm",ident];r=docker_command(cli,socket,args,deadline=deadline)
   if r.returncode:raise RuntimeError(f"docker cleanup:{kind}")
   removed[kind].append(ident)
 after=docker_inventory(cli,socket,label,deadline)
 if any(after.values()):raise RuntimeError("docker cleanup leak")
 return {"observed":current,"removed":removed,"after":after}
def remove_owned_tree(root,deadline=None):
 removed=[];count=[0]
 def visit(path,depth):
  if deadline is not None and time.monotonic()>deadline:raise TimeoutError("cleanup budget")
  if depth>64:raise RuntimeError("cleanup depth")
  for entry in os.scandir(path):
   count[0]+=1
   if count[0]>MAX_CLEANUP_ENTRIES:raise RuntimeError("cleanup entry budget")
   child=Path(entry.path);mode=entry.stat(follow_symlinks=False).st_mode
   if stat.S_ISDIR(mode):visit(child,depth+1);child.rmdir()
   else:child.unlink()
   removed.append(child.relative_to(root).as_posix())
 visit(root,0);root.rmdir();return sorted(removed)
def auth_needles(path):
 raw=path.read_bytes();values=[]
 try:
  stack=[(None,loads_strict(raw))]
  while stack:
   key,v=stack.pop()
   if isinstance(v,dict):stack += list(v.items())
   elif isinstance(v,list):stack += [(key,x) for x in v]
   elif isinstance(v,str) and isinstance(key,str) and re.search(r"token|secret|key|credential",key,re.I) and len(v.encode())>=8:values.append(v.encode())
 except json.JSONDecodeError as e:raise ValueError("auth JSON") from e
 return values
def needle_variants(needles):
 out=set(needles)
 for needle in needles:
  try:
   text=needle.decode("utf-8");out.add(json.dumps(text,ensure_ascii=True)[1:-1].encode());out.add(json.dumps(text,ensure_ascii=False)[1:-1].encode())
  except UnicodeDecodeError:pass
 return sorted(out)
def assert_no_needles(blobs,needles):
 if any(n and n in b for n in needle_variants(needles) for b in blobs):raise RuntimeError("secret/canary observed in evidence")

def execute(p,receipt_path,transcript_path):
 try:paths=validate_plan(p)
 except Exception as e:
  failure_receipt(receipt_path,type(e).__name__,str(e),digest(enc(p)));return {"schema_version":4,"status":"failed","plan_sha256":digest(enc(p)),"errors":[{"type":type(e).__name__,"message":str(e)}]}
 rfd=secure_create(receipt_path)
 try:log=Transcript(transcript_path)
 except Exception as e:
  write_all(rfd,enc({"schema_version":4,"status":"failed","plan_sha256":digest(enc(p)),"errors":[{"type":"transcript_create","message":str(e)}]})+b"\n");os.close(rfd);return {"schema_version":4,"status":"failed","errors":[{"type":"transcript_create","message":str(e)}]}
 receipt={"schema_version":4,"status":"failed","plan_sha256":digest(enc(p)),"phases":{},"errors":[]};transports=[];run_root=docker_cli=None;homes={};temps={};copied_auth={};auth_before={};source_auth_before=None;auth_changed=False;docker_before=docker_global_before=None;needles=[];canary_secret=os.urandom(32).hex().encode()
 try:
  source_auth_before=file_digest(paths["auth_source"]);needles=auth_needles(paths["auth_source"])+[canary_secret];log.set_needles(needles);run_root=paths["private_root"]/("qualifier-"+os.urandom(8).hex());run_root.mkdir(mode=0o700);work=run_root/"work";fixture=work/"fixture";bins=run_root/"bin";schema=run_root/"schema"
  discovery_home=run_root/"home-discovery";discovery_temp=run_root/"tmp-discovery"
  for d in (work,bins,schema,discovery_home,discovery_temp):d.mkdir(mode=0o700)
  source_before=copy_fixture(paths["fixture_root"],fixture);initial=fs_snapshot(fixture);state=work/"state-repair";state.mkdir(mode=0o700);receipt["auth_seed"]={}
  for name in ("missing_runtime","repair"):
   homes[name]=run_root/f"home-{name}";temps[name]=run_root/f"tmp-{name}";homes[name].mkdir(mode=0o700);temps[name].mkdir(mode=0o700);copied_auth[name]=homes[name]/"auth.json";copy_exact(paths["auth_source"],copied_auth[name],0o600);auth_before[name]=file_digest(copied_auth[name]);home_seed=fs_snapshot(homes[name])
   if set(home_seed)!={".","auth.json"}:raise RuntimeError("CODEX_HOME not auth-only")
   receipt["auth_seed"][name]={"only_auth_material":True,"auth_mode":home_seed["auth.json"]["mode"]}
  copy_exact(paths["codex_source"],bins/"codex",0o700);copy_exact(paths["code_host_source"],bins/"codex-code-mode-host",0o700);copy_exact(paths["server_source"],bins/"rust-engineering-mcp",0o700);docker_cli=bins/"docker";copy_exact(paths["docker_cli"],docker_cli,0o700)
  if file_digest(bins/"codex")!=p["codex_sha256"] or file_digest(bins/"codex-code-mode-host")!=p["code_host_sha256"] or file_digest(bins/"rust-engineering-mcp")!=p["server_sha256"] or file_digest(docker_cli)!=p["docker_sha256"]:raise RuntimeError("staged candidate identity")
  version=subprocess.run([str(bins/"codex"),"--version"],capture_output=True,text=True,check=True,timeout=10,env={"PATH":"/usr/bin:/bin","LC_ALL":"C","LANG":"C"})
  if version.stdout.strip()!=p["codex_version"]:raise RuntimeError("staged version")
  discovered=discover_features(bins/"codex",discovery_home,discovery_temp)
  if discovered!=p["feature_world"]:raise RuntimeError("feature world drift")
  receipt["feature_world"]={"sha256":digest(enc(discovered)),"count":len(discovered),"configured":configured_features(p)}
  generated=subprocess.run([str(bins/"codex"),"app-server","generate-json-schema","--experimental","--out",str(schema)],capture_output=True,text=True,timeout=20,env={"HOME":str(discovery_home),"CODEX_HOME":str(discovery_home),"TMPDIR":str(discovery_temp),"PATH":"/usr/bin:/bin","LC_ALL":"C","LANG":"C","PYTHONDONTWRITEBYTECODE":"1"})
  if generated.returncode:raise RuntimeError("schema generation")
  receipt["discovery_cleanup"]={"home_entries_removed":len(remove_owned_tree(discovery_home)),"temp_entries_removed":len(remove_owned_tree(discovery_temp)),"discarded_before_phases":True}
  for name in ("missing_runtime","repair"):
   if set(fs_snapshot(homes[name]))!={".","auth.json"}:raise RuntimeError(f"{name}:CODEX_HOME contaminated before phase")
  schema_evidence,schemas=validate_schema_bundle(schema,p["schema_bundle_sha256"]);receipt["protocol_schema"]=schema_evidence;canary=run_root/"authority-canary";fd=secure_create(canary);write_all(fd,canary_secret+b"\n");os.close(fd);canary_hash=file_digest(canary);receipt["docker_daemon"]=docker_daemon_evidence(docker_cli,paths["docker_socket"],p["runtime"]["image_id"],p["runtime"]["platform"]);event_margin=receipt["docker_daemon"]["event_margin_seconds"];clock_skew=receipt["docker_daemon"]["clock_skew_seconds"];docker_before=docker_inventory(docker_cli,paths["docker_socket"],p["docker_label"]);docker_global_before=docker_inventory(docker_cli,paths["docker_socket"])
  if any(docker_before.values()):raise RuntimeError("docker baseline not empty")
  private_before=private_inventory(run_root)
  repair_patch=None
  for name in ("missing_runtime","repair"):
   phase_start=time.time()+clock_skew-event_margin;phase_deadline=time.monotonic()+p["budgets"][name]["wall_seconds"];result,t,raw_patch=run_phase(name,p,bins/"codex",bins/"rust-engineering-mcp",docker_cli,homes[name],temps[name],fixture,state,log,phase_deadline,transports,schemas);receipt["phases"][name]=result;clean=t.close();result["cleanup"]=clean
   if name=="repair":repair_patch=raw_patch
   require_transport_closed(name,clean)
   phase_end=time.time()+clock_skew+event_margin;inventory=docker_inventory(docker_cli,paths["docker_socket"],p["docker_label"]);global_inventory=docker_inventory(docker_cli,paths["docker_socket"]);event_timeout=max(15,event_margin+5);events=docker_events(docker_cli,paths["docker_socket"],p["docker_label"],phase_start,phase_end,event_timeout);global_events=docker_events(docker_cli,paths["docker_socket"],None,phase_start,phase_end,event_timeout);result["docker"]=validate_phase_docker(name,events,inventory,p["runtime"]["image_id"]);result["docker"]["global_inventory"]=global_inventory;result["docker"]["global_events"]=global_events;result["docker"]["event_window"]={"since":phase_start,"until":phase_end,"timeout_seconds":event_timeout}
   new_global={k:set(global_inventory[k])-set(docker_global_before[k]) for k in global_inventory}
   if name=="missing_runtime" and (global_inventory!=docker_global_before or global_events):raise RuntimeError("missing phase touched Docker globally")
   if name=="repair" and any(not ids.issubset(set(inventory[k])) for k,ids in new_global.items()):raise RuntimeError("repair created unlabeled Docker resource")
   if name=="missing_runtime":
    after_missing=fs_snapshot(fixture)
    if after_missing!=initial:raise RuntimeError("missing phase changed fixture")
  final=fs_snapshot(fixture);source_after=fs_snapshot(paths["fixture_root"]);changed={k for k in set(initial)|set(final) if initial.get(k)!=final.get(k)}
  if "src/lib.rs" not in changed or changed!={"src/lib.rs"} or any(k=="target" or k.startswith("target/") for k in final) or source_after!=source_before or file_digest(canary)!=canary_hash:raise RuntimeError(f"confinement:{sorted(changed)}")
  repaired=(fixture/"src/lib.rs").read_bytes();assert_no_needles([repaired],needles);original_text=(paths["fixture_root"]/"src/lib.rs").read_text(encoding="utf-8");text=repaired.decode("utf-8")
  if not isinstance(repair_patch,str) or apply_update_patch(original_text,repair_patch)!=text:raise RuntimeError("raw patch content does not produce repaired source")
  preserved=receipt_path.parent/"repaired-src-lib.rs";fd=secure_create(preserved);write_all(fd,repaired);os.fsync(fd);os.close(fd);original=original_text.splitlines(keepends=True);diff="".join(difflib.unified_diff(original,text.splitlines(keepends=True),fromfile="source/src/lib.rs",tofile="repaired/src/lib.rs"))
  repaired_hash=file_digest(preserved);patch_hash=receipt["phases"]["repair"]["state"]["raw_patch_input_sha256"]
  candidate={"codex_sha256":file_digest(bins/"codex"),"code_host_sha256":file_digest(bins/"codex-code-mode-host"),"server_sha256":file_digest(bins/"rust-engineering-mcp"),"docker_sha256":file_digest(docker_cli),"version":version.stdout.strip()}
  if any(candidate[key]!=p[key] for key in ("codex_sha256","code_host_sha256","server_sha256","docker_sha256")) or candidate["version"]!=p["codex_version"]:raise RuntimeError("post-run candidate identity")
  receipt.update(status="passed",candidate=candidate,fixture={"source_before":source_before,"source_after":source_after,"working_initial":initial,"after_missing":after_missing,"working_final":final,"allowed_mutations":["src/lib.rs"],"target_absent":True,"rotations_sha256":digest(enc([source_before,initial,after_missing,final,source_after]))},repaired_source={"path":str(preserved),"mode":stat.S_IMODE(preserved.stat().st_mode),"sha256":repaired_hash,"contents":text,"diff":diff,"raw_patch_input_sha256":patch_hash,"patch_application_verified":True,"transition_sha256":digest(enc({"source":source_before["src/lib.rs"]["sha256"],"patch":patch_hash,"repaired":repaired_hash}))},canary={"sha256":canary_hash,"unchanged":True},private_inventory_before_phases=sanitized_snapshot(private_before))
 except Exception as e:receipt["errors"].append({"type":type(e).__name__,"message":str(e)});receipt["status"]="failed"
 finally:
  cleanup_started=time.monotonic();cleanup_deadline=cleanup_started+p["budgets"]["cleanup_seconds"]
  for t in transports:
   try:
    c=t.close(True);require_transport_closed(t.phase,c)
   except Exception as e:receipt["errors"].append({"type":type(e).__name__,"message":str(e)});receipt["status"]="failed"
  if paths is not None and docker_before is not None:
   try:
    cleanup=docker_cleanup(docker_cli or paths["docker_cli"],paths["docker_socket"],p["docker_label"],docker_before,cleanup_deadline);global_after=docker_inventory(docker_cli or paths["docker_cli"],paths["docker_socket"],deadline=cleanup_deadline)
    if docker_global_before is not None and global_after!=docker_global_before:raise RuntimeError("global Docker inventory drift")
    receipt["docker"]={"socket_sha256":digest(str(paths["docker_socket"]).encode()),"baseline":docker_before,"global_baseline":docker_global_before,"cleanup":cleanup,"global_after":global_after}
   except Exception as e:receipt["errors"].append({"type":"docker_cleanup","message":str(e)});receipt["status"]="failed"
  elif paths is not None:
   try:receipt["docker_inventory_on_exit"]=docker_inventory(docker_cli or paths["docker_cli"],paths["docker_socket"],p["docker_label"],cleanup_deadline)
   except Exception as e:receipt["errors"].append({"type":"docker_inventory_on_exit","message":str(e)});receipt["status"]="failed"
  if copied_auth:
   try:source_unchanged=source_auth_before is not None and file_digest(paths["auth_source"])==source_auth_before
   except Exception:source_unchanged=False
   receipt["auth_copy"]={"source_unchanged":source_unchanged,"phases":{}}
   for name,path in copied_auth.items():
    if path.exists():
     try:needles.extend(auth_needles(path))
     except Exception as e:receipt["errors"].append({"type":"auth_refresh_parse","message":str(e)});receipt["status"]="failed";auth_changed=True
    try:changed=not path.exists() or file_digest(path)!=auth_before[name]
    except Exception:changed=True
    auth_changed=auth_changed or changed;receipt["auth_copy"]["phases"][name]={"changed":changed}
    if not changed:
     try:path.unlink();receipt["auth_copy"]["phases"][name]["unlinked_independently"]=True
     except Exception as e:receipt["errors"].append({"type":"auth_unlink","message":str(e)});receipt["status"]="failed"
   if not source_unchanged:receipt["errors"].append({"type":"auth_source","message":"source auth changed"});receipt["status"]="failed"
   if auth_changed:receipt["errors"].append({"type":"auth_refresh","message":"isolated auth changed; protected run home preserved for recovery"});receipt["status"]="failed"
   log.set_needles(needles)
  try:log.close();receipt["transcript_sha256"]=file_digest(transcript_path);receipt["transcript_bytes"]=transcript_path.stat().st_size
  except Exception as e:receipt["errors"].append({"type":"transcript","message":str(e)});receipt["status"]="failed"
  if run_root is not None and run_root.exists():
   try:receipt["private_inventory_on_exit"]=sanitized_snapshot(private_inventory(run_root))
   except Exception as e:receipt["errors"].append({"type":"private_inventory","message":str(e)});receipt["status"]="failed"
  if paths is not None:
   try:receipt["source_fixture_on_exit"]=fs_snapshot(paths["fixture_root"])
   except Exception as e:receipt["errors"].append({"type":"fixture_inventory","message":str(e)});receipt["status"]="failed"
  if run_root is not None and run_root.exists() and not auth_changed:
   try:receipt["owned_cleanup"]={"removed":remove_owned_tree(run_root,cleanup_deadline),"owned_child_removed":not run_root.exists(),"approved_private_root_left":run_root.parent.exists()}
   except Exception as e:receipt["errors"].append({"type":"owned_cleanup","message":str(e)});receipt["status"]="failed"
  elif run_root is not None:receipt["owned_cleanup"]={"owned_child_removed":False,"preserved_for_auth_recovery":True,"path_sha256":digest(str(run_root).encode())}
  cleanup_elapsed=time.monotonic()-cleanup_started;receipt["cleanup_budget"]={"limit_seconds":p["budgets"]["cleanup_seconds"],"observed_seconds":round(cleanup_elapsed,3)}
  if cleanup_elapsed>p["budgets"]["cleanup_seconds"]:receipt["errors"].append({"type":"cleanup_timeout","message":"cleanup budget exceeded"});receipt["status"]="failed"
  try:
   blobs=[transcript_path.read_bytes(),enc(receipt)];preserved=receipt_path.parent/"repaired-src-lib.rs"  # NOSONAR -- both paths are fixed children of the owned output root.
   if preserved.exists():blobs.append(preserved.read_bytes())  # NOSONAR -- fixed owned-output child established above.
   assert_no_needles(blobs,needles)
  except Exception:
   error_types=sorted({str(x.get("type")) for x in receipt.get("errors",[]) if isinstance(x,dict) and x.get("type")})
   preserved=receipt_path.parent/"repaired-src-lib.rs"
   if preserved.exists():preserved.unlink()  # NOSONAR -- fixed owned-output child; deletion is evidence fail-closed cleanup.
   if transcript_path.exists():transcript_path.unlink()  # NOSONAR -- validated owned-output path; contaminated evidence must not persist.
   receipt={"schema_version":4,"status":"failed","plan_sha256":digest(enc(p)),"errors":[*({"type":x,"message":"redacted during evidence sanitization"} for x in error_types),{"type":"secret_scan","message":"contaminated transcript and preserved source were removed"}],"error_types_before_sanitization":error_types,"transcript_removed":True}
  write_all(rfd,enc(receipt)+b"\n");os.fsync(rfd);os.close(rfd)
 return receipt
def failure_receipt(path,typ,message,plan_hash=None):
 fd=secure_create(path);write_all(fd,enc({"schema_version":4,"status":"failed","plan_sha256":plan_hash,"errors":[{"type":typ,"message":message}]})+b"\n");os.fsync(fd);os.close(fd)
def main():
 a=argparse.ArgumentParser();a.add_argument("plan",type=Path);a.add_argument("--receipt",type=Path,required=True);a.add_argument("--transcript",type=Path,required=True);a.add_argument("--approved-plan-sha256",required=True);a.add_argument("--approved-repair-prompt-sha256",required=True);a.add_argument("--approved-missing-prompt-sha256",required=True);n=a.parse_args();raw=b""
 try:
  raw=n.plan.read_bytes();p=loads_strict(raw)  # NOSONAR -- execution requires the separately supplied approved SHA-256 before any plan action.
  if not isinstance(p,dict):raise ValueError("plan object")
  for x in (n.receipt,n.transcript):
   if not x.is_absolute() or x.parent!=Path(p["output_root"]):raise RuntimeError("evidence path")
  private_dir(Path(p["output_root"]),"output",True);ph=digest(enc(p))
  if n.approved_plan_sha256!=ph or n.approved_repair_prompt_sha256!=p["phases"]["repair"]["prompt_sha256"] or n.approved_missing_prompt_sha256!=p["phases"]["missing_runtime"]["prompt_sha256"]:failure_receipt(n.receipt,"approval","plan or prompt approval digest mismatch",ph);return 1
  r=execute(p,n.receipt,n.transcript);print(json.dumps({"status":r["status"],"receipt_sha256":file_digest(n.receipt)}));return 0 if r["status"]=="passed" else 1
 except Exception as e:
  try:
   if n.receipt.is_absolute() and n.receipt.parent.exists():failure_receipt(n.receipt,type(e).__name__,str(e),digest(raw) if raw else None)
  except Exception:pass
  print(f"qualifier failed:{e}",file=sys.stderr);return 1
if __name__=="__main__":raise SystemExit(main())
