#!/usr/bin/env python3
"""Judge candidate comparison methods against the criteria ADR-081 froze.

ADR-081 was committed BEFORE any simulation existed, precisely so that "the
thresholds were fixed before seeing which parameters produced the desired
result" is checkable. This script is the instrument, not the decision: it
measures every criterion of ADR-081 §1 at every drift point of §2 for every
candidate, and writes what it measured. It never tunes anything to make a
candidate pass, and it contains no threshold that ADR-081 does not state.

What actually computes the numbers is the product's own code. The simulation
lives in `crates/domain/src/benchmark_compare/simulation.rs`, a `#[cfg(test)]`
CHILD module of `crates/domain/src/benchmark_compare.rs`, so it drives the
shipped `decide`, `cluster_draw`, `bootstrap_ratio`, `quantile_sorted`,
`standard_deviation` and `SideSamples` directly. Nothing about the shipped
method changes; the candidates change only the interval and the standard error
handed to the shipped decision rule, which is the lever ADR-081 §5 authorizes.

Two tests in that module are the evidence that this is so, and both are run here
before any measurement is believed:

  * `the_local_bootstrap_loop_reproduces_the_products_bootstrap_ratio` holds the
    one duplicated loop byte-for-byte equal to `bootstrap_ratio`.
  * `the_harness_agrees_with_the_public_compare_on_the_shipped_estimator` holds
    the harness's shipped-estimator path exactly equal to the public `compare`
    on interval, minimum detectable ratio, effect ratio and verdict.

Usage:

    python3 -B scripts/simulate-m5-comparison-method.py

Everything is seeded; the same invocation produces the same numbers and the
seed is written into the receipt.
"""

from __future__ import annotations

import argparse
import datetime as _datetime
import json
import math
import os
import pathlib
import platform
import subprocess
import sys
import tarfile
import time

if not __debug__:
    raise RuntimeError("Optimized Python mode is rejected")

ROOT = pathlib.Path(__file__).resolve().parents[1]
DEFAULT_OUTPUT = ROOT / "docs/validation/M5-02-method-simulation.json"
DEFAULT_TARGET = ROOT / "target/m5-method-simulation"
TEST_PATH = "benchmark_compare::simulation::m5_02_requalification"
VALIDATION_TESTS = (
    "benchmark_compare::simulation::the_local_bootstrap_loop_reproduces_the_products_bootstrap_ratio",
    "benchmark_compare::simulation::the_harness_agrees_with_the_public_compare_on_the_shipped_estimator",
    "benchmark_compare::simulation::the_simulation_does_not_depend_on_the_thread_count",
    "benchmark_compare::simulation::student_t_quantiles_match_the_published_table",
    "benchmark_compare::simulation::standard_normal_cdf_matches_known_values",
    "benchmark_compare::simulation::every_candidate_decides_at_every_drift_point",
)
BEGIN = "M5_SIMULATION_JSON_BEGIN"
END = "M5_SIMULATION_JSON_END"

# ---------------------------------------------------------------------------
# ADR-081 §1 and §2, transcribed and nowhere else in this file.
# ---------------------------------------------------------------------------

# ADR-081 §2 fixes the drift range; a method that only holds at tau = 0 fails.
DRIFT_POINTS = (0.0, 0.01, 0.02, 0.05, 0.10)

# True effects the criteria of §1 name: the null for coverage and false
# positives, +5% (the material threshold itself) for power, and +/-10% for the
# incorrect `no_material_change`.
TRUE_EFFECTS = (0.0, 0.05, 0.10, -0.10)

MIN_REPLICATES = 10_000

CRITERIA = {
    "coverage": {
        "statement": "Cobertura del efecto verdadero por el intervalo publicado",
        "threshold": 0.93,
        "direction": "at_least",
        "measured_over": "every drift point of ADR-081 §2 and every true effect simulated; the reported number is the worst cell",
        "adr": "ADR-081 §1, row 1",
    },
    "directional_false_positive_rate": {
        "statement": "Falsos positivos direccionales bajo nulo verdadero (regression + improvement)",
        "threshold": 0.01,
        "direction": "at_most",
        "measured_over": "true effect 0 at every drift point; the reported number is the worst drift point",
        "adr": "ADR-081 §1, row 2",
    },
    "power_at_material_threshold": {
        "statement": "Potencia para detectar un efecto igual al umbral material del 5 %",
        "threshold": 0.80,
        "direction": "at_least",
        "measured_over": "true effect +0.05 at every drift point; the reported number is the worst drift point",
        "adr": "ADR-081 §1, row 3",
    },
    "incorrect_no_material_change_rate": {
        "statement": "`no_material_change` incorrecto cuando el efecto real supera el umbral",
        "threshold": 0.05,
        "direction": "at_most",
        "measured_over": "true effect +/-0.10 at every drift point; the reported number is the worst cell",
        "adr": "ADR-081 §1, row 4",
    },
    "budget_seconds": {
        "statement": "Presupuesto de una comparación, en segundos",
        "threshold": 30.0,
        "direction": "at_most",
        "measured_over": "measured wall time of one comparison, at the largest family this method resolves (25)",
        "adr": "ADR-081 §1, row 5",
    },
    "budget_peak_rss_mib": {
        "statement": "Presupuesto de una comparación, en MiB residentes",
        "threshold": 512.0,
        "direction": "at_most",
        "measured_over": "measured peak RSS of the process that ran it, at the largest family this method resolves (25)",
        "adr": "ADR-081 §1, row 5",
    },
}


