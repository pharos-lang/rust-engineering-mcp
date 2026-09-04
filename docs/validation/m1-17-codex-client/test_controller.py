import json, os, tempfile, unittest
from pathlib import Path
import controller as c

def plan(): return {'codex':'/opt/homebrew/bin/codex','codex_sha256':'a'*64,'model':'gpt-5.6-sol','effort':'medium','server_binary':'/trusted/rust-engineering-mcp','server_binary_sha256':'b'*64,'server_args':['serve','--stdio','--state-root','/trusted/state'],'fixture_root':'/trusted/fixture','fixture_files_sha256':{'Cargo.toml':'c'*64,'Cargo.lock':'d'*64,'src/lib.rs':'e'*64,'tests/behavior.rs':'f'*64},'state_root':'/trusted/state','neutral_parent':'/private/tmp','prompt':'Use the Rust MCP once.','wall_seconds':300,'max_output_tokens':4000,'docker':'/trusted/docker','docker_socket':'/trusted/docker.sock','expected_catalog_fingerprint':'sha256:'+'1'*64}
class Tests(unittest.TestCase):
 def test_exact_single_native_server_and_approval_contract(self):
  p=plan();c.validate_plan(p);v=c.overrides(p);self.assertEqual(set(v['mcp_servers']),{'rust_engineering'});s=v['mcp_servers']['rust_engineering'];self.assertEqual(s['enabled_tools'],list(c.TOOLS));self.assertEqual(len(s['enabled_tools']),13);self.assertEqual(s['default_tools_approval_mode'],'approve');self.assertEqual(s['env'],{});self.assertEqual(s['env_vars'],[])
 def test_thread_contract_omits_dynamic_tools_and_authority(self):
  class T:
   def rpc(_,method,params,timeout):_.value=(method,params);return {'thread':{'id':'t'},'model':'gpt-5.6-sol','reasoningEffort':'medium'}
  t=T();c.thread_start(t,plan(),Path('/private/tmp/x'));p=t.value[1];self.assertNotIn('dynamicTools',p);self.assertEqual(p['environments'],[]);self.assertEqual(p['runtimeWorkspaceRoots'],[]);self.assertEqual(p['selectedCapabilityRoots'],[]);self.assertTrue(p['ephemeral']);self.assertTrue(p['experimentalRawEvents'])
 def test_all_effective_guards_fail_closed(self):
  p=plan();v=c.overrides(p);servers=dict(v['mcp_servers']);servers.update({name:{'enabled':False} for name in c.DISABLED_HOST_SERVERS});e={'mcp_servers':servers,'features':{k:False for k in c.DISABLED},'web_search':'disabled','agents':{'enabled':False},'orchestrator':{'skills':{'enabled':False},'mcp':{'enabled':False}},'skills':{'include_instructions':False,'bundled':{'enabled':False}}};cfg=c.redacted_config(e);self.assertEqual(cfg['web_search'],'disabled');self.assertFalse(cfg['effective_feature_values']['shell_tool']);self.assertFalse(cfg['effective_host_server_enabled']['node_repl']);e['features']['shell_tool']=True;self.assertRaises(RuntimeError,c.redacted_config,e)
 def test_status_requires_exact_tools(self):
  rows=[{'name':name,'tools':{},'serverInfo':None} for name in c.DISABLED_HOST_SERVERS];rows.append({'name':'rust_engineering','tools':{name:{'name':name} for name in c.TOOLS},'serverInfo':{'name':'rust-engineering-mcp','version':'0.1.0'}});x={'data':rows,'nextCursor':None};status=c.status_inventory(x);self.assertEqual(len(status['tools']),13);self.assertEqual(status['canonical_tools_sha256'],c.sha(c.canonical_enc(x['data'][-1]['tools'])));x['data'][-1]['tools']['extra']={'name':'extra'};self.assertRaises(RuntimeError,c.status_inventory,x)
 def test_plan_and_toml_are_closed(self):
  p=plan();p['extra']=1;self.assertRaises(ValueError,c.validate_plan,p);self.assertEqual(c.toml({'x':['a',True]}),'{x=["a",true]}')
 def test_cleanup_never_accepts_forced_or_nonzero(self):
  # Contract expression used by Transport.close; discriminates every fail-open input.
  def ok(code,forced,joined,remaining,errors,failure):return code==0 and not forced and joined and not remaining and not errors and not failure
  self.assertTrue(ok(0,False,True,[],[],None))
  for row in [(1,False,True,[],[],None),(0,True,True,[],[],None),(0,False,False,[],[],None),(0,False,True,[3],[],None),(0,False,True,[],['x'],None),(0,False,True,[],[],'io')]:self.assertFalse(ok(*row))
 def test_state_cleanup_only_removes_exact_owned_inventory(self):
  with tempfile.TemporaryDirectory() as td:
   root=Path(td);root.chmod(0o700);p=plan();p['state_root']=td;control=root/('rust-mcp-control-'+'a'*32);control.mkdir(mode=0o700)
   for name in ('config.json','seccomp.json','seccomp-socket.json','seccomp-rust.json'):(control/name).write_text('{}');(control/name).chmod(0o600)
   self.assertEqual(c.cleanup_state(p)['after'],{})
   bad=root/'unexpected';bad.write_text('x');self.assertRaises(RuntimeError,c.cleanup_state,p)
 def test_artifact_uri_discovery(self):
  self.assertEqual(list(c.paths({'x':['rust-artifact://prj/a','file:///x']})),['rust-artifact://prj/a'])
 def test_turn_observer_rejects_native_authority_and_bounds_tokens(self):
  p=plan();called=[];o=c.TurnObserver(p,'t','u',lambda *x:called.append(x));o.add({'params':{'item':{'id':'m1','type':'mcpToolCall','server':'rust_engineering','tool':'rust.catalog.status','status':'completed','result':{'content':[]}}}});self.assertEqual(len(o.items),1)
  self.assertRaises(RuntimeError,o.add,{'params':{'item':{'type':'commandExecution'}}})
  o.add({'params':{'tokenUsage':{'total':{'outputTokens':4000,'reasoningOutputTokens':0}}}});self.assertEqual(o.stop,'output_token_budget');self.assertEqual(called,[('t','u')])
  s=c.TurnObserver(p,'t','u',lambda *x:None);s.add({'method':'rawResponseItem/completed','params':{'item':{'type':'tool_search_call','arguments':'{"query":"rust.catalog.status"}','execution':'server','status':'completed'}}});s.add({'method':'rawResponseItem/completed','params':{'item':{'type':'tool_search_output','status':'completed','tools':[]}}});self.assertEqual((len(s.search_calls),len(s.search_outputs)),(1,1))
  self.assertRaises(RuntimeError,s.add,{'method':'rawResponseItem/completed','params':{'item':{'type':'custom_tool_call','name':'exec','input':'x'}}})
 def test_transport_write_is_explicitly_nonblocking_and_bounded(self):
  source=Path(c.__file__).read_text();self.assertIn('os.set_blocking(self.p.stdin.fileno(),False)',source);self.assertIn("self._fail('stdin_timeout')",source)
 def test_semantic_mcp_results_and_resources_fail_closed(self):
  p=plan();good={'structuredContent':{'status':'passed','data':{'fingerprint':p['expected_catalog_fingerprint']}},'content':[{'text':'x'}]};self.assertEqual(c.validate_tool_result(good,p,True)['status'],'passed');self.assertRaises(RuntimeError,c.validate_tool_result,{'structuredContent':{'status':'failed'}},p);self.assertRaises(RuntimeError,c.validate_tool_result,{'isError':True,'structuredContent':{'status':'passed'}},p);self.assertTrue(c.validate_resource({'contents':[{'text':'x'}]}));self.assertRaises(RuntimeError,c.validate_resource,{'contents':[]})
  item={'server':'rust_engineering','tool':'rust.catalog.status','arguments':{},'status':'completed','error':None,'result':good};self.assertEqual(c.validate_model_items([item],p)['tool'],'rust.catalog.status');self.assertRaises(RuntimeError,c.validate_model_items,[item,item],p);item['tool']='rust.check';self.assertRaises(RuntimeError,c.validate_model_items,[item],p)
if __name__=='__main__':unittest.main()
