# Prerrequisitos de M1 desde la foundation M0

Esta lista conserva la línea base histórica al cerrar M0; los avances verificados
figuran debajo y en el tablero. No representa el estado pendiente de cada punto hoy.

La primera vertical era M1-01 `rust.project.inspect`. Conservar las trece tools del
tablero; `rust.dependencies.inspect` no es pública. M2 sigue fuera de alcance.

1. La foundation M0 admite solo enums de probes Go/Docker, sin Cargo,
   host mounts ni transferencia de source. M1-01 debe resolver una imagen/runtime
   Rust1.98.1 explícitamente aprobada y el camino acotado de fuentes/dependencias,
   con ADR y recalibración del perfil real antes de habilitar Cargo. No reutilizar
   fingerprints de probes como autorización de otro programa/configuración.
2. `project.open` valida un subconjunto estructural, no sustituye Cargo metadata.
   I/O propio permanece no-follow relativo a handles; no leer source desde paths
   canonicalizados y reabiertos. Resolver ProjectRef vivo antes de cada operación.
3. Check/Clippy/test pueden ejecutar build.rs/proc macros: fixtures Rust adversos
   bajo el sandbox real, con controles positivos de fs/red/env/children/recursos.
   El adversario de fixtures/security nunca se ejecuta en el host. Solo hay evidencia
   de containment de probes en M0, no de Cargo/build.rs/proc macros.
4. MCP debe integrar jobs costosos en workers acotados, cancelación/kill-tree,
   backpressure y deadlines de entrada/salida. Resolver limitación de primer request
   inline de rmcp antes de operaciones largas; no implementar JSON-RPC paralelo.
5. Conectar logs/diffs al ArtifactStore y Resources mínimos ADR-011/014/028. Revalidar
   autorización/retención, URI opaca y límites; mapear errores internos, exponer TTL
   restante y considerar presupuestos compartidos. Store actual es efímero, sin disco.
6. Catalog CLI: autenticidad de manifest/distribución, antirollback durable,
   archive extraction segura, staging/activación atómicos, límites y provenance real.
   SQLite actual recibe bytes/manifest esperado confiable, no autentica un publisher.
7. Construir índices desde facts SQLite y embeddings del provider verificado; no
   confiar metadata/vectores de una importación sin validar. El índice actual es
   memory:// por generación; persistencia/import son M1. SQLite decide facts/filtros.
8. Integrar la feature local en la distribución M1, fuera del reactor. Modelo fijado
   E5 y ORT aprobados; loader sin paths/downloads. CLI explícita de aprovisionamiento,
   fuente/provenance de modelo y native artifact, benchmarks ES/EN/startup/CPU/RAM.
   Whitespace de pasajes normalizado; resultados lexical-first actuales son foundation,
   no un ranking híbrido estabilizado ni evidencia de utilidad experimental.
9. RustSec real local con freshness, sin refresh del runtime. RSA fixture es un input
   audit mínimo, no un lock compilable ni un matching engine ya implementado.
10. Antes de publicar: licencia explícita del owner, revisión de redistribución de
    dependencias/modelo/native runtime, clientes MCP reales, runners nativos en cada
    target anunciado y experimento de utilidad definido en M1-16. No hay release M0.

11. Para cada nueva tool: DTOs anidados con schemas cerrados, snapshot de schema
    versionado, revisión de ToolStatus → isError y errores fijos que no reflejen
    entradas. Presupuestar el envelope MCP completo, incluyendo facts, provenance,
    evidence y metadata, en éxito y fallback. El adapter SQLite ya limita el JSON
    de summaries a128KiB; esto no sustituye el presupuesto del envelope público.

El gate normal usa CARGO_INCREMENTAL=0. `scripts/gate.py full` necesita assets locales
provisionados; missing no se convierte en skip/passed. La advertencia de mantenimiento
paste1.0.15 permanece visible, sin ignores de vulnerabilidades. Fuentes e inventario
actual: implementation-status.md, ci.md y validation/M0-12.md.

## Avance verificado de prerrequisitos

Workers/admisión ADR-030 integrados localmente: core201 tests y smoke23/23.
Runtime Rust/Cargo1.98.1 Linux ARM64 instalado por autorización explícita, fijado
por image ID y reverificado ([evidencia](validation/M1-01-runtime.md)). La captura
ADR-031 está integrada (`e2d6ae0`, smoke18/18). El gateway Rust pasa seis escenarios
reales y el gate core245 ([evidencia](validation/M1-01-rust-gateway.md)); revisión
Opus5 resuelta. Readiness de bootstrap, espera de cleanup en MCP y metadata
están implementados en ADR-032, con validación M1-01 en [evidencia](validation/M1-01.md).

M1-03 conecta el punto5 para logs de check: ArtifactStore efímero, Resources con
autorización live/owner/retención y presupuestos; diffs y persistencia siguen
pendientes. No convierte los puntos6..9 de catálogo/import/RustSec en terminados.

M1-03 integrated (`96fc984`/`4ddc696`), core332 and six exact Docker tests; Resources
authorize live ProjectRefs. M1-04 reuses that publication lifecycle for formatting;
no extra assets. Later audit/catalog provisioning and authenticated imports remain
separate pending work.

M1-04 integrated5e97806/e8253a7 with post-merge format smoke1/1. M1-05 uses the
already approved Clippy0.1.98 runtime; no extra image or dependency acquisition.

M1-05 integrated464fffb/57ca421, actual Clippy smoke1/1. M1-06 uses the same
approved image for R2 and adds actual test-runtime cleanup evidence. RustSec
acquisition/snapshot trust and in-process versus external audit remain M1-07
decisions; existing local advisory metadata is not an authenticated import.

M1-06 integratedf10304d/d23e718, actual test smoke1/1. M1-07 uses host-expected
RustSec JSON snapshots (ADR-038), not historical Git metadata or signed imports.
Its development dependency acquisition is recorded in validation/M1-07-dependencies.md.
Runtime still has no advisory/model/tool acquisition; M1-10 trust/distribution remains.

M1-08 / ADR-039: `rust.diagnostics.explain` accepts only an ASCII `E0000`-shaped
code and obtains bounded text from the approved installed rustc through the same
calibrated, network-denied gateway and joined workers. No project_ref, project source,
resource URI or host rustc execution is needed. Unknown codes return unavailable;
no heuristic explanation substitutes for compiler evidence. Returned text includes
content SHA, immutable runtime identity and latest_known artifact provenance/freshness.
No toolchain/image/model acquisition or native-platform qualification is implied.

M1-09 / ADR-040: `rust.quality.gate` composes fast(fmt/check/strict Clippy) or
standard(+default30s tests/offline audit) over one captured source generation, with
per-stage status, selection, repair detail and runtime evidence. One240s joined
worker; ordinary failures continue, interruption/uncertain cleanup aborts. Logs are
published as a bounded authorized group with final retention/ProjectRef checks;
rollback removes only new IDs, preserving earlier live logs. Omitted nonempty
streams make the quality verdict conservative even when command execution completed.
MCP body/envelope budgets retain stage rows and explicit omissions. No downloads,
source edits, global catalog import or new platform support. M1-10..17 remain pending.
