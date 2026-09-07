# Package R01 — Independent Research: Machine-Readable Formats, Versions, and Exit Codes of Four Cargo Plugins

**Researcher:** Gemini 3.8 Flash (High)  
**Target Runtime:** Linux ARM64 guest (`aarch64-unknown-linux-gnu`), Rust 1.98.1 installed at `/opt/rust` (system install without `rustup`).  
**Constraint Compliance:** Read-only mode executed strictly via file viewing tools. Zero terminal/shell commands executed. Offline run relying entirely on orchestrator-fetched data in `sources/`. Every claim supported by local artifacts cites `sources/<file>`; all other assertions are explicitly designated as *unverified (from training data)*.

---

## 1. `cargo-nextest`

### A. Version, Release Assets, Checksums & License
* **Latest Stable Version & Date:** `0.9.143` (`cargo-nextest 0.9.143`, tag `cargo-nextest-0.9.143`), released on `2026-08-04T22:25:46Z` [`sources/releases-summary.json`].
* **Linux aarch64 Prebuilt Binaries:** Official prebuilt binaries exist for both GNU and musl:
  * **GNU (`glibc`):** `cargo-nextest-0.9.143-aarch64-unknown-linux-gnu.tar.gz` (size: 11,243,965 bytes; SHA-256: `2a64b3566a92508550a7ab29c3e8db25472ca37730ecb4d22100b6aa440c2a68`) [`sources/releases-summary.json`]. Minimum glibc requirement: 2.27 (Ubuntu 18.04) [`sources/nextest-prebuilt.txt`].
  * **musl (statically linked):** `cargo-nextest-0.9.143-aarch64-unknown-linux-musl.tar.gz` (size: 8,878,811 bytes; SHA-256: `0560ce0ce017f368c54b5db86588370fc03b470985490fa81200e62a657dda05`) [`sources/releases-summary.json`, `sources/nextest-prebuilt.txt`].
* **Checksum Files / Attestation:**
  * Separate SHA-256 checksum files are published alongside: `cargo-nextest-0.9.143-aarch64-unknown-linux-gnu.sha256` and `cargo-nextest-0.9.143-aarch64-unknown-linux-musl.sha256` [`sources/releases-summary.json`].
  * BLAKE2b digest files are also published alongside (`.b2`) [`sources/releases-summary.json`].
  * Code signing policy: Windows binaries are digitally signed via SignPath.io [`sources/nextest-prebuilt.txt`].
* **License (SPDX):** `Apache-2.0 OR MIT` *[unverified (from training data)]*.

### B. Runtime Prerequisites Inside Guest
* Requires only standard `cargo` / `rustc` and operating system C runtime (or no runtime dependencies if using the static `musl` binary) [`sources/nextest-prebuilt.txt`].
* Does not require `rustup` or `llvm-tools` [`sources/nextest-prebuilt.txt`].

### C. Machine-Readable Outputs: Flags & Formats
* **JUnit XML Output:**
  * Enabled via repository configuration file `.config/nextest.toml` under `[profile.<name>.junit]` [`sources/nextest-junit.txt`, `sources/nextest-config.txt`]:
    * `path = "junit.xml"`: Destination path relative to `target/nextest/<profile>/` [`sources/nextest-junit.txt`].
    * `report-name`: Name of report (default `"nextest-run"`) [`sources/nextest-junit.txt`].
    * `store-success-output = false`: (default `false`) Whether to store `<system-out>`/`<system-err>` for passing tests [`sources/nextest-junit.txt`].
    * `store-failure-output = true`: (default `true`) Whether to store output for failing tests [`sources/nextest-junit.txt`].
    * `report-skipped = "none" | "ignored" | "all"`: (default `"none"`) Controls emission of `<skipped>` elements [`sources/nextest-junit.txt`].
    * `flaky-fail-status = "failure" | "success"`: (default `"failure"`) Controls whether flaky-fail tests appear as `<failure>` or success [`sources/nextest-junit.txt`].
  * Adheres to Jenkins JUnit XML standard: `<testsuites>` root, each test binary forms `<testsuite>`, each test forms `<testcase>` [`sources/nextest-junit.txt`].
* **Libtest JSON Output:**
  * Flags: `--message-format libtest-json` or `--message-format libtest-json-plus` [`sources/nextest-libtest-json.txt`].
  * **Prerequisite Flag/Env:** **Must** set `NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1` in the environment [`sources/nextest-libtest-json.txt`].
  * Version option: `--message-format-version 0.1` [`sources/nextest-libtest-json.txt`].
  * `libtest-json-plus` appends an extra `nextest` field to the unstable libtest JSON output [`sources/nextest-libtest-json.txt`].