def utc_now() -> str:
    return (
        _datetime.datetime.now(_datetime.timezone.utc)
        .replace(microsecond=0)
        .isoformat()
        .replace("+00:00", "Z")
    )


def wilson(successes: int, total: int, z: float = 1.959963984540054) -> list[float]:
    """95% Wilson interval, so a borderline criterion is visibly borderline."""
    if total <= 0:
        return [0.0, 0.0]
    p = successes / total
    denominator = 1.0 + z * z / total
    center = (p + z * z / (2 * total)) / denominator
    spread = (
        z * ((p * (1 - p) / total + z * z / (4 * total * total)) ** 0.5) / denominator
    )
    return [round(max(0.0, center - spread), 6), round(min(1.0, center + spread), 6)]


# ---------------------------------------------------------------------------
# Independent cross-check against a number the product already published
# ---------------------------------------------------------------------------
#
# Everything below re-implements the method from scratch, in Python, over the
# REAL criterion bytes in fixtures/benchmark-datasets/. It shares no code with
# the crate. Its only job is to reproduce a number ADR-073 §4 published, so a
# reader has one point where the crate, this script and the written record are
# checked against each other instead of against themselves.

# ADR-073 §4, "Corrección (2026-09-08)": comparing `m5/control` alone (a family
# of one) between `criterion-run-2.tar` as baseline and `criterion-candidate.tar`
# as candidate, the v2 method with a single execution per side returns this.
PUBLISHED = {
    "source": "docs/adr/ADR-073-benchmark-method-and-dataset.md §4",
    "benchmark": "m5/control",
    "baseline": "fixtures/benchmark-datasets/criterion-run-2.tar",
    "candidate": "fixtures/benchmark-datasets/criterion-candidate.tar",
    "family_size": 1,
    "executions_per_side": 1,
    "effect_ratio": -0.1231,
    "interval": [-0.1494, -0.0751],
    "minimum_detectable_ratio": 0.0490,
}

_MASK = (1 << 64) - 1
_GAMMA = 0x9E37_79B9_7F4A_7C15


class _SplitMix64:
    """The generator `benchmark_compare.rs` writes out, written out again."""

    def __init__(self, state: int) -> None:
        self.state = state & _MASK

    @classmethod
    def for_benchmark(cls, key: str, root_seed: int) -> "_SplitMix64":
        digest = 0xCBF2_9CE4_8422_2325
        for byte in key.encode():
            digest ^= byte
            digest = (digest * 0x0000_0100_0000_01B3) & _MASK
        return cls(root_seed ^ digest)

    def next_u64(self) -> int:
        self.state = (self.state + _GAMMA) & _MASK
        z = self.state
        z = ((z ^ (z >> 30)) * 0xBF58_476D_1CE4_E5B9) & _MASK
        z = ((z ^ (z >> 27)) * 0x94D0_49BB_1331_11EB) & _MASK
        return z ^ (z >> 31)

    def index(self, bound: int) -> int:
        return 0 if bound == 0 else (self.next_u64() * bound) >> 64


def _median(sorted_values: list[float]) -> float:
    count = len(sorted_values)
    if count == 0:
        return 0.0
    middle = count // 2
    if count % 2 == 0:
        return (sorted_values[middle - 1] + sorted_values[middle]) / 2.0
    return sorted_values[middle]


def _quantile(sorted_values: list[float], q: float) -> float:
    count = len(sorted_values)
    if count == 0:
        return 0.0
    position = (count - 1) * min(max(q, 0.0), 1.0)
    lower = math.floor(position)
    upper = min(lower + 1, count - 1)
    return sorted_values[lower] + (position - lower) * (
        sorted_values[upper] - sorted_values[lower]
    )


def _stdev(values: list[float]) -> float:
    count = len(values)
    if count < 2:
        return 0.0
    mean = sum(values) / count
    return math.sqrt(sum((value - mean) ** 2 for value in values) / (count - 1))


def _per_iteration(archive: pathlib.Path, benchmark: str) -> list[list[float]]:
    """One capture is one execution, so one cluster of raw per-iteration times."""
    with tarfile.open(archive) as tar:
        member = tar.extractfile(f"./{benchmark}/new/sample.json")
        if member is None:
            raise SystemExit(f"{archive} has no sample.json for {benchmark}")
        sample = json.load(member)
    return [
        [time_ns / iterations for time_ns, iterations in zip(sample["times"], sample["iters"])]
    ]


def _cluster_draw(rng: _SplitMix64, executions: list[list[float]]) -> float:
    draw: list[float] = []
    count = len(executions)
    for _ in range(count):
        execution = executions[rng.index(count)]
        for _ in range(len(execution)):
            draw.append(execution[rng.index(len(execution))])
    draw.sort()
    return _median(draw)


