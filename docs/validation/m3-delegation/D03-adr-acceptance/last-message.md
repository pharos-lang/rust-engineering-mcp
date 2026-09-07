Task: Package D03 documentation fixes on `ai/m3-quality`.

Result: Completed. ADR-064 and ADR-065 are formally Accepted effective 2026-09-06 with security, final-review, runtime, and Rust-security evidence links.

Files changed with SHA-256:

- `docs/adr/ADR-064-quality-job-seccomp-profile.md` — `55084f4f23a1672aed03cde1d715b9cac77ac79cce37f41d7f51466267af0c2e`
- `docs/adr/ADR-065-coverage-target-volume.md` — `e000816e4a0a8d5ea2b88cefdf0ad171e266b1965e357e83b06bf89aa39759de`
- `docs/adr/README.md` — `4ba488ea4632f8269dde02a2ae75ef4f0fd478f608fc4096984c00a5575346ff`
- `docs/adr/ADR-060-bounded-job-execution-and-mcp-tasks.md` — `53af8d1765fe8a2d6804a26170007de786b0ad09f92198b0c593a90e77c698a9`
- `docs/validation/M3-02.md` — `8afa4128b7c4967aa91bdd8bc38df951ea63341dc5207926a13d3c6915fd1fff`
- `docs/tools.md` — `c1ef66514469d91779f9d9e115f343a767713e59f66c202b2da6213c76e16ab0`
- `README.md` — `a8f21d3159178c71f997a37bc5307b65ae2ea9470192f0751ed5329d088a0874`
- `CHANGELOG.md` — `4e89a83e27f22f40119fca7a2d4660aaf31507d961382f43531fb974e205fd89`

Checks executed:

- `git diff --check` — passed.
- Relative-link check across all eight edited Markdown files — passed.
- Trailing-whitespace/tab check — passed.
- Receipt verification: M3 runtime `59/59`; Rust security `20/20`.
- No Cargo, Docker, installs, downloads, commits, pushes, or changes to the excluded files.

Evidence:

- VF-02: normalized counts in `docs/tools.md`, `README.md`, and `CHANGELOG.md` with breakdown `nextest 19`, `Tasks lifecycle 4`, `coverage 8`, `SemVer 18`, `mutation 10`.
- VF-03: refreshed ADR index entries 060–065.
- VF-04: clarified carried-forward nextest samples in M3-02 and ADR-060.
- VF-05: added Inspector CLI limitations as footnotes in M3-02.
- VF-06: documented transient `unavailable` contention and meaningful re-polling in `docs/tools.md`.

Risks: The worktree contained extensive pre-existing changes outside this package; they were preserved.

Decisions: Historical `55/55` references in ADR-065 Consequences/Sources were left unchanged because those sections were explicitly protected and VF-02 limited normalization to the three named files.

Open issues: None for Package D03.