* **Execution Flags & Configuration Keys:**
  * `--profile <name>` (or `-P <name>`): Selects profile from `.config/nextest.toml` [`sources/nextest-config.txt`].
  * `--no-fail-fast` / `fail-fast = false`: Disables cancelling the run on first failure [`sources/nextest-config.txt`, `sources/llvmcov-readme.txt`].
  * **Retries:** `retries = <count>` or `retries = { backoff = "fixed" | "exponential", count = <N>, delay = "<dur>", max-delay = "<dur>", jitter = true }` [`sources/nextest-retries.txt`]. CLI: `--retries <N>`, env: `NEXTEST_RETRIES` [`sources/nextest-retries.txt`].
  * **Flaky Results:** `flaky-result = "pass" | "fail"`. CLI: `--flaky-result <pass|fail>`, env: `NEXTEST_FLAKY_RESULT` [`sources/nextest-retries.txt`].
  * **Timeouts:** `slow-timeout = "<dur>"` or `slow-timeout = { period = "<dur>", terminate-after = <N>, on-timeout = "fail" | "pass", grace-period = "<dur>" }` [`sources/nextest-timeouts.txt`]. Global: `global-timeout = "<dur>"` [`sources/nextest-timeouts.txt`].
  * **Leaks:** `leak-timeout = "<dur>"` (default 100ms) or `leak-timeout = { period = "<dur>", result = "fail" }` [`sources/nextest-leaky.txt`].
* **Representation in JUnit Properties:**
  * Flaky tests (passed on retry) contain `<flakyFailure>` or `<flakyError>` elements [`sources/nextest-junit.txt`, `sources/nextest-retries.txt`].
  * Rerun tests that continued to fail contain `<rerunFailure>` or `<rerunError>` elements [`sources/nextest-junit.txt`, `sources/nextest-retries.txt`].
  * Tests configured with `flaky-result = "fail"` contain both `<failure>` and `<flakyFailure>`/`<flakyError>` elements by default [`sources/nextest-junit.txt`, `sources/nextest-retries.txt`].
  * Leaky tests marked failing output `(test failed: exited with code 0, but leaked handles)` in failure message [`sources/nextest-leaky.txt`].

### D. Exit Codes
* **Documented Upstream Page:** `https://nexte.st/docs/machine-readable/exit-codes/` returned **HTTP 404** when fetched by the orchestrator [`sources/index.txt`, `sources/nextest-exit-codes.txt`, `sources/nextest-exit-codes.html`].
* **Exit Code Definitions:**
  * `0`: Success — all tests passed (flaky tests are treated as successful by default unless `flaky-result = "fail"`) [`sources/nextest-retries.txt`].
  * `100`: Test run failure (one or more tests failed, or flaky tests failed when `flaky-result = "fail"`, or leaky tests failed when `leak-timeout.result = "fail"`) *[unverified (from training data)]*.
  * `101`: Test runner execution failure (Cargo build failure, CLI parsing error, invalid config) *[unverified (from training data)]*.
  * `104`: Test run cancelled or timed out (global timeout reached, Ctrl-C / signal interruption) *[unverified (from training data)]*.

### E. Sandboxed / Offline Pitfalls
* **Writes Outside Target Directory:** Nextest writes artifacts strictly to `target/nextest/<profile>/` (such as `junit.xml`) [`sources/nextest-junit.txt`]. It does not write to the source directory unless user config in `~/.config/nextest/` explicitly directs it to do so [`sources/nextest-config.txt`].
* **Network Attempts:** None. Nextest executes pre-built test binaries without querying registries or external networks [`sources/nextest-prebuilt.txt`].
* **Process / PID Management:**
  * Creates a dedicated Unix process group for each test process [`sources/nextest-timeouts.txt`].
  * On timeout or cancellation, sends `SIGTERM` to the process group, waits `grace-period` (default 10 seconds), and sends `SIGKILL` (`kill -9`) if processes linger [`sources/nextest-timeouts.txt`].
  * Containers must permit process group signaling and process killing.

---

## 2. `cargo-llvm-cov`

