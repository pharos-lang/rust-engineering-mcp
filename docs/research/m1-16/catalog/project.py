"""Build the draft local research projection; no SQLite, model or network work."""
import datetime, hashlib, json
from pathlib import Path
import tomllib

ROOT=Path(__file__).resolve().parents[1]
CORPUS=ROOT/'m1-16-corpus'
OUT=Path(__file__).resolve().parent
facts=json.loads((CORPUS/'selection/facts.json').read_text())['facts']
labels=json.loads((CORPUS/'selection/tasks-and-labels.json').read_text())
assert len(facts)==16

def digest(data):return hashlib.sha256(data).hexdigest()
def save(name,value):
    (OUT/name).write_text(json.dumps(value,ensure_ascii=False,sort_keys=True,indent=2)+'\n')

evidence=[]
for fact in facts:
    for source in fact['sources']:
        data=(CORPUS/source['corpus_path']).read_bytes()
        assert digest(data)==source['sha256'],source['corpus_path']
        evidence.append({'identity':fact['name']+'@'+fact['version'],'corpus_path':source['corpus_path'],'sha256':source['sha256']})
    registry=fact['registry_metadata']; body=(CORPUS/registry['body_path']).read_bytes()
    assert digest(body)==registry['sha256']
    row=json.loads(body)['version']
    assert row['yanked']==fact['yanked']
    assert row['num']==fact['version']
    assert int(datetime.datetime.fromisoformat(row['created_at'].replace('Z','+00:00')).timestamp())==fact['published_at']
    manifest=tomllib.loads((CORPUS/fact['sources'][0]['corpus_path']).read_text())
    pkg=manifest['package']
    assert pkg['name']==fact['name'] and pkg['version']==fact['version']
    assert pkg.get('rust-version')==fact['declared_msrv']
    assert pkg.get('license')==fact['license_expression']
    assert pkg.get('repository')==fact['repository']
    assert sorted(manifest.get('features',{}))==sorted(fact['features'])
    evidence.append({'identity':fact['name']+'@'+fact['version'],'corpus_path':registry['body_path'],'sha256':registry['sha256'],'url':registry['url'],'captured_utc':registry['captured_utc']})

annotations={}
annotation_sources={}
for fact in facts:
    name,version=fact['name'],fact['version']; paths=[]; text=''
    if name=='toml':
        paths=[f'selection/sources/toml-{version}/README.md',f'selection/sources/toml-{version}/Cargo.toml']
        assert 'serde]-compatible' in (CORPUS/paths[0]).read_text()
        assert {'serde','parse'} <= set(tomllib.loads((CORPUS/paths[1]).read_text())['features']['default'])
        text=f'For recorded version {version}: Serde-compatible TOML decoder/encoder. Default features include serde and parse; decoding into Serde structures needs those capabilities when defaults are disabled. Format-preserving editing is a separate toml_edit use case.'
    elif name=='unicode-normalization':
        paths=[f'selection/sources/{name}-{version}/README.md'];src=(CORPUS/paths[0]).read_text()
        assert 'UnicodeNormalization' in src and '.nfc().collect::<String>()' in src and 'Dependencies\' MSRVs evolve independently' in src
        text='Unicode normalization: import UnicodeNormalization and call .nfc().collect::<String>() for canonical composition NFC. This is normalization, not grapheme segmentation. The retained README explicitly warns that dependency MSRVs evolve independently.'
    elif name=='serde_json':
        paths=[f'selection/sources/{name}-{version}/src/map.rs'];src=(CORPUS/paths[0]).read_text()
        assert 'By default the map is backed by a [`BTreeMap`]' in src and 'feature of serde_json to use [`IndexMap`] instead' in src
        text='JSON object Map<String, Value> uses BTreeMap by default; preserve_order switches the backing to IndexMap. For default BTreeMap key ordering, do not enable preserve_order. This concerns object map representation, not canonical JSON certification.'
    elif name=='async-channel':
        paths=[f'selection/sources/{name}-{version}/src/lib.rs'];src=(CORPUS/paths[0]).read_text()
        assert 'async multi-producer multi-consumer' in src and 'one of all existing consumers' in src and 'Both sides are cloneable' in src and 'pub fn bounded<T>' in src
        text='Async bounded MPMC channel: bounded(capacity) limits capacity; Sender and Receiver are cloneable. Each message is received by only one existing consumer, not broadcast to every receiver. Sending/receiving are awaitable channel operations.'
    if text:
        ident=name+'@'+version
        annotations[ident]=text
        annotation_sources[ident]=[{'corpus_path':path,'sha256':digest((CORPUS/path).read_bytes())} for path in paths]

