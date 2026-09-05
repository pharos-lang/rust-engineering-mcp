# Trazabilidad spec → M2–M8, limitaciones y decisiones

Estado: **Planned**. Fuente: [spec completa](../spec/rust-engineering-mcp-propuesta-v0.3.md). El registro final enumera todos los encabezados numéricos, incluidos subapartados; las líneas permiten ubicar la fuente exacta. Cada fila tiene disposición, corte, evidencia requerida y gate. Una fila de mantenimiento no reabre M1: exige preservar su comportamiento y verificar regresiones cuando se amplíe la frontera. “Suggested/Deferred” preserva el carácter sugerido/futuro de la sección, no elimina requisitos normativos.

Los entregables §97 sin número propio se descomponen así: M0/M1 → [baseline](baseline-2026-09-05.md); M2 → M2-01..07; M3 → M3-01..06; M4 → M4-01..06; M5 → M5-01..05; M6 → M6-01..06; M7 → M7-G0 y solo con Go M7-01..06; M8 → M8-01..09. 1.0.0 → M8-02 contratos estables/SemVer, M8-08 security model, M8-07 cross-platform/signing, M8-06 CLI/guías, M8-04 protocolo y M8-03 upgrades; se exige el [checklist de readiness](m8-stabilization.md), no una fecha de release.

Rutas y gates: [maestro G1–G9](m2-m8.md); [M2](m2-safe-mutation.md), [M3](m3-quality.md), [M4](m4-security.md), [M5](m5-performance.md), [M6](m6-analyzer.md), [M7](m7-remote.md), [M8](m8-stabilization.md); [D01–D26](adr-backlog-m2-m8.md). Las rutas de recibos futuras descritas por G5 no son evidencia existente.

## Contradicciones y ambigüedades resueltas o abiertas

| ID | Fuente / tensión | Disposición del Technical Owner | Corte y prueba / gate |
| --- | --- | --- | --- |
| C01 | Prompt: main CI histórico vs HEAD actual | Conservar ambos; aa61bce tiene runs posteriores, release conserva 452acdbf | Baseline live y JSON públicos; G5 |
| C02 | §86 nombra dependencies.inspect vs §23.9/§122/AGENTS | No pertenece a trece tools M1 ni se agrega a M2 | Snapshots y censo M8-01; G1 |
| C03 | §97 aspiración 1.0 cross-platform vs ADR-048 macOS ARM64 core | Abierta D13: calificar familias con native positives o cambiar alcance explícitamente antes de readiness | M8-07 native/artifact matrix; G4/G7 |
| C04 | §96 Docker fuera MVP vs gateway Docker aceptado ADR-031 | Container de ejecución preaprovisionado no equivale a distribución Docker del servidor | Inventario M8-07; G2/G7 |
| C05 | Lecturas RO ADR-013/031 vs mutación M2 | Writer nuevo host-authorized; guest solo genera candidato; no convertir tool M1 | M2-01/02, D01/02/04; G1/G2 |
| C06 | Hashes/locks/rollback vs atomicidad y external writer | Abierta D02: preflight+flock no CAS, journal no atomic multi-file; parent moved puede escapar root | M2-01 native race/crash y rechazo sin garantía; G2/G5 |
| C07 | Source capture no atómica ni ABA universal | Mantener limitación; plan/revalidación no añade snapshot consistente por afirmación | M2-01 y M6-01; source identity/races G2/G4 |
| C08 | Cargo home vacío M1 vs dependency.add/plugins | Abierta D05: datos offline preaprovisionados explícitos, missing no pass, nunca descarga runtime | M2-04 y M3-01; real registry cache oracle G2/G7 |
| C09 | Manifest cambia fingerprint vs receipt tras commit | D01 separa autorización de receipt y referencia invalidada, IDs no bearer | M2-01 reopen/replay/receipt G1/G2 |
| C10 | §51 async vs cinco versiones MCP y rmcp 3.2.0 | Spike Tasks antes de promesa; bounded fallback explícito según capability | M3-02 D06, wire/cancel G1/G3/G4 |
| C11 | §77 artifacts “avanzados M3” pero Resources M1 ya existe | Preservar URI/semántica M1, formato nuevo versionado y privado | M3-01 D17 owner/TTL/quota; G1/G3/G6 |
| C12 | quality strict/release §48 vs enum cerrado M1 | D19 decide contrato nuevo/versionado antes de ampliar; fast/standard congelados | M4-05 snapshots y partial no-pass; G1/G4 |
| C13 | Supply-chain facts más ricos que catálogo actual | Unknown si fuente ausente; schema/migración D22 solo por necesidad | M4-04 truth fixtures; G4/G6 |
| C14 | §32 analyzer sugiere más que §97 M6 | Symbols/references/diagnostics/actions; hover/definition/rename no se añaden implícitamente | M6-01..06 D25; G1/G8 |
| C15 | M7 en secuencia de versiones vs condición explícita | No-go actual: sin caso aprobado, Deferred; M8 continúa tras decisión M7 | M7-G0 acta, no HTTP; G9 |
| C16 | Firma de distribución vs tag Git unsigned | Attestations OIDC assets sí verificadas, no afirmar firma Git | M8-07 D14 verify/redownload; G7 |
| C17 | README actual vs CHANGELOG/SECURITY/compatibility con texto histórico ambiguo | Deuda documental, sin defecto runtime confirmado. Corregir al primer cambio público de implementación; preservar hechos release | M2-07 docs actuales y M8-06 reproducción; G5/G7 |
| C18 | core/local features vs bundles sugeridos §108 | No prometer paquete quality/security/full ni E5/ORT/catalog oficial; D15/16 condicionados a demanda | M8-07 inventario y permisos; G7 |
| C19 | M1-16 saturado vs KPIs/claims §83/118 | No equivalencia/causalidad/calidad general; nuevo claim requiere protocolo independiente | M8-01/08 evidence review; G4/G8 |
| C20 | Campos opcionales/additive durante 0.x | Closed schemas/enums/client parsers pueden romper; freeze y matriz real | M8-02/04 D11; G1/G6 |
| C21 | Prompt original “solo plan” vs última instrucción “commit/merge e implementar M2” | Dos fases: cerrar/integrar docs primero; después M2 autorizado, sin M3 ni release automática | planning-validation y handoff; G5/G9 |
| C22 | Payload M2 vs code actions M6 | D01 define edits internos; antiobjetivo es tool pública arbitraria, D25 versiona operación/formato nuevos | M2-01/M6-04 unknown-format; G1/G6 |
| C23 | Locks M2 vs semántica trece tools M1 | Lecturas no adquieren writer lock ni error nuevo; mantienen captura/admisión; caches se invalidan | M2-01/07 timings y schemas bajo contención; G1/G4 |
| C24 | M7 remote vs adapter M2 solo macOS | M7 source RO, excluye mutaciones; otra ampliación requiere D13 target positivo | M7-03/06 discovery/denials; G2/G7 |