### A. Version, Release Assets, Checksums & License
* **Latest Stable Version & Date:** `0.9.0` (tag `v0.9.0`), released on `2026-08-16T20:19:51Z` [`sources/releases-summary.json`].
* **Linux aarch64 Prebuilt Binaries:** Official prebuilt binaries exist for both GNU and musl:
  * **GNU:** `cargo-llvm-cov-aarch64-unknown-linux-gnu.tar.gz` (size: 1,613,151 bytes; SHA-256: `9af53b273e50d01d8bde8785de8541f6738cc4375248cd7683aec8b5768b9d21`) [`sources/releases-summary.json`].
  * **musl (static executable):** `cargo-llvm-cov-aarch64-unknown-linux-musl.tar.gz` (size: 1,640,975 bytes; SHA-256: `3c299780e109d59fd77044e64734421d43c0067b92937a49be302fee66d04727`) [`sources/releases-summary.json`, `sources/llvmcov-readme.txt`].
* **Checksum Files / Attestation:**
  * Release assets are immutable and publish GitHub **Artifact Attestations** (SLSA provenance v1 verifiable with `gh attestation verify`) since version 0.8.5 [`sources/llvmcov-readme.txt`].
  * Separate `.sha256` files are not published in release assets; digests are fetched via GitHub Release API [`sources/releases-summary.json`].
* **License (SPDX):** `Apache-2.0 OR MIT` [`sources/llvmcov-readme.txt`].

### B. Runtime Prerequisites Inside Guest
* Requires LLVM coverage tools `llvm-profdata` and `llvm-cov` compatible with the rustc LLVM version (LLVM 19–22 for Rust 1.82–1.98) [`sources/llvmcov-readme.txt`].
* **No-rustup environment constraint:** Rustup would normally install `llvm-tools-preview`. In this `/opt/rust` non-rustup installation, the official component `llvm-tools-1.98.1-aarch64-unknown-linux-gnu.tar.xz` (official SHA-256: `caaf950c65f3e428247dbe9c173d142b7072b2134962a61924c01e39f6b6dc1e` [`sources/rust-llvm-tools-sha.txt`]) must be extracted into `/opt/rust` or placed on PATH, or the environment variables `LLVM_COV` and `LLVM_PROFDATA` must explicitly point to those binaries [`sources/llvmcov-readme.txt`].
* `CARGO_LLVM_COV_SETUP`: Controls automatic setup behavior if components are missing [`sources/llvmcov-readme.txt`].

### C. Machine-Readable Outputs: Flags & Formats
* **Export Formats:**
  * `--json`: Export in LLVM JSON format (calls `llvm-cov export -format=text`). If `--output-path` is omitted, prints to stdout [`sources/llvmcov-readme.txt`]. Injects a root metadata object:
    ```json
    {
      "cargo_llvm_cov": {
        "version": "<cargo-llvm-cov-version>",
        "manifest_path": "/absolute/path/to/Cargo.toml"
      }
    }
    ```
    [`sources/llvmcov-readme.txt`].
  * `--summary-only`: Export only file summary information in coverage data (works with `--json`, `--lcov`, `--cobertura`) [`sources/llvmcov-readme.txt`].
  * `--lcov`: Export in LCOV info format [`sources/llvmcov-readme.txt`].
  * `--cobertura`: Export in Cobertura XML format [`sources/llvmcov-readme.txt`].
  * `--codecov`: Export in Codecov Custom Coverage JSON format [`sources/llvmcov-readme.txt`].
  * `--html`: Generates HTML report in `--output-dir` (default `target/llvm-cov/html`) [`sources/llvmcov-readme.txt`].
  * `--text`: Generates text coverage report [`sources/llvmcov-readme.txt`].
* **File & Scope Flags:**
  * `--output-path <PATH>`: Destination file for `--json`, `--lcov`, `--cobertura`, or `--text` [`sources/llvmcov-readme.txt`].
  * `--output-dir <DIR>`: Destination directory for `--html` or `--text` (default `target/llvm-cov`) [`sources/llvmcov-readme.txt`].
  * `--workspace`: Cover all packages in workspace [`sources/llvmcov-readme.txt`].
  * `--ignore-filename-regex <PATTERN>`: Regular expression to exclude files from report [`sources/llvmcov-readme.txt`].
* **Two-Phase Generation Flow:**
  * Phase 1 (Test run without report generation): `cargo llvm-cov --no-report [flags]` runs tests and writes raw `.profraw` instrumentation artifacts [`sources/llvmcov-readme.txt`].
  * Phase 2 (Generate one or more report formats without re-running tests): `cargo llvm-cov report --json --output-path cov.json`, `cargo llvm-cov report --lcov --output-path lcov.info`, etc. [`sources/llvmcov-readme.txt`].
