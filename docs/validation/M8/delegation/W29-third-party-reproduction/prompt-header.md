# W29 — M8-06: reproducción de las guías públicas por un tercero (README/CLI/tools/security/client docs)

Modelo solicitado: Claude Sonnet 5 (`claude -p --model sonnet --effort high`). Rol: **tercero** que instala, configura y opera el producto siguiendo únicamente la documentación pública, sin leer el código ni los registros de validación. Orquestador: Claude Fable 5.1. Sin subagentes. **Nunca en segundo plano: Bash acepta `timeout` hasta 600000 ms; `run_in_background` está PROHIBIDO.** No commit. **Único archivo que puedes escribir:** `docs/validation/M8/06-reproduction.md`. No modifiques nada más (los defectos que encuentres se registran, no se arreglan).

## Reglas del tercero

Usa solo: `README.md`, `docs/tools.md`, `docs/client-configuration.md`, `docs/security-model.md`, `SECURITY.md`, `docs/compatibility.md`, `docs/publication.md`, `CHANGELOG.md`, y la salida de `--help` del binario. **No leas** `crates/`, `scripts/`, `docs/validation/`, `docs/adr/` ni `docs/roadmap/` (si la guía te obliga a hacerlo para completar un paso, eso es un hallazgo). Trabaja en un directorio limpio bajo `target/m8-third-party/` (sin `/tmp` literal), con `HOME` real solo cuando la guía lo exija para un cliente.

## Recorrido obligatorio (registra cada comando, exit code y extracto)

1. **Instalación**: sigue la guía de instalación del README para macOS ARM64 (archive core — usa el archive del ensayo local `target/m8-release/…` si la guía indica «descarga» y no hay release publicada; anota que no había release 0.8.0 publicada); alternativa `cargo build --release --locked --offline` si el README la documenta. Verifica lo que el README dice que verifiques (SHA-256, `version`).
2. **Configuración**: sigue `docs/client-configuration.md` para Claude Code y Codex (config real en un `HOME`/`CODEX_HOME` temporal cuando sea posible) y `doctor --json` pasivo con los flags que la guía indica; sin Docker: comprueba que la guía explica qué ocurre y qué devuelve cada modalidad `unavailable/degraded` (compáralo con lo observado en un `tools/call` real por stdio, p. ej. `rust.check` sin runtime, `rust.catalog.status` sin catálogo).
3. **Operación**: `serve --stdio` con un fixture propio (crea un crate mínimo con `cargo init` bajo tu directorio), `rust.project.open`, `rust.project.inspect` (esperado `unavailable` sin Docker según la guía), `contract --json`, `doctor`; procedimiento de backup/restore/rollback del README §Operación (simúlalo con un `--state-root` vacío y comprueba `doctor.mutation_journals` si la guía lo menciona).
4. **Incidente**: sigue `SECURITY.md`/`docs/security-model.md` para «qué hacer si…» (reporte de vulnerabilidad, snapshot RustSec obsoleto, journal pendiente antes de bajar de versión) y comprueba que las instrucciones son ejecutables.
5. **Consistencia**: señala todo texto histórico que contradiga el estado actual (conteos, versiones, «en desarrollo», hosts, releases inexistentes), enlaces rotos, comandos que no existen o difieren del `--help`, y expectativas no documentadas.

## Entregable `docs/validation/M8/06-reproduction.md`

Tabla por paso (documento y sección seguida, comando, exit code, resultado esperado según la guía, resultado observado, `ok|desviación`), lista de **hallazgos** `F-n` con severidad (P1 = la guía no permite completar el paso; P2 = resultado distinto al documentado; P3 = texto obsoleto/ambiguo), y un veredicto: «reproducible por tercero: sí/no, con N desviaciones». Sé honesto: no completes pasos leyendo código.


Nota operativa: ejecuta comandos con rutas **relativas** desde la raíz del repo (p. ej. `target/release/rust-engineering-mcp version --json`); evita `cd` y `/tmp` (usa `target/m8-third-party/`).


**Invocación del binario (obligatorio por el allowlist de tu sesión):** escribe exactamente `target/release/rust-engineering-mcp <subcomando> …` (sin `./`, sin rutas absolutas, sin encadenar con `;`/`&&` otros comandos en la misma línea; para capturar salida usa `> target/m8-third-party/<archivo>` y léelo con `cat`). Para pipelines JSON-RPC usa `printf … | target/release/rust-engineering-mcp serve --stdio --root …`.