## Deudas, limitaciones y controles M1 que cada ampliación debe atender

| ID | Hecho baseline / fuente | Disposición y corte | Evidencia requerida / gate |
| --- | --- | --- | --- |
| L01 | ADR-024/029/031: macOS ARM64/APFS positivo; Linux/Windows portable | Preservar fail-closed en M2-01; D13/M8-07 para targets nuevos | Native no-follow/reparse/parent rename; G2/G4/G7 |
| L02 | ADR-031 source subset 4096/depth32/path100 ASCII/file1MiB/total16MiB | Límites explícitos M2-01/02 y M6-01; no silencioso scope universal | Boundary/oversize/export tests; G2/G3 |
| L03 | Root handles sin privilegios contra mounts/devices/host ACL | Host TCB explícito; D02 writer y M4-06 review | Native threat model/denials; G2/G8 |
| L04 | Symlink/hardlink/nonregular y configs Cargo rechazados | Preservar ante staging/export/edit; D04 | M2-02/03 hostile fixture; G2/G4 |
| L05 | Captura no atómica, ProjectFingerprint distinto source digest | D01/02 y M6 snapshot identity | M2-01 stale/ABA/manifest invalidation; G1/G2 |
| L06 | Source RO y sin exporter de candidatos | D04 staging tmpfs cuota real; Docker volume no quota demostrada | M2-02 exporter active-mutator/extra files/cancel; G2–G5 |
| L07 | Rust1.98.1 e imagen Docker exacta, cargo home vacío, std/path-only | D05/D21/D26 aprovisionamiento explícito y recalibración | M2-04/M3-01/M4-03/M6-01 offline real y hashes; G2/G7 |
| L08 | Gateway único worker sin queue | M3-01 admite jobs bounded; M7-04 solo si Go | Saturation/fairness/admission/cleanup; G3/G5 |
| L09 | Bootstrap largo denegado hasta ready; project.open inline 10s | No relajar al agregar tools; M3-02/M8-04 | Primera llamada/EOF/partial frame/protocol timing; G3/G4 |
| L10 | Stdio 1MiB/10s/16 slots y cancel suprime respuesta conservando slot | Mantener; Tasks M3-02 no libera antes cleanup | Slow peer/flood/cancel races; G3/G4 |
| L11 | Artifact memoria16MiB global/1MiB owner/256KiB artifact/TTL1h | D17 privado persistente sin romper URI M1 | M3-01 quotas/eviction/TTL/owner/crash; G3/G6 |
| L12 | Redacción literal no secret scanner | Artifacts source contienen posibles secretos; acceso privado y escaneo evidencias | M2-07/M4-06 retention/secret fixtures; G2/G8 |
| L13 | RustSec host snapshot hash, no publisher trust oficial, 24h/7d | Preservar freshness y separar del catálogo; compartir audit en deny | M4-01/04 source/advisory/suppressions; G4/G6 |
| L14 | Catalog Ed25519/antirollback reserve→activate; runtime load-once | No sync runtime ni downgrade floors; D12/D15 | M8-03/07 crash/rotation/poisoning/session reload; G2/G6 |
| L15 | SQLite3.53.2 facts; FTS5 léxico; LanceDB0.31/ORT1.24.2/E5 derivados | D22 facts nuevos solo verificados; D16 distribución opcional | M4-04/M8-03/07 truth/index mismatch/rebuild; G4/G6/G7 |
| L16 | No assets/model/trust/Docker/toolchain/fixtures distribuidos; publish=false | Conservar hasta decisiones D13–D16 y autorización release | M8-07 inventory/licenses/notices/SBOM/smoke; G7 |
| L17 | Release smoke hardcode13 y un solo archive | Cuando se prepare release nueva adaptar expectativas con contrato calificado | M2-07 preparación documentada; M8-07 packaged discovery; G1/G7 |
| L18 | Gate23/23/Inspector/Codex históricos source-bound, no evidencia nueva | Nuevo código exige gate nuevo y client flow | Cierre de cada hito; G4/G5/G8 |
| L19 | Catálogo búsqueda y experimento pequeño no prueba calidad general | Mantener claims acotados; no investigación inventada | M8-01/08 claims-to-source review; G4/G8 |
| L20 | Versiones SDK/protocolo/herramientas pueden cambiar | Verificar oficial y lock antes de decisión estratégica | Todos los cortes; exact versions+fixtures G1/G4/G7 |
| L21 | Native full requiere assets host y perfiles core/local | Ausencia explícita bloquea el gate dependiente; no instalar silenciosamente | Cierres M2..M8 full receipt; G4/G5/G7 |
| L22 | Licencias Apache-2.0/MIT y requisitos de notices del inventario actual | Recalcular por cada plugin/dependencia nueva; no heredar aprobación de licencia | Cada cierre y M8-07 SBOM/notices; G7 |

