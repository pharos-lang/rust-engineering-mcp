# Visión general y arquitectura hexagonal

Rust Engineering MCP es un servidor MCP local que expone evidencia verificable
sobre proyectos Rust (compilación, lints, tests, seguridad, catálogo de
crates, rendimiento y ahora rust-analyzer) a un agente, en vez de darle acceso
a un shell. La versión de trabajo actual es `0.9.0-rc.1` (36 tools; ver
[`decisions.md`](decisions.md) y [`reference/tools.md`](../reference/tools.md)).
Este capítulo cubre por qué el producto es Rust+Tokio, cómo se dividen sus
crates y qué invariante hexagonal se fuerza en CI. El resto de decisiones de
dominio, protocolo, ejecución, mutación, catálogo, jobs, analyzer y
rendimiento tienen su propio capítulo (ver el índice en
[`docs/README.md`](../README.md)).

## Rust y Tokio como plataforma

Decisiones: ADR-001.

**Decisión.** El servidor se implementa en Rust estable, con Tokio como
runtime async para I/O, procesos y cancelación. El toolchain y el MSRV están
fijados en el propio repositorio (`rust-toolchain.toml`, `Cargo.toml`
`rust-version = "1.98.1"`), `Cargo.lock` se mantiene para el binario, los
tipos son concretos con errores tipados (nunca `anyhow`/`Box<dyn Error>`
genérico en rutas de producción) y todo `unsafe` propio exige justificación y
test local.

**Contexto.** El servidor integra procesos Cargo externos, I/O asíncrono,
parseo de datos no confiables (JSON de Cargo, TOML, JUnit, LSP) y necesita
distribuirse como binario autocontenido cross-platform.

**Alternativas consideradas y por qué su rechazo sigue explicando un límite
actual.**
- TypeScript o Python: peor ajuste para control de procesos nativos y
  distribución sin runtime externo; se habría necesitado un empaquetador
  adicional para lograr lo que Rust da con `cargo build --release`.
- Rust síncrono (sin Tokio): no ajusta a stdio concurrente, cancelación
  cooperativa ni streaming de procesos hijos sin bloquear el hilo del
  protocolo.

**Consecuencias.** El pinning de versión (`Cargo.toml` usa `=` en
dependencias estratégicas) y el MSRV verificado en vivo contra
`Cargo.toml`/`rust-toolchain.toml` son parte del contrato de mantenimiento.
Rust **no sustituye** el sandboxing del sistema operativo — ese principio se
opera concretamente en [`execution-and-security.md`](execution-and-security.md)
(ADR-008/009/025) y en la frontera de distribución de 1.0
([`reference/compatibility.md`](../reference/compatibility.md), ADR-087).

**Estado actual.** Implementado; el pinning se verifica en vivo contra el
manifest. El MSRV fue `1.97.1` originalmente y pasó a `1.98.1` por el
addendum de ADR-021 del 2026-09-03.

## Arquitectura hexagonal y fronteras de crate

Decisiones: ADR-004.

**Decisión.** `crates/domain` y `crates/application` no dependen de `rmcp`,
JSON-RPC, stdio, Cargo CLI, SQLite ni LanceDB. Los ports se crean solo en
fronteras reales que protejan el dominio o habiliten pruebas — no una capa de
interfaces vacía por cada concepto. El workspace se mantiene deliberadamente
austero: **8 crates**, no una capa por adapter teórico:

| Crate | Responsabilidad |
| --- | --- |
| `domain` | Tipos, invariantes, evaluación de freshness; solo depende de Serde. Ver [`domain-and-application.md`](domain-and-application.md). |
| `application` | Casos de uso, ports, `ProjectRegistry`, admisión de ejecución; depende solo de `domain`. |
| `project-adapter` | I/O protegido (no-follow/reparse-safe), TOML/semver acotado, SHA-256, reloj monotónico, journal de mutación. |
| `execution-adapter` | Único punto de creación de procesos del producto: `RustGateway`, gateways de mutación/coverage/semver/performance/analyzer. |
| `catalog-adapter` | SQLite autoritativo, FTS5, bundles firmados, auditoría RustSec propia. |
| `semantic-adapter` | Embeddings locales (fastembed/E5) y LanceDB derivado. |
| `artifact-adapter` | Store de artifacts efímero en memoria (M1) y sus cuotas/redacción. |
| `mcp-server` | El único crate que depende de `rmcp`; DTOs, schemas, CLI, capability document. |

