#!/usr/bin/env python3
import base64,importlib.util,json,os,shutil,socket,stat,subprocess,sys,tempfile,textwrap,time,unittest
from pathlib import Path
S=importlib.util.spec_from_file_location("qualifier",Path(__file__).with_name("codex-model-qualifier.py"));q=importlib.util.module_from_spec(S);S.loader.exec_module(q)
MISSING_MESSAGE=q.MISSING_RUNTIME_MESSAGE
THREAD_SCHEMA={"definitions":{"SandboxMode":{"enum":["read-only","workspace-write","danger-full-access"],"type":"string"}},"type":"object","additionalProperties":False,"properties":{
 "model":{"type":"string"},"ephemeral":{"type":"boolean"},"cwd":{"type":"string"},"approvalPolicy":{"enum":["never"]},"sandbox":{"$ref":"#/definitions/SandboxMode"},"serviceName":{"type":"string"},"baseInstructions":{"type":"string"},"developerInstructions":{"type":"string"},"config":{"type":"object"}}}
TURN_SCHEMA={"definitions":{"SandboxPolicy":{"oneOf":[{"title":"WorkspaceWriteSandboxPolicy","type":"object","additionalProperties":False,"required":["type"],"properties":{"type":{"enum":["workspaceWrite"],"type":"string"},"writableRoots":{"type":"array","items":{"type":"string"}},"networkAccess":{"type":"boolean"},"excludeTmpdirEnvVar":{"type":"boolean"},"excludeSlashTmp":{"type":"boolean"}}}]}},"type":"object","additionalProperties":False,"required":["input","threadId"],"properties":{"threadId":{"type":"string"},"input":{"type":"array","items":{"type":"object","required":["type","text"],"properties":{"type":{"enum":["text"]},"text":{"type":"string"}}}},"cwd":{"type":"string"},"approvalPolicy":{"enum":["never"]},"sandboxPolicy":{"anyOf":[{"$ref":"#/definitions/SandboxPolicy"},{"type":"null"}]},"model":{"type":"string"},"effort":{"type":"string"}}}

def payload(status,ref=None,code=None,runtime=None):
 data={"project_ref":ref} if ref else (None if status=="blocked" else {})
 if runtime is not None:data={"runtime":runtime}
 return {"data":data,"diagnostics":[{"code":code}] if code else [],"duration_ms":1,"error_code":"SANDBOX_DENIED" if status=="blocked" else None,"error_message":MISSING_MESSAGE if status=="blocked" else None,"evidence":{"kind":"local"},"status":status,"summary":status,"truncation":{"diagnostics_omitted":0}}
def tool(name,status,ref=None,code=None,arg_ref=None,path=None,runtime=None,ident=None):
 args={}
 if arg_ref:args["project_ref"]=arg_ref
 if path:args["path"]=path
 return {"id":ident or name+str(ref)+status,"type":"mcpToolCall","server":"rust_engineering","tool":name,"status":"failed" if status=="blocked" else "completed","arguments":args,"error":None,"result":{"content":[],"structuredContent":payload(status,ref,code,runtime)}}