scope='Research projection, corpus 2026-09-04; only listed cached versions plus captured registry metadata, not a global or live registry. Dependency and advisory rows were not acquired and are omitted from this projection; empty recorded lists do not establish absence or safety. Declared package MSRV/license do not prove transitive compatibility, working integration or legal approval.'
groups={}
for fact in facts:groups.setdefault(fact['name'],[]).append(fact)
records=[]
for name,group in sorted(groups.items()):
    description=' '.join(group[0]['description'].split())+' '+scope
    for fact in group:
        ident=name+'@'+fact['version']
        if ident in annotations:
            description+=' Authored source-grounded annotation: '+annotations[ident]
            description+=' Annotation sources: '+ '; '.join(x['corpus_path']+' sha256:'+x['sha256'] for x in annotation_sources[ident])+'.'
    assert len(description.encode())<=4096 and all(ord(c)>=32 and ord(c)!=127 for c in description)
    assert all(f['repository']==group[0]['repository'] for f in group)
    versions=[{'version':f['version'],'yanked':f['yanked'],'rust_version':f['declared_msrv'],'license':f['license_expression'],'published_at':f['published_at'],'features':sorted(f['features']),'dependencies':[],'advisories':[]} for f in group]
    records.append({'name':name,'description':description,'repository':group[0]['repository'],'updated_at':None,'versions':versions})
# Reproduction preserves capture time once minted; this is projection assembly, not registry observation.
stampfile=OUT/'projection-capture.json'
if stampfile.exists():stamp=json.loads(stampfile.read_text())
else:
    now=datetime.datetime.now(datetime.timezone.utc);stamp={'captured_utc':now.isoformat(),'epoch_seconds':int(now.timestamp())};save(stampfile.name,stamp)
provenance={'source_kind':'registry_snapshot','source_id':'researchprojection:m1-16:corpus-2026-09-04:selection-v1-draft','created_at':stamp['epoch_seconds'],'observed_at':stamp['epoch_seconds'],'integrity':'unverified','network_used':False}
save('records.json',records);save('provenance.json',provenance)
save('baseline-projection.json',{'records':records,'provenance':provenance})
save('source-evidence.json',{'status':'draft_not_frozen','inputs':{'facts_sha256':digest((CORPUS/'selection/facts.json').read_bytes()),'labels_sha256':digest((CORPUS/'selection/tasks-and-labels.json').read_bytes())},'verified_sources':evidence,'annotations':annotations,'annotation_sources':annotation_sources,'projection_capture':stamp,'network_scope':'Projection assembly itself made no network request; input registry requests are attributed individually.','baseline_policy':'Only records and provenance are participant input; labels/qrels/source-evidence remain controller-only.'})
queries={
'S01':{'en':'TOML configuration Serde parsing','es':'TOML configuración deserializar Serde'},
'S02':{'en':'Unicode canonical composition NFC','es':'Unicode composición canónica NFC'},
'S03':{'en':'JSON object sorted keys BTreeMap','es':'JSON objeto claves ordenadas BTreeMap'},
'S04':{'en':'async bounded channel MPMC clone receivers','es':'canal asíncrono acotado MPMC receptores clonables'}}
rows=[]
for task in labels:
    for language,query in queries[task['id']].items():
        assert len(query.encode())<=256 and len(query.split())<=16
        rows.append({'id':task['id']+'-'+language,'intent_id':task['id'],'language':language,'query':query,'filters':{'msrv_lte':task['declared_msrv_lte'],'allow_yanked':False,'include_prerelease':False},'accepted_identities':task['accepted_identities'],'qrels':[{'identity':row['identity'],'relevance':int(row['label']=='accepted')} for row in task['labels']]})
save('queries-qrels.draft.json',{'status':'draft_requires_principal_review_not_frozen','judgment_scope':'Closed 16-version research corpus labels; no general package relevance claim.','rank_unit':'crate; accepted exact version also required after authoritative filters','queries':rows})
save('projection-receipt.json',{'status':'draft_not_frozen','crate_count':len(records),'version_count':sum(len(x['versions']) for x in records),'max_description_utf8_bytes':max(len(x['description'].encode()) for x in records),'records_sha256':digest((OUT/'records.json').read_bytes()),'provenance_sha256':digest((OUT/'provenance.json').read_bytes()),'baseline_sha256':digest((OUT/'baseline-projection.json').read_bytes()),'source_rows_verified':len(evidence),'snapshot_fingerprint':'pending_actual_bundle_not_records_hash'})
print(json.dumps(json.loads((OUT/'projection-receipt.json').read_text()),indent=2))
