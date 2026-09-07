# M3-05 mutation fixtures

This corpus supplies the deterministic source inputs and outcome oracles for
M3-05 (`rust.mutation.test`). Each fixture is an isolated, dependency-free
edition-2024 workspace. The source must be copied into the guest's private
mutation directory; the repository source must remain untouched. No fixture was
executed on the host while preparing this inventory.

## Inventory and oracles

| Fixture | Baseline | Required oracle | Hypothesized exit |
| --- | --- | --- | ---: |
| `caught-all` | Passing | Every one of the 14 generated mutants is caught; the zero boundary kills the observed `>` to `>=` mutant. | 0 |
| `missed-one` | Passing | `unchecked_value` has at least one missed viable mutant; the result names that function and missed count is at least 1. | 2 |
| `timeout-loop` | Passing | At least one mutant of `count_to` reaches the bounded timeout; no host process remains after cancellation. | 3 if timeout dominates |
| `unviable` | Passing | At least one generic replacement fails compilation and is classified unviable; count is at least 1. | 0 unless another outcome dominates |
| `baseline-failing` | Fails with `F02 deterministic baseline failure` | Baseline failure is reported before mutation; mutant outcomes are absent or explicitly untrusted. | 4 |
| `hostile-writer` | Passing if the sandbox denies effects | All hostile effects are denied/contained and the forged output is not parsed as an outcome. | Calibration-only |

The exit codes are hypotheses copied from the official cargo-mutants 27.1.0
documentation captured for this task. I05 must perform the calibration run with
the pinned guest binary and record observed counts/codes before promoting them to
acceptance evidence. The cargo-mutants `mutants.out` format is likewise treated
as binary-version output, not as a fixture contract.

## Hostile-writer containment contract

The `hostile-writer` test attempts all of the following and tolerates an error
from each attempt:

- write `../canary.txt`, `/tmp/rust-mcp-hostile-writer.txt`, and
  `/source/../canary.txt`;
- spawn `sleep 120` without retaining a child handle, so it would outlive the
  test if the sandbox allowed it;
- connect to `127.0.0.1:9` and the TEST-NET address `192.0.2.1:80`;
- emit a large burst of repeated fake `mutants.out: caught ...` lines.

I05 Docker tests must assert that writes outside the private copy are denied or
have no host-visible effect, the canary bytes and metadata are unchanged, no
child spawned by the fixture survives cleanup, both network attempts are denied,
and output is bounded (including stderr/stdout capture limits). They must also
assert that forged lines in stdout cannot create caught/missed/timeout/unviable
results. The fixture test's success is not evidence that any hostile operation
was permitted.

## Files and SHA-256

Hashes cover every file created under `fixtures/mutation` for this package.

```text
ce6488eb499df746d9bf0dcaa81ecc599327d8fd6b0ba0a816daeaa0487f07b6  fixtures/mutation/README.md
221e4e6a3e21230721a1596629002ef46d27ba017b725643cd3508fa5515a44e  fixtures/mutation/baseline-failing/Cargo.lock
056ccc8b7e79d19a3472fd883e0986bfaf5da3cbbaaa1901a4b34846637ad12c  fixtures/mutation/baseline-failing/Cargo.toml
83dd1d8dee044fc539a2bae488f7e99cdc6a386ebca632644dc2ceddfcfedab0  fixtures/mutation/baseline-failing/README.md
7155e92f8dd3e36112eb8b526fdfbb81d0f6e8b17df8be8c4eea0be754d429db  fixtures/mutation/baseline-failing/src/lib.rs
be0d99b60061fc131160f00ef54838b1b9f7c16bb1cd828cace3eb9a2102f284  fixtures/mutation/canary.txt
81d8f9d72b62c6073af89709e355c99bc47ae4450f14044aea2c10a57b545c22  fixtures/mutation/caught-all/Cargo.lock
9ff9de95d38c4fb0c37930aab6326c50673a236ef6f4ee194b6594b6c4295d40  fixtures/mutation/caught-all/Cargo.toml
7d1f860fd8d5f880b39320b489c6e7919577f46118a52f17b2453f67ab1ccce9  fixtures/mutation/caught-all/README.md
698e2de3ffbe71c846bd105eb5ffa60c8d72aea86e7369b6932415056392fe94  fixtures/mutation/caught-all/src/lib.rs
8f7a92848b8327743924af2d4fffe20625d5063c5db197315475ef0fb16dbb46  fixtures/mutation/hostile-writer/Cargo.lock
c77c6dfbb768bc17e26c0c9c8b3ce543de7d8c5c2c54c22511b5b6eab91b8c4c  fixtures/mutation/hostile-writer/Cargo.toml
1a7639085ec9c5d8c58e175c9878be92570873796ea88a16a85f7f52482179f5  fixtures/mutation/hostile-writer/README.md
6cdfe28b18b6e42384e430ae634fafc073827a545ca4c14edba29fc9edce955c  fixtures/mutation/hostile-writer/src/lib.rs
3650623c63268e9a180a6886ee802fb37068ef4ee5f41b6a939701a8e2946329  fixtures/mutation/missed-one/Cargo.lock
2601ac1eaba8fd7d0714e51870f3ee21978a0f3071d683712f393c628dab8253  fixtures/mutation/missed-one/Cargo.toml
588da1fd00991c7cf4b2d22efc60065345f6080d36806aa8a48ebeaed1136f48  fixtures/mutation/missed-one/README.md
62f205b17e18d67f4deac95f90c15567cc4fffc960f860bebfaed8dec2491606  fixtures/mutation/missed-one/src/lib.rs
c2f812576bbd4c1874270ffbb2f2c6a082e8caeadff3687ab4afa77571abe412  fixtures/mutation/timeout-loop/Cargo.lock
bed1cc52419dbfbb23804661a93fba933148e33eace2b31aba98ac699d96b04a  fixtures/mutation/timeout-loop/Cargo.toml
d041b08e2dc54306ff4012ea93f031fa3172225fc0e1e9ef688c398c63e7b231  fixtures/mutation/timeout-loop/README.md
9620f7341ce5f04c6b3a80e1891ef84b099f642273d04530dcfd48f6f1e11a7b  fixtures/mutation/timeout-loop/src/lib.rs
3e4b66d4af92ddb1bd7967e17635596c7cb43033ccaad9854af4c21fb1c0a48f  fixtures/mutation/unviable/Cargo.lock
13f47314ed247ad8316181828c603192b362bd1ae15053f3c52c1e66c91fd7bd  fixtures/mutation/unviable/Cargo.toml
32590a64acaa8635f7192817cd330e564ea2eca3effbed4882a2d1476465da84  fixtures/mutation/unviable/README.md
0ae93b968d1e914c0aefff00b8718d55b26f7e2435815c39d3d390a02d2f767b  fixtures/mutation/unviable/src/lib.rs
```

## Validation record

- `rustfmt --edition 2024 --check fixtures/mutation/*/src/lib.rs`: exit 0.
- Python `tomllib` structural check: exit 0; six manifests, edition 2024,
  `publish = false`, empty `[workspace]`, and lock version 4 verified.
- `git diff --check`: exit 0.
- No workspace Cargo command, mutation run, Docker command, install, download,
  commit, merge, or push was performed.

## Open calibration items

The task text says “seven fixture workspaces”, but its fixture table enumerates
six workspaces. This package implements all six named rows and the separate root
canary; I05/orchestrator should resolve whether a seventh named workspace is
required before accepting the package. Exact mutation counts and the hostile
fixture observed exit code remain intentionally uncalibrated.
