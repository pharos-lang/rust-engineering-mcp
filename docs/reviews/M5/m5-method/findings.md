<!--
Texto íntegro y sin editar del revisor independiente (G8) del método de medición
y los contratos públicos M5. Se conserva tal cual lo entregó, incluidos los
findings que después resultaron basados en un árbol anterior: la revisión es
evidencia de lo que se revisó y cuándo, no un documento que el revisado pueda
corregir. La disposición del owner está en `disposition.md` y el resumen en
`review.md`.

Revisor: Claude Opus 5, read-only, esfuerzo alto. Fecha: 2026-09-08.
Inputs congelados: `inputs.json`.
-->

# G8 Independent Review — M5 Performance (`ai/m5-performance`)

## Verdict

**Block.** The milestone's *posture* is genuinely good — the M5-01 blocker is real and was refused rather than massaged, the vendor bounds were not widened to make a test pass, `BenchmarkExit`/`BloatExit` stay `CALIBRATED = false`, the exact-size/estimated-attribution split in `rust.binary.bloat` is the best-engineered thing in the milestone, and the dataset format is genuinely fail-closed on both the producer and the reader path. But the central artifact — the comparison method — quantifies the wrong source of variance, and I produced a **false `improvement` verdict from the project's own committed real guest captures on source code that did not change**. The reported confidence interval measures within-execution sampling dispersion only; the dominant term, between-execution drift, is absent from it, and on this milestone's own fixture that drift reaches 14% for an unchanged benchmark against a 5% material threshold. Separately, the four tools ADR-076 defines are absent from `tools/list` under every condition, so `tools/list` still returns 27 and the four M5 schemas have no snapshot; and both calibration receipts — the entire empirical basis of D23 and the whole M5-02 oracle — were captured on a container image that ADR-077 explicitly forbids, seven and twenty-four minutes before the qualified image existed. Any one of the three P1s blocks on its own.

---

## Findings

### P1-1 — The confidence interval and the MDR do not measure the variance that decides the verdict; a false `improvement` is reachable on the project's own real data

`crates/domain/src/benchmark_compare.rs:503-536` (`bootstrap_ratio`), `:723-727` (MDR), `crates/execution-adapter/src/performance_port.rs:219-269` (`pool`).

**What is wrong.** The interval is a bootstrap over the samples *inside* one dataset. It therefore estimates how much the median would move if you redrew samples from the same execution. It says nothing about how much the median moves *between* executions — which is the quantity the verdict is actually about, because baseline and candidate are two different executions by construction (`dataset_compatibility` at `:598` requires different `execution_fingerprint`s).

Two mechanisms:

- **run_count = 1**: between-execution drift is not represented in the interval at all.
- **run_count = 3 (the default, ADR-073 §2)**: `pool()` at `performance_port.rs:225-269` concatenates the samples of the three independent repetitions into one `Vec<RawSample>` with no per-run marker, and `bootstrap_ratio` then resamples all 90 as if iid. That is an unclustered bootstrap over a clustered sample, which understates the standard error by the design effect.

**Recomputation.** I transcribed the module exactly (SplitMix64, `index`, `median_sorted`, `quantile_sorted`, `tukey_outliers`, `bootstrap_ratio`, `decide`) and reproduced the crate's own oracles to confirm fidelity: the 20% fixture gives effect `0.19521`, CI `(0.18789, 0.20369)`, MDR `0.01181`; the noisy fixture gives `precision_below_threshold` with MDR `0.80663`. Then, under the null (identical code both sides), 3 pooled repetitions × 30 samples, 10 000 resamples, 400 trials, family = 1:

| run-to-run drift τ | REGRESS | IMPROVE | false material verdicts | believed SE | actual SD | understated by |
|---|---|---|---|---|---|---|
| 0.0% | 0 | 0 | **0.0%** | 0.00190 | 0.00189 | **1.00×** |
| 0.5% | 0 | 0 | 0.0% | 0.00206 | 0.00449 | 2.18× |
| 1.0% | 0 | 0 | 0.0% | 0.00244 | 0.00864 | 3.54× |
| 2.0% | 0 | 0 | 0.0% | 0.00355 | 0.01792 | 5.04× |
| 3.0% | 8 | 9 | **4.2%** | 0.00432 | 0.02768 | 6.41× |
| 5.0% | 45 | 34 | **19.8%** | 0.00527 | 0.04742 | 9.00× |

With one repetition per side (the common "run baseline, run candidate, compare" flow) it is worse: at τ = 2% the false rate is 7.2% and at τ = 3% it is **22.0%**, with the believed SE understated 9.5× and 14.2×. The τ = 0 row is the control that isolates the cause: with no between-execution drift the believed SE equals the actual SD to three digits and the false rate is exactly zero. The method is correct for a single homogeneous execution and wrong precisely for the two-execution comparison it exists to perform.

**Demonstration on the milestone's own committed bytes.** I extracted `fixtures/benchmark-datasets/*.tar` and ran the frozen method over the real `sample.json` files. Per `fixtures/benchmark-datasets/README.md:12`, `criterion-candidate.tar` is "la misma fixture con `work_unit` haciendo un 25 % más de operaciones" — so `work_noisy` (behind `m5/control`) and `work_slower` (behind `m5/slower_125`) are **unchanged source** in all three archives. Their medians across the three archives:

```
m5/control       3203.5   3300.7   2894.4 ns   -> 14.0% spread for unchanged code
m5/slower_125    4093.4   4101.7   3908.7 ns   ->  4.9% spread for unchanged code
```

Comparing `m5/control`, baseline = `criterion-run-2.tar`, candidate = `criterion-candidate.tar`, family_size = 1:

```
verdict = improvement
effect_ratio = -0.1231
confidence_interval = (-0.1492, -0.0756)
minimum_detectable_ratio = 0.0488   (<= 0.05, so the precision gate passes)
```

**A `improvement` verdict, with an interval that excludes the material threshold, for source code that did not change.** At family_size = 3 the same pair escapes only because Bonferroni lifts the MDR from 0.0488 to 0.0563 — a margin of 0.0063 against the 0.05 gate. family_size = 1 is not exotic: it is any bench target with one benchmark, and `bench_target` is a published input (ADR-076 §3).

The same run shows the MDR is demonstrably optimistic: `m5/slower_125` run-1 vs candidate reports MDR `0.0367` ("I could detect a 3.7% effect") while that unchanged benchmark actually moved `-0.0451`.

And the `regression` the milestone is proud of, `m5/reference` run-1 vs candidate, reports CI `(+0.1242, +0.1353)` — width 1.1%, roughly **13× narrower** than the between-execution drift the same fixture exhibits for unchanged code. The direction is right; the precision attached to it is not credible, and `criterion_dataset.rs:1531` pins it (`assert!(reference.confidence_interval.0 > 0.05)`).

**Why the existing oracles missed it.** `crates/execution-adapter/src/criterion_dataset.rs:1491-1513` checks the self-compare control on `m5/reference` only, and no test ever compares `criterion-run-2.tar` against `criterion-candidate.tar`. The two by-construction-unchanged benchmarks are never used as controls.

**Why it matters.** This is the exact failure the DoD names as blocking: "casos ruidosos dan inconclusive" (`docs/roadmap/m5-performance.md:128`) is falsified, and "P2 de método/evidencia que falsee veredicto bloquean" (`:124`). A user is shown a directional verdict and a numeric interval for a difference the method cannot see.

**Fix.** Three options, in preference order. (a) Make the resampling unit the repetition: record a run index on `RawSample`, resample repetitions with replacement and then samples within them (cluster bootstrap), so between-run variance enters the interval; require `run_count >= 2` per side for any directional verdict. (b) If (a) is out of scope, require ≥ 2 independent repetitions per side and estimate the between-run component explicitly, adding it to the SE before computing the interval and the MDR. (c) At minimum, and only as a stopgap: publish in `Method` that the interval is a within-execution interval that excludes between-execution drift, refuse `regression`/`improvement` when `run_count == 1`, and add `m5/control`/`m5/slower_125` run-2-vs-candidate as a negative oracle. Option (c) alone does not make the verdicts correct; it makes them honest.

---

### P1-2 — The four tools are dispatched but never advertised; `tools/list` still returns 27 and the four schemas have no snapshot

`crates/mcp-server/src/stdio.rs:611-660` (`list_tools`), `:361-373` (dispatch), `:612-617` (`definition()`), `crates/mcp-server/src/stdio/security_tool.rs:27-33`.

`list_tools` builds a `vec![...]` of 22 definitions and pushes five more under `deny/unsafe_scan/supply_chain/quality_v2/miri::advertised()`. **There is no push for `benchmark`, `benchmark_compare`, `profile` or `bloat`.** They *are* fields on `EngineeringServer` (`:156-159`), they *are* dispatched at `:361-373`, and `definition()` resolves them at `:612-617` — all gated on `advertised()`, which returns `true` unconditionally (`security_tool.rs:32`; the env var is a test hook that can only force `true` earlier). So the four tools answer `tools/call` and are invisible to `tools/list`.

Consequences, all verified:
- `crates/mcp-server/tests/snapshots/` holds 28 files = 27 tool snapshots + `doctor-report.json`. **No M5 snapshot exists.** `git diff main...HEAD -- crates/mcp-server/tests/` is empty: no test or snapshot changed on this branch.
- Five files still assert 27 and pass: `tests/protocol.rs:716`, `tests/catalog_status.rs:184`, `tests/crate_inspect.rs:188`, `tests/crate_search.rs:188`, `tests/inspection_runtime/nextest.rs:37`.
- ADR-076 "Consequences" states "El inventario pasa a treinta y una tools y cinco archivos de test que afirman el conteo deben actualizarse junto con la lista ordenada de nombres" and "Cuatro snapshots nuevos se añaden". Neither happened. The ADR is Accepted as the contract and the code does not implement it.
- G1 requires "Cada contrato nuevo pasa ADR → decisión → **snapshot** → wire tests → docs". G4 requires a stock model-driven client doing "discovery→llamada positiva". Neither is satisfiable: an undiscoverable tool cannot be discovered.
- Per-tool coverage is a spot check, not a contract: e.g. `stdio/bloat/tests.rs:190-208` asserts only the name, `additionalProperties == false` and a timeout default. Nothing pins the field set or the enum spellings.

