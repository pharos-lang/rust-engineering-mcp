# nextest fixture corpus

Each crate is an isolated edition-2024 workspace with no dependencies. These
fixtures are inputs for the nextest adapter and Docker integration tests; do not
run them on the host. Counts are the expected default-profile oracle.

| Fixture | Expected outcome |
| --- | --- |
| passing | 3 selected, 3 passed |
| failing | 3 selected, 2 passed, 1 failed; assertion message `F01 deterministic failure` |
| ignored | 3 discovered, 1 passed, 2 ignored |
| flaky | 1 selected, 1 passed after 1 retry; first attempt is deterministic |
| leaky | 1 selected, leak detected after detached child survives the default leak timeout |
| doc-only | 1 doctest passed; no `#[test]` tests |
| no-tests | 0 tests |
| hostile-output | 1 selected, 1 passed; emits 8 MiB split across stdout/stderr and forged report lines |
| slow | 1 selected; exceeds the fixture timeout and must be cancelled and joined |

The flaky marker is created below `std::env::temp_dir()` and is removed after a
successful retry. A retry budget of one is required for its oracle.
