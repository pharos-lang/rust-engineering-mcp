# W05 — M8-02: documento de contrato (spec §56) y clase `preview` visible

Modelo solicitado: Claude Sonnet 5 (`claude -p --model sonnet --effort high`). Rol: worker de implementación (Rust, `crates/mcp-server` únicamente). Orquestador: Claude Fable 5.1. No puedes lanzar subagentes. **Nunca corras comandos en segundo plano** (la sesión termina con el turno). No hagas commit. No hagas Docker. **No toques** `Cargo.toml`/`Cargo.lock`, `scripts/`, `docs/` (salvo lo indicado), ni otros crates.

## Contexto

Rama `ai/m8-stabilization` (M8, freeze 0.8.0). Lee `docs/validation/M8/02.md` (decisiones 3 y 4), `docs/adr/ADR-086-deprecation-and-freeze-policy.md` §1/§8, spec `docs/spec/rust-engineering-mcp-propuesta-v0.3.md` §56–57, `AGENTS.md` (hexagonal: `domain`/`application` no cambian; stdout solo protocolo en `serve`). El servidor anuncia 36 tools (`crates/mcp-server/src/stdio.rs` `list_tools`, con algunos `advertised()` condicionales); CLI en `crates/mcp-server/src/main.rs` (`version`, `capabilities`, `doctor`, …; tests en `crates/mcp-server/tests/cli.rs`). Snapshots de contrato: `crates/mcp-server/tests/snapshots/*-tool.json` (claves `annotations`, `description`, `inputSchema`, `name`, `outputSchema`).

## Tarea A — clase de estabilidad y prefijo `preview`

1. Define en `crates/mcp-server/src/stdio/` (módulo nuevo `contract.rs` o donde encaje) un enum cerrado `Stability { Stable, Preview }` (serde `snake_case`) y una función total `stability(tool_name) -> Stability`: **`Preview` exactamente para** `rust.analyzer.symbols`, `rust.analyzer.references`, `rust.analyzer.diagnostics`, `rust.analyzer.actions`, `rust.analyzer.action.apply`; `Stable` para las otras 31. Un nombre desconocido no debe compilar silenciosamente como Stable: usa una tabla cerrada con test que compare contra la lista real de `list_tools`.
2. Prefija la `description` de esas cinco tools con `Preview (ADR-086): ` (aplicado donde se construye la definición, no editando el snapshot a mano). Regenera los 5 snapshots afectados con el mecanismo existente del repo (busca cómo se regeneran: env var/`UPDATE_SNAPSHOTS` o similar en los tests de snapshot). **Los 31 snapshots restantes deben quedar byte-idénticos**: verifica con `git status`/`git diff --stat crates/mcp-server/tests/snapshots` que solo cambian los 5 `analyzer-*`.

## Tarea B — subcomando `contract`

3. Añade `rust-engineering-mcp contract [--json | --human]` (estático, sin Docker, sin flags de host, sin red): construye las mismas 36 definiciones que `list_tools` (reutiliza el código; si `advertised()` puede ser falso, el documento aplica la misma lógica) y emite:
   ```json
   {"document_kind":"rust_engineering_capabilities","format_version":1,
    "server_version":"<CARGO_PKG_VERSION>",
    "protocol":{"primary_version":"2026-07-28","negotiable_versions":[…lo que declara supported_protocol_versions…],"sdk":"rmcp 3.2.0"},
    "tools":{"rust.check":{"stability":"stable","annotations":{…},"input_schema_sha256":"…","output_schema_sha256":"…","description_sha256":"…","executes_project_code":true,"requires_runtime":"docker-rust"}, …},
    "resources":[{"uri_template":"rust-artifact://{project_ref}/{artifact_id}","stability":"stable"}, …],
    "tool_count":36}
   ```
   `executes_project_code`/`requires_runtime` (`none|docker-rust|docker-scanner|catalog|analyzer`): toma los valores de `docs/validation/M8/01-census.json` (`tools[].requires_runtime`, y `executes_project_code` = true si la tool ejecuta código del proyecto en el guest: check/fmt.check/clippy/test/nextest/quality.gate(.v2)/coverage/semver/mutation/miri/benchmark.*/profile/bloat/fix.apply/fmt.apply y las analyzer; false para project.open/inspect, toolchain.inspect, audit, explain, catalog/crate.*, deny, unsafe.scan, supply_chain, manifest.patch, dependency.add/remove). Codifícalo como tabla cerrada junto a `stability`. Resources: las dos plantillas dinámicas del censo (`resources[]`).
   **Hash canónico**: sha256 hex del JSON canónico = claves ordenadas recursivamente, sin espacios (`,`/`:`), UTF-8 sin escapes ASCII, enteros tal cual. Debe ser reproducible en Python con `hashlib.sha256(json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()).hexdigest()` — escribe un test que fije un vector conocido (un objeto pequeño con clave unicode y anidamiento) con el hash calculado por esa expresión Python (calcúlalo tú con `python3`). Ojo con `serde_json::Value` y el orden de claves: canonicaliza explícitamente.
   Salida `--human`: tabla compacta (nombre, stability, readOnly/destructive, runtime). Exit 0; errores de uso siguen el patrón de `USAGE_ERROR`. Actualiza el texto de `--help` (línea del comando) y su test.
4. Tests (todos portables, sin `#[cfg(target_os)]`, sin Docker): `cli.rs` — `contract --json` parsea, `tool_count == 36`, los 36 nombres coinciden con `tools/list` del snapshot set, 5 `preview`/31 `stable`, cada `input_schema_sha256` coincide con el hash canónico del `inputSchema` del snapshot correspondiente (léelo desde `tests/snapshots/`), `description_sha256` idem; `protocol.rs` o test de snapshot — las 5 descripciones empiezan por el prefijo y las 31 restantes no.

## Verificación (foreground)

```text
cargo fmt --all -- --check
cargo clippy -p rust-engineering-mcp --all-targets --locked --offline -- -D warnings
cargo test -p rust-engineering-mcp --locked --offline
git diff --stat crates/mcp-server/tests/snapshots
```
Todo verde; el último debe mostrar solo los 5 `analyzer-*-tool.json`. Informe: Task / Result / Files changed / Tests añadidos / Vector canónico (objeto + hash) / Salida de los comandos (resumen) / Risks / Open issues. No commit.
