# Backlog de decisiones M2–M8

Estado global: **Proposed**. Ninguna entrada es un ADR Accepted. Los IDs D01–D26 son IDs de planificación, no números ADR reservados. El Technical Owner decide y crea el ADR real durante la implementación, antes del primer corte dependiente. “Fecha límite” significa una puerta del roadmap, no una fecha de calendario. G1–G9 del [maestro](m2-m8.md) y el milestone enlazado fijan los gates.

## D01 — Autoridad de mutación y DTOs

- **Status:** Proposed.
- **Context:** Autoridad de mutación y DTOs amplía o concreta una frontera heredada de M1; véanse [baseline](baseline-2026-09-05.md) y [trazabilidad](traceability-m2-m8.md).
- **Decision propuesta:** Separar preview, commit y receipt con grants host, plan digest e idempotencia; receipt exige grant vigente y mismo principal host tras invalidación del ProjectRef; definir payload interno de byte edits sin editor público genérico.
- **Alternatives considered:** Tres modos en cada tool; plan Resource más commit; receipt CLI.
- **Consequences:** Amplía estado y superficie pública; no aceptar IDs como credencial.
- **Fecha límite:** M2-01.
- **Evidencia para decidir:** Snapshots de trece tools intactos; pérdida de respuesta, replay y reautorización.

## D02 — Publicación, exclusión y recuperación

- **Status:** Proposed.
- **Context:** Publicación, exclusión y recuperación amplía o concreta una frontera heredada de M1; véanse [baseline](baseline-2026-09-05.md) y [trazabilidad](traceability-m2-m8.md).
- **Decision propuesta:** Resolver root-relative escritura bajo rename concurrente y exclusión real; journal durable no equivale a atomicidad multiarchivo.
- **Alternatives considered:** Namespace exclusivo controlado por host; primitive kernel demostrable; rechazo de escrituras sin garantía.
- **Consequences:** Bloquea cualquier commit M2 hasta prueba positiva; hash preflight y flock solos son insuficientes.
- **Fecha límite:** puerta Go/No-go D02 antes de M2-01; No-go bloquea M2 y dependientes, sin degradación de seguridad.
- **Evidencia para decidir:** APFS native race/crash/disk-full/rollback; demostrar que no hay overwrite ajeno ni escape por parent movido. Contingencia: broker/identidad host con namespace exclusivo probado; reducir garantías exige owner y cambio explícito ADR/spec, nunca solo lock de servidor.

## D03 — Editor TOML y semántica Cargo

- **Status:** Proposed.
- **Context:** Editor TOML y semántica Cargo amplía o concreta una frontera heredada de M1; véanse [baseline](baseline-2026-09-05.md) y [trazabilidad](traceability-m2-m8.md).
- **Decision propuesta:** Operaciones tagged, preservación de comentarios, selección package/kind e interpretación de workspace.
- **Alternatives considered:** toml_edit fijado; editor propio reducido; Cargo add como generador aislado.
- **Consequences:** Dependencia estratégica y gramática pública nuevas; no JSON Pointer libre.
- **Fecha límite:** M2-01; ampliar antes M2-04.
- **Evidencia para decidir:** Corpus comentarios/alias/target/inheritance/features/lints/profiles y Cargo oracle.

## D04 — Staging guest y exportación

- **Status:** Proposed.
- **Context:** Staging guest y exportación amplía o concreta una frontera heredada de M1; véanse [baseline](baseline-2026-09-05.md) y [trazabilidad](traceability-m2-m8.md).
- **Decision propuesta:** Copia source escribible con cuota real; exportar solo después de terminar mutadores; validar scope en host.
- **Alternatives considered:** tmpfs acotado; volumen con quota demostrada; export byte protocol frente tar.
- **Consequences:** Reemplaza source RO solo dentro de staging; nunca host bind W.
- **Fecha límite:** M2-02.
- **Evidencia para decidir:** Gate Docker recalibrado, extra files/links/races/output floods, cancel/cleanup.

## D05 — Resolución offline y lockfile