class Tests(unittest.TestCase):
 def assert_mode_failed(self,mode):
  with tempfile.TemporaryDirectory() as td:
   plan,private,out,fixture,auth=self.make_case(Path(td).resolve(),mode);result=q.execute(plan,out/"receipt.json",out/"transcript.jsonl");stored=json.loads((out/"receipt.json").read_text());self.assertEqual(result["status"],"failed");self.assertEqual(stored["status"],"failed");self.assertTrue(stored["errors"]);self.assertTrue(all(isinstance(x.get("type"),str) and x["type"] for x in stored["errors"]))
 def test_strict_json_rejects_non_finite_numbers(self):
  for raw in ('{"x":NaN}','{"x":Infinity}','{"x":-Infinity}'):
   with self.assertRaises(ValueError):q.loads_strict(raw)
 def test_negative_numbers_and_exact_args(self):
  good={"budgets":{"missing_runtime":{"wall_seconds":30,"max_output_tokens":1,"rpc_timeout_seconds":10},"repair":{"wall_seconds":30,"max_output_tokens":1,"rpc_timeout_seconds":10},"cleanup_seconds":30}}
  bad=[]
  for value in (-1,True,30.0):
   row=json.loads(json.dumps(good));row["budgets"]["repair"]["wall_seconds"]=value;bad.append(row)
  for p in bad:
   with self.assertRaises(ValueError):q.validate_numbers(p)
  with self.assertRaises(ValueError):q.parse_server_args(["serve","--stdio","--root","$FIXTURE","--state-root","$STATE"],"missing_runtime",Path("/x"),Path("/s"))
  plan={"docker_cli":"/docker","docker_socket":"/sock"}
  args=["serve","--stdio","--root","$FIXTURE","--docker","/other","--docker-socket","/sock","--state-root","$STATE","--rust-image","sha256:"+"a"*64]
  with self.assertRaises(ValueError):q.parse_server_args(args,"repair",Path("/x"),Path("/s"),plan)
 def test_exclusive_evidence_and_patch_confinement(self):
  with tempfile.TemporaryDirectory() as td:
   path=Path(td)/"receipt";fd=q.secure_create(path);os.close(fd);self.assertEqual(stat.S_IMODE(path.stat().st_mode),0o600)
   with self.assertRaises(FileExistsError):q.secure_create(path)
  raw={"id":"r","call_id":"c","status":"completed","name":"apply_patch","input":json.dumps({"patch":"*** Begin Patch\n*** Update File: src/lib.rs\n@@\n-x\n+y\n*** End Patch"})}
  self.assertEqual(q.parse_patch_input(raw)["call_id"],"c")
  self.assertEqual(q.apply_update_patch("x\n",json.loads(raw["input"])["patch"]),"y\n")
  with self.assertRaises(RuntimeError):q.apply_update_patch("different\n",json.loads(raw["input"])["patch"])
  raw["input"]=json.dumps({"patch":"*** Update File: ../secret"})
  with self.assertRaises(RuntimeError):q.parse_patch_input(raw)
 def test_state_machine_exact_missing_and_runtime(self):
  plan={"missing_error_message":MISSING_MESSAGE,"runtime":{"image_id":"sha256:"+"a"*64,"rust_version":"1.90.0","cargo_version":"1.90.0","platform":"linux/aarch64"}}
  runtime={**plan["runtime"],"configuration_fingerprint":"sha256:"+"b"*64,"execution_fingerprint":"sha256:"+"c"*64}
  patch="*** Begin Patch\n*** Update File: src/lib.rs\n@@\n-bad\n+good\n*** End Patch"
  repair=[{"id":"plan","type":"plan","status":"completed","text":"Inspect, check, repair and re-check."},tool("rust.project.open","passed","p1",path="/f"),tool("rust.project.inspect","passed",arg_ref="p1"),tool("rust.check","failed",code="E0502",arg_ref="p1"),{"id":"f","type":"fileChange","status":"completed","changes":[{"path":"src/lib.rs","kind":{"type":"update"},"diff":"@@\n-bad\n+good\n"}]},tool("rust.project.open","passed","p2",path="/f"),tool("rust.check","passed",arg_ref="p2",runtime=runtime)]
  actions=[{"kind":"raw:mcp_list_tools"},{"kind":"item:mcpToolCall"},{"kind":"item:mcpToolCall"},{"kind":"e0502"},{"kind":"raw_patch_call","patch":patch},{"kind":"raw_patch_output"},{"kind":"fileChange"},{"kind":"item:mcpToolCall"},{"kind":"item:mcpToolCall"}]
  self.assertEqual(q.phase_state("repair",repair,actions,plan,Path("/f"))["runtime"]["image_id"],plan["runtime"]["image_id"])
  post_inspect=tool("rust.project.inspect","passed",arg_ref="p2")
  inspected=q.phase_state("repair",[*repair[:-1],post_inspect,repair[-1]],actions,plan,Path("/f"))
  self.assertEqual(inspected["post_repair_inspect_count"],1)
  with self.assertRaises(RuntimeError):q.phase_state("repair",[*repair[:-1],post_inspect,post_inspect,repair[-1]],actions,plan,Path("/f"))
  without_plan=q.phase_state("repair",repair[1:],actions,plan,Path("/f"))
  self.assertEqual(without_plan["model_plan_item_count"],0)
  missing=[tool("rust.project.open","passed","pm",path="/f"),tool("rust.check","blocked",arg_ref="pm")]
  self.assertEqual(q.phase_state("missing_runtime",missing,[{"kind":"raw:mcp_list_tools"},{"kind":"item:mcpToolCall"},{"kind":"item:mcpToolCall"}],plan,Path("/f"))["claim"],"product_runtime_unconfigured")
  probe=tool("rust.project.open","blocked",path=".");probe["result"]["structuredContent"]["error_code"]="INVALID_PROJECT";probe["result"]["structuredContent"]["error_message"]="Project structure is invalid or unsupported"
  recovered=q.phase_state("missing_runtime",[probe,*missing],[{"kind":"item:mcpToolCall"},{"kind":"item:mcpToolCall"},{"kind":"item:mcpToolCall"}],plan,Path("/f"))
  self.assertEqual(recovered["relative_project_probe_count"],1)
  with self.assertRaises(RuntimeError):q.phase_state("missing_runtime",[probe,probe,*missing],[],plan,Path("/f"))
  with self.assertRaises(RuntimeError):q.phase_state("missing_runtime",missing,[{"kind":"item:mcpToolCall"},{"kind":"raw:mcp_list_tools"},{"kind":"item:mcpToolCall"}],plan,Path("/f"))
  missing[-1]["result"]["structuredContent"]["data"]={}
  with self.assertRaises(RuntimeError):q.phase_state("missing_runtime",missing,[],plan,Path("/f"))
 def test_native_resource_read_is_bound_to_emitted_artifact(self):
  uri="rust-artifact://prj_123/art_456"
  started={"type":"mcpToolCall","server":"rust_engineering","tool":"read_mcp_resource","status":"inProgress","arguments":{"server":"rust_engineering","uri":uri},"error":None,"result":None}
  q.validate_native_resource_read(started,{uri})
  completed={**started,"status":"completed","result":{"content":[{"type":"resource","resource":{"uri":uri,"mimeType":"text/plain","text":"compiler output"}}],"structuredContent":None}}
  q.validate_native_resource_read(completed,{uri})
  with self.assertRaises(RuntimeError):q.validate_native_resource_read(started,{"rust-artifact://prj_other/art_other"})
  completed["result"]["content"][0]["resource"]["uri"]="rust-artifact://prj_other/art_other"
  with self.assertRaises(RuntimeError):q.validate_native_resource_read(completed,{uri})
 def test_native_resource_text_envelope_verifies_bytes_and_digest(self):
  uri="rust-artifact://prj_123/art_456";raw=b"compiler output\n"
  meta={"retention_remaining_seconds":60,"sha256":q.digest(raw),"size_bytes":len(raw),"truncated":False}
  envelope={"server":"rust_engineering","uri":uri,"ttlMs":0,"cacheScope":"private","contents":[{"uri":uri,"mimeType":"application/octet-stream","blob":base64.b64encode(raw).decode(),"_meta":meta}]}
  item={"type":"mcpToolCall","server":"rust_engineering","tool":"read_mcp_resource","status":"completed","arguments":{"server":"rust_engineering","uri":uri},"error":None,"result":{"content":[{"type":"text","text":json.dumps(envelope)}],"structuredContent":None}}
  q.validate_native_resource_read(item,{uri})
  envelope["contents"][0]["blob"]=base64.b64encode(b"different").decode();item["result"]["content"][0]["text"]=json.dumps(envelope)
  with self.assertRaises(RuntimeError):q.validate_native_resource_read(item,{uri})
 def test_absolute_file_change_can_be_the_bound_patch_evidence(self):
  fixture=Path("/private/work/fixture")
  change={"id":"f","type":"fileChange","status":"completed","changes":[{"path":str(fixture/"src/lib.rs"),"kind":{"type":"update","move_path":None},"diff":"@@\n-bad\n+good\n"}]}
  patch=q.patch_from_file_change(change,fixture)
  self.assertEqual(q.apply_update_patch("bad\n",patch),"good\n")
  change["changes"][0]["path"]="/private/other/src/lib.rs"
  with self.assertRaises(RuntimeError):q.patch_from_file_change(change,fixture)
 def test_exact_output_contract(self):
  value=payload("passed");self.assertEqual(set(value),q.OUTPUT_KEYS);value["extra"]=1
  with self.assertRaises(RuntimeError):q.output_payload({"result":{"structuredContent":value}})
 def test_frozen_schema_drives_exact_sandbox_spelling(self):
  with tempfile.TemporaryDirectory() as td:
   root=Path(td);(root/"v2").mkdir();(root/"v2/ThreadStartParams.json").write_text(json.dumps(THREAD_SCHEMA));(root/"v2/TurnStartParams.json").write_text(json.dumps(TURN_SCHEMA));schemas=q.protocol_schemas(root,q.closed_bundle_digest(root));plan={"model":"gpt-5.6-sol","effort":"medium"};thread,turn=q.build_protocol_params(schemas,Path("/fixture"),"repair safely",plan)
   self.assertEqual(thread["sandbox"],"workspace-write");self.assertEqual(turn["sandboxPolicy"]["type"],"workspaceWrite");self.assertNotIn("readOnlyAccess",turn["sandboxPolicy"])
   broken=dict(thread);broken["sandbox"]="workspaceWrite";self.assertFalse(q.schema_valid(broken,THREAD_SCHEMA,THREAD_SCHEMA))
   no_workspace=json.loads(json.dumps(THREAD_SCHEMA));no_workspace["definitions"]["SandboxMode"]["enum"]=["read-only"]
   bad_schemas={**schemas,"documents":{**schemas["documents"],"ThreadStartParams":no_workspace}}
   with self.assertRaises(RuntimeError):q.build_protocol_params(bad_schemas,Path("/fixture"),"repair safely",plan)

 def test_codex_0153_effective_config_representation_is_normalized_strictly(self):
  with tempfile.TemporaryDirectory() as td:
   plan,private,out,fixture,auth=self.make_case(Path(td).resolve())
   server=Path(plan["server_source"]);args=q.parse_server_args(plan["phases"]["missing_runtime"]["server_args"],"missing_runtime",fixture,private/"state",plan,Path(plan["docker_cli"]));raw=q.overrides(server,args,q.MISSING_TOOLS,plan)
   observed=dict(raw["mcp_servers"]["rust_engineering"]);observed.pop("env_vars");observed["environment_id"]="local";observed["startup_timeout_sec"]=45.0;observed["tool_timeout_sec"]=300.0
   config={k:v for k,v in raw.items() if "." not in k and k!="mcp_servers"};config["mcp_servers"]={"rust_engineering":observed};config["features"]={k.removeprefix("features."):v for k,v in raw.items() if k.startswith("features.")};config["agents"]={"enabled":False,"max_depth":None,"default_subagent_model":None};config["orchestrator"]={"skills":{"enabled":False},"mcp":{"enabled":False}};config["skills"]={"include_instructions":False,"bundled":{"enabled":False}}
   class T:
    def rpc(self,method,params):
     self.assertions=(method,params)
     return {"config":config,"layers":[{"name":{"type":"sessionFlags"},"config":{}},{"name":{"type":"project","dotCodexFolder":"/private/path"},"config":{"mcp_servers":{"other":{"enabled":False}}},"disabledReason":"untrusted"},{"name":{"type":"user","file":"/private/config"},"config":{}},{"name":{"type":"system","file":"/etc/config"},"config":{}}]}
   omitted_false=next(k for k,v in config["features"].items() if not v);config["features"].pop(omitted_false)
   result=q.validate_effective(T(),plan,server,args,q.MISSING_TOOLS,fixture,"missing_runtime")
   self.assertEqual(result["sanitized"]["env_vars"],[]);self.assertEqual(result["approved_effective_sha256"],plan["phases"]["missing_runtime"]["effective_config_sha256"])
   config["mcp_servers"]["rust_engineering"]["environment_id"]="remote"
   with self.assertRaises(RuntimeError):q.validate_effective(T(),plan,server,args,q.MISSING_TOOLS,fixture,"missing_runtime")
   config["mcp_servers"]["rust_engineering"]["environment_id"]="local";config["features"].pop("mcp_2026_07_28")
   with self.assertRaises(RuntimeError):q.validate_effective(T(),plan,server,args,q.MISSING_TOOLS,fixture,"missing_runtime")

 def make_case(self,root,mode="ok",large_server=False,many_entries=False):
  private=root/"private";out=root/"out";fixture=root/"fixture"
  for d in (private,out,fixture):d.mkdir(mode=0o700)
  (fixture/"src").mkdir();(fixture/"src/lib.rs").write_text("bad\n");(fixture/"Cargo.toml").write_text("[package]\nname='x'\nversion='0.1.0'\n")
  auth=root/"auth.json";auth.write_text(json.dumps({"token":"super-secret-token-value"}));auth.chmod(0o600)
  server=root/"server";server.write_bytes(b"x"*(q.MAX_FILE_BYTES+1) if large_server else b"binary");server.chmod(0o700)
  code_host=root/"codex-code-mode-host";code_host_source=root/"codex-code-mode-host.c";code_host_source.write_text("#include <unistd.h>\nint main(void) { sleep(300); return 0; }\n")
  compiler=shutil.which("cc")
  if compiler is None:raise unittest.SkipTest("a C compiler is required for process-identity qualification tests")
  subprocess.run([compiler,str(code_host_source),"-o",str(code_host)],check=True,capture_output=True)
  docker=root/"docker";docker.write_text("#!/bin/sh\n[ \"$1\" = --host ] || exit 9\n[ \"$2\" = unix://"+str(root/"docker.sock")+" ] || exit 8\nshift 2\nif [ \"$1\" = version ]; then echo '29.7.2 arm64 linux'; exit 0; fi\nif [ \"$1\" = info ]; then date -u '+%Y-%m-%dT%H:%M:%SZ'; exit 0; fi\nif [ \"$1\" = image ] && [ \"$2\" = inspect ]; then echo sha256:"+"a"*64+"; exit 0; fi\nif [ \"$1\" = events ] && [ -f \"$(dirname \"$0\")/docker-events\" ]; then\n echo '{\"Action\":\"create\",\"Actor\":{\"Attributes\":{\"image\":\"sha256:"+"a"*64+"\"}},\"id\":\"c1\"}'\n echo '{\"Action\":\"start\",\"Actor\":{\"Attributes\":{\"image\":\"sha256:"+"a"*64+"\"}},\"id\":\"c1\"}'\nfi\nexit 0\n");docker.chmod(0o700)
  docker_socket=socket.socket(socket.AF_UNIX);docker_socket.bind(str(root/"docker.sock"));docker_socket.close()
  descriptors={n:{"name":n,"inputSchema":{},"outputSchema":{}} for n in q.TOOLS}
  schema_template=root/"schema-template";schema_template.mkdir(mode=0o700);(schema_template/"v2").mkdir();(schema_template/"v2/ThreadStartParams.json").write_text(json.dumps(THREAD_SCHEMA));(schema_template/"v2/TurnStartParams.json").write_text(json.dumps(TURN_SCHEMA));inventory_count=600 if many_entries else 0
  if inventory_count:
   (schema_template/"bulk").mkdir()
   for i in range(inventory_count):(schema_template/"bulk"/f"{i:04}.json").write_text("{}")
  schema_digest=q.closed_bundle_digest(schema_template)
  repair_args=["serve","--stdio","--root","$FIXTURE","--docker","$DOCKER","--docker-socket",str(root/"docker.sock"),"--state-root","$STATE","--rust-image","sha256:"+"a"*64]
  feature_world={**{name:False for name in q.DISABLED},"future_feature_not_allowlisted":True,"mcp_2026_07_28":False,"code_mode_host":True,"skip_host_skill_discovery":False}
  literal={"descriptors":descriptors,"repair_tools":list(q.REPAIR_TOOLS),"missing_tools":list(q.MISSING_TOOLS),"disabled":list(q.DISABLED),"feature_world":feature_world,"thread_schema":THREAD_SCHEMA,"turn_schema":TURN_SCHEMA,"inventory_count":inventory_count,"mode":mode,"docker":str(docker),"socket":str(root/"docker.sock"),"missing":MISSING_MESSAGE,"runtime":{"image_id":"sha256:"+"a"*64,"rust_version":"1.90.0","cargo_version":"1.90.0","platform":"linux/aarch64","configuration_fingerprint":"sha256:"+"b"*64,"execution_fingerprint":"sha256:"+"c"*64}}
  fake=root/"codex";fake.write_text("#!/usr/bin/env python3\nDATA="+repr(literal)+"\n"+textwrap.dedent('''
import json,os,subprocess,sys,time
from pathlib import Path
if '--version' in sys.argv:print('codex-cli 0.153.0');raise SystemExit
if sys.argv[1:3]==['features','list']:
 [print(name,'stable',str(value).lower()) for name,value in DATA['feature_world'].items()];raise SystemExit
if sys.argv[1:4]==['app-server','generate-json-schema','--experimental']:
 out=Path(sys.argv[sys.argv.index('--out')+1]);(out/'v2').mkdir();(out/'v2/ThreadStartParams.json').write_text(json.dumps(DATA['thread_schema']));(out/'v2/TurnStartParams.json').write_text(json.dumps(DATA['turn_schema']))
 if DATA['inventory_count']:
  (out/'bulk').mkdir()
  [(out/'bulk'/f'{i:04}.json').write_text('{}') for i in range(DATA['inventory_count'])]
 raise SystemExit
joined=' '.join(sys.argv);phase='repair' if '--rust-image' in joined else 'missing_runtime';cwd=os.getcwd();state=str(Path(cwd).parent/'state-repair');staged_docker=str(Path(sys.argv[0]).parent/'docker')
code_host=subprocess.Popen([str(Path(sys.argv[0]).with_name('codex-code-mode-host')),'300'])
time.sleep(.3)
required=['--root',cwd]
if phase=='repair':required += ['--docker',staged_docker,'--docker-socket',DATA['socket'],'--state-root',state,'--rust-image','sha256:'+'a'*64]
if not all(x in joined for x in required) or any(x in joined for x in ('--catalog-store','--rustsec-snapshot')):raise SystemExit(12)
def send(x):print(json.dumps(x),flush=True)
def event(item,thread='th'):send({'method':'item/completed','params':{'threadId':thread,'turnId':'tu','item':item}})
def payload(status,ref=None,code=None,runtime=None):
 data={'project_ref':ref} if ref else (None if status=='blocked' else {})
 if code:data={'log':{'uri':'rust-artifact://prj_fixture/art_diagnostic'}}
 if runtime is not None:data={'runtime':runtime}
 return {'data':data,'diagnostics':[{'code':code}] if code else [],'duration_ms':1,'error_code':'SANDBOX_DENIED' if status=='blocked' else None,'error_message':DATA['missing'] if status=='blocked' else None,'evidence':{'kind':'local'},'status':status,'summary':status,'truncation':{'diagnostics_omitted':0}}
def call(name,status,ref=None,code=None,arg_ref=None,runtime=None,thread='th'):
 args={'path':cwd} if name=='rust.project.open' else ({'project_ref':arg_ref} if arg_ref else {})
 event({'id':name+str(ref)+status,'type':'mcpToolCall','server':'rust_engineering','tool':name,'status':'failed' if status=='blocked' else 'completed','arguments':args,'error':None,'result':{'content':[],'structuredContent':payload(status,ref,code,runtime)}},thread)
for line in sys.stdin:
 m=json.loads(line);method=m.get('method');ident=m.get('id')
 if ident is None:continue
 if method=='initialize':send({'id':ident,'result':{'platformFamily':'unix','platformOs':'macos','userAgent':'fake'}})
 elif method=='model/list':send({'id':ident,'result':{'data':[{'id':'gpt-5.6-sol','supportedReasoningEfforts':[{'reasoningEffort':'medium'}]}],'nextCursor':None}})
 elif method=='config/read':
  tools=DATA['repair_tools'] if phase=='repair' else DATA['missing_tools'];args=['serve','--stdio','--root',cwd]
  if phase=='repair':args += ['--docker',staged_docker,'--docker-socket',DATA['socket'],'--state-root',state,'--rust-image','sha256:'+'a'*64]
  server={'command':str(Path(sys.argv[0]).parent/'rust-engineering-mcp'),'args':args,'cwd':'/','env':{},'env_vars':[],'enabled':True,'required':True,'enabled_tools':tools,'disabled_tools':[x for x in DATA['descriptors'] if x not in tools],'startup_timeout_sec':45,'tool_timeout_sec':300,'default_tools_approval_mode':'approve'}
  features={x:(x in ('mcp_2026_07_28','code_mode_host','skip_host_skill_discovery')) for x in DATA['feature_world']}
  if DATA['mode']=='feature_drift':features['future_feature_not_allowlisted']=True
  cfg={'model':'gpt-5.6-sol','model_reasoning_effort':'medium','model_provider':'openai','web_search':'enabled' if DATA['mode']=='config' else 'disabled','sandbox_mode':'workspace-write','approval_policy':'never','notify':[],'project_doc_max_bytes':0,'project_doc_fallback_filenames':[],'developer_instructions':'','instructions':'','features':features,'agents':{'enabled':False},'orchestrator':{'skills':{'enabled':False},'mcp':{'enabled':False}},'skills':{'include_instructions':False,'bundled':{'enabled':False}},'mcp_servers':{'rust_engineering':server}}
  send({'id':ident,'result':{'config':cfg,'layers':[{'type':'commandLine'}]}})
 elif method=='mcpServerStatus/list':
  tools=DATA['repair_tools'] if phase=='repair' else DATA['missing_tools'];descriptors={x:DATA['descriptors'][x] for x in tools}
  if DATA['mode']=='descriptor_drift':descriptors[tools[0]]={**descriptors[tools[0]],'description':'unapproved'}
  send({'id':ident,'result':{'data':[{'name':'rust_engineering','runtimeStatus':None,'tools':descriptors,'serverInfo':{'name':'rust-engineering-mcp','version':'x'}}],'nextCursor':None}})
 elif method=='thread/start':send({'id':ident,'result':{'thread':{'id':'th','sessionId':'th'},'threadProvider':'openai','instructionSources':[]}})
 elif method=='turn/start':
  send({'id':ident,'result':{'turn':{'id':'tu','status':'inProgress'}}})
  mode=DATA['mode']
  if mode=='timeout':time.sleep(60);continue
  if phase=='missing_runtime':
   if mode!='late_discovery':send({'method':'rawResponseItem/completed','params':{'threadId':'th','turnId':'tu','item':{'type':'mcp_list_tools','id':'list1','tools':[{'name':x} for x in DATA['missing_tools']]}}})
   call('rust.project.open','passed','pm',thread='foreign' if mode=='foreign' else 'th')
   if mode=='late_discovery':send({'method':'rawResponseItem/completed','params':{'threadId':'th','turnId':'tu','item':{'type':'mcp_list_tools','id':'list1','tools':[{'name':x} for x in DATA['missing_tools']]}}})
   if mode=='duplicate_raw':
    send({'method':'rawResponseItem/completed','params':{'threadId':'th','turnId':'tu','item':{'type':'message','id':'duplicate'}}})
    send({'method':'rawResponseItem/completed','params':{'threadId':'th','turnId':'tu','item':{'type':'reasoning','id':'duplicate'}}})
   if mode=='auth_change':Path(os.environ['CODEX_HOME'],'auth.json').write_text(json.dumps({'token':'refreshed-secret-token'}))
   if mode=='fixture':Path(cwd,'Cargo.toml').write_text('tampered')
   if mode=='raw_type':send({'method':'rawResponseItem/completed','params':{'threadId':'th','turnId':'tu','item':{'type':'unexpected_raw'}}})
   if mode=='raw_missing':send({'method':'rawResponseItem/completed','params':{'threadId':'th','turnId':'tu','item':{'type':'custom_tool_call','id':'r','call_id':'c','name':'apply_patch','status':'completed','input':json.dumps({'patch':'*** Update File: src/lib.rs'})}}})
   call('rust.check','blocked',arg_ref='pm')
  else:
   if DATA['inventory_count']:
    state_bulk=Path(state,'bulk');state_bulk.mkdir();tmp_bulk=Path(os.environ['TMPDIR'],'bulk');tmp_bulk.mkdir()
    [(state_bulk/f'{i:04}.state').write_text('x') for i in range(DATA['inventory_count'])]
    [(tmp_bulk/f'{i:04}.tmp').write_text('x') for i in range(DATA['inventory_count'])]
   Path(staged_docker).parent.joinpath('docker-events').write_text('observed')
   event({'id':'plan1','type':'plan','status':'completed','text':'Inspect, check, repair and re-check.'})
   send({'method':'rawResponseItem/completed','params':{'threadId':'th','turnId':'tu','item':{'type':'mcp_list_tools','id':'list1','tools':[{'name':x} for x in DATA['repair_tools']]}}})
   call('rust.project.open','passed','p1');call('rust.project.inspect','passed',arg_ref='wrong' if mode=='ref' else 'p1');call('rust.check','failed',code='OTHER' if mode=='e0502' else 'E0502',arg_ref='p1')
   send({'method':'rawResponseItem/completed','params':{'threadId':'th','turnId':'tu','item':{'type':'reasoning','id':'reason1'}}})
   if mode=='disallowed':call('rust.test','passed',arg_ref='p1')
   patch=json.dumps({'patch':'*** Begin Patch\\n*** Update File: src/lib.rs\\n@@\\n-bad\\n+good\\n*** End Patch'})
   send({'method':'rawResponseItem/completed','params':{'threadId':'th','turnId':'tu','item':{'type':'custom_tool_call','id':'raw1','call_id':'call1','name':'apply_patch','status':'completed','input':patch}}})
   send({'method':'rawResponseItem/completed','params':{'threadId':'th','turnId':'tu','item':{'type':'message','id':'message1'}}})
   send({'method':'rawResponseItem/completed','params':{'threadId':'th','turnId':'tu','item':{'type':'custom_tool_call_output','id':'raw2','call_id':'call1','status':'completed','output':'Done'}}})
   Path(cwd,'src/lib.rs').write_text('different\\n' if mode=='patch_mismatch' else 'good\\n')
   if mode=='target':Path(cwd,'target').mkdir()
   event({'id':'file1','type':'fileChange','status':'completed','changes':[{'path':'src/lib.rs','kind':{'type':'update'},'diff':'@@\\n-bad\\n+good\\n'}]});call('rust.project.open','passed','p2');runtime=dict(DATA['runtime']);runtime['image_id']='sha256:'+'d'*64 if mode=='runtime' else runtime['image_id'];call('rust.check','passed',arg_ref='p2',runtime=runtime)
   if mode=='canary':Path(staged_docker).parent.parent.joinpath('authority-canary').write_text('tampered')
   if mode=='binary_tamper':Path(staged_docker).with_name('rust-engineering-mcp').write_bytes(b'tampered')
  send({'method':'thread/tokenUsage/updated','params':{'threadId':'th','turnId':'tu','tokenUsage':{'outputTokens':17}}});send({'method':'turn/completed','params':{'threadId':'th','turnId':'tu','turn':{'id':'tu','status':'completed','error':None}}})
code_host.terminate();code_host.wait(timeout=5)
'''));fake.chmod(0o700)
  phases={"repair":{"prompt":"repair "+mode,"prompt_sha256":q.digest(("repair "+mode).encode()),"server_args":repair_args,"enabled_tools":list(q.REPAIR_TOOLS),"effective_config_sha256":"0"*64,"descriptor_sha256":q.digest(q.enc({x:descriptors[x] for x in q.REPAIR_TOOLS}))},"missing_runtime":{"prompt":"missing "+mode,"prompt_sha256":q.digest(("missing "+mode).encode()),"server_args":["serve","--stdio","--root","$FIXTURE"],"enabled_tools":list(q.MISSING_TOOLS),"effective_config_sha256":"0"*64,"descriptor_sha256":q.digest(q.enc({x:descriptors[x] for x in q.MISSING_TOOLS}))}}
  plan={"schema_version":4,"codex_source":str(fake),"codex_sha256":q.file_digest(fake),"codex_version":"codex-cli 0.153.0","code_host_source":str(code_host),"code_host_sha256":q.file_digest(code_host),"server_source":str(server),"server_sha256":q.file_digest(server),"docker_cli":str(docker),"docker_sha256":q.file_digest(docker),"docker_socket":str(root/"docker.sock"),"docker_label":q.PRODUCT_DOCKER_LABEL,"auth_source":str(auth),"private_root":str(private),"output_root":str(out),"fixture_root":str(fixture),"fixture_snapshot":q.fs_snapshot(fixture),"model":"gpt-5.6-sol","effort":"medium","budgets":{"missing_runtime":{"wall_seconds":30,"max_output_tokens":100,"rpc_timeout_seconds":10},"repair":{"wall_seconds":30,"max_output_tokens":100,"rpc_timeout_seconds":10},"cleanup_seconds":30},"schema_bundle_sha256":schema_digest,"feature_world":feature_world,"allowed_config_layer_types":["builtIn","commandLine"],"runtime":{"image_id":"sha256:"+"a"*64,"rust_version":"1.90.0","cargo_version":"1.90.0","platform":"linux/aarch64"},"missing_error_message":MISSING_MESSAGE,"phases":phases}
  for name in phases:phases[name]["effective_config_sha256"]=q.expected_effective_hash(plan,name)
  q.remove_owned_tree(schema_template)
  return plan,private,out,fixture,auth

 def test_fake_end_to_end_source_immutable_and_cleanup(self):
  with tempfile.TemporaryDirectory() as td:
   plan,private,out,fixture,auth=self.make_case(Path(td).resolve());source=q.fs_snapshot(fixture);auth_hash=q.file_digest(auth);receipt=out/"receipt.json";transcript=out/"transcript.jsonl";result=q.execute(plan,receipt,transcript)
   self.assertEqual(result["status"],"passed",result);self.assertTrue(result["repaired_source"]["patch_application_verified"]);self.assertEqual(q.fs_snapshot(fixture),source);self.assertEqual(q.file_digest(auth),auth_hash);self.assertTrue(private.exists());self.assertEqual(list(private.iterdir()),[]);self.assertEqual(stat.S_IMODE((out/"repaired-src-lib.rs").stat().st_mode),0o600);self.assertEqual(list(result["phases"]),["missing_runtime","repair"]);self.assertNotIn(b"super-secret-token-value",receipt.read_bytes()+transcript.read_bytes())
   for phase in result["phases"].values():
    self.assertEqual(phase["inventory"]["descriptor_sha256"],phase["inventory"]["descriptor_sha256_approved"])
    self.assertTrue(phase["cleanup"]["code_host_observed"])
    self.assertTrue(any(row["role"]=="codex-code-mode-host" and row["executable_sha256"]==plan["code_host_sha256"] for row in phase["cleanup"]["descendant_identities"]))
 def test_large_staged_binary_is_excluded_from_fixture_budget(self):
  with tempfile.TemporaryDirectory() as td:
   plan,private,out,fixture,auth=self.make_case(Path(td).resolve(),large_server=True);result=q.execute(plan,out/"receipt.json",out/"transcript.jsonl")
   self.assertEqual(result["status"],"passed",result.get("errors"));self.assertEqual(list(result["phases"]),["missing_runtime","repair"]);self.assertEqual(result["private_inventory_before_phases"]["bin"]["contents_excluded"],"staged identities recorded separately")
 def test_large_private_and_schema_bundle_entry_inventories(self):
  with tempfile.TemporaryDirectory() as td:
   plan,private,out,fixture,auth=self.make_case(Path(td).resolve(),many_entries=True);result=q.execute(plan,out/"receipt.json",out/"transcript.jsonl")
   self.assertEqual(result["status"],"passed",result.get("errors"));self.assertIn("schema/bulk/0599.json",result["private_inventory_before_phases"]);self.assertIn("work/state-repair/bulk/0599.state",result["private_inventory_on_exit"]);self.assertIn("tmp-repair/bulk/0599.tmp",result["private_inventory_on_exit"]);self.assertTrue(result["owned_cleanup"]["owned_child_removed"]);self.assertEqual(list(private.iterdir()),[])
 def test_fake_rejects_foreign_raw_and_disallowed_with_receipts(self):
  for mode in ("foreign","raw_type","raw_missing","disallowed","duplicate_raw","late_discovery"):
   with self.subTest(mode=mode),tempfile.TemporaryDirectory() as td:
    plan,private,out,fixture,auth=self.make_case(Path(td).resolve(),mode);result=q.execute(plan,out/"receipt.json",out/"transcript.jsonl");self.assertEqual(result["status"],"failed");self.assertTrue((out/"receipt.json").exists());self.assertTrue(private.exists())
 def test_negative_runtime_identity(self):self.assert_mode_failed("runtime")
 def test_negative_e0502_identity(self):self.assert_mode_failed("e0502")
 def test_negative_project_ref_binding(self):self.assert_mode_failed("ref")
 def test_negative_effective_config(self):self.assert_mode_failed("config")
 def test_negative_phase_descriptor_drift(self):self.assert_mode_failed("descriptor_drift")
 def test_negative_unallowlisted_feature_drift(self):self.assert_mode_failed("feature_drift")
 def test_negative_canary_tamper(self):self.assert_mode_failed("canary")
 def test_negative_post_run_candidate_identity(self):self.assert_mode_failed("binary_tamper")
 def test_negative_fixture_tamper(self):self.assert_mode_failed("fixture")
 def test_negative_target_artifact(self):self.assert_mode_failed("target")
 def test_negative_patch_content_binding(self):self.assert_mode_failed("patch_mismatch")
 def test_negative_schema_bundle(self):
  with tempfile.TemporaryDirectory() as td:
   plan,private,out,fixture,auth=self.make_case(Path(td).resolve());plan["schema_bundle_sha256"]="f"*64;result=q.execute(plan,out/"receipt.json",out/"transcript.jsonl");self.assertEqual(result["status"],"failed")
 def test_negative_docker_lifecycle_and_image(self):
  empty={"containers":[],"volumes":[],"networks":[]}
  with self.assertRaises(RuntimeError):q.validate_phase_docker("repair",[{"action":"create","image":"sha256:"+"d"*64}],empty,"sha256:"+"a"*64)
  with self.assertRaises(RuntimeError):q.validate_phase_docker("missing_runtime",[{"action":"start","image":"sha256:"+"a"*64}],empty,"sha256:"+"a"*64)
 def test_negative_missing_docker_inventory(self):
  inventory={"containers":["unexpected"],"volumes":[],"networks":[]}
  with self.assertRaises(RuntimeError):q.validate_phase_docker("missing_runtime",[],inventory,"sha256:"+"a"*64)
 def test_negative_state_root_escape(self):
  with self.assertRaises(ValueError):q.run_phase("repair",{},Path("/codex"),Path("/server"),Path("/docker"),Path("/home"),Path("/tmp"),Path("/owned/fixture"),Path("/outside"),None,0,[],{})
 def test_auth_mutation_fails_and_preserves_owned_home(self):
  with tempfile.TemporaryDirectory() as td:
   plan,private,out,fixture,auth=self.make_case(Path(td).resolve(),"auth_change");source=q.file_digest(auth);result=q.execute(plan,out/"receipt.json",out/"transcript.jsonl")
   self.assertEqual(result["status"],"failed");self.assertTrue(result["auth_copy"]["phases"]["missing_runtime"]["changed"]);self.assertEqual(q.file_digest(auth),source);self.assertEqual(len(list(private.iterdir())),1);self.assertTrue(result["owned_cleanup"]["preserved_for_auth_recovery"])
 def test_transcript_rejects_secret_before_write(self):
  with tempfile.TemporaryDirectory() as td:
   path=Path(td)/"t.jsonl";log=q.Transcript(path);log.set_needles([b"protected-value"])
   with self.assertRaises(RuntimeError):log.add("x","event",{"value":"protected-value"})
   log.close();self.assertNotIn(b"protected-value",path.read_bytes())
 def test_transcript_rejects_unicode_secret_in_json_escaped_form(self):
  with tempfile.TemporaryDirectory() as td:
   path=Path(td)/"t.jsonl";log=q.Transcript(path);log.set_needles(["tøkén-value".encode()])
   with self.assertRaises(RuntimeError):log.add("x","event",{"value":"tøkén-value"})
   log.close();self.assertEqual(path.read_bytes(),b"")
 def test_prompt_anticoaching_rejects_host_action_names(self):
  with tempfile.TemporaryDirectory() as td:
   plan,private,out,fixture,auth=self.make_case(Path(td).resolve());plan["phases"]["repair"]["prompt"]="Please use APPLY_PATCH";plan["phases"]["repair"]["prompt_sha256"]=q.digest(plan["phases"]["repair"]["prompt"].encode())
   with self.assertRaises(ValueError):q.validate_plan(plan,live=False)
 def test_timeout_forces_close_and_writes_receipt(self):
  with tempfile.TemporaryDirectory() as td:
   plan,private,out,fixture,auth=self.make_case(Path(td).resolve(),"timeout");plan["budgets"]["missing_runtime"]["wall_seconds"]=3;original=q.validate_numbers;q.validate_numbers=lambda unused:None
   try:result=q.execute(plan,out/"receipt.json",out/"transcript.jsonl")
   finally:q.validate_numbers=original
   stored=json.loads((out/"receipt.json").read_text());records=list(map(json.loads,(out/"transcript.jsonl").read_text().splitlines()));process=next(x["data"] for x in records if x["kind"]=="process");cleanup=next(x["data"] for x in records if x["kind"]=="cleanup");self.assertEqual(result["status"],"failed");self.assertEqual(stored["status"],"failed");self.assertTrue(cleanup["forced"]);self.assertEqual(cleanup["pgid"],process["pid"]);self.assertEqual(cleanup["remaining_pids"],[])
 def test_transport_rejects_foreign_pid_without_killing_it(self):
  with tempfile.TemporaryDirectory() as td:
   root=Path(td);log=q.Transcript(root/"t.jsonl");transport=q.Transport("pid",[sys.executable,"-c","import time; time.sleep(.3)"],root,root,root,log,time.monotonic()+5);transport.observed.add(os.getpid());result=transport.close();log.close()
   self.assertIn(os.getpid(),result["remaining_pids"]);self.assertIn("foreign or reused pid",result["failure"])
 def test_transport_close_is_idempotent_and_invalid_close_is_typed(self):
  with tempfile.TemporaryDirectory() as td:
   root=Path(td);log=q.Transcript(root/"t.jsonl");transport=q.Transport("pid",[sys.executable,"-c","pass"],root,root,root,log,time.monotonic()+5);first=transport.close();second=transport.close();log.close();self.assertIs(first,second);self.assertTrue(transport.closed)
  with self.assertRaises(q.TransportCloseError):q.require_transport_closed("repair",{"exit_code":1,"forced":False,"threads_joined":True,"remaining_pids":[],"failure":None})
 def test_transport_rejects_unapproved_descendant_executable(self):
  with tempfile.TemporaryDirectory() as td:
   root=Path(td);launcher=root/"launcher.py";launcher.write_text("import subprocess,time\np=subprocess.Popen(['/bin/sleep','2'])\ntime.sleep(1)\np.terminate()\np.wait()\n")
   approved=root/"approved";approved.write_bytes(b"not executed")
   log=q.Transcript(root/"t.jsonl");transport=q.Transport("pid",[sys.executable,str(launcher)],root,root,root,log,time.monotonic()+5,allowed_executables={str(approved):("codex-code-mode-host",q.file_digest(approved))},required_role="codex-code-mode-host")
   time.sleep(.5);result=transport.close();log.close();self.assertIn("unexpected descendant executable:sleep",result["failure"]);self.assertFalse(result["code_host_observed"])
 def test_cleanup_failure_is_recorded_in_failure_receipt(self):
  with tempfile.TemporaryDirectory() as td:
   plan,private,out,fixture,auth=self.make_case(Path(td).resolve());original=q.docker_cleanup
   def fail_cleanup(*args,**kwargs):raise RuntimeError("forced docker cleanup failure")
   q.docker_cleanup=fail_cleanup
   try:result=q.execute(plan,out/"receipt.json",out/"transcript.jsonl")
   finally:q.docker_cleanup=original
   stored=json.loads((out/"receipt.json").read_text());self.assertEqual(result["status"],"failed");self.assertEqual(stored["status"],"failed");self.assertTrue(any(x["type"]=="docker_cleanup" for x in stored["errors"]))
 def test_main_malformed_plan_receipt(self):
  with tempfile.TemporaryDirectory() as td:
   root=Path(td).resolve();out=root/"out";out.mkdir(mode=0o700);plan=root/"bad.json";plan.write_text("{");receipt=out/"receipt.json"
   old=list(__import__('sys').argv);__import__('sys').argv=["q",str(plan),"--receipt",str(receipt),"--transcript",str(out/"t"),"--approved-plan-sha256","x","--approved-repair-prompt-sha256","x","--approved-missing-prompt-sha256","x"]
   try:self.assertEqual(q.main(),1)
   finally:__import__('sys').argv=old
   self.assertTrue(receipt.exists());self.assertEqual(json.loads(receipt.read_text())["status"],"failed")
if __name__=="__main__":unittest.main()
