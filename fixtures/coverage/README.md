# Coverage fixtures

These fixtures are hand-written and must only be executed through the approved
Docker gateway. Calibration against cargo-llvm-cov 0.9.0 in the approved image
pins `known-counts` to lines 4/4, regions 8/9 and functions 2/2. The fourth line
is the executed test body; line coverage is therefore 100% while one region in
the unselected `else` arm remains uncovered.

`shared-file-workspace` is pinned to three canonical files after lexical
normalization: `a/src/lib.rs`, `b/src/lib.rs` and one `shared.rs`, with aggregate
lines 0/3, regions 0/9 and functions 0/3. The two LLVM spellings containing
`a/src/../../` and `b/src/../../` must never double-count that shared file.
`zero-denominator` deliberately contains no instrumentable function; the pinned
plugin refuses report generation with `no coverage data found`. The gateway must
retain that bounded incomplete evidence and expose no fabricated percentage.
The domain constructor separately pins a parsed `(0, 0)` denominator to `None`.

`no-tests` remains an otherwise-valid crate with uncovered instrumentable code.
`containment` is the ADR-065 negative control: source bytes never acquire execute
permission, the report volume remains `noexec`, and IPv4 socket creation remains
denied in the coverage run phase. `slow-build` supplies a deterministic active
child for timeout and cancellation; the runtime cases require joined cleanup
before returning.