To the milestone's credit, `docs/validation/M5/handoff.md:83-85` states the tools are not registered in `stdio.rs`. That makes it a declared gap, not a hidden one — but it is still a gap that blocks the DoD line "Cuatro tools entregan artifacts/medidas reales" and blocks G1's snapshot requirement.

**Fix.** Push the four definitions in `list_tools`, add the four snapshots, update the five count assertions to 31 and the ordered name list in `protocol.rs:717+`, and extend the invariance test to cover the new snapshots.

---

### P1-3 — Both calibration receipts and the entire M5-02 fixture oracle were captured on an image ADR-077 forbids

`docs/validation/M5/01-benchmark-calibration.json:3-4`, `docs/validation/M5/04-bloat-calibration.json:3-4`, `docs/validation/M5/provisioning.json:18-19,63`, `docs/adr/ADR-077-m5-runtime-admission.md:26-37`, `crates/execution-adapter/src/performance_port.rs:53-57`.

```
M5-04-bloat-calibration.json   captured_at_utc 2026-09-08T22:11:33Z  image sha256:e9ecc40d…
M5-01-benchmark-calibration.json captured_at_utc 2026-09-08T22:28:22Z  image sha256:e9ecc40d…
M5-provisioning.json           started_at 22:35:07  finished_at 22:35:54  image sha256:0e21c561…
```

`sha256:e9ecc40d…` occurs in exactly three places in the repo: the two receipts and a hardcoded test string at `criterion_dataset.rs:1369`. It is in no ADR, no provisioning receipt and no admission list. ADR-077 says of the one admitted digest: *"el puerto de performance exige esa imagen y solo esa … ejecutar una medición sobre otra imagen produciría una declaración que el producto no puede sostener"*, and `performance_port.rs:56` pins `M5_IMAGE = sha256:0e21c561…`.

So by the project's own rule, both calibrations — and therefore the three committed `criterion-*.tar` fixtures that are the whole M5-02 oracle and the source of every number in ADR-073's frozen-parameters table — are measurements the product declares it cannot sustain. `docs/validation/M5/01-blocker.json:53` compounds it by asserting the captures came "from this image" in a document that pins `0e21c561…` at `:8`.

Mitigation worth recording: `M5-04-bloat-calibration.json:9` `binary_sha256 e3eaea0d…` is byte-identical to the `cargo-bloat` hash in `M5-provisioning.json`, so the analyzer binary was the same across the two images. The *image* claim remains wrong.

Secondary: `M5-matrix.md:56-57` presents a 13-row table under "Imagen `sha256:0e21c561…`" but `M5-runtime.json`'s top-level `image_id` is `sha256:25ed3626…` (the M4 digest), and `M5-00-admission-runtime.json:5` is likewise `25ed3626…`.

**Fix.** Re-capture both calibrations on `0e21c561…` and regenerate the three fixture archives from it, or add an explicit, ADR-level disposition recording that D23's parameters and the M5-02 oracle rest on an unadmitted image. Correct `M5-01-blocker.json:53` and the matrix's image heading either way.

---

### P2-1 — A degenerate sample set makes the precision gate vacuous and yields a zero-width interval

`crates/domain/src/benchmark_compare.rs:478-493` (`standard_deviation`), `:664-667` (the gate), `:723-727` (MDR).

If every sample on both sides is equal, every bootstrap resample has the same median, `standard_deviation(&ratios)` returns `0.0`, `minimum_detectable_ratio` is `0.0`, and `0.0 > 0.05` is false — the "precision insufficient" gate cannot fire. Recomputed:

```
30 identical baseline samples, 30 identical candidate samples 6% apart
-> verdict = regression, CI = (0.060000, 0.060000), SE = 6.9e-18, MDR = 0.000000
```

A zero-*observed* dispersion is not infinite precision; it is an absence of information about dispersion. This is reachable through timer quantisation and, more importantly, it is the exact shape of the threat the milestone itself names — "benchmark que falsifica output" (`docs/roadmap/m5-performance.md:99`). The samples come from the project's own harness, so a project can emit constants and obtain a maximally confident directional verdict with a zero-width 95% interval.

**Fix.** Treat a degenerate bootstrap as insufficient precision: if the bootstrap SE is zero (or the sample set has fewer than some small number of distinct values), return `inconclusive` with a new `degenerate_dispersion` reason rather than passing the gate.

---

### P2-2 — "Unknown stays unknown" is true of one field only; four hardware/config fields are neither compared nor shown

`crates/domain/src/benchmark_compare.rs:550-601`, `crates/execution-adapter/src/performance_port.rs:161-173`, `crates/domain/src/benchmark.rs:385-394,443-465`.

Checked: `format`, `unit`, `harness`, `harness_version`, `rust_version`, `cargo_version`, `image_digest`, `platform`, `selection`, `hardware.arch`, `hardware.cpu_model` (unknown on either side ⇒ `UnknownHardware`, `:590-594`), `hardware.quotas`, `execution_fingerprint`.

