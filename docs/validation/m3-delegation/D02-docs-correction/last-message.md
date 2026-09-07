## Task

Package D02 — corregir documentación pública contra la evidencia real de M3.

## Result

Completado en la rama `ai/m3-quality`, sin commits, Docker, Cargo, instalaciones ni descargas.

Se actualizaron:

- 22 tools registradas.
- Gate Docker M3: 55/55.
- Gate Rust de seguridad: 20/20.
- Suite no-Docker: 1.066 pasaron, 0 fallaron, 109 ignoradas.
- Tasks implementado, pero no anunciado ni calificado para clientes.
- M3-06 permanece pendiente.
- Store persistente y CLI `quality-artifacts recover|prune` documentados.
- Imagen guest y límites de plataforma corregidos.
- Hash vigente del recibo: `910c4f82d6421b06b58102ee03c73257e97e4078dab1981e1f01bc6e52678053`.

## Files changed with SHA-256

- `README.md` — `cd6710b34170c2fce979fc86a81480d373112596e88f1b46c2d65883fe560de2`
- `CHANGELOG.md` — `5adff34f487a702a41061f7d6f17f003db04f07fee43ea3198ab26ce6a3b15fd`
- `SECURITY.md` — `d9cfd92ce85b0155198aec10cd133e320881d2a39e3a2bb3c318078d8136c6ad`
- `docs/architecture.md` — `2ba36c60f65214969f0a41da747b0e10e36e60b761738f24dcefe663aab230ab`
- `docs/implementation-status.md` — `a2fcd7490e61589b2cac584dd93e82b82b93e1f38311c23890970bc2e1be5ba8`
- `docs/validation/M3-04.md` — `d808fa350bfa6d6b0e38a6015894fd15d7e4623de6a9156c868fd5c79c28497a`
- `docs/validation/M3-05.md` — `5fbbb81a03f46dace031b0644a6aba93a297b0ad64e7710b03354a3a7a97f7e7`

## Tests executed

- `git diff --check` — OK.
- Relative-link check sobre los 7 archivos editados — OK.
- Verificación de SHA-256 del recibo vigente — OK.
- Verificación de ausencia del hash superseded en M3-04/M3-05 — OK.
- No se ejecutaron Cargo, Docker ni tests de runtime, conforme a la instrucción.

## Evidence

- Counts y image ID: `docs/validation/M3-runtime.json`, `docs/validation/M3-rust-security.json`.
- 22 tools y `TASKS_ADVERTISEMENT_READY = false`: `crates/mcp-server/src/stdio.rs`.
- Suite no-Docker: `docs/validation/m3-delegation/W3-security-fixes/last-message.md`.
- Provisioning, versiones y hashes: `docs/validation/M3-provisioning.json`.
- Seccomp: `docs/adr/ADR-064-quality-job-seccomp-profile.md`.
- Coverage volume: `docs/adr/ADR-065-coverage-target-volume.md`.
- Store, lifecycle y baselines: ADR-060–063.
- Snapshot de mutation deliberadamente actualizado: `docs/validation/m3-delegation/W3-security-fixes/last-message.md`.

## Risks

Tasks aún no está calificado para clientes y M3-06 no ha corrido. ADR-064 y ADR-065 conservan su estado formal pendiente de revisión de milestone.

## Decisions

Se preservaron todos los cambios preexistentes fuera del alcance documental y no se tocaron archivos asignados al integrador.

## Open issues

Ninguno dentro del alcance de este paquete.