* **Doctests Status:**
  * Unstable / experimental. Doc tests are disabled by default because nightly-only rustc features are required [`sources/llvmcov-readme.txt`]. `--doctests` requires nightly Rust compiler [`sources/llvmcov-readme.txt`].

### D. Exit Codes
* **Pass-Through of Test Status:** Default behavior passes through the exact exit code of the test runner (`cargo test` or `cargo nextest`) [`sources/llvmcov-readme.txt`].
* **Report-Only Success:** `--ignore-run-fail` causes `cargo-llvm-cov` to exit with status `0` if tests failed but report generation succeeded [`sources/llvmcov-readme.txt`].
* **Threshold Failures (Status `1`):** Exits with status `1` if coverage thresholds are violated:
  * `--fail-under-functions <MIN>`, `--fail-under-lines <MIN>`, `--fail-under-file-lines <MIN>`, `--fail-under-regions <MIN>` [`sources/llvmcov-readme.txt`].
  * `--fail-uncovered-lines <MAX>`, `--fail-uncovered-regions <MAX>`, `--fail-uncovered-functions <MAX>` [`sources/llvmcov-readme.txt`].

### E. Sandboxed / Offline Pitfalls
* **Build Artifact Location:** By default, places intermediate build artifacts in `<cargo_target_dir>/llvm-cov-target` (overridden via `CARGO_LLVM_COV_TARGET_DIR` or `CARGO_LLVM_COV_BUILD_DIR`) [`sources/llvmcov-readme.txt`].
* **Network Access:** Passes flags through to Cargo; respects `--offline`, `--frozen`, `--locked` [`sources/llvmcov-readme.txt`].
* **Clean Behavior:** By default cleans old build artifacts unless `--no-clean` or `--no-report` is passed [`sources/llvmcov-readme.txt`].
* **Environment Variables:** `LLVM_COV`, `LLVM_PROFDATA`, `CARGO_LLVM_COV_TARGET_DIR`, `CARGO_LLVM_COV_BUILD_DIR`, `CARGO_LLVM_COV_SETUP`, `LLVM_PROFILE_FILE_NAME`, `RUSTC_WRAPPER` [`sources/llvmcov-readme.txt`].

---

## 3. `cargo-semver-checks`

### A. Version, Release Assets, Checksums & License
* **Latest Stable Version & Date:** `v0.50.0` (tag `v0.50.0`), released on `2026-08-01T17:02:28Z` [`sources/releases-summary.json`].
* **Linux aarch64 Prebuilt Binaries:** Official prebuilt binaries exist for both GNU and musl:
  * **GNU:** `cargo-semver-checks-aarch64-unknown-linux-gnu.tar.gz` (size: 7,866,573 bytes; SHA-256: `e35f435ea322659381f52e7034bb4f0470108f5b267d29f13cf08152fa4af29b`) [`sources/releases-summary.json`].
  * **musl (static executable):** `cargo-semver-checks-aarch64-unknown-linux-musl.tar.gz` (size: 7,909,759 bytes; SHA-256: `3c44160c3fdd93f72d2f5c84774c370a63e09d0d17ae0cdcc3dde346f30a1348`) [`sources/releases-summary.json`].
* **Checksum Files / Attestation:**
  * No separate `.sha256` or attestation files in release assets; digests verified via GitHub Release API [`sources/releases-summary.json`].
* **License (SPDX):** `Apache-2.0 OR MIT` [`sources/semver-readme.txt`].

### B. Runtime Prerequisites Inside Guest
* **Rustdoc JSON / Nightly Compatibility:**
  * Uses `rustdoc` JSON output to inspect crate APIs [`sources/semver-readme.txt`].
  * While rustdoc JSON format is unstable internally, each stable release of `cargo-semver-checks` **explicitly supports the then-current stable and beta Rust releases** [`sources/semver-readme.txt`].
  * **Works on stable Rust 1.98.1 without nightly** [`sources/semver-readme.txt`]. (Nightly Rust is supported only on a best-effort basis and frequently breaks when internal rustdoc formats change [`sources/semver-readme.txt`]).
* If compiled from source, requires `cmake` for `libz-ng-sys`; prebuilt binaries bypass this prerequisite [`sources/semver-readme.txt`].

### C. Machine-Readable Outputs: Flags & Formats
* **Baseline Selection Flags:**
  * `--baseline-root <MANIFEST_ROOT>`: Local directory containing the baseline crate source manifest [`sources/semver-readme.txt`].
  * `--baseline-rustdoc <JSON_PATH>`: Direct path to pre-generated rustdoc JSON file [`sources/semver-readme.txt`].
  * `--baseline-rev <REV>`: Git revision to extract baseline from [`sources/semver-readme.txt`].
  * `--baseline-version <X.Y.Z>`: Version to fetch from registry (crates.io) [`sources/semver-readme.txt`].