**Contexto.** El dominio debe seguir estable pese al churn de `rmcp`, Cargo,
SQLite, LanceDB o del sandbox subyacente; una reescritura de adapter no debe
forzar cambios de dominio.

**Alternativas consideradas y por qué su rechazo sigue explicando un límite
actual.**
- Un módulo único acoplado: más difícil de asegurar y de testear en
  aislamiento; imposible de forzar por CI.
- Un crate por concepto desde el día uno (el layout de 10 crates que
  proponía la especificación original, §13): ceremonia sin evidencia de
  necesidad. El layout real de 8 crates, más grueso, es una divergencia
  aceptada y registrada como deuda técnica en su momento, no una ADR
  numerada — la razón de fondo (evitar scaffolding especulativo) es la misma
  que ADR-021 aplica al bootstrap.

**Consecuencias.** Implementación por cortes verticales ejecutables en vez de
capas de interfaces vacías. `scripts/check-architecture.py` fuerza la regla en
cada gate:
- inspecciona `cargo metadata` para prohibir que `domain`/`application`
  dependan de `rmcp`, `rusqlite`, `lancedb`, `tokio` (más allá de lo mínimo) o
  crates de proceso;
- prohíbe por regex el uso de `std::fs`, `std::process`, `rmcp::`,
  `rusqlite::`, `lancedb::` o `serde_json::Value` como tipo interno dentro de
  `domain`/`application`;
- prohíbe `Command::new`, `std::process::Command` y
  `tokio::process::Command` en cualquier crate que no sea
  `execution-adapter` (la misma regla que sostiene el Execution Gateway
  único de [`execution-and-security.md`](execution-and-security.md)).

**Estado actual.** Implementado y forzado continuamente en el gate `core` y
`full` (`scripts/gate.py`, etapa `architecture`).

**Limitación.** El enforcement es un script Python de regex y metadata de
Cargo, no una garantía a nivel de compilador Rust. Código generado por macro,
un alias re-exportado o una dependencia transitiva que reintroduzca el mismo
símbolo bajo otro nombre podrían en principio evadirlo. No existe un test
Rust independiente (por ejemplo, un lint de workspace o un `cargo-deny`
`bans` por dependencia directa) que duplique esta garantía a otro nivel.

## Bootstrap ejecutable mínimo (histórico en su alcance de comportamiento; vigente en sus convenciones)

Decisiones: ADR-021.

**Decisión histórica.** El workspace inició como un solo paquete binario
virtual, `publish = false`, edición 2024, con MSRV fijado, y con la regla de
no agregar una dependencia hasta tener un consumidor real dentro del propio
producto — la misma disciplina que ADR-004 aplica a los crates.

**Qué sigue vigente.** Las convenciones de toolchain, edición y MSRV
(`Cargo.toml`, `rust-toolchain.toml`, hoy en `1.98.1` tras el addendum del
2026-09-03 registrado en el propio ADR-021) y la disciplina de "no
scaffolding especulativo".

**Qué es histórico y no debe repetirse como estado actual.** Las
afirmaciones de comportamiento del binario original — "solo `--help`/
`--version`", "`serve --stdio` no arranca un servidor real" — describen el
commit inicial del bootstrap, no el producto de hoy. El binario actual tiene
un servidor MCP stdio funcional con 36 tools registradas
(`crates/mcp-server/src/stdio/*.rs`; ver [`reference/tools.md`](../reference/tools.md)).

## Lenguaje interno, tipos y dependencias no adoptadas

La comunicación interna usa tipos Rust fuertemente tipados
(`crates/domain/src/*.rs`), nunca `serde_json::Value` como modelo general
(forzado también por `scripts/check-architecture.py`). El uso de código
untrusted-execution está confinado al Execution Gateway (ver
[`execution-and-security.md`](execution-and-security.md)).

