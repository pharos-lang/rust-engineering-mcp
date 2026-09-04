"""Reproduce v2 prompt overlay; original v1 prompts/labels remain unchanged."""
import hashlib,json
from pathlib import Path
p=Path(__file__).resolve().parent
rows=[]
for source in sorted(p.parent.glob('S0?-??.txt')):
    text=source.read_text()
    if source.stem.endswith('-en'):
        old='source hash references'
        new='the authoritative snapshot_fingerprint and provenance source_id supplied by the experiment'
    else:
        old='referencias a hashes fuente'
        new='el snapshot_fingerprint autoritativo y provenance source_id suministrados por el experimento'
    assert old in text
    output=text.replace(old,new)
    (p/source.name).write_text(output)
    rows.append({'file':source.name,'sha256':hashlib.sha256(output.encode()).hexdigest(),'original_sha256':hashlib.sha256(source.read_bytes()).hexdigest()})
(p/'manifest.json').write_text(json.dumps({'status':'v2_draft_overlay_not_frozen','recipe':'render.py','identity_rule':'Require snapshot fingerprint and provenance source_id available identically in both arms; no mandatory raw README hash.','prompts':rows},indent=2)+'\n')
print(json.dumps(rows,indent=2))
