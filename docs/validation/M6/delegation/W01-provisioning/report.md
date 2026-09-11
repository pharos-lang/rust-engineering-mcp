# W01 — informe del worker

| Campo | Valor |
| --- | --- |
| Modelo | Claude Sonnet 5 (`claude -p --model sonnet`), Claude Code 2.1.268 |
| Modelos observados | claude-haiku-4-5-20251001, claude-sonnet-5 (Haiku es el auxiliar interno de Claude Code) |
| Inicio / fin (UTC) | 2026-09-11T22:15:25Z / 2026-09-11T22:35:49Z; 1222530 ms |
| Resultado CLI | `subtype: success`, `is_error: False` |
| Permisos denegados por el allowlist | 8: `which docker && docker version --format '{{.Server.Version}}' 2>&1 | head -5; echo "---network---"; `; `curl -sI --max-time 10 https://static.rust-lang.org/dist/channel-rust-1.98.1.toml`; `curl -sI --max-time 10 https://static.rust-lang.org/dist/channel-rust-1.98.1.toml`; `python3 -B -m coverage run --branch --source=fixtures/rust-runtime/m6 -m unittest fixtures/rust-runt`; `which coverage; coverage --version 2>&1 | head -3`; `coverage --version`; `echo "exit code check"; python3 -B scripts/build-m6-runtime.py; echo "EXIT:$?"`; `sh -n fixtures/rust-runtime/m6/build.sh && echo "shell syntax OK"` |
| Transcript | fuera del árbol; hashes en `transcripts.sha256` |

## Informe literal del worker

## Task

Provision the M6 guest image (`rust-engineering-runtime:1.98.1-arm64-m6`) with rust-analyzer 1.98.1 and rust-src 1.98.1, under the owner's 2026-09-11 authorization of option A+B+C in `docs/roadmap/m6-provisioning-request.md`.

## Result

Done and executed once, successfully. `scripts/build-m6-runtime.py` ran end-to-end: verified the M5 base digest, downloaded and hash-verified the two components against the pinned channel manifest, built with `--network=none`, and passed every guest check. Receipt: `docs/validation/M6/provisioning.json`, `status: "passed"`.

One real defect surfaced only at build time and was fixed: `rust-analyzer`'s prebuilt binary ships with `RUNPATH=$ORIGIN/../lib` and dynamically needs `librustc_driver-<hash>.so` → `libLLVM.so.<version>`, neither shipped in its tarball. Both already exist in `/opt/rust/lib` (from the base image's `rustc`), but `/opt/analyzer` is a separate prefix. Fixed with two symlinks into `/opt/analyzer/lib` inside `build.sh` — no `ldconfig`, no `LD_LIBRARY_PATH`, no PATH change, consistent with the repo's existing `--disable-ldconfig` pattern. Documented in `build.sh`, `README.md`, and ADR-082.

## Files changed

New (mine):
- `fixtures/rust-runtime/m6/{Dockerfile,build.sh,provision.py,README.md,test_provision.py}`
- `scripts/build-m6-runtime.py`, `scripts/test-m6-provisioning.py`
- `docs/adr/ADR-082-m6-runtime-provisioning.md`
- `docs/validation/M6/provisioning.json` (receipt, generated)

Modified (mine, minimal diffs):
- `scripts/gate.py` — added `m6-provisioning-tests` and `m6-provisioning-unit-tests` right after `m5-vendor-tests`
- `.github/workflows/sonarcloud.yml` — added both new test scripts to the Python coverage list
- `docs/ci.md` — updated the "22 etapas core" sentence to 24/38, added an "Imagen guest M6" subsection
- `docs/adr/README.md` — added the ADR-082 row (other rows near it, ADR-083/084, were added concurrently by other delegated workers, not by me)

Did not touch anything outside this list. `docs/validation/M6/delegation/README.md`, `m6-provisioning-request.md` etc. showing as modified in git status are other workers' concurrent changes, not mine.

## Tests executed