Never referenced anywhere in `dataset_compatibility`: **`os_kernel`, `cpu_cores`, `cpu_governor`, `virtualization`, `declared_toolchain`, `configuration_fingerprint`**. `IncompatibilityReason` has no variant able to express any of them.

`cpu_governor` is the sharpest case. `performance_port.rs:170` hardcodes `cpu_governor: None` with the comment "No governor is observable from inside the container; unknown stays unknown rather than becoming 'performance'" — the value is honest, but it is a hardware field the runtime cannot observe that does **not** block the comparison. ADR-073 §3 states: *"Un campo de hardware que el runtime no puede observar se serializa ausente y bloquea la comparación"*. ADR-073 §5's enumerated list only blocks on unknown `cpu_model`. **The two sections of the same ADR contradict each other, and the code implements §5.** The frequency governor is the single environmental parameter most able to manufacture a fake regression, and two datasets taken under different governors compare cleanly today.

`configuration_fingerprint` is documented at `benchmark.rs:454` as "Digest over the frozen run configuration" — the one digest that would catch a differing run configuration — and it is never consulted, against `docs/roadmap/m5-performance.md:66` ("Compare verifica ownership/format/plugin/units/benchmark/**config**/hardware antes de estadística").

**Fix.** Either extend the blocking set (and the `IncompatibilityReason` enum) to `configuration_fingerprint`, `os_kernel`, `cpu_cores` and `virtualization`, or amend ADR-073 §3 so it states what §5 actually implements and records why `cpu_governor` being permanently unknown does not block. The current text asserts a guarantee the code does not provide.

---

### P2-3 — The compare output carries no provenance at all, so "diferencias visibles" is unmet even for the differences it does detect

`crates/mcp-server/src/stdio/benchmark_compare.rs:84-94`, `crates/mcp-server/src/stdio/benchmark_compare/schemas.rs:135-157`.

The complete `Data` struct is `project_ref`, `semantics`, `baseline_artifact_id`, `candidate_artifact_id`, `report`; `Report` is `method`, `comparisons`, counts, `baseline_only`, `candidate_only`, `incompatibility_reasons`, `complete`. **Neither dataset's provenance appears** — not the CPU model, not the platform, not the image digest, not the quotas. `rust.benchmark.run` publishes all of it (`stdio/benchmark/schemas.rs:168-216`); compare drops it.

And when compatibility *does* fail, `incompatibility_reasons` publishes bare tags with no values: a caller told `["cpu_model"]` cannot see which two CPUs, and cannot obtain them from this tool. `docs/roadmap/m5-performance.md:55` requires "hardware/OS/config deben ser compatibles y **diferencias visibles**". ADR-076 §4's output inventory silently omits provenance rather than flagging the gap.

**Fix.** Echo both datasets' provenance (or a bounded projection of it) in the compare result, and carry the two observed values on each incompatibility reason.

---

### P2-4 — The fixture `control` is still asserted as a 1.00x control in five places, including the Rust source and the matrix

Honest, with the receipt cited: `fixtures/benchmark/README.md:123-135` ("El benchmark `control` no es un control 1,00x … la medición real en el guest lo desmiente"), `docs/validation/M5/01-benchmark-calibration.json:189`, and implicitly `fixtures/benchmark-datasets/README.md:11`.

Still wrong:
- `fixtures/benchmark/src/lib.rs:12` — `//! | work_noisy | n | 1.00x (self-compare control)|`
- `fixtures/benchmark/src/lib.rs:64-70` — "the only difference an adapter can observe against `reference` is measurement noise. **The expected design ratio is 1.00x.**" This is the precise proposition the guest measurement refuted (−2.9% is a systematic instruction-path effect, not noise), in the source file itself, with no pointer to the receipt.
- `fixtures/benchmark/README.md:17` — table row "1.00x, noise / self-compare control" (rescued only 106 lines later).
- `fixtures/benchmark/README.md:27` — "it exists so an adapter has a self-compare case that should report 'no meaningful change'."
- `docs/validation/M5/matrix.md:99-102` — the matrix's **only** mention of `control/reference` prints three host ratios (1,013, 1,002, 0,976), actively reinforcing ≈1.00×, and the matrix never states the retraction anywhere.
- `docs/validation/M5/handoff.md` — no mention at all. The document a successor reads first never says a fixture named `control` is not a control.

**On the real self-compare control (question 10):** it exists and is correctly identified — `criterion-run-1.tar` vs `criterion-run-2.tar`, same source, two executions, exercised at `criterion_dataset.rs:1491-1513`. Recomputed, family = 3: `m5/reference` → `inconclusive` (effect −3.95%, CI (−5.67%, +0.19%), MDR 5.19%), `m5/control` → `inconclusive`, `m5/slower_125` → `no_material_change`. No false direction — a genuine pass. But note how thin: the CI's lower bound is already **−5.67%**, past the −5% threshold, and the verdict was withheld only because the interval also reached +0.19% and the MDR gate fired at 5.19% vs 5.00%. Two caveats: the test's assertion accepts either `NoMaterialChange` or `Inconclusive`, so it pins nothing; and no receipt records the verdict it actually produced.