- **Status:** Proposed.
- **Context:** Resolución offline y lockfile amplía o concreta una frontera heredada de M1; véanse [baseline](baseline-2026-09-05.md) y [trazabilidad](traceability-m2-m8.md).
- **Decision propuesta:** Cache/toolchain explícitos host, config construida desde allowlist tipada sin wrappers/runners/linkers/net/http/credentials y hash por job, resolución offline y commit coherente manifest+lock.
- **Alternatives considered:** Snapshot Cargo registry autorizado; vendor cerrado; limitar disponibilidad a datos existentes.
- **Consequences:** Nueva distribución opcional de assets; prohibida adquisición por runtime.
- **Fecha límite:** M2-04.
- **Evidencia para decidir:** Dependencia registry real, cache missing, checksum mismatch, app/lib/workspace, no egress.

## D06 — Task execution común

- **Status:** Proposed.
- **Context:** Task execution común amplía o concreta una frontera heredada de M1; véanse [baseline](baseline-2026-09-05.md) y [trazabilidad](traceability-m2-m8.md).
- **Decision propuesta:** JobExecutor neutral y lifecycle unido; spike Tasks rmcp exacto antes de prometer protocolo.
- **Alternatives considered:** Tasks negociadas; síncrono acotado; polling application sin stack JSON-RPC propio.
- **Consequences:** Persistencia de jobs no implica resumir procesos después de crash.
- **Fecha límite:** M3-01; wire antes M3-02.
- **Evidencia para decidir:** Negociación cinco versiones, cancel race, EOF, IDs ajenos, cleanup y admission slots.

## D07 — Transporte remoto

- **Status:** Proposed.
- **Context:** Transporte remoto amplía o concreta una frontera heredada de M1; véanse [baseline](baseline-2026-09-05.md) y [trazabilidad](traceability-m2-m8.md).
- **Decision propuesta:** Elegir Streamable HTTP rmcp conforme versión vigente después del caso real.
- **Alternatives considered:** Continuar stdio; túnel gestionado; servicio HTTP.
- **Consequences:** No endpoint ni dependencia HTTP si Deferred.
- **Fecha límite:** M7-01, solo después G0 Go.
- **Evidencia para decidir:** Caso aprobado, amenaza TLS/Origin/Host/sessions y wire real.

## D08 — Identity, tenancy y grants

- **Status:** Proposed.
- **Context:** Identity, tenancy y grants amplía o concreta una frontera heredada de M1; véanse [baseline](baseline-2026-09-05.md) y [trazabilidad](traceability-m2-m8.md).
- **Decision propuesta:** Identidad validada, actor/tenant/grants y revocación controlados por host.
- **Alternatives considered:** Un tenant dedicado; multi-tenant; IdP empresarial compatible.
- **Consequences:** Identificador de sesión o project ID nunca es autorización.
- **Fecha límite:** M7-02.
- **Evidencia para decidir:** Issuer/audience/expiry/revocation/cross-tenant tests y privacy review.

## D09 — Ingress y executor remoto

- **Status:** Proposed.
- **Context:** Ingress y executor remoto amplía o concreta una frontera heredada de M1; véanse [baseline](baseline-2026-09-05.md) y [trazabilidad](traceability-m2-m8.md).
- **Decision propuesta:** Proyecto preaprovisionado y sandbox remoto independiente de transporte.
- **Alternatives considered:** Workers dedicados; pool aislado; continuar local.
- **Consequences:** No reuse del host Docker local como prueba suficiente remota.
- **Fecha límite:** M7-03.
- **Evidencia para decidir:** Remote escape/egress/secrets/resource boundary y cleanup en target exacto.

## D10 — Cuotas y operación remota

- **Status:** Proposed.
- **Context:** Cuotas y operación remota amplía o concreta una frontera heredada de M1; véanse [baseline](baseline-2026-09-05.md) y [trazabilidad](traceability-m2-m8.md).
- **Decision propuesta:** Admisión justa, budgets por tenant, SLI/SLO y failure policy derivados del caso.
- **Alternatives considered:** Cuotas dedicadas; scheduler compartido; capacidad fija sin queue.
- **Consequences:** Coste operacional y responsabilidad definidos antes de aceptar tenant.
- **Fecha límite:** M7-04.
- **Evidencia para decidir:** Load/starvation/outage/incident/restore con tiempos y bytes medidos.