def cross_check(root_seed: int, resamples: int, alpha: float = 0.05) -> dict:
    baseline = _per_iteration(ROOT / PUBLISHED["baseline"], PUBLISHED["benchmark"])
    candidate = _per_iteration(ROOT / PUBLISHED["candidate"], PUBLISHED["benchmark"])
    effect = _median(sorted(candidate[0])) / _median(sorted(baseline[0])) - 1.0
    rng = _SplitMix64.for_benchmark(PUBLISHED["benchmark"], root_seed)
    ratios = []
    for _ in range(resamples):
        baseline_median = _cluster_draw(rng, baseline)
        candidate_median = _cluster_draw(rng, candidate)
        ratios.append(
            candidate_median / baseline_median - 1.0 if baseline_median > 0.0 else 0.0
        )
    standard_error = _stdev(ratios)
    ratios.sort()
    interval = [_quantile(ratios, alpha / 2.0), _quantile(ratios, 1.0 - alpha / 2.0)]
    # The frozen MDR of ADR-073 §4: (z_{1-alpha/2 adjusted} + z_{0.80}) * SE.
    mdr = (1.959963984540054 + 0.841621233572914) * standard_error
    reproduced = {
        "effect_ratio": round(effect, 4),
        "interval": [round(interval[0], 4), round(interval[1], 4)],
        "minimum_detectable_ratio": round(mdr, 4),
    }
    matches = (
        reproduced["effect_ratio"] == PUBLISHED["effect_ratio"]
        and reproduced["interval"] == PUBLISHED["interval"]
        and reproduced["minimum_detectable_ratio"]
        == PUBLISHED["minimum_detectable_ratio"]
    )
    return {
        "what": (
            "an implementation of the frozen method written from scratch in this "
            "script, sharing no code with the crate, run over the real criterion bytes "
            "in fixtures/benchmark-datasets/, against a number ADR-073 §4 already "
            "published for exactly this pair"
        ),
        "published": PUBLISHED,
        "reproduced": reproduced,
        "reproduced_full_precision": {
            "effect_ratio": effect,
            "interval": interval,
            "minimum_detectable_ratio": mdr,
        },
        "resamples": resamples,
        "matches_to_published_precision": matches,
        "what_it_establishes": (
            "the seed derivation, the two-stage cluster draw, the type-7 quantile, the "
            "sample standard deviation and the MDR formula are all understood the same "
            "way by the crate, by this script and by the written record"
        ),
    }


# ---------------------------------------------------------------------------
# Running the Rust harness
# ---------------------------------------------------------------------------


def cargo_environment(target_dir: pathlib.Path) -> dict[str, str]:
    environment = dict(os.environ)
    # A private target directory: a full gate may be running in `target/` and
    # this must neither block on its lock nor invalidate its artifacts.
    environment["CARGO_TARGET_DIR"] = str(target_dir)
    return environment