## Registro exhaustivo de secciones numeradas

| Requisito/sección spec | Disposición | Corte responsable | Evidencia requerida | Gate |
| --- | --- | --- | --- | --- |
| 1 — Resumen ejecutivo (L13) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 2 — Decisiones principales (L62) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 3 — Objetivo (L102) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 4 — No objetivos (L123) | Preservar exclusión | M8-01 | Censo negativo: sin herramientas genéricas/UPX/consejos universales; scope review | G1/G8 |
| 5 — Problema que resuelve (L143) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 6 — Principio fundamental: evidence-driven coding (L174) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 7 — Cómo ayuda a los agentes (L208) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 7.1 — Reduce alucinaciones técnicas (L210) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 7.2 — Mejora los ciclos de reparación (L227) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 7.3 — Permite decisiones basadas en el proyecto (L251) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 7.4 — Mejora seguridad (L270) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 7.5 — Mejora rendimiento (L285) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 8 — Alineación con MCP actual (L300) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 8.1 — Transportes (L304) | Herencia + Conditional | M7-G0/M7-01/M8-04 | stdio probado M1; HTTP solo con caso Go; matriz wire/transport | G1/G2/G4 |
| 8.2 — MCP y JSON-RPC: el envelope de comunicación (L337) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 8.3 — JSON Schema: contrato de inputs y outputs (L401) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 8.4 — Salida de las tools: `structuredContent` (L510) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 8.5 — Stateless por defecto (L592) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 8.6 — Tool annotations (L612) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 9 — No todo debe ser una Tool (L631) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 9.1 — Tools (L637) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 9.2 — Resources (L653) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 9.3 — Prompts (L672) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 10 — Arquitectura propuesta (L691) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 11 — Razones para usar Rust (L751) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 11.1 — Integración natural (L755) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 11.2 — Distribución (L778) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 11.3 — Seguridad de memoria (L790) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 11.4 — Performance (L805) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 12 — SDK MCP (L816) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 13 — Estructura del repositorio (L842) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 14 — Modelo de proyecto (L899) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 15 — Project fingerprint (L938) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 16 — Context snapshot (L962) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 17 — Contrato estándar de las tools (L1005) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 18 — Diagnóstico estructurado (L1047) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 19 — Captura de salida de Cargo (L1074) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 20 — Tool design rules (L1090) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 21 — Taxonomía de operaciones (L1111) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 22 — Catálogo de tools (L1140) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 23 — Tools MVP — 0.1.0 (L1146) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 23.1 — `rust.project.open` (L1148) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 23.2 — `rust.project.inspect` (L1175) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 23.3 — `rust.toolchain.inspect` (L1194) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 23.4 — `rust.check` (L1211) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 23.5 — `rust.clippy` (L1237) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 23.6 — `rust.fmt.check` (L1264) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 23.7 — `rust.test` (L1276) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 23.8 — `rust.dependencies.audit` (L1301) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 23.9 — Inspección interna de dependencias (sin tool pública M1) (L1317) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 23.10 — `rust.diagnostics.explain` (L1334) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 23.11 — `rust.quality.gate` (L1356) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 24 — Por qué `quality.gate` es importante (L1389) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 25 — Tools 0.2.x — edición segura (L1413) | Planned | M2-01..07 | Cinco tools, permisos, diff, precondición, locks y recovery; matriz M2/candidate bytes/receipts | G1–G6/G8–G9 |
| 25.1 — `rust.fmt.apply` (L1419) | Planned | M2-02 | Rustfmt real; no-op/check final/source exacto/config hostil | G2–G5 |
| 25.2 — `rust.fix.apply` (L1432) | Planned | M2-03 | Cargo fix real en staging; check candidato, scope, active cancel | G2–G5 |
| 25.3 — `rust.dependency.add` (L1440) | Planned | M2-04 | TOML semántico+offline resolver; manifest/lock coherentes y cache missing | G1–G6 |
| 25.4 — `rust.dependency.remove` (L1450) | Planned | M2-05 | Remove package/kind/alias/herencia; lock/no-op y Cargo oracle | G1–G6 |
| 25.5 — `rust.manifest.patch` (L1456) | Planned | M2-01/M2-06 | Features/profiles/workspace dependencies/lints; TOML comments + Cargo oracle | G1–G6 |
| 26 — Tools 0.3.x — calidad avanzada (L1473) | Planned | M3-01..06 | Cuatro tools sobre job lifecycle y artifacts autorizados | G1–G9 |
| 26.1 — `rust.test.nextest` (L1477) | Planned | M3-01 | nextest real/JUnit; failure/retries/ignored/leaks/doc tests boundary | G3–G5 |
| 26.2 — `rust.coverage` (L1494) | Planned | M3-03 | LLVM conteos y merge multi-package; LCOV/JSON/HTML, partial | G3–G6 |
| 26.3 — `rust.mutation.test` (L1513) | Planned | M3-05 | Baseline pass, caught/missed/unviable/timeout, diff guest y host intacto | G2–G5 |
| 26.4 — `rust.semver.check` (L1531) | Planned | M3-04 | Dos baselines/source/toolchain/features; breaking fixtures y unknown | G1/G4–G6 |
| 27 — Tools 0.4.x — seguridad avanzada (L1541) | Planned | M4-01..06 | Cuatro tools security y hardening; fuentes/freshness/suppressions | G1–G9 |
| 27.1 — `rust.deny` (L1545) | Planned | M4-01 | deny licenses/bans/sources, audit compartido; suppression expiry | G2/G4–G6 |
| 27.2 — `rust.unsafe.scan` (L1560) | Planned | M4-02 | Unsafe AST/span por origen; comentarios/cfg/macros y cobertura declarada | G1/G4–G5 |
| 27.3 — `rust.miri` (L1582) | Planned | M4-03 | Miri UB real/unsupported; nightly/sysroot exacto y containment | G2–G5/G7 |
| 27.4 — `rust.supply_chain.inspect` (L1597) | Planned | M4-04 | Grafo/source/checksums/advisories; yanked known/unknown; facts no scores | G1/G4–G6 |
| 28 — Tools 0.5.x — performance (L1613) | Planned | M5-01..05 | Cuatro tools performance, ruido y provenance medidos | G1–G9 |
| 28.1 — `rust.benchmark.run` (L1617) | Planned | M5-01 | Samples/warmup/repeticiones/hardware/OS/dataset versionado | G3–G5 |
| 28.2 — `rust.benchmark.compare` (L1625) | Planned | M5-02 | Baseline identity, MDR/noise/CI/ratio; self compare y mismatch | G1/G4–G6 |
| 28.3 — `rust.profile.flamegraph` (L1642) | Planned | M5-03 | Permiso host, profiler real, stacks conocidos, artifact privado/cancel | G2–G5/G7 |
| 28.4 — `rust.binary.bloat` (L1657) | Planned | M5-04 | cargo-bloat real, binary identity/stripped/LTO/unknown | G3–G5/G7 |
| 29 — Tool que NO se recomienda: `suggest_optimizations` (L1669) | Preservar exclusión | M8-01 | Censo negativo: sin herramientas genéricas/UPX/consejos universales; scope review | G1/G8 |
| 30 — Tool que NO se recomienda: `idiomatic_rust` (L1697) | Preservar exclusión | M8-01 | Censo negativo: sin herramientas genéricas/UPX/consejos universales; scope review | G1/G8 |
| 31 — Tool que NO se recomienda como core: `generate_test` (L1713) | Preservar exclusión | M8-01 | Censo negativo: sin herramientas genéricas/UPX/consejos universales; scope review | G1/G8 |
| 32 — Integración con rust-analyzer (L1731) | Planned | M6-01..06 | RA exacto; symbols/references/diagnostics/actions con writer M2 | G1–G9 |
| 33 — Consulta de documentación (L1757) | Suggested/Deferred | M8-01 | Intake de propuestas, no requisito de tool nueva M2–M8; documentar decisión/demanda antes de ampliar alcance | G1/G8/G9 |
| 33.1 — Documentación local (L1765) | Suggested/Deferred | M8-01 | Intake de propuestas, no requisito de tool nueva M2–M8; documentar decisión/demanda antes de ampliar alcance | G1/G8/G9 |
| 33.2 — Documentación remota (L1780) | Suggested/Deferred | M8-01 | Intake de propuestas, no requisito de tool nueva M2–M8; documentar decisión/demanda antes de ampliar alcance | G1/G8/G9 |
| 34 — Catálogo local de crates y conocimiento del ecosistema (L1794) | Herencia + Planned | M4-04/M8-03/M8-07 | SQLite facts/FTS5 y LanceDB derivado, offline snapshots/freshness/antirollback; D22 solo si facts nuevos; D15/16 opcionales | G2/G4–G7 |
| 34.1 — Principio: offline-first (L1819) | Herencia + Planned | M4-04/M8-03/M8-07 | SQLite facts/FTS5 y LanceDB derivado, offline snapshots/freshness/antirollback; D22 solo si facts nuevos; D15/16 opcionales | G2/G4–G7 |
| 34.2 — SQLite como fuente de verdad (L1841) | Herencia + Planned | M4-04/M8-03/M8-07 | SQLite facts/FTS5 y LanceDB derivado, offline snapshots/freshness/antirollback; D22 solo si facts nuevos; D15/16 opcionales | G2/G4–G7 |
| 34.3 — LanceDB como índice semántico del MVP (L1901) | Herencia + Planned | M4-04/M8-03/M8-07 | SQLite facts/FTS5 y LanceDB derivado, offline snapshots/freshness/antirollback; D22 solo si facts nuevos; D15/16 opcionales | G2/G4–G7 |
| 34.4 — Búsqueda híbrida (L1955) | Herencia + Planned | M4-04/M8-03/M8-07 | SQLite facts/FTS5 y LanceDB derivado, offline snapshots/freshness/antirollback; D22 solo si facts nuevos; D15/16 opcionales | G2/G4–G7 |
| 34.5 — `CompositeCatalog` (L1998) | Herencia + Planned | M4-04/M8-03/M8-07 | SQLite facts/FTS5 y LanceDB derivado, offline snapshots/freshness/antirollback; D22 solo si facts nuevos; D15/16 opcionales | G2/G4–G7 |
| 34.6 — Datos del proyecto vs catálogo global (L2060) | Herencia + Planned | M4-04/M8-03/M8-07 | SQLite facts/FTS5 y LanceDB derivado, offline snapshots/freshness/antirollback; D22 solo si facts nuevos; D15/16 opcionales | G2/G4–G7 |
| 34.7 — Gestión de actualizaciones del catálogo (L2081) | Herencia + Planned | M4-04/M8-03/M8-07 | SQLite facts/FTS5 y LanceDB derivado, offline snapshots/freshness/antirollback; D22 solo si facts nuevos; D15/16 opcionales | G2/G4–G7 |
| 34.8 — Entornos con red restringida (L2125) | Herencia + Planned | M4-04/M8-03/M8-07 | SQLite facts/FTS5 y LanceDB derivado, offline snapshots/freshness/antirollback; D22 solo si facts nuevos; D15/16 opcionales | G2/G4–G7 |
| 34.9 — Embeddings offline (L2178) | Herencia + Planned | M4-04/M8-03/M8-07 | SQLite facts/FTS5 y LanceDB derivado, offline snapshots/freshness/antirollback; D22 solo si facts nuevos; D15/16 opcionales | G2/G4–G7 |
| 34.10 — Provenance y freshness (L2226) | Herencia + Planned | M4-04/M8-03/M8-07 | SQLite facts/FTS5 y LanceDB derivado, offline snapshots/freshness/antirollback; D22 solo si facts nuevos; D15/16 opcionales | G2/G4–G7 |
| 34.11 — `rust.catalog.status` (L2280) | Herencia + Planned | M4-04/M8-03/M8-07 | SQLite facts/FTS5 y LanceDB derivado, offline snapshots/freshness/antirollback; D22 solo si facts nuevos; D15/16 opcionales | G2/G4–G7 |
| 34.12 — `rust.crate.search` (L2313) | Herencia + Planned | M4-04/M8-03/M8-07 | SQLite facts/FTS5 y LanceDB derivado, offline snapshots/freshness/antirollback; D22 solo si facts nuevos; D15/16 opcionales | G2/G4–G7 |
| 34.13 — `rust.crate.inspect` (L2349) | Herencia + Planned | M4-04/M8-03/M8-07 | SQLite facts/FTS5 y LanceDB derivado, offline snapshots/freshness/antirollback; D22 solo si facts nuevos; D15/16 opcionales | G2/G4–G7 |
| 34.14 — Layout local (L2372) | Herencia + Planned | M4-04/M8-03/M8-07 | SQLite facts/FTS5 y LanceDB derivado, offline snapshots/freshness/antirollback; D22 solo si facts nuevos; D15/16 opcionales | G2/G4–G7 |
| 34.15 — Integridad del índice vectorial (L2401) | Herencia + Planned | M4-04/M8-03/M8-07 | SQLite facts/FTS5 y LanceDB derivado, offline snapshots/freshness/antirollback; D22 solo si facts nuevos; D15/16 opcionales | G2/G4–G7 |
| 34.16 — Versionado del catálogo y migraciones (L2429) | Herencia + Planned | M4-04/M8-03/M8-07 | SQLite facts/FTS5 y LanceDB derivado, offline snapshots/freshness/antirollback; D22 solo si facts nuevos; D15/16 opcionales | G2/G4–G7 |
| 35 — Seguridad del servidor (L2466) | Herencia + Planned | M2-01/M3-01/M4-06/M7-03 | Threat model de cada nuevo efecto; native IO, gateway, egress, env, process tree, quotas; matriz seguridad | G2–G5/G8 |
| 36 — Modelo de amenaza (L2486) | Herencia + Planned | M2-01/M3-01/M4-06/M7-03 | Threat model de cada nuevo efecto; native IO, gateway, egress, env, process tree, quotas; matriz seguridad | G2–G5/G8 |
| 37 — Execution Gateway (L2510) | Herencia + Planned | M2-01/M3-01/M4-06/M7-03 | Threat model de cada nuevo efecto; native IO, gateway, egress, env, process tree, quotas; matriz seguridad | G2–G5/G8 |
| 38 — Política de comandos (L2540) | Herencia + Planned | M2-01/M3-01/M4-06/M7-03 | Threat model de cada nuevo efecto; native IO, gateway, egress, env, process tree, quotas; matriz seguridad | G2–G5/G8 |
| 39 — Política de filesystem (L2562) | Herencia + Planned | M2-01/M3-01/M4-06/M7-03 | Threat model de cada nuevo efecto; native IO, gateway, egress, env, process tree, quotas; matriz seguridad | G2–G5/G8 |
| 40 — Variables de entorno (L2594) | Herencia + Planned | M2-01/M3-01/M4-06/M7-03 | Threat model de cada nuevo efecto; native IO, gateway, egress, env, process tree, quotas; matriz seguridad | G2–G5/G8 |
| 41 — Red (L2628) | Herencia + Planned | M2-01/M3-01/M4-06/M7-03 | Threat model de cada nuevo efecto; native IO, gateway, egress, env, process tree, quotas; matriz seguridad | G2–G5/G8 |
| 42 — Sandboxing (L2668) | Herencia + Planned | M2-01/M3-01/M4-06/M7-03 | Threat model de cada nuevo efecto; native IO, gateway, egress, env, process tree, quotas; matriz seguridad | G2–G5/G8 |
| 43 — Ejecución de código (L2690) | Herencia + Planned | M2-01/M3-01/M4-06/M7-03 | Threat model de cada nuevo efecto; native IO, gateway, egress, env, process tree, quotas; matriz seguridad | G2–G5/G8 |
| 44 — Timeouts (L2715) | Herencia + Planned | M2-01/M3-01/M4-06/M7-03 | Threat model de cada nuevo efecto; native IO, gateway, egress, env, process tree, quotas; matriz seguridad | G2–G5/G8 |
| 45 — Output limits (L2735) | Herencia + Planned | M2-01/M3-01/M4-06/M7-03 | Threat model de cada nuevo efecto; native IO, gateway, egress, env, process tree, quotas; matriz seguridad | G2–G5/G8 |
| 46 — Configuración (L2763) | Herencia + Planned | M2-01/M3-01/M4-06/M7-03 | Threat model de cada nuevo efecto; native IO, gateway, egress, env, process tree, quotas; matriz seguridad | G2–G5/G8 |
| 47 — Jerarquía de configuración (L2816) | Herencia + Planned | M2-01/M3-01/M4-06/M7-03 | Threat model de cada nuevo efecto; native IO, gateway, egress, env, process tree, quotas; matriz seguridad | G2–G5/G8 |
| 48 — Quality Gates (L2849) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 48.1 — Fast (L2855) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 48.2 — Standard (L2867) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 48.3 — Strict (L2881) | Planned | M4-05 | Strict: etapas explícitas, no ampliar enum M1 sin decisión D19 | G1/G4–G6 |
| 48.4 — Release (L2897) | Planned | M4-05 | Release: baseline SemVer/deny/coverage según policy; incomplete bloquea | G1/G4–G6 |
| 49 — Resultados de quality gate (L2915) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 50 — Workflows para agentes (L2946) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 50.1 — Crear una implementación (L2950) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 50.2 — Resolver error de compilación (L2971) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 50.3 — Revisar dependencia nueva (L2992) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 50.4 — Investigar performance (L3015) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 51 — Long-running operations (L3036) | Planned | M3-01/M3-02 | JobExecutor; spike rmcp Tasks y fallback acotado explícito | G1/G3–G5 |
| 52 — Cache (L3065) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 53 — Estrategia de actualizaciones (L3103) | Herencia + Planned | M8-01..04/M8-06 | Censo, freeze, SemVer, matriz cliente/wire, CLI y upgrades; DTOs por milestone | G1/G4/G6–G9 |
| 53.1 — SemVer (L3109) | Herencia + Planned | M8-01..04/M8-06 | Censo, freeze, SemVer, matriz cliente/wire, CLI y upgrades; DTOs por milestone | G1/G4/G6–G9 |
| 54 — Versionado durante 0.x (L3137) | Herencia + Planned | M8-01..04/M8-06 | Censo, freeze, SemVer, matriz cliente/wire, CLI y upgrades; DTOs por milestone | G1/G4/G6–G9 |
| 55 — No usar `version` como parámetro obligatorio de cada tool (L3150) | Herencia + Planned | M8-01..04/M8-06 | Censo, freeze, SemVer, matriz cliente/wire, CLI y upgrades; DTOs por milestone | G1/G4/G6–G9 |
| 56 — Capability document (L3168) | Herencia + Planned | M8-01..04/M8-06 | Censo, freeze, SemVer, matriz cliente/wire, CLI y upgrades; DTOs por milestone | G1/G4/G6–G9 |
| 57 — Tool stability (L3222) | Herencia + Planned | M8-01..04/M8-06 | Censo, freeze, SemVer, matriz cliente/wire, CLI y upgrades; DTOs por milestone | G1/G4/G6–G9 |
| 58 — Deprecación (L3244) | Herencia + Planned | M8-01..04/M8-06 | Censo, freeze, SemVer, matriz cliente/wire, CLI y upgrades; DTOs por milestone | G1/G4/G6–G9 |
| 59 — Compatibilidad MCP (L3262) | Herencia + Planned | M8-01..04/M8-06 | Censo, freeze, SemVer, matriz cliente/wire, CLI y upgrades; DTOs por milestone | G1/G4/G6–G9 |
| 60 — Distribución (L3281) | Herencia + Planned | M8-07 | ADR-047/048; inventario/notices/SBOM/attestations/redescarga y decisión D13–D16; crates.io sigue off salvo nueva decisión | G6–G9 |
| 61 — GitHub Releases (L3294) | Herencia + Planned | M8-07 | ADR-047/048; inventario/notices/SBOM/attestations/redescarga y decisión D13–D16; crates.io sigue off salvo nueva decisión | G6–G9 |
| 62 — crates.io (L3322) | Herencia + Planned | M8-07 | ADR-047/048; inventario/notices/SBOM/attestations/redescarga y decisión D13–D16; crates.io sigue off salvo nueva decisión | G6–G9 |
| 63 — `cargo-binstall` (L3344) | Herencia + Planned | M8-07 | ADR-047/048; inventario/notices/SBOM/attestations/redescarga y decisión D13–D16; crates.io sigue off salvo nueva decisión | G6–G9 |
| 64 — Docker / containers (L3352) | Herencia + Planned | M8-07 | ADR-047/048; inventario/notices/SBOM/attestations/redescarga y decisión D13–D16; crates.io sigue off salvo nueva decisión | G6–G9 |
| 65 — UPX (L3374) | Preservar exclusión | M8-01 | Censo negativo: sin herramientas genéricas/UPX/consejos universales; scope review | G1/G8 |
| 66 — Firma y supply chain del propio MCP (L3390) | Herencia + Planned | M8-07 | ADR-047/048; inventario/notices/SBOM/attestations/redescarga y decisión D13–D16; crates.io sigue off salvo nueva decisión | G6–G9 |
| 67 — Observabilidad (L3423) | Herencia + Planned | M3-01/M5-05/M8-05 | Métricas locales por fase, quotas, startup/RSS/dispatch y raw samples; no generalizar M1-16 | G3–G5/G8 |
| 68 — Métricas locales (L3444) | Herencia + Planned | M3-01/M5-05/M8-05 | Métricas locales por fase, quotas, startup/RSS/dispatch y raw samples; no generalizar M1-16 | G3–G5/G8 |
| 69 — Error model (L3463) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 70 — Tool availability (L3535) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 71 — Dependency strategy del MCP (L3560) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 72 — `anyhow` vs `thiserror` (L3600) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 73 — Tipos de dominio (L3613) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 74 — Concurrencia (L3632) | Herencia + Planned | M2-01/M3-01/M4-06/M7-03 | Threat model de cada nuevo efecto; native IO, gateway, egress, env, process tree, quotas; matriz seguridad | G2–G5/G8 |
| 75 — Locks (L3661) | Planned | M2-01 | Lock por identidad física; exclusión real, crash y recovery; flock no basta | G2–G6 |
| 76 — Target directory aislado (L3684) | Herencia + Planned | M2-01/M3-01/M4-06/M7-03 | Threat model de cada nuevo efecto; native IO, gateway, egress, env, process tree, quotas; matriz seguridad | G2–G5/G8 |
| 77 — Gestión de artifacts (L3711) | Herencia + Planned | M3-01/M8-03 | Resource M1 congelado; disco privado versionado/TTL/quota/owner y migración | G2–G6 |
| 78 — Tamaño de contexto y agentes (L3736) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 79 — Machine readability primero (L3761) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 80 — Testing del MCP (L3775) | Herencia + Planned | M2-07/M3-06/M4-06/M5-05/M6-06/M7-06/M8-09 | Matriz unit/fixture/integration/contract/protocol/security y native positivo; recibos nuevos, skips no pass | G4/G5/G8/G9 |
| 80.1 — Unit tests (L3779) | Herencia + Planned | M2-07/M3-06/M4-06/M5-05/M6-06/M7-06/M8-09 | Matriz unit/fixture/integration/contract/protocol/security y native positivo; recibos nuevos, skips no pass | G4/G5/G8/G9 |
| 80.2 — Fixtures (L3792) | Herencia + Planned | M2-07/M3-06/M4-06/M5-05/M6-06/M7-06/M8-09 | Matriz unit/fixture/integration/contract/protocol/security y native positivo; recibos nuevos, skips no pass | G4/G5/G8/G9 |
| 80.3 — Integration tests (L3810) | Herencia + Planned | M2-07/M3-06/M4-06/M5-05/M6-06/M7-06/M8-09 | Matriz unit/fixture/integration/contract/protocol/security y native positivo; recibos nuevos, skips no pass | G4/G5/G8/G9 |
| 80.4 — Contract tests (L3828) | Herencia + Planned | M2-07/M3-06/M4-06/M5-05/M6-06/M7-06/M8-09 | Matriz unit/fixture/integration/contract/protocol/security y native positivo; recibos nuevos, skips no pass | G4/G5/G8/G9 |
| 80.5 — MCP protocol tests (L3845) | Herencia + Planned | M2-07/M3-06/M4-06/M5-05/M6-06/M7-06/M8-09 | Matriz unit/fixture/integration/contract/protocol/security y native positivo; recibos nuevos, skips no pass | G4/G5/G8/G9 |
| 81 — Security tests (L3862) | Herencia + Planned | M2-07/M3-06/M4-06/M5-05/M6-06/M7-06/M8-09 | Matriz unit/fixture/integration/contract/protocol/security y native positivo; recibos nuevos, skips no pass | G4/G5/G8/G9 |
| 82 — Performance del MCP (L3880) | Herencia + Planned | M3-01/M5-05/M8-05 | Métricas locales por fase, quotas, startup/RSS/dispatch y raw samples; no generalizar M1-16 | G3–G5/G8 |
| 83 — KPIs del producto (L3898) | Herencia + Planned | M3-01/M5-05/M8-05 | Métricas locales por fase, quotas, startup/RSS/dispatch y raw samples; no generalizar M1-16 | G3–G5/G8 |
| 84 — Diseño para múltiples agentes (L3939) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 85 — Descriptions de tools (L3965) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 86 — Naming de tools (L3988) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 87 — Estrategia de prompts MCP (L4025) | Suggested/Deferred | M8-01 | Intake de propuestas, no requisito de tool nueva M2–M8; documentar decisión/demanda antes de ampliar alcance | G1/G8/G9 |
| 87.1 — `review-rust-change` (L4031) | Suggested/Deferred | M8-01 | Intake de propuestas, no requisito de tool nueva M2–M8; documentar decisión/demanda antes de ampliar alcance | G1/G8/G9 |
| 87.2 — `prepare-rust-release` (L4046) | Suggested/Deferred | M8-01 | Intake de propuestas, no requisito de tool nueva M2–M8; documentar decisión/demanda antes de ampliar alcance | G1/G8/G9 |
| 87.3 — `investigate-rust-performance` (L4059) | Suggested/Deferred | M8-01 | Intake de propuestas, no requisito de tool nueva M2–M8; documentar decisión/demanda antes de ampliar alcance | G1/G8/G9 |
| 88 — Recursos MCP sugeridos (L4072) | Suggested/Deferred | M8-01 | Intake de propuestas, no requisito de tool nueva M2–M8; documentar decisión/demanda antes de ampliar alcance | G1/G8/G9 |
| 89 — Estrategia de conocimientos Rust (L4087) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 90 — Reglas propias que sí pueden tener valor (L4107) | Suggested/Deferred | M8-01 | Intake de propuestas, no requisito de tool nueva M2–M8; documentar decisión/demanda antes de ampliar alcance | G1/G8/G9 |
| 91 — Integración con políticas de proyecto (L4125) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 92 — Evitar recomendaciones universales de performance (L4149) | Preservar exclusión | M8-01 | Censo negativo: sin herramientas genéricas/UPX/consejos universales; scope review | G1/G8 |
| 93 — Performance profiles (L4166) | Suggested/Deferred | M8-01 | Intake de propuestas, no requisito de tool nueva M2–M8; documentar decisión/demanda antes de ampliar alcance | G1/G8/G9 |
| 94 — Estrategia MVP (L4198) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 95 — MVP 0.1.0 (L4219) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 96 — Lo que queda fuera del MVP (L4298) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 97 — Roadmap (L4321) | Planned/Conditional | M2-01..M8-09 | Registro de milestones debajo; sin release ficticia M7 | G1–G9 |
| 98 — ADRs iniciales (L4536) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 99 — Ejemplo de use case interno (L4565) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 100 — Adapter Cargo (L4591) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 101 — Adapter MCP (L4607) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 102 — Ejemplo conceptual de tool (L4623) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 103 — Cancelación (L4646) | Herencia + Planned | M2-01/M3-01/M4-06/M7-03 | Threat model de cada nuevo efecto; native IO, gateway, egress, env, process tree, quotas; matriz seguridad | G2–G5/G8 |
| 104 — Reproducibilidad (L4667) | Herencia + Planned | M2-01/M3-01/M4-06/M7-03 | Threat model de cada nuevo efecto; native IO, gateway, egress, env, process tree, quotas; matriz seguridad | G2–G5/G8 |
| 105 — Dependencias y lockfile (L4685) | Herencia + Planned | M2-01/M3-01/M4-06/M7-03 | Threat model de cada nuevo efecto; native IO, gateway, egress, env, process tree, quotas; matriz seguridad | G2–G5/G8 |
| 106 — Toolchain (L4704) | Herencia + Planned | M2-01/M3-01/M4-06/M7-03 | Threat model de cada nuevo efecto; native IO, gateway, egress, env, process tree, quotas; matriz seguridad | G2–G5/G8 |
| 107 — Tool installation (L4731) | Herencia + Planned | M2-01/M3-01/M4-06/M7-03 | Threat model de cada nuevo efecto; native IO, gateway, egress, env, process tree, quotas; matriz seguridad | G2–G5/G8 |
| 108 — Distribution bundles (L4760) | Herencia + Planned | M8-07 | ADR-047/048; inventario/notices/SBOM/attestations/redescarga y decisión D13–D16; crates.io sigue off salvo nueva decisión | G6–G9 |
| 109 — Self-update (L4790) | Suggested/Deferred | M8-01 | Intake de propuestas, no requisito de tool nueva M2–M8; documentar decisión/demanda antes de ampliar alcance | G1/G8/G9 |
| 110 — CLI (L4810) | Herencia + Planned | M8-01..04/M8-06 | Censo, freeze, SemVer, matriz cliente/wire, CLI y upgrades; DTOs por milestone | G1/G4/G6–G9 |
| 111 — `doctor` (L4836) | Herencia + Planned | M8-01..04/M8-06 | Censo, freeze, SemVer, matriz cliente/wire, CLI y upgrades; DTOs por milestone | G1/G4/G6–G9 |
| 112 — Estrategia de publicación (L4863) | Herencia + Planned | M8-07 | ADR-047/048; inventario/notices/SBOM/attestations/redescarga y decisión D13–D16; crates.io sigue off salvo nueva decisión | G6–G9 |
| 113 — Documentación mínima (L4893) | Herencia + Planned | M8-01..04/M8-06 | Censo, freeze, SemVer, matriz cliente/wire, CLI y upgrades; DTOs por milestone | G1/G4/G6–G9 |
| 114 — Ejemplo de configuración de un cliente MCP (L4909) | Herencia + Planned | M8-01..04/M8-06 | Censo, freeze, SemVer, matriz cliente/wire, CLI y upgrades; DTOs por milestone | G1/G4/G6–G9 |
| 115 — Diferenciador frente a simplemente permitir terminal (L4928) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 116 — Riesgos del proyecto (L4986) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 116.1 — Tool explosion (L4990) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 116.2 — Duplicar rust-analyzer (L5002) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 116.3 — Duplicar el razonamiento del agente (L5010) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 116.4 — Ejecución insegura (L5018) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 116.5 — Outputs gigantes (L5026) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 116.6 — Ecosistema externo cambiante (L5034) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 116.7 — Toolchains incompatibles (L5042) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 116.8 — Scope creep (L5050) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 116.9 — Peso de LanceDB y pipeline de embeddings (L5058) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 116.10 — Staleness del catálogo offline (L5075) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 117 — Criterios de aceptación del MVP (L5092) | Herencia + Planned | M2-07/M3-06/M4-06/M5-05/M6-06/M7-06/M8-09 | Matriz unit/fixture/integration/contract/protocol/security y native positivo; recibos nuevos, skips no pass | G4/G5/G8/G9 |
| 118 — Experimento de validación (L5121) | Hecho histórico acotado | M8-01/M8-08 | M1-16 saturado; no equivalencia ni causalidad; cualquier claim nuevo exige protocolo nuevo | G4/G8 |
| 119 — Evolución futura orientada a agentes (L5163) | Suggested/Deferred | M8-01 | Intake de propuestas, no requisito de tool nueva M2–M8; documentar decisión/demanda antes de ampliar alcance | G1/G8/G9 |
| 120 — Posibles integraciones futuras (L5189) | Suggested/Deferred | M8-01 | Intake de propuestas, no requisito de tool nueva M2–M8; documentar decisión/demanda antes de ampliar alcance | G1/G8/G9 |
| 121 — Recomendación final (L5211) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 122 — Decisión recomendada de alcance inmediato (L5240) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 123 — Referencias técnicas verificadas (L5287) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
| 124 — Conclusión (L5320) | Herencia/guía + mantenimiento | M8-01; cierre de cada milestone | Conservar semántica M1 según ADRs y matriz de cierre; censo contrato/código y regresión ante cada adición | G1–G9 |