## D11 — Freeze y SemVer

- **Status:** Proposed.
- **Context:** Freeze y SemVer amplía o concreta una frontera heredada de M1; véanse [baseline](baseline-2026-09-05.md) y [trazabilidad](traceability-m2-m8.md).
- **Decision propuesta:** Inventariar contratos y fijar ventanas de deprecación/0.x→1.0 antes de cambios.
- **Alternatives considered:** Compatibilidad estricta; opt-in versionado; ruptura explícita en 0.8.
- **Consequences:** Ni campo opcional es automáticamente compatible con clientes cerrados.
- **Fecha límite:** M8-01.
- **Evidencia para decidir:** Censo clientes, schemas/enums/defaults/CLI, migration guides y dos RC iguales.

## D12 — Migraciones y rollback

- **Status:** Proposed.
- **Context:** Migraciones y rollback amplía o concreta una frontera heredada de M1; véanse [baseline](baseline-2026-09-05.md) y [trazabilidad](traceability-m2-m8.md).
- **Decision propuesta:** Formatos versionados, preflight/backup y upgrade/rollback explícitos.
- **Alternatives considered:** Migración side-by-side; reconstrucción de derivados; export/import.
- **Consequences:** No downgrade de floors, grants ni transaction state para recuperar servicio.
- **Fecha límite:** M2-01 para formatos journal/receipt y bloqueo de downgrade gestionado; M8-03 para migraciones generales.
- **Evidencia para decidir:** Unknown version, crash, disk full, antirollback floor y journal pendiente.

## D13 — Calificación por target para 1.0

- **Status:** Proposed.
- **Context:** Calificación por target para 1.0 amplía o concreta una frontera heredada de M1; véanse [baseline](baseline-2026-09-05.md) y [trazabilidad](traceability-m2-m8.md).
- **Decision propuesta:** Resolver aspiración cross-platform con native positive y artifacts por familia o cambio explícito de alcance.
- **Alternatives considered:** Linux/macOS/Windows positivos; subconjunto justificado con ADR/spec actualizada.
- **Consequences:** CI portable no es soporte funcional; sin decisión no hay readiness 1.0.
- **Fecha límite:** M8-07; alcance antes M8-02.
- **Evidencia para decidir:** Native FS/sandbox/cancel/network tests más archive install smoke por target.

## D14 — Distribución y provenance

- **Status:** Proposed.
- **Context:** Distribución y provenance amplía o concreta una frontera heredada de M1; véanse [baseline](baseline-2026-09-05.md) y [trazabilidad](traceability-m2-m8.md).
- **Decision propuesta:** Conservar OIDC source/tag/workflow/run/digest y añadir incident/rotation/offline policy verificables.
- **Alternatives considered:** OIDC+bundles verificables; keys offline si necesidad; no publicación adicional.
- **Consequences:** No exigir HSM arbitrario ni confundir tag sin firma con assets sin provenance.
- **Fecha límite:** M8-07.
- **Evidencia para decidir:** Redescarga/verificación, dependencia comprometida, rollback y revocation drills.

## D15 — Catálogo oficial y trust

- **Status:** Proposed.
- **Context:** Catálogo oficial y trust amplía o concreta una frontera heredada de M1; véanse [baseline](baseline-2026-09-05.md) y [trazabilidad](traceability-m2-m8.md).
- **Decision propuesta:** Solo por demanda aprobar publisher, política de keys/freshness/revocation/licencias.
- **Alternatives considered:** Snapshots del host actuales; publisher oficial; mirror empresarial.
- **Consequences:** No es prerrequisito inventado para M2 ni promesa de catálogo distribuido.
- **Fecha límite:** Antes distribuir catálogo oficial; opcional M8-07.
- **Evidencia para decidir:** Signed import/antirollback/poisoning/rotation y procedencia/licencias por dataset.

