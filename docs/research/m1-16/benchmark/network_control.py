"""Harmless loopback-only positive/negative controls for the broker host profile."""
import errno,json,socket,subprocess,sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'m1-16-controller'))
from broker import HOST_PROFILE
rows=[]
for family,host in [(socket.AF_INET,'127.0.0.1'),(socket.AF_INET6,'::1')]:
 with socket.socket(family,socket.SOCK_STREAM) as listener:
  listener.bind((host,0));listener.listen(4);port=listener.getsockname()[1]
  code='import socket,json\ns=socket.socket('+str(int(family))+',socket.SOCK_STREAM);s.settimeout(2)\ntry:\n s.connect(('+repr(host)+','+str(port)+'));print(json.dumps({"connected":True}))\nexcept OSError as e:print(json.dumps({"connected":False,"errno":e.errno}))\nfinally:s.close()'
  outside=subprocess.run([sys.executable,'-c',code],capture_output=True,text=True,env={},timeout=5)
  inside=subprocess.run(['/usr/bin/sandbox-exec','-p',HOST_PROFILE,sys.executable,'-c',code],capture_output=True,text=True,env={},timeout=5)
  a=json.loads(outside.stdout);b=json.loads(inside.stdout)
  rows.append({'family':int(family),'outside':a,'inside':b,'passed':outside.returncode==inside.returncode==0 and a['connected'] and not b['connected'] and b['errno'] in [errno.EPERM,errno.EACCES]})
receipt={'host_profile':HOST_PROFILE,'scope':'loopback IPv4/IPv6 connect controls only; no outbound Internet probe; no general syscall/network audit','controls':rows,'passed':all(r['passed'] for r in rows)}
Path(__file__).with_name('network-control.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps(receipt));sys.exit(0 if receipt['passed'] else 1)
