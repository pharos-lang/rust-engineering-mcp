"""Reproduce authorized formatting-only normalization and draft corpus hashes.

Original fixtures were authored directly; this retained normalization recipe does
not reconstruct or alter their semantics. Run only before a new corpus freeze.
"""
from pathlib import Path
import hashlib,json,subprocess
ROOT=Path(__file__).resolve().parent
FILES=[ROOT/'repair'/task/relative for task in ['R01','R02','R03','R04'] for relative in ['hidden/behavior.rs','reference/src/lib.rs']]
subprocess.run(['<LOCAL_HOME>/.cargo/bin/rustfmt','--edition','2024',*[str(p) for p in FILES]],check=True)
subprocess.run(['<LOCAL_HOME>/.cargo/bin/rustfmt','--check','--edition','2024',*[str(p) for p in FILES]],check=True)
files={str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(ROOT.rglob('*')) if p.is_file() and p.name!='SHA256SUMS.json' and '__pycache__' not in p.parts}
(ROOT/'SHA256SUMS.json').write_text(json.dumps({'status':'draft_not_frozen','algorithm':'sha256','files':files},indent=2)+'\n')
print(json.dumps({'files_hashed':len(files),'formatted_paths':[str(p.relative_to(ROOT)) for p in FILES]},indent=2))