## D16 — Distribución E5/ORT/LanceDB

- **Status:** Proposed.
- **Context:** Distribución E5/ORT/LanceDB amplía o concreta una frontera heredada de M1; véanse [baseline](baseline-2026-09-05.md) y [trazabilidad](traceability-m2-m8.md).
- **Decision propuesta:** Decidir assets/model licenses/ABI/size/update y target exacto antes de empaquetar perfil local.
- **Alternatives considered:** Core actual; bundle separado; instalación administrativa.
- **Consequences:** Índice reconstruible, SQLite autoritativo; no descarga implícita.
- **Fecha límite:** Antes bundle local; opcional M8-07.
- **Evidencia para decidir:** Notices/SBOM/model hash/ABI/search oracle y smoke offline.

## D17 — Artifacts avanzados persistentes

- **Status:** Proposed.
- **Context:** Artifacts avanzados persistentes amplía o concreta una frontera heredada de M1; véanse [baseline](baseline-2026-09-05.md) y [trazabilidad](traceability-m2-m8.md).
- **Decision propuesta:** Store privado owner-bound con formato, quotas, TTL, autorización y recuperación.
- **Alternatives considered:** Memoria ampliada; disco privado; Resources paginados.
- **Consequences:** No ampliar memoria sin presupuesto; journals M2 nunca se evictan por TTL.
- **Fecha límite:** M3-01.
- **Evidencia para decidir:** Quota eviction/TTL/secret/owner/path/crash/native IO y compatibilidad URI M1.

## D18 — Cobertura y baseline SemVer

- **Status:** Proposed.
- **Context:** Cobertura y baseline SemVer amplía o concreta una frontera heredada de M1; véanse [baseline](baseline-2026-09-05.md) y [trazabilidad](traceability-m2-m8.md).
- **Decision propuesta:** Formato/métricas explícitos, merge compatible y dos snapshots autorizados.
- **Alternatives considered:** LLVM/LCOV/HTML; baseline ProjectRef; artifact baseline firmado por owner.
- **Consequences:** Partial/unsupported/no baseline no produce pass.
- **Fecha límite:** M3-03/M3-04.
- **Evidencia para decidir:** Oráculos LLVM y API breaking fixtures, mismatch/source/provenance.

## D19 — Preguntas de seguridad y quality profiles

- **Status:** Proposed.
- **Context:** Preguntas de seguridad y quality profiles amplía o concreta una frontera heredada de M1; véanse [baseline](baseline-2026-09-05.md) y [trazabilidad](traceability-m2-m8.md).
- **Decision propuesta:** Compartir audit RustSec; deny agrega licenses/bans/sources; strict/release agregan etapas explícitas.
- **Alternatives considered:** Componer port audit existente; delegar todo a deny; profiles nuevos aislados.
- **Consequences:** Nuevos profiles podrían requerir nuevo contrato versionado; decidir antes de ampliar enum M1.
- **Fecha límite:** M4-01; profiles antes M4-05.
- **Evidencia para decidir:** Mismo advisory sin doble conteo, supresión expiry/reason/owner y enums M1 intactos.

## D20 — Unsafe scan

- **Status:** Proposed.
- **Context:** Unsafe scan amplía o concreta una frontera heredada de M1; véanse [baseline](baseline-2026-09-05.md) y [trazabilidad](traceability-m2-m8.md).
- **Decision propuesta:** Scanner sintáctico con spans y cobertura de cfg/macros/origen declarada.
- **Alternatives considered:** Parser Rust fijado; geiger; tooling compiler.
- **Consequences:** Conteo no demuestra ausencia de UB ni seguridad global.
- **Fecha límite:** M4-02.
- **Evidencia para decidir:** Strings/comments/unsafe blocks/functions/extern/cfg/macro fixtures.

## D21 — Miri y hardening

