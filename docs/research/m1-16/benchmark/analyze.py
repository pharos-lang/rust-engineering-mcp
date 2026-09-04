"""Deterministic post-run consistency analysis; no server or inference."""
import hashlib,json
from pathlib import Path
b=Path(__file__).resolve().parent;r=json.loads((b/'run-01/receipt.json').read_text())
qs={q['id']:q for q in json.loads((b.parent/'m1-16-catalog/queries-qrels.draft.json').read_text())['queries']}
def version(s):return tuple((list(map(int,s.split('.')))+[0,0,0])[:3])
checks=[]
for p in sorted((b/'run-01').glob('*-r*.json')):
 data=json.loads(p.read_text())['structuredContent']['data']['search']
 query_id='-'.join(p.stem.split('-')[2:4]);filters=qs[query_id]['filters']
 for row in data['results']:
  v=row['facts']['selected_version'];checks.append(not v['yanked'] and '-' not in v['version'].split('+')[0] and v['rust_version'] is not None and version(v['rust_version'])<=version(filters['msrv_lte']))
rank_stable=all(len({json.dumps(x['ranked'],sort_keys=True) for x in r['measurements'] if x['mode']==mode and x['query_id']==qid})==1 for mode in ['lexical','semantic','hybrid'] for qid in qs)
baseline=json.loads((b.parent/'m1-16-driver/research-output/baseline-projection.json').read_text())
print('baseline keys',list(baseline))
result={'scope':'post-run filter/rank/identity consistency, no additional inference','all_returned_rows_pass_requested_filters':all(checks),'returned_row_checks':len(checks),'rankings_identical_across_three_repetitions':rank_stable,'baseline_snapshot_matches':baseline['snapshot_fingerprint']==r['snapshot_fingerprint'],'receipt_sha256':hashlib.sha256((b/'run-01/receipt.json').read_bytes()).hexdigest()}
assert checks and all(checks) and rank_stable and result['baseline_snapshot_matches']
(b/'run-01/analysis.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result))
