from pathlib import Path
import json,hashlib,urllib.request,urllib.parse,datetime,time
root=Path(__file__).resolve().parent
path=root/'selection/facts.json';doc=json.loads(path.read_text());receipts=[]
for fact in doc['facts']:
 name,version=fact['name'],fact['version'];url='https://crates.io/api/v1/crates/'+urllib.parse.quote(name,safe='')+'/'+urllib.parse.quote(version,safe='')
 stamp=datetime.datetime.now(datetime.timezone.utc).isoformat();entry={'identity':name+'@'+version,'url':url,'requested_utc':stamp,'method':'GET','user_agent':'RustEngineeringMCP-M1-16-Research/0.1 (metadata-only reproducibility study)','timeout_seconds':20,'body_limit_bytes':1048576}
 try:
  req=urllib.request.Request(url,headers={'User-Agent':entry['user_agent'],'Accept':'application/json'})
  with urllib.request.urlopen(req,timeout=20) as response:
   raw=response.read(1048577)
   if len(raw)>1048576:raise ValueError('body_limit')
   entry.update(http_status=response.status,final_url=response.url,captured_utc=datetime.datetime.now(datetime.timezone.utc).isoformat())
  target=Path('selection/registry-responses')/(name+'@'+version+'.json');(root/target).parent.mkdir(parents=True,exist_ok=True);(root/target).write_bytes(raw)
  entry.update(body_path=str(target),sha256=hashlib.sha256(raw).hexdigest())
  payload=json.loads(raw)['version']
  if payload['num']!=version or payload['crate']!=name:raise ValueError('identity_mismatch')
  if type(payload.get('yanked')) is not bool:raise ValueError('missing_yanked')
  publication=datetime.datetime.fromisoformat(payload['created_at'].replace('Z','+00:00'))
  mismatches={}
  for field,key in [('license_expression','license'),('declared_msrv','rust_version')]:
   if fact.get(field)!=payload.get(key):mismatches[field]={'cache':fact.get(field),'api':payload.get(key)}
  entry.update(status='acquired',divergences=mismatches)
  fact.update(yanked=payload['yanked'],published_at=int(publication.timestamp()),registry_created_at=payload['created_at'],registry_metadata={'url':url,'captured_utc':entry['captured_utc'],'sha256':entry['sha256'],'body_path':str(target),'license_expression':payload.get('license'),'declared_msrv':payload.get('rust_version'),'divergences':mismatches})
  fact['unknown_fields']=[v for v in fact['unknown_fields'] if v not in ['yanked','published_at']]
 except Exception as exc:entry.update(status='blocked',error_type=type(exc).__name__,error=str(exc))
 receipts.append(entry)
 print(entry['identity'],entry['status'],entry.get('divergences',entry.get('error')),flush=True)
 (root/'selection/registry-receipts.json').write_text(json.dumps(receipts,indent=2)+'\n')
 path.write_text(json.dumps(doc,indent=2,ensure_ascii=False)+'\n')
 time.sleep(1)