- **Status:** Proposed.
- **Context:** Miri y hardening amplía o concreta una frontera heredada de M1; véanse [baseline](baseline-2026-09-05.md) y [trazabilidad](traceability-m2-m8.md).
- **Decision propuesta:** Nightly/Miri/sysroot exactos, gateway y recalibración de containment.
- **Alternatives considered:** Runtime separado; imagen derivada aprobada; unavailable si no calificado.
- **Consequences:** No toolchain channel moving ni privilegios implícitos.
- **Fecha límite:** M4-03/M4-06.
- **Evidencia para decidir:** UB/unsupported control real; active cancellation; escape/egress/secret tests.

## D22 — Persistencia de facts enriquecidos

- **Status:** Proposed.
- **Context:** Persistencia de facts enriquecidos amplía o concreta una frontera heredada de M1; véanse [baseline](baseline-2026-09-05.md) y [trazabilidad](traceability-m2-m8.md).
- **Decision propuesta:** Añadir solo hechos con fuente verificable y migración SQLite explícita.
- **Alternatives considered:** Facts actuales con unknown; schema nuevo; snapshot separado por fuente.
- **Consequences:** No inferir hecho desde búsqueda vectorial o score.
- **Fecha límite:** Antes facts nuevos en M4-04 o migración M8-03.
- **Evidencia para decidir:** Yanked/checksum/source/provenance truth oracle y migration rollback.

## D23 — Método de benchmark y size report

- **Status:** Proposed.
- **Context:** Método de benchmark y size report amplía o concreta una frontera heredada de M1; véanse [baseline](baseline-2026-09-05.md) y [trazabilidad](traceability-m2-m8.md).
- **Decision propuesta:** Congelar protocolo estadístico, baseline identity y dataset versionado.
- **Alternatives considered:** Criterion; harness libtest; report externo preprovisionado.
- **Consequences:** Tres repeticiones no prueban causalidad ni generalización.
- **Fecha límite:** M5-01/M5-04.
- **Evidencia para decidir:** Raw samples, noise/MDR/CI/units/stripped-LTO y self comparison.

## D24 — Profiling y privilegios

- **Status:** Proposed.
- **Context:** Profiling y privilegios amplía o concreta una frontera heredada de M1; véanse [baseline](baseline-2026-09-05.md) y [trazabilidad](traceability-m2-m8.md).
- **Decision propuesta:** Capability explícita con mínimo privilegio y target realmente probado.
- **Alternatives considered:** Profiler en sandbox; servicio host estrecho; denegar plataforma sin enforcement.
- **Consequences:** Denegación sola no termina feature normativa flamegraph.
- **Fecha límite:** M5-03.
- **Evidencia para decidir:** Stack workload conocido, permisos negados, cancel y artifacts no ejecutables.

## D25 — Contrato analyzer y acciones

- **Status:** Proposed.
- **Context:** Contrato analyzer y acciones amplía o concreta una frontera heredada de M1; véanse [baseline](baseline-2026-09-05.md) y [trazabilidad](traceability-m2-m8.md).
- **Decision propuesta:** Symbols/references/diagnostics/actions tipados; apply reutiliza MutationPlan M2.
- **Alternatives considered:** Cinco tools propuestas; actions como Resource; aplicar mediante modo tipado.
- **Consequences:** No segundo writer ni ampliar a rename/hover sin scope explícito.
- **Fecha límite:** M6-01; acciones antes M6-04.
- **Evidencia para decidir:** Snapshots, Unicode/overlap/URI externa/Command/resource ops, M2 crash/replay.

## D26 — Runtime rust-analyzer y LSP

- **Status:** Proposed.
- **Context:** Runtime rust-analyzer y LSP amplía o concreta una frontera heredada de M1; véanse [baseline](baseline-2026-09-05.md) y [trazabilidad](traceability-m2-m8.md).
- **Decision propuesta:** Versión exacta, trust, sync/readiness/cancel y bounded subprocess a través gateway.
- **Alternatives considered:** Instancia por snapshot; pool acotado por identidad; análisis sin build scripts/proc macros.
- **Consequences:** LSP interno no es stack MCP paralelo; invalidación obligatoria.
- **Fecha límite:** M6-01.
- **Evidencia para decidir:** Config hostil, nunca ready, stale diagnostics, UTF encodings, flood y teardown.
