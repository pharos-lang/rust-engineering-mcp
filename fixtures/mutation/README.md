# Mutation fixture corpus

These are isolated, dependency-free edition-2024 workspaces for the M3-05
`rust.mutation.test` vertical. Do not run them on the host; I05 must run each
fixture in the pinned guest and in a private mutation copy.

| Fixture | Oracle | Expected exit |
| --- | --- | ---: |
| `caught-all` | All 14 generated mutants are caught; the arithmetic and boolean tests assert exact values and the zero boundary. | 0 |
| `missed-one` | At least one viable mutant in `unchecked_value` is missed because its test only verifies that it does not panic; missed count is at least 1. | 2 |
| `timeout-loop` | At least one mutated `count_to` run does not terminate and is classified as a timeout. | 3 when the timeout outcome dominates |
| `unviable` | At least one generic mutation fails to compile because the replacement does not type-check for unconstrained `T`; unviable count is at least 1. | 0 unless another outcome dominates |
| `baseline-failing` | The baseline assertion fails before mutation; no mutant result is trustworthy. | 4 |
| `hostile-writer` | The test attempts out-of-root writes, a surviving child, loopback/public network access, and a large forged-output burst. I05 must prove containment and must not trust the forged lines. | Calibration-only |

The exit-code column is a hypothesis from the official cargo-mutants
documentation. I05 must record the observed codes and counts from the pinned
binary before treating this table as calibrated evidence. The generated
`mutants.out` contents and schema are also versioned by that binary, not by
these fixtures.

`canary.txt` is intentionally outside every individual fixture directory. The
hostile containment test must verify that its bytes and metadata are unchanged.