**Fix.** Correct `lib.rs:12` and `:64-70` (the source is the primary artifact), the two README table/prose lines, and the matrix paragraph; add one sentence to the handoff.

---

### P2-5 — No MDR, confidence interval or observed verdict appears in any M5 receipt

`grep -rn "mdr\|minimum_detectable\|confidence_interval\|observed_verdict\|\"verdict\"" docs/validation/M5-*.json` returns **nothing**. `M5-01-benchmark-calibration.json:167-180` records only `expected_verdict` — "no_material_change or inconclusive, never a direction" and "regression" — never an observed one, and closes `"status": "passed"` at `:191`.

`docs/roadmap/m5-performance.md:127-128` makes it an acceptance criterion: "Samples/warmup/variabilidad/CI/MDR **son visibles** y casos ruidosos dan inconclusive". No M5 evidence makes CI or MDR visible, and (per P1-1) the "casos ruidosos dan inconclusive" half is false. There is also no M5 gate receipt of any kind — no `cargo test` log, nothing recording that `real_guest_datasets` ever ran. The handoff concedes this at `:88`.

Related, from the same file: `M5-matrix.md:99-102` asserts "Tres ejecuciones en el host dieron 1,256, 1,331 y 1,302 … y 1,013, 1,002 y 0,976"; only `1,256`/`1,013` are corroborated anywhere, by `fixtures/benchmark/README.md:40-42`, which discloses that run used `--sample-size 10` — below the method's own 30 floor — and calls itself "a single unreplicated observation". The matrix quotes those host numbers without the sample size, while receipted *guest* ratios (1.2408, 1.2945) sit unused in the calibration receipt.

**Fix.** Record observed verdicts, intervals and MDRs for the three oracles in the calibration receipt; drop or annotate the unreceipted host ratios; produce a gate receipt.

---

### P2-6 — Bonferroni pushes the interval below what 10 000 resamples can resolve, and the doc comment's accuracy claim is scoped to the unadjusted level

`crates/domain/src/benchmark_compare.rs:31-34`, `:503-536`, `:805-807`.

Bonferroni is applied correctly and in both the right places — `alpha = method.adjusted_alpha()` feeds the interval quantiles at `:531-532` **and** the `z_two_sided` used by the MDR at `:807`/`:724`. That is right, and it is more than many implementations do.

But the tail quantile is `0.025/n`, and `quantile_sorted` positions it at `9999 × 0.025/n` in a sorted list of 10 000 draws:

| family n | alpha/2 | position | order statistics used |
|---|---|---|---|
| 25 | 1.0e-3 | 9.999 | #10, #11 |
| 50 | 5.0e-4 | 4.999 | #5, #6 |
| 100 | 2.5e-4 | 2.500 | #3, #4 |
| 250 | 1.0e-4 | 1.000 | #1, #2 |

From n ≈ 50 the endpoints are extreme order statistics with large Monte-Carlo variance; from n ≈ 250 the interval endpoint *is* the minimum of the 10 000 draws. A criterion bench target with 50+ benchmarks is ordinary. The comment at `:31-33` — "Ten thousand keeps the Monte-Carlo error of a 95% percentile interval well below the reporting precision" — is true of the *unadjusted* 95% level and is not true of the level actually computed. Measured against a 200 000-resample reference the B=10 000 interval was 3.1% narrower at n = 30; the bias direction is inward, i.e. anti-conservative, though small relative to P1-1.

**Fix.** Scale `bootstrap_resamples` with the adjusted alpha (e.g. require at least ~10/alpha), or cap the family size at which a percentile interval is claimed and report `inconclusive` beyond it. Either way, correct the comment to state which level the 10 000 figure covers.

---

### P3 findings