* **Scope & Feature Flags:**
  * `-p, --package <SPEC>`: Package to verify [`sources/semver-readme.txt`].
  * `--target <TRIPLE>`: Target triple to compile rustdoc for (e.g. `aarch64-unknown-linux-gnu`) [`sources/semver-readme.txt`].
  * Feature configuration: `--all-features`, `--default-features`, `--only-explicit-features`, `--features <LIST>`, `--baseline-features <LIST>`, `--current-features <LIST>` [`sources/semver-readme.txt`].
* **Machine-Readable Format (`--output-format json`):**
  * Flag: `--output-format json` *[unverified (from training data)]*. Emits structured JSON reporting crate name, baseline version, current version, lints evaluated, list of semver violations with file and line numbers, and recommended semver version bump level (`major`, `minor`, `patch`).
* **Behavior When Baseline Lacks a Library Target:**
  * SemVer checks only apply to library targets (`lib`). When the baseline lacks a `lib` target (e.g. binary-only crate), `cargo-semver-checks` prints a message/warning indicating no library target was found to check and exits with `0` (success / skipped) *[unverified (from training data)]*.

### D. Exit Codes
* Documented exit code specification [`sources/semver-readme.txt`]:
  * `0`: Check completed without deny-level SemVer violations [`sources/semver-readme.txt`].
  * `100`: Check completed and found one or more deny-level SemVer violations [`sources/semver-readme.txt`].
  * `101`: Check could not complete (rustdoc build failure, connectivity problem, or invalid baseline) [`sources/semver-readme.txt`].
  * Other non-zero: Command-line parsing / usage errors [`sources/semver-readme.txt`].

### E. Sandboxed / Offline Pitfalls
* **CRITICAL Network Attempt:** By default, `cargo-semver-checks` queries `crates.io` to lookup and download the baseline version [`sources/semver-readme.txt`]. In an offline / air-gapped sandbox, this **will fail with exit code 101**. You **must** supply `--baseline-root <PATH>` or `--baseline-rustdoc <PATH>` to prevent all registry network calls [`sources/semver-readme.txt`].
* **Git Worktree Search:** `--baseline-rev` walks up filesystem looking for `.git/`. Can be constrained via `GIT_DIR`, `GIT_CEILING_DIRECTORIES`, and `GIT_DISCOVERY_ACROSS_FILESYSTEM` [`sources/semver-readme.txt`].
* **Environment Variables:** `RUSTDOCFLAGS` (can pass `--cfg` options for conditional compilation) [`sources/semver-readme.txt`].

---

## 4. `cargo-mutants`

### A. Version, Release Assets, Checksums & License
* **Latest Stable Version & Date:** `v27.1.0` (tag `v27.1.0`), released on `2026-06-02T15:17:34Z` [`sources/releases-summary.json`].
* **Linux aarch64 Prebuilt Binaries:** **NONE**.
  * Confirmed from `sources/releases-summary.json`: `assets_filtered` is empty `[]`, and all 3 published release assets are for `x86_64` only [`sources/releases-summary.json`].
  * **Consequence:** Prebuilt binaries do not exist for `aarch64-unknown-linux-gnu`. A **source build (`cargo install cargo-mutants --locked`) is strictly required** within the guest environment before execution.
* **Checksum Files / Attestation:** N/A for aarch64 (no assets published) [`sources/releases-summary.json`].
* **License (SPDX):** `MIT` *[unverified (from training data)]*.

### B. Runtime Prerequisites Inside Guest
* Requires a Rust toolchain with Cargo.
* Requires a **writable workspace or temporary directory copy** [`sources/mutants-inplace.txt`, `sources/mutants-baseline.txt`].
* By default, copies the entire source tree to a temporary directory (under `std::env::temp_dir()`, e.g. `/tmp/cargo-mutants-*` *[unverified (from training data)]*) before applying mutations [`sources/mutants-inplace.txt`].
* If `--in-place` is specified, mutates the source checkout in place [`sources/mutants-inplace.txt`].

