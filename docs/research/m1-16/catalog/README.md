# Draft M1-16 research projection

records.json is the raw Vec<CrateRecord> input for the bundle emitter;
provenance.json is the corresponding serialized Provenance. Output bundle/store
paths and signing trust remain the emitter/controller's responsibility. This is a
15-crate/16-version research subset, not a live/global crates.io snapshot.

project.py verifies all retained manifest, README/API and registry response hashes,
checks scalar facts against manifests/registry bytes, and groups both TOML versions.
Descriptions normalize whitespace and append authored source-grounded API notes.
Annotation source paths/hashes are embedded in the description so both arms can
observe them. Maximum description is1736 UTF-8 bytes; no control characters.
No model/runtime inference, SQLite compilation or additional acquisition occurred.

Dependencies/advisories were not acquired. Their schema-required lists are empty
recorded rows; every description explicitly warns that these do not prove absence
or safety. updated_at is null because crate update time was not acquired. Package
published_at comes from its captured version metadata. Source identity names the
research projection; created_at/observed_at are actual assembly time, retained for
reproducibility. Input acquisition timestamps/URLs/hashes live in source-evidence.
Integrity unverified describes pre-bundle staging provenance; it is not a claim
about any later signature verification.

baseline-projection.json contains exactly records.json plus provenance.json, with
no extra facts/README/labels. It is the only plain baseline candidate input here.
source-evidence.json and queries-qrels.draft.json are controller-only preparation,
not additional baseline context. Qrels are derived from existing corpus labels;
eight ES/EN queries and filters are drafts, not a frozen or measured benchmark.
Crate-rank evaluation must also check the accepted exact version after filtering.

Projection hashes reproduced unchanged on two consecutive executions of project.py.
The records hash is not the authoritative snapshot fingerprint: obtain that identity
from the actual emitted bundle. Principal review must decide freeze, signing and
symmetric participant payloads before any measurement.