- **`benchmark_compare.rs:500`** — "Paired percentile bootstrap of the ratio of medians" contradicts its own next sentence and the code: each group is resampled independently. Independent is the correct choice here (the two sides are different code with possibly different sample counts); the word "paired" is simply wrong and misleads a reader auditing the method. It does not reach any published string. Rename it "two-sample percentile bootstrap".
- **`stdio/bloat/schemas.rs:129,166`** — `analysis_build_symbols_forced` and `estimated` are documented "Always `true`" but typed `bool`, so the emitted JSON Schema is `{"type":"boolean"}`. A schema-only consumer has no contractual guarantee. Constrain them to `const: true`.
- **Constant `summary` strings asserting outcomes that did not occur.** `summary` is a serialized field (`stdio/security_tool.rs:249-254`). `stdio/bloat.rs:388` emits "Exact measured file size and cargo-bloat's estimated attribution" even under `SizeMismatch`, where the sibling `error_message` says the attribution is *not* published as a description of the measured file, and even when `measured` is `None`. `stdio/benchmark_compare.rs:361` emits "Observed difference between two measurements under the frozen method" even when the result is `INCOMPATIBLE_DATASETS` with `method == None` — the method was never applied and no difference was observed. Make the summary a function of the outcome.
- **MDR gate ordering** (`:664-667`) suppresses a verdict the interval already resolves: a true 50% regression with high dispersion (SE 0.05 → MDR 0.14) is reported `inconclusive` even when the interval is `[0.40, 0.60]`. It errs safe and ADR-073 §4 specifies this order, but "precision insufficient to discriminate the threshold" is not the right description of a case where the interval excludes the threshold by an order of magnitude.
- **`inconclusive_reasons`** is not sorted or deduplicated (`:645-683`), unlike `IncompatibilityReason` which is (`:282-286`). Minor wire inconsistency.
- **`application/src/benchmark_compare.rs:102-105`** states "the execution adapter … supplies the implementation" of `DatasetDecoder`. It does not: the only production impl is `JsonDatasetDecoder` in `mcp-server/src/stdio/benchmark_compare.rs:197-210`. The decoder itself is correct and fail-closed; the doc comment is wrong about where it lives, and it does put durable-artifact JSON parsing in the MCP layer.
- **ADR-076's cargo-bloat citations.** `src/main.rs:694-696` is unverifiable from this tree (cargo-bloat's source is absent, build context deleted) and the receipt at `M5-04-bloat-calibration.json:83` restates the ADR's assertion verbatim in a `cause` field rather than recording an observation — the citation is circular. And the `--profile release-lto` mechanism disagrees three ways: ADR-076:136 says `CARGO_PROFILE_RELEASE_LTO`, the receipt at `:75` says `CARGO_PROFILE_<NAME>_STRIP` citing lines 690-696, and the observed error at `:74` names `CARGO_PROFILE_RELEASE`. The *outcome* (exit 1, that error string) is properly receipted; the *explanation* published in the ADR is not the one the receipt records.
- **Matrix/handoff state disagreements.** `M5-handoff.md:6` says "Tres de los cinco cortes están calificados nativamente"; its own table at `:32-36` marks two. `M5-matrix.md:51` says M5-05 is "In progress"; `M5-handoff.md:36` says "No ejecutado". The third qualified cut only materialises by counting "Admisión de imagen", which the matrix itself lists outside the cut numbering (`:52`).
- **`M5-matrix.md:66` and `M5-03-runtime.json:120-136`** — both cancellation rows claim "cancelado, árbol unido"; the receipts record only `observed_running: true` and `residue_after: {containers: [], volumes: []}`. No cancel signal, no join, no post-cancel process check. The observation and the no-residue check are real; the cancellation and the join are asserted, not recorded.
- **`M5-01-blocker.json:32`** — "Its compiled subset alone is roughly 20 MiB" is the only number that rules out pruning as an alternative to the blocker, and it appears in no `observed` block anywhere. Everything else in that blocker is receipted precisely.

**No P0.** I found nothing that silently corrupts data at rest, escapes containment, or fabricates a measurement out of nothing; the failures are in what the numbers *mean*, not in whether they were taken.

---

## Verified

Things I checked adversarially and found sound, with the reasoning:

