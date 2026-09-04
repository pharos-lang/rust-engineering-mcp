# M1-16 local reproducible pilot

Protocol: [v2](../../validation/M1-16-protocol.md), architecture [ADR046](../../adr/ADR-046-bounded-utility-experiment.md).
This directory retains sources, private research corpus, qualifiers and measured
receipts. Generated fixtures use the public seed42 and are not approved publisher
distributions. Original registry/source URLs and hashes remain with the corpus;
selection participants receive only the identical emitted projection (A) or facts
from its imported SQLite snapshot (B). Hidden/labels/reference data stay outside
participant workspaces.

Reproduction scripts retain exact executed bytes and location-dependent paths.
Copy reproduction/controller to target/m1-16-controller and reproduction/driver
to target/m1-16-driver in this checkout, and corpus/catalog to their matching
target/m1-16-* directories. Driver Cargo manifests resolve repository crates from
that layout. Build trusted driver offline with pinned1.98.1, never execute project
Rust on the host. Requires previously approved Docker, E5 and native ORT bytes;
no script is an authorization to download/install substitutes.

Actual participant execution requires a fresh private results directory, supported
host Codex login, exact pinned CLI/code-mode-host, protocol approval and a newly
verified hash freeze. Historical artifact paths are receipts, not portable names.
Inspect the exact files/configuration and never relabel a replay as the original
experiment. No model/credential/binary caches are stored in this documentation.

All task and infrastructure outcomes are retained. Automated gates are separate
from blinded source/evidence review; neither compilation nor an unreviewed model
answer is success. This pilot does not qualify native Linux/Windows, license
redistribution, stock client setup or the complete M1 release DoD.

The actual 24-run pilot and independent review are complete. Both arms passed all
12 first/final candidates, leaving no discordant pair and no observed success
advantage; B used more interactions, elapsed time and tokens in this one execution.
See the [measured report](measurement/REPORT.md).


Final runner config additionally fixes expected_catalog (snapshot fingerprint,
complete model identity/index metadata, document count) from qualified status.
Both arms perform setup availability checks before the model window. The corpus
date used for scoring is UTC provenance observed_at. Source review/dispositions and
failed as well as successful requalification receipts are in qualification/.
Analyzer runs after reviewed-evaluation.json exists, preserves all24 planned slots,
and reports missing/infra outcomes separately. No automatic replays or installs.