def build_harness(target_dir: pathlib.Path) -> pathlib.Path:
    """Builds the release test binary and returns its path.

    `--locked --offline` on purpose: this script must not resolve, fetch or
    rewrite `Cargo.lock`.
    """
    command = [
        "cargo",
        "test",
        "--locked",
        "--offline",
        "--release",
        "-p",
        "rust-engineering-domain",
        "--lib",
        "--no-run",
        "--message-format=json",
    ]
    completed = subprocess.run(
        command,
        cwd=ROOT,
        env=cargo_environment(target_dir),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    if completed.returncode != 0:
        sys.stderr.write(completed.stderr)
        raise SystemExit("the domain test binary did not build")
    executable = None
    for line in completed.stdout.splitlines():
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        if message.get("reason") != "compiler-artifact":
            continue
        path = message.get("executable")
        if path and "rust_engineering_domain" in path:
            executable = path
    if executable is None:
        raise SystemExit("no rust-engineering-domain test executable was produced")
    return pathlib.Path(executable)


def run_binary(
    binary: pathlib.Path, test: str, environment: dict[str, str]
) -> tuple[str, float, int, int]:
    """Runs one test and returns (stdout, wall seconds, peak RSS bytes, status).

    `os.wait4` gives the rusage of THIS child rather than the high-water mark
    over every child this process ever had, which is what makes a per-candidate
    memory number meaningful.
    """
    merged = dict(os.environ)
    merged.update(environment)
    started = time.monotonic()
    process = subprocess.Popen(
        [str(binary), test, "--exact", "--nocapture", "--test-threads=1"],
        cwd=ROOT,
        env=merged,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
    )
    output = process.stdout.read() if process.stdout else ""
    _, status, usage = os.wait4(process.pid, 0)
    elapsed = time.monotonic() - started
    # ru_maxrss is bytes on Darwin and kibibytes on Linux.
    peak = usage.ru_maxrss if sys.platform == "darwin" else usage.ru_maxrss * 1024
    return output, elapsed, peak, status


def payload(output: str) -> dict:
    if BEGIN not in output or END not in output:
        raise SystemExit("the harness produced no JSON document")
    return json.loads(output.split(BEGIN, 1)[1].split(END, 1)[0])


# ---------------------------------------------------------------------------
# Criteria
# ---------------------------------------------------------------------------


def evaluate(points: list[dict], replicates: int) -> dict[str, dict]:
    """Every criterion of ADR-081 §1 for every candidate, from the raw cells."""
    per_candidate: dict[str, dict] = {}
    for point in points:
        per_candidate.setdefault(point["candidate"], []).append(point)

    results: dict[str, dict] = {}
    for candidate, cells in per_candidate.items():
        cells = sorted(cells, key=lambda cell: (cell["true_effect"], cell["drift_sd"]))

        coverage_cells = [
            {
                "drift_sd": cell["drift_sd"],
                "true_effect": cell["true_effect"],
                "value": cell["coverage"],
                "replicates": cell["replicates"],
                "interval": wilson(
                    round(cell["coverage"] * cell["replicates"]), cell["replicates"]
                ),
            }
            for cell in cells
        ]
        null_cells = [cell for cell in cells if cell["true_effect"] == 0.0]
        power_cells = [cell for cell in cells if cell["true_effect"] == 0.05]
        material_cells = [cell for cell in cells if abs(cell["true_effect"]) == 0.10]

        false_positive_cells = [
            {
                "drift_sd": cell["drift_sd"],
                "true_effect": cell["true_effect"],
                "value": cell["directional_rate"],
                "replicates": cell["replicates"],
                "interval": wilson(
                    cell["verdict_regression"] + cell["verdict_improvement"],
                    cell["replicates"],
                ),
            }
            for cell in null_cells
        ]
        power_rows = [
            {
                "drift_sd": cell["drift_sd"],
                "true_effect": cell["true_effect"],
                "value": cell["directional_correct_rate"],
                "replicates": cell["replicates"],
                "interval": wilson(cell["verdict_regression"], cell["replicates"]),
                "supplementary": {
                    "interval_beyond_threshold_rate": cell[
                        "interval_beyond_threshold_rate"
                    ],
                    "interval_excludes_zero_rate": cell["interval_excludes_zero_rate"],
                    "not_no_material_change_rate": round(
                        1.0 - cell["no_material_change_rate"], 6
                    ),
                },
            }
            for cell in power_cells
        ]
        material_rows = [
            {
                "drift_sd": cell["drift_sd"],
                "true_effect": cell["true_effect"],
                "value": cell["no_material_change_rate"],
                "replicates": cell["replicates"],
                "interval": wilson(
                    cell["verdict_no_material_change"], cell["replicates"]
                ),
            }
            for cell in material_cells
        ]

        def worst(rows: list[dict], direction: str) -> dict | None:
            if not rows:
                return None
            return (
                min(rows, key=lambda row: row["value"])
                if direction == "at_least"
                else max(rows, key=lambda row: row["value"])
            )

        results[candidate] = {
            "coverage": {
                "cells": coverage_cells,
                "worst": worst(coverage_cells, "at_least"),
            },
            "directional_false_positive_rate": {
                "cells": false_positive_cells,
                "worst": worst(false_positive_cells, "at_most"),
            },
            "power_at_material_threshold": {
                "cells": power_rows,
                "worst": worst(power_rows, "at_least"),
            },
            "incorrect_no_material_change_rate": {
                "cells": material_rows,
                "worst": worst(material_rows, "at_most"),
            },
            "replicates_per_point": replicates,
            "meets_replicate_floor": replicates >= MIN_REPLICATES,
        }
    return results


def verdict_for(name: str, value: float | None) -> str:
    if value is None:
        return "not_measured"
    criterion = CRITERIA[name]
    if criterion["direction"] == "at_least":
        return "pass" if value >= criterion["threshold"] else "fail"
    return "pass" if value <= criterion["threshold"] else "fail"


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--replicates", type=int, default=MIN_REPLICATES)
    parser.add_argument("--probe-replicates", type=int, default=2_000)
    parser.add_argument("--threads", type=int, default=max(1, (os.cpu_count() or 2) - 4))
    parser.add_argument(
        "--seed",
        default=None,
        help="decimal or 0x-prefixed; defaults to the harness's own SIMULATION_ROOT_SEED",
    )
    parser.add_argument("--out", type=pathlib.Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--target-dir", type=pathlib.Path, default=DEFAULT_TARGET)
    parser.add_argument(
        "--skip-probe",
        action="store_true",
        help="skip the resample-count sensitivity probe",
    )
    arguments = parser.parse_args()

    if arguments.replicates < MIN_REPLICATES:
        sys.stderr.write(
            f"warning: ADR-081 §1 requires at least {MIN_REPLICATES} replicates per "
            f"point; the receipt will record {arguments.replicates} and say the floor "
            "was not met\n"
        )

    arguments.target_dir.mkdir(parents=True, exist_ok=True)
    binary = build_harness(arguments.target_dir)
    print(f"harness: {binary}", file=sys.stderr)

    base_environment: dict[str, str] = {}
    if arguments.seed is not None:
        base_environment["M5_SIM_SEED"] = arguments.seed

    # 1. The harness has to be shown to be the product before its numbers count.
    validation = []
    for test in VALIDATION_TESTS:
        _, elapsed, _, status = run_binary(binary, test, base_environment)
        validation.append(
            {
                "test": test,
                "result": "pass" if status == 0 else "fail",
                "seconds": round(elapsed, 3),
            }
        )
        print(f"validation {test.rsplit('::', 1)[1]}: {validation[-1]['result']}", file=sys.stderr)
    if any(entry["result"] != "pass" for entry in validation):
        raise SystemExit("the harness failed its own equivalence tests; numbers withheld")

    # 2. The simulation.
    simulation_environment = dict(base_environment)
    simulation_environment.update(
        {
            "M5_SIM_MODE": "simulate",
            "M5_SIM_REPLICATES": str(arguments.replicates),
            "M5_SIM_THREADS": str(arguments.threads),
            "M5_SIM_DRIFTS": ",".join(str(value) for value in DRIFT_POINTS),
            "M5_SIM_EFFECTS": ",".join(str(value) for value in TRUE_EFFECTS),
            "M5_SIM_EXECUTIONS": "3,5,10",
        }
    )
    print(
        f"simulating {arguments.replicates} replicates x {len(DRIFT_POINTS)} drift points "
        f"x {len(TRUE_EFFECTS)} true effects on {arguments.threads} threads",
        file=sys.stderr,
    )
    output, elapsed, _, status = run_binary(binary, TEST_PATH, simulation_environment)
    if status != 0:
        raise SystemExit("the simulation run failed")
    run = payload(output)
    print(f"simulation wall: {elapsed / 60.0:.1f} min", file=sys.stderr)

    # 2b. The independent cross-check, seeded from the crate's OWN constant as
    #     the harness reported it, so this script never restates it.
    method_seed = int(run["method_constants"]["bootstrap_seed_hex"], 16)
    reproduction = cross_check(method_seed, run["method_constants"]["bootstrap_resamples"])
    print(
        "cross-check against ADR-073 §4's published interval: "
        + ("reproduced" if reproduction["matches_to_published_precision"] else "MISMATCH"),
        file=sys.stderr,
    )
    if not reproduction["matches_to_published_precision"]:
        sys.stderr.write(
            "warning: the from-scratch implementation did not reproduce the number "
            "ADR-073 §4 publishes. The receipt records the mismatch; do not read the "
            "simulation as confirmed until it is explained.\n"
        )

    # 3. The resample-count lever, which ADR-081 §5 also authorizes. It is a
    #    probe and not a candidate: it is run at fewer replicates than §1 asks
    #    for, and the receipt says so rather than quietly counting it.
    probe = None
    if not arguments.skip_probe:
        probe_environment = dict(simulation_environment)
        probe_environment.update(
            {
                "M5_SIM_REPLICATES": str(arguments.probe_replicates),
                "M5_SIM_EXECUTIONS": "3",
                "M5_SIM_EFFECTS": "0.0",
                "M5_SIM_RESAMPLES": "40000",
            }
        )
        print("probing the resample-count lever (40 000 resamples)", file=sys.stderr)
        probe_output, _, _, probe_status = run_binary(
            binary, TEST_PATH, probe_environment
        )
        if probe_status == 0:
            probe_run = payload(probe_output)
            probe = {
                "question": "does raising the resample count from 10 000 to 40 000 move coverage?",
                "answer_is_structural": (
                    "the gap ADR-073 §4 names is a bias of the cluster bootstrap's scale, "
                    "not Monte-Carlo error of the percentile, so more resamples cannot "
                    "close it; this probe measures that rather than asserting it"
                ),
                "replicates_per_point": arguments.probe_replicates,
                "meets_replicate_floor": arguments.probe_replicates >= MIN_REPLICATES,
                "resamples": 40_000,
                "points": [
                    {
                        "candidate": point["candidate"],
                        "drift_sd": point["drift_sd"],
                        "true_effect": point["true_effect"],
                        "coverage": point["coverage"],
                        "directional_rate": point["directional_rate"],
                    }
                    for point in probe_run["points"]
                ],
            }

    # 4. Budget, measured rather than estimated.
    floor_output, _, floor_rss, _ = run_binary(
        binary,
        "benchmark_compare::simulation::the_cluster_shortfall_is_the_documented_factor",
        base_environment,
    )
    del floor_output
    budgets = []
    for candidate in run["candidates"]:
        entry = {"candidate": candidate["id"], "families": []}
        for family, repetitions in ((1, 5), (25, 1)):
            budget_environment = dict(base_environment)
            budget_environment.update(
                {
                    "M5_SIM_MODE": "budget",
                    "M5_SIM_CANDIDATE": candidate["id"],
                    "M5_SIM_FAMILY": str(family),
                    "M5_SIM_REPETITIONS": str(repetitions),
                }
            )
            budget_output, _, peak, budget_status = run_binary(
                binary, TEST_PATH, budget_environment
            )
            if budget_status != 0:
                continue
            measured = payload(budget_output)
            entry["families"].append(
                {
                    "family_size": family,
                    "repetitions": repetitions,
                    "seconds_per_comparison": round(
                        measured["seconds_per_comparison"], 6
                    ),
                    "seconds_per_public_compare": (
                        round(measured["seconds_per_public_compare"], 6)
                        if measured.get("seconds_per_public_compare") is not None
                        else None
                    ),
                    "process_peak_rss_mib": round(peak / (1024 * 1024), 2),
                }
            )
        budgets.append(entry)
        print(f"budget {candidate['id']}: {entry['families']}", file=sys.stderr)

    budget_by_candidate = {entry["candidate"]: entry for entry in budgets}
    process_floor_mib = round(floor_rss / (1024 * 1024), 2)

    # 5. Assemble.
    measured = evaluate(run["points"], arguments.replicates)
    candidates_report = []
    for candidate in run["candidates"]:
        name = candidate["id"]
        cells = measured.get(name, {})
        budget_entry = budget_by_candidate.get(name, {"families": []})
        worst_family = max(
            budget_entry["families"],
            key=lambda family: family["seconds_per_comparison"],
            default=None,
        )
        criteria_report = {}
        for key in (
            "coverage",
            "directional_false_positive_rate",
            "power_at_material_threshold",
            "incorrect_no_material_change_rate",
        ):
            block = cells.get(key, {})
            worst = block.get("worst")
            value = worst["value"] if worst else None
            criteria_report[key] = {
                **CRITERIA[key],
                "worst_value": value,
                "worst_at": (
                    {"drift_sd": worst["drift_sd"], "true_effect": worst["true_effect"]}
                    if worst
                    else None
                ),
                "worst_wilson_95": worst["interval"] if worst else None,
                "verdict": verdict_for(key, value),
                "per_point": block.get("cells", []),
            }
            if key == "power_at_material_threshold":
                # The frozen rule emits `regression` exactly when the interval's
                # lower endpoint exceeds +5%. With a TRUE effect of +5% that is
                # the event "the interval lies entirely above the true value",
                # which is a NON-COVERAGE event. So at this point, and for any
                # interval whatsoever, power <= 1 - coverage. Both numbers are in
                # this receipt at the same cells, so the bound is checkable here
                # rather than taken on trust.
                paired = {
                    (cell["drift_sd"], cell["true_effect"]): cell["value"]
                    for cell in criteria_report["coverage"]["per_point"]
                }
                criteria_report[key]["structural_bound"] = {
                    "identity": (
                        "verdict == regression at a true effect of exactly +5% IS the "
                        "event {interval lower endpoint > true effect}, a one-sided "
                        "non-coverage event; therefore power <= 1 - coverage at the "
                        "same cell, for any interval and any drift model"
                    ),
                    "consequence": (
                        "power >= 0.80 would require coverage <= 0.20 at that point, "
                        "which contradicts the >= 0.93 the coverage row of the same "
                        "table demands. The two rows cannot both hold."
                    ),
                    "per_point": [
                        {
                            "drift_sd": cell["drift_sd"],
                            "power": cell["value"],
                            "coverage_at_same_cell": paired.get(
                                (cell["drift_sd"], cell["true_effect"])
                            ),
                            "bound_1_minus_coverage": (
                                round(
                                    1.0
                                    - paired[(cell["drift_sd"], cell["true_effect"])],
                                    6,
                                )
                                if (cell["drift_sd"], cell["true_effect"]) in paired
                                else None
                            ),
                        }
                        for cell in block.get("cells", [])
                    ],
                }
                criteria_report[key]["other_readings_of_detect"] = {
                    "note": (
                        "ADR-081 says 'potencia para detectar un efecto igual al umbral "
                        "material del 5 %' without naming which event counts as "
                        "detection. Three readings are reported; only the first is the "
                        "product's own directional verdict, and only the owner may "
                        "decide that a different one is what the frozen row meant."
                    ),
                    "as_the_frozen_rule_decides": "verdict == regression (the value above)",
                    "as_adr_073_defines_detection_for_the_mdr": (
                        "the interval excludes ZERO on the true effect's side, which is "
                        "the convention behind MDR = (z_{1-alpha/2} + z_{0.80}) * SE"
                    ),
                    "as_not_calling_it_unchanged": "verdict != no_material_change",
                    "per_point": [
                        {
                            "drift_sd": cell["drift_sd"],
                            "verdict_regression": cell["value"],
                            "interval_excludes_zero": cell["supplementary"][
                                "interval_excludes_zero_rate"
                            ],
                            "not_no_material_change": cell["supplementary"][
                                "not_no_material_change_rate"
                            ],
                            "interval_beyond_threshold": cell["supplementary"][
                                "interval_beyond_threshold_rate"
                            ],
                        }
                        for cell in block.get("cells", [])
                    ],
                }
            if key == "coverage":
                # ADR-081's coverage row does not name a true effect. The strict
                # reading above takes the worst over every effect simulated; this
                # is the narrower reading, over the true null alone, published
                # beside it so the two cannot be confused.
                null_only = [
                    cell
                    for cell in block.get("cells", [])
                    if cell["true_effect"] == 0.0
                ]
                worst_null = (
                    min(null_only, key=lambda cell: cell["value"])
                    if null_only
                    else None
                )
                criteria_report[key]["worst_at_true_null_only"] = worst_null
                criteria_report[key]["verdict_at_true_null_only"] = verdict_for(
                    key, worst_null["value"] if worst_null else None
                )
        seconds = worst_family["seconds_per_comparison"] if worst_family else None
        rss = (
            max(family["process_peak_rss_mib"] for family in budget_entry["families"])
            if budget_entry["families"]
            else None
        )
        criteria_report["budget_seconds"] = {
            **CRITERIA["budget_seconds"],
            "worst_value": seconds,
            "verdict": verdict_for("budget_seconds", seconds),
            "note": (
                "the timed region is the comparison, not the fabrication of its "
                "inputs. The random-effects candidate is charged no bootstrap because "
                "it runs none; every candidate is charged the leave-one-execution-out "
                "jackknife, which only BCa reads, so its number is an upper bound by "
                "about 0.1% for the bootstrap candidates and by about 70 microseconds "
                "for the random-effects one. For the shipped estimator the receipt also "
                "carries `seconds_per_public_compare`, the public `compare` entry point "
                "on real BenchmarkDatasets, next to it."
            ),
        }
        criteria_report["budget_peak_rss_mib"] = {
            **CRITERIA["budget_peak_rss_mib"],
            "worst_value": rss,
            "verdict": verdict_for("budget_peak_rss_mib", rss),
            "process_floor_mib": process_floor_mib,
            "note": (
                "the whole test process, so it includes the harness itself and the "
                f"generated datasets; the same binary running a trivial test peaks at "
                f"{process_floor_mib} MiB, which is the floor to read this against"
            ),
        }
        failing = sorted(
            key
            for key, block in criteria_report.items()
            if block["verdict"] == "fail"
        )
        candidates_report.append(
            {
                **candidate,
                "criteria": criteria_report,
                "meets_every_criterion": not failing
                and all(
                    block["verdict"] == "pass" for block in criteria_report.values()
                ),
                "failing_criteria": failing,
                "budget": budget_entry["families"],
            }
        )

    passing = [
        entry["id"] for entry in candidates_report if entry["meets_every_criterion"]
    ]
    failing = [
        {"candidate": entry["id"], "failing_criteria": entry["failing_criteria"]}
        for entry in candidates_report
        if not entry["meets_every_criterion"]
    ]

    # A criterion no candidate reaches is what ADR-081 calls unreachable, and the
    # ADR says the correct response is to declare it so, not to move it.
    unreachable = []
    for key in CRITERIA:
        verdicts = {
            entry["criteria"][key]["verdict"] for entry in candidates_report
        }
        if verdicts and verdicts <= {"fail"}:
            best = None
            for entry in candidates_report:
                value = entry["criteria"][key]["worst_value"]
                if value is None:
                    continue
                if best is None or (
                    value > best[1]
                    if CRITERIA[key]["direction"] == "at_least"
                    else value < best[1]
                ):
                    best = (entry["id"], value)
            unreachable.append(
                {
                    "criterion": key,
                    "threshold": CRITERIA[key]["threshold"],
                    "direction": CRITERIA[key]["direction"],
                    "best_candidate": best[0] if best else None,
                    "best_value": best[1] if best else None,
                }
            )

    receipt = {
        "schema": "rust-engineering-mcp.m5-02-method-simulation.v1",
        "captured_at_utc": utc_now(),
        "captured_by": "scripts/simulate-m5-comparison-method.py",
        "decision": "docs/adr/ADR-081-benchmark-statistical-requalification.md",
        "method_under_test": "docs/adr/ADR-073-benchmark-method-and-dataset.md §4",
        "status": (
            "measurement only. This receipt judges candidates against criteria frozen "
            "before it existed; it changes nothing about the shipped method and takes "
            "no decision. Enabling or refusing directional verdicts is the owner's."
        ),
        "reproduce": (
            "python3 -B scripts/simulate-m5-comparison-method.py "
            f"--replicates {arguments.replicates} --threads {arguments.threads}"
        ),
        "seed": run["seed"],
        "seed_hex": run["seed_hex"],
        "seed_note": (
            "seed of the SIMULATION's data generator. The method's own "
            f"BOOTSTRAP_SEED is {run['method_constants']['bootstrap_seed_hex']} and is "
            "untouched: ADR-081 §4 forbids changing it or its derivation."
        ),
        "replicates_per_point": arguments.replicates,
        "meets_replicate_floor": arguments.replicates >= MIN_REPLICATES,
        "threads": arguments.threads,
        "thread_independence": (
            "each replicate's seed is a pure function of (executions, drift, true "
            "effect, replicate index), so the tallies do not depend on the thread "
            "count; `the_simulation_does_not_depend_on_the_thread_count` asserts it"
        ),
        "wall_seconds": round(elapsed, 1),
        "environment": {
            "host": platform.platform(),
            "machine": platform.machine(),
            "cpu_count": os.cpu_count(),
            "python": platform.python_version(),
            "rustc": subprocess.run(
                ["rustc", "--version"],
                cwd=ROOT,
                capture_output=True,
                text=True,
                check=False,
            ).stdout.strip(),
            "git_commit": subprocess.run(
                ["git", "rev-parse", "HEAD"],
                cwd=ROOT,
                capture_output=True,
                text=True,
                check=False,
            ).stdout.strip(),
        },
        "how_the_product_was_driven": {
            "harness": "crates/domain/src/benchmark_compare/simulation.rs",
            "kind": (
                "a #[cfg(test)] CHILD module of crates/domain/src/benchmark_compare.rs, "
                "so it can call that module's private items directly"
            ),
            "product_functions_called": [
                "decide (the frozen decision rule, for EVERY candidate)",
                "cluster_draw",
                "bootstrap_ratio (for the equivalence assertion)",
                "quantile_sorted",
                "median_sorted",
                "standard_deviation",
                "sorted_copy",
                "fewer_than_two_distinct_values",
                "inverse_standard_normal_cdf",
                "SplitMix64 and SplitMix64::for_benchmark (the method's own seed derivation)",
                "SideSamples",
                "ComparisonMethod::frozen",
                "compare (the public entry point, in the agreement test and the budget)",
            ],
            "what_is_not_the_product": (
                "the resampling LOOP of bootstrap_ratio is written out again because "
                "that function returns only the finished interval and the alternative "
                "estimators need the distribution behind it. Every primitive inside the "
                "loop is the product's. The duplication is held byte-for-byte equal by "
                "the_local_bootstrap_loop_reproduces_the_products_bootstrap_ratio, and "
                "the whole shipped path is held equal to the public compare by "
                "the_harness_agrees_with_the_public_compare_on_the_shipped_estimator."
            ),
            "production_behaviour_changed": (
                "none. The only edit to a shipped file is an eight-line "
                "`#[cfg(test)] mod simulation;` declaration in benchmark_compare.rs."
            ),
            "validation_tests": validation,
            "independent_cross_check": reproduction,
        },
        "drift_model": {
            **run["drift_model"],
            "is_an_assumption": True,
            "what_depends_on_it": [
                "the MAGNITUDE of every coverage, false-positive, power and "
                "no_material_change number in this receipt",
                "which drift point is the worst one for a given candidate",
            ],
            "what_does_not_depend_on_it": [
                "that a nonparametric cluster bootstrap over k clusters understates the "
                "standard error by sqrt(k/(k-1)) — 1.22x at k=3 — which is an algebraic "
                "property of the outer resampling stage, not of the noise distribution",
                "that percentile endpoints of a scale estimated from k clusters carry no "
                "t_{k-1} widening, so they are narrower than the level they publish",
                "that both errors point the SAME way: the published interval is more "
                "confident than 0.95 warrants, never less",
                "that with the frozen rule `low > +5%` a true effect of exactly +5% can "
                "be called `regression` only when the interval's lower endpoint exceeds "
                "the true value, which for ANY interval with coverage c has probability "
                "at most (1-c)/2 — 2.5% at the nominal level. The power row of §1 is "
                "therefore bounded by the calibration the coverage row demands, under "
                "any drift model whatsoever.",
            ],
            "calibrated_from": (
                "fixtures/benchmark-datasets/criterion-{baseline,candidate}-{1,2,3}.tar, "
                "the six real captures on the ADR-077 admitted image: 30 samples per "
                "execution, within-execution coefficient of variation 0.018-0.070 "
                "(median 0.033), between-execution range 6.1%-28.7% of the smaller "
                "median, which is the range ADR-081 §2 cites for its drift points"
            ),
        },
        "method_constants": run["method_constants"],
        "criteria_frozen_by_adr_081": CRITERIA,
        "drift_points": list(DRIFT_POINTS),
        "true_effects": list(TRUE_EFFECTS),
        "candidates": candidates_report,
        "candidates_meeting_every_criterion": passing,
        "candidates_failing": failing,
        "criteria_no_candidate_reaches": unreachable,
        "structural_findings": [
            {
                "finding": (
                    "the power row and the coverage row of ADR-081 §1 cannot both be "
                    "satisfied by ANY method that keeps the frozen decision rule"
                ),
                "why": (
                    "`regression` is emitted exactly when the interval's lower endpoint "
                    "exceeds +5%. At a TRUE effect of exactly +5% that event is 'the "
                    "interval lies entirely above the truth', a one-sided non-coverage "
                    "event, so power <= 1 - coverage at that point. Power >= 0.80 "
                    "requires coverage <= 0.20 there; the coverage row requires >= 0.93."
                ),
                "depends_on_the_drift_model": False,
                "depends_on_the_estimator": False,
                "checkable_in_this_receipt": (
                    "candidates[].criteria.power_at_material_threshold.structural_bound.per_point"
                ),
            },
            {
                "finding": (
                    "the cluster bootstrap's standard error is short by sqrt(k/(k-1)) "
                    "and the percentile endpoints carry no t_{k-1} widening"
                ),
                "why": (
                    "the outer stage draws k clusters with replacement from k, whose "
                    "variance has expectation ((k-1)/k) of the between-execution "
                    "variance. It is a property of the resampling stage, not of the "
                    "noise distribution."
                ),
                "depends_on_the_drift_model": False,
                "depends_on_the_estimator": True,
                "checkable_in_this_receipt": (
                    "compare the `percentile_kN` and `cluster_scaled_kN` coverage rows, "
                    "which differ ONLY by that factor"
                ),
            },
            {
                "finding": (
                    "raising the resample count cannot close the coverage gap"
                ),
                "why": (
                    "10 000 resamples already put the Monte-Carlo error of a percentile "
                    "far below the gap; the gap is a bias in the scale the bootstrap "
                    "estimates, and a bias does not shrink with more draws from the same "
                    "biased distribution."
                ),
                "depends_on_the_drift_model": False,
                "depends_on_the_estimator": True,
                "checkable_in_this_receipt": "sensitivity_probe_resample_count",
            },
            {
                "finding": (
                    "the MAGNITUDE of every coverage, false-positive and "
                    "no_material_change number here is a property of the drift model"
                ),
                "why": (
                    "the model is an assumption about how a host moves between "
                    "executions; another model gives another pair of numbers. What "
                    "survives a change of model is the ORDERING of the candidates and "
                    "the two structural facts above."
                ),
                "depends_on_the_drift_model": True,
                "depends_on_the_estimator": False,
                "checkable_in_this_receipt": "drift_model",
            },
        ],
        "sensitivity_probe_resample_count": probe,
        "not_covered_here": [
            "ADR-081 §3's real controls on guest captures: reproducible positives and "
            "negatives on the admitted image. This receipt is the simulation half only.",
            "ADR-081 §6's independent statistical review, which this receipt is an "
            "input to and not a substitute for.",
        ],
    }

    arguments.out.parent.mkdir(parents=True, exist_ok=True)
    arguments.out.write_text(json.dumps(receipt, indent=1) + "\n", encoding="utf-8")
    print(f"wrote {arguments.out}", file=sys.stderr)
    print(
        f"meets every criterion: {passing or 'none'}",
        file=sys.stderr,
    )
    for entry in failing:
        print(
            f"  {entry['candidate']}: fails {', '.join(entry['failing_criteria'])}",
            file=sys.stderr,
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