- **Inverse normal CDF.** Acklam's rational approximation, transcribed exactly, checked against Wichura AS241 (`statistics.NormalDist().inv_cdf`) over 400 000 points spanning (1e-12, 1−1e-12): worst relative error **1.13e-9**, matching the code's own claim of ~1.2e-9 at `:373`. At every level the method actually uses (family 1…1000, p = 1 − 0.025/n) the worst error is 3.9e-9 absolute. The frozen constants are exact: `Z_TWO_SIDED_95` differs from Φ⁻¹(0.975) by 6.7e-16 and `Z_POWER_80` from Φ⁻¹(0.80) by 4.4e-16. `:374-377` correctly returns `None` outside (0,1) and never panics. **Sound.**
- **Bonferroni is applied to the interval as well as the decision** — `:806-807` computes both `alpha` and `z_two_sided` from `adjusted_alpha()`, `:531-532` uses `alpha/2` and `1−alpha/2` for the percentile endpoints, `:724` uses the adjusted `z` for the MDR. `adjusted_confidence_level = 1 − 0.05/n` at `:184`. Verified numerically at n = 5 (0.99, alpha 0.01). This is the correct construction and it is easy to get wrong.
- **PRNG.** SplitMix64 at `:333-339` is the standard constants and shifts. `for_benchmark` (`:323-331`) mixes FNV-1a of the key into the fixed root seed, so each benchmark draws its own stream and the stream does not depend on the benchmark's position — confirmed by `a_benchmark_result_does_not_depend_on_its_position_in_the_report` (`:1563`) and by my own recomputation. `index()` (`:344-350`) is multiply-shift with bias ≤ bound/2⁶⁴, negligible. I specifically tested **seed grinding**: the stream is a function of the benchmark key, which the project controls, so I ran the same sample data under 200 different benchmark names near the threshold — CI endpoints moved by at most 0.0014 and all 200 gave the same verdict. Not an attack surface. **Sound.**
- **Percentile bootstrap construction.** It is a proper percentile interval (`quantile_sorted` type-7 on the sorted bootstrap ratios), and the resampling is independent per group, which is the right choice for two non-paired executions. My transcription reproduces the crate's oracles to five digits. The construction is correct; what is wrong (P1-1) is the population it resamples from, not the mechanics.
- **Tukey fences.** `:463-476` uses type-7 Q1/Q3 with 1.5·IQR, counts only, and `compare_one:748-749` computes them over the full sorted set that the statistic also uses. `outliers_are_reported_and_kept_in_the_compared_sample_set` (`:1023`) pins that the median is still 5.5 with the 100.0 outlier present. Verified by hand on the n=10 fixture: Q1=3.25, Q3=7.75, fences [−3.5, 14.5], one outlier. **No post-hoc discarding anywhere.** Sound and matches ADR-073 §4.
- **MDR is not equated to the threshold.** `(z_adj + z_0.80) · SE` at `:723-727` is the standard two-sided minimum-detectable-effect form and is dimensionally consistent (SE is in ratio units). It is a separate published field (`schemas.rs:127-129`) and is checked against, not set to, the threshold. `dispersion_larger_than_the_effect_is_inconclusive` (`:1104`) reaches MDR 0.807 with a real 1% effect. `inconclusive` is genuinely reachable and, on the real fixture, fires on 4 of 6 comparisons. **Sound in form**, undermined only by which variance feeds `SE`.
- **`decide()` ordering** (`:633-684`). I walked every branch. `Missing` short-circuits first with zeroed numbers and the doc at `:228-230` says a zero there is an absence. Truncated is recorded and then *forces* `Inconclusive` at `:680-682` regardless of what the interval said. Strict inequalities mean exactly-on-the-threshold falls through to `Inconclusive` — pinned at `:1239-1242`. `ZeroOrNegativeBaseline` is unreachable in practice because `RawSample::validate` (`:265-274`) requires `total_ns > 0`, so it is correctly defensive rather than dead. I could not find a fall-through that produces a *wrong* directional verdict from a correctly-estimated interval. The ordering is right.
- **Dataset versioning is genuinely fail-closed, on both paths.** Producer: `BenchmarkDataset::new` (`benchmark.rs:510-524`) stamps `format`/`format_version` from crate constants and never takes them from a caller. Reader: `JsonDatasetDecoder` (`stdio/benchmark_compare.rs:198-210`) bounds the size, deserializes into a `deny_unknown_fields` struct, and calls `validate()`, which rejects any format string or version that is not exactly v1 (`benchmark.rs:531-536`); `Analyzer::load_dataset` (`application/src/benchmark_compare.rs:197-201`) calls `validate()` again; and the store-level gate at `:142-144` requires `QualityArtifactKind::BenchmarkDataset` + `PayloadFormatVersion::BenchmarkDatasetV1` before a byte is read. `compare` catches a mismatch a third time at `:555-561`. A v2 payload is refused, never coerced — pinned by `an_unknown_dataset_format_is_incompatible_before_any_statistic` (`:1429`). The format string is a full identifier, not a bare integer, so a foreign producer reusing `format_version: 1` is still rejected (`benchmark.rs:24-27`). **This is done properly.**
- **Exact size vs estimated attribution in `rust.binary.bloat`.** Structurally separated into sibling `Option` objects (`stdio/bloat/schemas.rs:203-207`), with `MeasuredBinary` described as "Facts this product measured itself … an exact measurement of one file" (`:110-115`) and `BloatAttribution` as "Every field here comes from `cargo-bloat`, never from this product's own measurement, and none of it is exact" (`:158-160`). `reported_file_size_bytes` explicitly points at `measured.size_bytes` for the exact value (`:170`); every ranking row says "Estimated bytes attributed to". `estimated: true` is the only construction (`performance_port.rs:677`), enforced by the domain invariant `BloatObservation::consistent()` (`domain/src/bloat.rs:212-224`) and refused at `application/src/bloat.rs:73`. A disagreement between the two sizes downgrades completeness to `SizeMismatch` rather than publishing the ranking as a description of the file. `analysis_build_symbols_forced` declares that the measured file is an analysis build and not the shippable artifact. **A reader cannot take one for the other.** This is the strongest part of M5.
- **No causality, no generalization, no recommendation.** I read every field name, enum spelling, doc comment and tool description across the four modules against spec §29 (line 1697, `suggest_optimizations`) and §92 (line 4177, universal performance advice). Nothing violates either. `semantics` is a published field carrying `observed_difference_between_two_measurements_without_attribution` (`stdio/benchmark_compare.rs:349`). `Regression`/`Improvement` carry conventional causal freight as words, but their doc comments define them purely as "materially slower/faster than the baseline one" and the four names are prescribed by the roadmap at `:67`. The trimming order treats `Regression` and `Improvement` identically, so truncation is not biased toward bad news. **Clean.**
- **M4 entry facts.** `M5-matrix.md:13-18`'s PR #15 merge, head SHA `e1be3a37…`, `mergedAt 2026-09-08T17:58:12Z` and 10/10 SUCCESS checks were re-verified live against `gh` and match exactly.
- **The M5-01 blocker and the refusal to widen the bound.** `M5-01-runtime.json:19-37` records 6014 files / 779 dirs / 6793 entries / 156 267 469 bytes and four over-cap files against the `SourceBundle` limits, byte-identically in `M5-01-blocker.json:10-28`. `M5-01-blocker.json:40-42`: "The vendor bounds were **NOT** raised … widening them to make one test pass would weaken a qualified security bound without a decision or a re-qualification. No measurement was fabricated and no threshold was lowered." I checked: `MATERIAL_THRESHOLD_RATIO` is still 0.05 and both `CALIBRATED` flags are still `false`. **This is the right call and it is documented correctly.**
- **`cargo test -p rust-engineering-domain --offline --locked` passes** on the current bytes.