### C. Machine-Readable Outputs: Flags & Formats
* **`mutants.out/` Directory Layout:**
  * Created in the original source directory by default; overridden via `--output <DIR>`, `CARGO_MUTANTS_OUTPUT`, or `output` configuration key [`sources/mutants-out.txt`].
  * Rotates previous output directory to `mutants.out.old/` and deletes older `mutants.out.old/` [`sources/mutants-out.txt`].
  * Directory contents:
    * `lock.json`: File held with `fs2` file lock during execution (contains start time, version, username, hostname) [`sources/mutants-out.txt`].
    * `mutants.json`: Complete list of all discovered and generated mutants, written before testing starts [`sources/mutants-out.txt`].
    * `outcomes.json`: Primary machine-readable file describing all test outcomes, summary counts, and cargo-mutants version [`sources/mutants-out.txt`].
    * `diff/`: Directory containing a diff file for each mutation relative to unmutated source [`sources/mutants-out.txt`].
    * `logs/`: Directory with build/test logs for each mutant plus the baseline [`sources/mutants-out.txt`].
    * `caught.txt`: List of mutants caught by tests [`sources/mutants-out.txt`].
    * `missed.txt`: List of mutants not caught by any test [`sources/mutants-out.txt`].
    * `timeout.txt`: List of mutants that caused test timeouts [`sources/mutants-out.txt`].
    * `unviable.txt`: List of mutants that failed to compile [`sources/mutants-out.txt`].
    * `previously_caught.txt`: Cumulative list of caught mutants when running with `--iterate` [`sources/mutants-out.txt`].
* **Execution & Control Flags:**
  * `--in-place`: Mutates and tests code directly in the source directory rather than copying to a temporary directory [`sources/mutants-inplace.txt`].
    * Incompatible with `--jobs` (`-j > 1`) [`sources/mutants-inplace.txt`].
    * Inserts `/* ~ changed by cargo-mutants ~ */` comment marker into code [`sources/mutants-inplace.txt`].
  * `--output <DIR>`: Directs `mutants.out` output files to a specific location [`sources/mutants-out.txt`].
  * `--timeout <SECS>`: Explicit test timeout in seconds [`sources/mutants-timeouts.txt`].
  * `--build-timeout <SECS>`: Explicit compilation timeout to prevent infinite const evaluation hangs [`sources/mutants-timeouts.txt`].
  * `--jobs <N>` / `-j <N>`: Parallel test jobs (requires tree copies; incompatible with `--in-place`) [`sources/mutants-inplace.txt`].
  * `--shard <K/N>`: Partition mutants across CI jobs *[unverified (from training data)]*.
  * `--baseline skip`: Skips unmutated baseline build/test run [`sources/mutants-baseline.txt`]. Multiplier timeouts cannot be used with skip; timeout defaults to 300s if `--timeout` is not specified [`sources/mutants-timeouts.txt`].
  * `--list --json`: Lists mutants in JSON format without testing *[unverified (from training data)]*.
  * `--test-tool nextest`: Runs tests using `cargo-nextest` instead of `cargo test` [`sources/mutants-nextest.txt`]. Can pass arguments via `--cargo-arg=--profile=mutants` [`sources/mutants-nextest.txt`]. Note: `nextest` does not run doctests, so behaviors only tested in doctests will be reported as missed [`sources/mutants-nextest.txt`].

### D. Exit Codes
* Documented exit code specification [`sources/mutants-exit.txt`]:
  * `0`: Success! Every viable mutant that was tested was caught by a test [`sources/mutants-exit.txt`].
  * `1`: Usage error (bad command-line arguments, etc.) [`sources/mutants-exit.txt`].
  * `2`: Found some mutants that were not covered by tests [`sources/mutants-exit.txt`].
  * `3`: Some tests timed out (infinite loop or timeout too low) [`sources/mutants-exit.txt`].
  * `4`: Baseline tests are already failing or hanging before any mutations are applied [`sources/mutants-exit.txt`].
  * `5`: The new side of `--in-diff` does not match text in tree [`sources/mutants-exit.txt`].
  * `6`: `--in-diff` diff is not a valid diff [`sources/mutants-exit.txt`].
  * `70`: Internal error occurred [`sources/mutants-exit.txt`].

### E. Sandboxed / Offline Pitfalls
* **Writes Outside Target Directory:**
  * By default creates `mutants.out/` and `mutants.out.old/` directly in the source directory [`sources/mutants-out.txt`]. In a read-only root, this will fail unless `--output /path/to/writable/dir` is specified.
  * Default mode copies the workspace to `/tmp/cargo-mutants-*`. In constrained containers, `/tmp` tmpfs can easily run out of space.
  * With `--in-place`, modifies source files directly in the working tree [`sources/mutants-inplace.txt`].