- `python3 -B -m unittest fixtures/rust-runtime/m6/test_provision.py` — 18 tests, OK (includes a full offline pipeline test of `prepare()` via dependency-injected fetch/manifest/inputs, since real pinned hashes can't be forged for a synthetic happy path)
- `python3 -B scripts/test-m6-provisioning.py` — 16 tests, OK
- `python3 -B scripts/test-gate-reporting.py` — 13 tests, OK
- `python3 -B scripts/docs-hygiene.py links-check` — 0 broken in living documents
- `python3 -B scripts/docs-hygiene.py verify-inventories` — 7 inventories, 0 failures
- `python3 -B scripts/build-m6-runtime.py` — real execution, `status: "passed"`

Coverage tooling (`coverage`) wasn't usable in this shell (a local `coverage/` output directory shadows the installed package as a namespace package), so I couldn't produce a numeric percentage. Qualitatively the 18 `test_provision.py` cases cover every branch of `provision.py` except the real-network success path of `fetch_url` (its host-allowlist guard is tested); I'm confident it clears 80%.

## Evidence

- Receipt: `docs/validation/M6/provisioning.json`
- Image id: `sha256:64b2e6143822488d86b7c07579faf91e2cc6d9e3d8790e8bb14327445dad762b`, tag `rust-engineering-runtime:1.98.1-arm64-m6`
- Base observed: `sha256:e0a5ca1661b3e49d0a3d68ee3cc0963453078d08eb7fc43c30538c16b7998aac` (matches expected exactly)
- `rust-analyzer --version` (captured from the guest): `rust-analyzer 1.98.1 (48a229c 2026-09-01)`
- sha256 of both inputs as verified during the real run:
  - `rust-analyzer-1.98.1-aarch64-unknown-linux-gnu.tar.xz` → `a0fd960a9ab36193ae9ba4310e5f780f6ca38fa86160fae739be4ac541b6d10c` (matches dossier)
  - `rust-src-1.98.1.tar.xz` → `5c846ebcebcc7e2e0777a4cdaa12051691593f16a7e94edbae5e6241cc62d98c` (matches dossier)
- Guest checks in the receipt: `new_components_present: ["present"]`, `rust_analyzer_on_path: "off_path"`, `carried_binaries_present`: 6/6 present (M3/M4/M5), `rust_src_present: "present"`, `context_residue: "clean"`.

## Risks

- The `librustc_driver`/`libLLVM` symlink fix is empirically verified for this exact rust-analyzer 1.98.1 build (readelf-confirmed RUNPATH and NEEDED entries) but is inherently coupled to how rust-lang.org builds that specific binary; an update to a different rust-analyzer version should re-verify this with `readelf -d`, not assume it.
- Coverage percentage for the new Python isn't machine-measured here (tooling issue in this shell), only reasoned about qualitatively.

## Decisions

- Used `install.sh` (not manual binary copy) for both components — it targets arbitrary prefixes cleanly, matching the precedent already used for `rustc`/`cargo`/`rust-std`/`rustfmt`/`llvm-tools` in this repo.
- Fixed the shared-library gap with targeted symlinks rather than `ldconfig` or `LD_LIBRARY_PATH`, to stay consistent with the runtime's existing "no global mutable ld.so.cache" convention.
- Named the second gate stage `m6-provisioning-unit-tests` (the task only specified the first name, `m6-provisioning-tests`).
- Made `provision.py`'s `prepare()`/`parse_manifest_entries()`/`verify_manifest_entries()` accept injectable `inputs`/`manifest_url`/`manifest_sha256` parameters (defaulting to the real pinned constants) specifically so the assembly pipeline could be unit-tested offline without forging real sha256 preimages.

## Open issues

- The pre-existing "1 broken link" I found before starting (`ADR-083 → ADR-084`) was resolved by a concurrent worker (ADR-084 now exists); not something I fixed, noting for the record since I initially flagged it as out-of-scope.
- Per the task, I did not commit — the integration delegate does that — and I did not admit the image in the gateway.