---

## Overstated or unsupported claims

1. **ADR-073 §3, "Unknown permanece unknown … y bloquea la comparación."** True only of `cpu_model`. Contradicted by §5 of the same ADR and by the code.
2. **ADR-076 §2 and Consequences.** "Los DTO M1–M4 declaran su propio enum cerrado por tool mediante `#[schemars(with = …)]`" — no `#[schemars(with = …)]` in the workspace references any of the five shared enums; the real (and stronger) guarantee is that `crates/domain` has no `schemars` dependency at all, so those enums have no `JsonSchema` impl and *cannot* reach a schema. "Cuatro snapshots nuevos se añaden" and "El inventario pasa a treinta y una tools" — neither happened.
3. **ADR-076 §6's `src/main.rs:694-696` citation** is circular with its own receipt and unverifiable from this tree; the `--profile release-lto` env-var mechanism it publishes contradicts both the receipt and the observed error.
4. **ADR-073 §4's characterisation of the interval.** "Intervalo: bootstrap percentil con 10 000 remuestreos" is accurate about the mechanics and silent about what population is resampled. Given the default `run_count = 3` and `pool()`, the interval is an unclustered bootstrap over a clustered sample; nothing in the ADR, the `Method` DTO or the tool description says so.
5. **`benchmark_compare.rs:31-33`'s Monte-Carlo claim** is scoped to the unadjusted 95% level and does not hold at the Bonferroni-adjusted level for families ≳ 50.
6. **`docs/roadmap/m5-performance.md:128`, "casos ruidosos dan inconclusive."** Falsified by simulation (up to 22% false directional verdicts) and by a concrete false `improvement` on the milestone's own committed archives.
7. **`M5-01-benchmark-calibration.json:191` `"status": "passed"`** on a receipt containing no observed verdict, no interval and no MDR — only `expected_verdict`.
8. **`M5-matrix.md:99-102`'s "tres ejecuciones en el host"** — two of the six ratios are corroborated nowhere, and the corroborated pair comes from a `--sample-size 10` run the fixture README itself calls "a single unreplicated observation".
9. **`M5-01-blocker.json:53`, "captured from this image."** They were captured from `e9ecc40d…`; the document pins `0e21c561…`.
10. **`fixtures/benchmark/src/lib.rs:70`, "The expected design ratio is 1.00x"** and `:64-70`'s "the only difference … is measurement noise" — refuted by the project's own guest measurement (−2.9%) and retracted only in a different file.
11. **`M5-handoff.md:6`, "Tres de los cinco cortes están calificados nativamente"** — its own table marks two.
12. **`application/src/benchmark_compare.rs:102-105`** — the decoder is not supplied by the execution adapter.

---

## Limitations of this review

- I did not run Docker, so every runtime claim (image contents, guest behaviour, profiling capability, cancellation, cleanup) rests on the receipts, which I cross-checked against each other and against source but could not reproduce.
- I ran `cargo test -p rust-engineering-domain` only. I did not run the full workspace test suite, clippy, `scripts/gate.py`, or the native ignored tests, so I cannot state whether the full gate passes on these bytes. No M5 gate receipt exists to compare against.
- My statistical conclusions rest on (a) an exact transcription of `benchmark_compare.rs` into Python and Rust, validated by reproducing the crate's own oracles to five significant digits, and (b) a Gaussian random-effects model for run-to-run drift. The *direction* and *mechanism* of the SE understatement are model-independent (an unclustered bootstrap over a clustered sample always understates SE under positive intra-cluster correlation), but the exact false-positive percentages depend on that model. The false `improvement` on `criterion-run-2.tar` vs `criterion-candidate.tar` is not model-dependent: it is a recomputation on the committed bytes.
- That false verdict assumes `fixtures/benchmark-datasets/README.md:12` is accurate that only `work_unit` changed in the candidate archive. If more changed, the finding weakens — but then `M5-01-benchmark-calibration.json:174-180`'s "known_direction" oracle is not isolating the change it claims to isolate, which is a defect of comparable weight. I could not verify the candidate's source from the tar, which contains only criterion output.
- I did not audit the profiling capability, containment or SVG sanitisation (ADR-074 / M5-03) beyond reading the receipts, nor the M2/M3/M4 code these tools sit on. Opus 5 High review of profiling permission and containment is required separately by the DoD and is not covered here.
- I did not attempt to determine whether the four tools' input/output schemas would break anything once they *are* advertised, because no snapshot exists to compare against. That check must be redone after P1-2 is fixed.