* **Network Attempts:** Does not make network calls directly, but sub-invokes `cargo build`/`cargo test`. Must pass `--cargo-arg=--offline` if dependencies are vendored or cached offline.
* **CPU / Memory Usage:** Running `--jobs N` spawns multiple full rustc builds simultaneously, multiplying memory consumption.

---

## Comparative Matrix

| Tool | Latest Version & Date | Linux aarch64 Prebuilt Asset | License (SPDX) | Runtime Prerequisites | Machine-Readable Formats | Exit Codes |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **`cargo-nextest`** | `0.9.143`<br>(2026-08-04) [`sources/releases-summary.json`] | `cargo-nextest-0.9.143-aarch64-unknown-linux-gnu.tar.gz`<br>`...-musl.tar.gz` [`sources/releases-summary.json`] | `Apache-2.0 OR MIT` *[unverified (training)]* | Cargo & libc (or none for static musl) [`sources/nextest-prebuilt.txt`] | JUnit XML (`junit.xml`), Libtest JSON (`libtest-json`, `libtest-json-plus`) [`sources/nextest-junit.txt`, `sources/nextest-libtest-json.txt`] | `0` (pass), `100` (test fail), `101` (runner fail), `104` (timeout/cancel) *[unverified (training)]* |
| **`cargo-llvm-cov`** | `0.9.0`<br>(2026-08-16) [`sources/releases-summary.json`] | `cargo-llvm-cov-aarch64-unknown-linux-gnu.tar.gz`<br>`...-musl.tar.gz` [`sources/releases-summary.json`] | `Apache-2.0 OR MIT` [`sources/llvmcov-readme.txt`] | LLVM tools (`llvm-profdata`, `llvm-cov`) via `/opt/rust` tarball or env vars [`sources/llvmcov-readme.txt`, `sources/rust-llvm-tools-sha.txt`] | `--json`, `--summary-only`, `--lcov`, `--cobertura`, `--codecov`, `--html`, `--text` [`sources/llvmcov-readme.txt`] | Pass-through from tests; `0` with `--ignore-run-fail`; `1` on threshold failure [`sources/llvmcov-readme.txt`] |
| **`cargo-semver-checks`** | `v0.50.0`<br>(2026-08-01) [`sources/releases-summary.json`] | `cargo-semver-checks-aarch64-unknown-linux-gnu.tar.gz`<br>`...-musl.tar.gz` [`sources/releases-summary.json`] | `Apache-2.0 OR MIT` [`sources/semver-readme.txt`] | Rustdoc (works on stable 1.98.1 without nightly) [`sources/semver-readme.txt`] | `--output-format json` *[unverified (training)]*; `--baseline-rustdoc` JSON input [`sources/semver-readme.txt`] | `0` (pass), `100` (SemVer violations), `101` (execution / network fail) [`sources/semver-readme.txt`] |
| **`cargo-mutants`** | `v27.1.0`<br>(2026-06-02) [`sources/releases-summary.json`] | **NONE** (x86_64 only; source build required) [`sources/releases-summary.json`] | `MIT` *[unverified (training)]* | Cargo, writable workspace copy or `--in-place` [`sources/mutants-inplace.txt`] | `mutants.out/` (`outcomes.json`, `mutants.json`, `diff/`, `logs/`, `*.txt`) [`sources/mutants-out.txt`]; `--list --json` *[unverified (training)]* | `0` (clean), `1` (args), `2` (missed), `3` (timeout), `4` (baseline fail), `5`/`6` (diff), `70` (internal) [`sources/mutants-exit.txt`] |

---

## Sources List

All source artifacts read from `sources/index.txt` (recorded 2026-09-05 UTC):

1. **`nextest-machine-readable`** [`sources/nextest-machine-readable.txt`]:  
   URL: `https://nexte.st/docs/machine-readable/` (HTTP 200)
2. **`nextest-junit`** [`sources/nextest-junit.txt`]:  
   URL: `https://nexte.st/docs/machine-readable/junit/` (HTTP 200)
3. **`nextest-libtest-json`** [`sources/nextest-libtest-json.txt`]:  
   URL: `https://nexte.st/docs/machine-readable/libtest-json/` (HTTP 200)
4. **`nextest-exit-codes`** [`sources/nextest-exit-codes.txt`, `sources/nextest-exit-codes.html`]:  
   URL: `https://nexte.st/docs/machine-readable/exit-codes/` (**HTTP 404**)
5. **`nextest-prebuilt`** [`sources/nextest-prebuilt.txt`]:  
   URL: `https://nexte.st/docs/installation/pre-built-binaries/` (HTTP 200)
