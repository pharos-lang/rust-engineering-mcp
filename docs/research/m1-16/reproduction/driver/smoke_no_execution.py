"""Trusted driver IPC denial smoke only: never sends a valid execute or MCP init."""
import json,pathlib,selectors,signal,subprocess,time
root=pathlib.Path(__file__).resolve().parents[2]
binary=root/'target/debug/m1-16-trusted-driver'
base=pathlib.Path(__file__).resolve().parent
results=[]
def read(child):
 with selectors.DefaultSelector() as selector:
  selector.register(child.stdout,selectors.EVENT_READ)
  assert selector.select(3),'response timeout'
  return json.loads(child.stdout.readline(1048577))
def send(child,value):
 child.stdin.write(json.dumps(value).encode()+b'\n');child.stdin.flush();return read(child)
init={'mode':'raw','server_binary':str(root/'target/debug/rust-engineering-mcp'),'root':str(base),'state_root':str(base/'UNUSED_STATE'),'docker_socket':str(base/'UNUSED_SOCKET')}
for cancel in [False,True]:
 child=subprocess.Popen([str(binary)],env={},stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
 try:
  assert send(child,init)=={'ready':True,'ipc_version':1,'server_pid':None,'negotiated_protocol':None}
  assert send(child,{'op':'execute','files':[{'path':'../escape','text':'x'}],'command':'check'})=={'driver_error':'denied_source_path'}
  assert send(child,{'op':'close','extra':True})=={'driver_error':'invalid_request'}
  if cancel:
   child.send_signal(signal.SIGINT);response=read(child);assert response['driver_error']=='cancelled' and response['success'] is False
   assert response['execution_joined'] is True and response['server_joined'] is False
   assert child.wait(timeout=3)==1
  else:
   assert send(child,{'op':'close'})=={'closed':True,'execution_joined':True,'server_joined':False,'stderr':None};assert child.wait(timeout=3)==0
  results.append({'case':'signal_permanent_cancel' if cancel else 'closed_ipc_denials','status':'passed'})
 finally:
  if child.poll() is None:child.kill();child.wait(timeout=3)
  for stream in [child.stdin,child.stdout,child.stderr]:stream.close()
assert not (base/'UNUSED_STATE').exists()
(base/'smoke-results.json').write_text(json.dumps({'status':'passed','no_valid_execution_requests':True,'cases':results},indent=2)+'\n')