**Limitación documental (no de código).** La propuesta original sugería un
conjunto de dependencias "core" — `thiserror`, `anyhow`, `camino`,
`tempfile`, `cargo_metadata` — como base idiomática. Ninguna de esas cinco
es una dependencia del workspace hoy: el producto usa enums de error
cerrados hechos a mano (coherente con "vocabulario cerrado de status/
error_code" de ADR-022, ver
[`domain-and-application.md`](domain-and-application.md)) y `rustix` para
el I/O de bajo nivel en macOS en vez de `camino`/`tempfile`. Esto funciona y
está bien testeado, pero no existe una ADR que registre explícitamente la
sustitución — se documenta aquí para que la razón ("por qué no `thiserror`")
no se pierda junto con la especificación retirada.

## Por qué no un LLM interno, por qué pocas tools y por qué no shell

Estos tres no-objetivos atraviesan todo el producto y se detallan en su
capítulo propio, pero se resumen aquí porque enmarcan el resto de decisiones
de este documento:

- **Sin LLM interno en el core** (ADR-005) — ver
  [`domain-and-application.md`](domain-and-application.md).
- **Sin shell arbitrario, un único Execution Gateway** (ADR-008/010) — ver
  [`execution-and-security.md`](execution-and-security.md).
- **Tools compuestas y de alto valor en vez de un wrapper genérico de
  comandos** — el producto nunca implementó `suggest_optimizations`,
  `idiomatic_rust` ni `generate_test` como tools core; en su lugar expone
  evidencia real vía `rust.clippy`, los tools de analyzer y
  `rust.benchmark.*`/`rust.profile.flamegraph`/`rust.binary.bloat` (ver
  [`analyzer.md`](analyzer.md) y [`performance.md`](performance.md)). El
  producto tampoco intenta ser un IDE completo, un reemplazo de
  rust-analyzer o de Cargo, una plataforma CI/CD, un agente autónomo, un
  generador de código con LLM, un shell remoto genérico ni una herramienta
  de mutación sin límites — cada uno de esos no-objetivos originales sigue
  vigente hoy, no solo en el MVP 0.1.0.

## Diferenciadores frente a acceso de terminal crudo

Frente a darle a un agente un shell sin restricciones, el producto aporta:
seguridad (Execution Gateway único, sin shell arbitrario, deny-by-default),
estructura (DTOs tipados con JSON Schema en vez de texto libre), descubribilidad
(`tools/list` y el capability document — ver
[`mcp-and-contracts.md`](mcp-and-contracts.md)), control de política (grants
explícitos del host, roots confiables), eficiencia de contexto (artifacts
por referencia, paginación, presupuestos de respuesta), composición
(`rust.quality.gate`/`.v2`) y evidencia reproducible (metadata de
rustc/cargo/target/features/perfil en cada resultado en vez de afirmaciones
sin respaldo).

## Qué queda deliberadamente fuera y dónde vive esa decisión

Un conjunto de capacidades futuras se mencionan en la especificación
original sin mandato concreto: incremento de contexto por análisis de
impacto, fuzzing (`cargo-fuzz`), Loom, Kani/Prusti, `cargo-msrv`/`-hack`/
`-minimal-versions`/`-expand`/`-outdated`/`-udeps`, bundles de instalación
por perfil (`core`/`quality`/`security`/`full`), autoactualización del
binario y Homebrew/Scoop/WinGet. Ninguna tiene código ni ADR; son
aspiracionales, no una limitación de la implementación actual. `cargo-bloat`
es la única excepción: listado en la especificación como integración
*futura* y en realidad ya implementado desde M5 (`rust.binary.bloat`, ver
[`performance.md`](performance.md)).

El transporte remoto (HTTP/M7) está formalmente diferido por una decisión
registrada, no simplemente pendiente — ver
[`mcp-and-contracts.md`](mcp-and-contracts.md#rmcp-como-frontera-mcp)
y `.planning/deferred-commitments.md`.

## Precedencia de diseño

Ante conflicto, el producto prioriza correctness, seguridad, ergonomía para
agentes, reproducibilidad, rendimiento y por último amplitud de features —
el mismo orden que fija `AGENTS.md` para decisiones de ingeniería del propio
repositorio.