6. **`nextest-config`** [`sources/nextest-config.txt`]:  
   URL: `https://nexte.st/docs/configuration/` (HTTP 200)
7. **`nextest-retries`** [`sources/nextest-retries.txt`]:  
   URL: `https://nexte.st/docs/features/retries/` (HTTP 200)
8. **`nextest-leaky`** [`sources/nextest-leaky.txt`]:  
   URL: `https://nexte.st/docs/features/leaky-tests/` (HTTP 200)
9. **`nextest-timeouts`** [`sources/nextest-timeouts.txt`]:  
   URL: `https://nexte.st/docs/features/slow-tests/` (HTTP 200)
10. **`nextest-releases-api`** [`sources/releases-summary.json`]:  
    URL: `https://api.github.com/repos/nextest-rs/nextest/releases/latest` (HTTP 200)
11. **`llvmcov-readme`** [`sources/llvmcov-readme.txt`]:  
    URL: `https://raw.githubusercontent.com/taiki-e/cargo-llvm-cov/main/README.md` (HTTP 200)
12. **`llvmcov-releases-api`** [`sources/releases-summary.json`]:  
    URL: `https://api.github.com/repos/taiki-e/cargo-llvm-cov/releases/latest` (HTTP 200)
13. **`semver-readme`** [`sources/semver-readme.txt`]:  
    URL: `https://raw.githubusercontent.com/obi1kenobi/cargo-semver-checks/main/README.md` (HTTP 200)
14. **`semver-releases-api`** [`sources/releases-summary.json`]:  
    URL: `https://api.github.com/repos/obi1kenobi/cargo-semver-checks/releases/latest` (HTTP 200)
15. **`mutants-out`** [`sources/mutants-out.txt`]:  
    URL: `https://mutants.rs/mutants-out.html` (HTTP 200)
16. **`mutants-exit`** [`sources/mutants-exit.txt`]:  
    URL: `https://mutants.rs/exit-codes.html` (HTTP 200)
17. **`mutants-timeouts`** [`sources/mutants-timeouts.txt`]:  
    URL: `https://mutants.rs/timeouts.html` (HTTP 200)
18. **`mutants-inplace`** [`sources/mutants-inplace.txt`]:  
    URL: `https://mutants.rs/in-place.html` (HTTP 200)
19. **`mutants-baseline`** [`sources/mutants-baseline.txt`]:  
    URL: `https://mutants.rs/baseline.html` (HTTP 200)
20. **`mutants-nextest`** [`sources/mutants-nextest.txt`]:  
    URL: `https://mutants.rs/nextest.html` (HTTP 200)
21. **`mutants-releases-api`** [`sources/releases-summary.json`]:  
    URL: `https://api.github.com/repos/sourcefrog/cargo-mutants/releases/latest` (HTTP 200)
22. **`rust-llvm-tools-sha`** [`sources/rust-llvm-tools-sha.txt`]:  
    URL: `https://static.rust-lang.org/dist/llvm-tools-1.98.1-aarch64-unknown-linux-gnu.tar.xz.sha256` (HTTP 200)
23. **`rust-manifest`** [`sources/rust-manifest.txt`]:  
    URL: `https://static.rust-lang.org/dist/channel-rust-1.98.1.toml.sha256` (HTTP 200)

---

## Uncertainties

1. **`cargo-nextest` Exit Codes:** Upstream URL `https://nexte.st/docs/machine-readable/exit-codes/` returned HTTP 404 in the offline snapshot [`sources/index.txt`, `sources/nextest-exit-codes.txt`]. While exit code `0` is verified in `sources/nextest-retries.txt`, codes `100`, `101`, and `104` are derived from *training data (unverified)*.
2. **`cargo-semver-checks` JSON Schema & Lib Target Output:** The schema details for `--output-format json` and the exact CLI output string when a crate lacks a `[lib]` target are not documented in `sources/semver-readme.txt` and are labelled *unverified (from training data)*.
3. **`cargo-mutants` Sharding & JSON Listing Flags:** While `mutants-baseline.txt` mentions CI sharding, the exact CLI options `--shard <K/N>` and `--list --json` are not documented in the provided markdown pages and are labelled *unverified (from training data)*.
4. **SPDX Licenses for `cargo-nextest` and `cargo-mutants`:** Explicit license headings are omitted from their respective documentation subsets in `sources/` and are confirmed from *training data (unverified)* (`Apache-2.0 OR MIT` for nextest; `MIT` for mutants).
