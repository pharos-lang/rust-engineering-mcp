# ADR-089 — Registro de riesgos residuales 1.0

Date: 2026-09-14

## Context

El checklist 1.0 de
[`docs/roadmap/m8-stabilization.md`](../roadmap/m8-stabilization.md) exige dos
casillas de seguridad: «Security model coincide con enforcement y cada
capability tiene oráculo nativo; auditoría independiente sin P0/P1» y
«Registro de riesgos residuales con ADR de aceptación, alcance, condición de
reevaluación y re-review M8-08; security model público los enumera». G2
([`docs/roadmap/m2-m8.md`](../roadmap/m2-m8.md)) pide para cada corte actores,
activos, fronteras, abuso, control, oráculo y riesgo residual; el plan añade
que una revisión automática no se denomina auditoría humana.

El threat model de M8-08
([`08-threat-model.md`](../validation/M8/08-threat-model.md)) modela 0.8.0 sobre
el único host positivo de [ADR-087](ADR-087-1.0-host-scope.md): 8 fronteras y
53 controles citados, de los que 31 tienen oráculo nativo, 12 unit/contract,
4 evidencia histórica y 6 ninguno. Ninguna capability positiva del security
model carece de oráculo; los huecos son propiedades declaradas como no
garantizadas. Ese análisis produce 18 riesgos residuales.

El owner autorizó en sesión el 2026-09-14 continuar M8 asumiendo las decisiones
necesarias para una primera versión estable en macOS. Bajo esa autorización el
orquestador (Claude Fable 5.1) registra aquí la aceptación.

## Decision

1. **Se aceptan los riesgos RR-01 … RR-18** con el alcance, la severidad y la
   condición de reevaluación de la tabla siguiente, para la primera versión
   estable en macOS ARM64 con gateway Docker Linux ARM64. La aceptación no
   amplía ninguna capability ni convierte una limitación en garantía.
2. **La auditoría independiente de 1.0 es una revisión de modelo** (Claude
   Opus 5, High, solo lectura, alcance declarado sobre el threat model, el
   security model y el código). **No es una auditoría humana ni un pentest**, y
   ningún documento la presentará así (RR-01).
3. **Re-review obligatoria en M8-08.** Tras la auditoría independiente y sus
   correcciones, M8-08 repite la revisión de este registro: cada `RR-n` se
   confirma, se reclasifica o se cierra con evidencia, y cualquier P0/P1 nuevo
   bloquea readiness en lugar de añadirse como riesgo aceptado. Cualquier P2 de
   seguridad sigue la regla de bug bar del plan (bloquea readiness salvo
   disposición del owner).
4. **Una condición de reevaluación cumplida suspende la aceptación** de ese
   riesgo hasta un ADR que lo reevalúe. Anunciar un host adicional, un
   transporte remoto o un catálogo/modelo oficial activa a la vez RR-01 y los
   riesgos de su alcance.
5. `docs/security-model.md` enumera estos identificadores en la sección
   «Riesgos residuales 1.0» y enlaza este ADR y el threat model.

### Registro

| ID | Riesgo aceptado | Severidad | Alcance | Mitigación existente | Condición de reevaluación |
| --- | --- | --- | --- | --- | --- |
| RR-01 | La auditoría independiente es una revisión de modelo (Opus 5 High, read-only), no una auditoría humana ni un pentest; puede omitir clases de fallo que un pentest encontraría | Media | Todo el producto 1.0 | Threat model con citas, oráculos nativos por hito, revisiones independientes previas (matrices M1–M8) | Antes de anunciar un host distinto de macOS ARM64, un transporte remoto (M7) o un catálogo/modelo oficial (D15/D16); ante un P0/P1 reportado tras la release; si el owner contrata una auditoría humana |
| RR-02 | Linux x86_64 y Windows x86_64 no calificados; Linux ARM64 y macOS x86_64 no anunciados ([ADR-087](ADR-087-1.0-host-scope.md)) | Baja | Hosts no macOS | Adapters fallan cerrados; CI de portabilidad; sin artifact | Subprograma D13 con adapter no-follow/reparse-safe, oráculos G4 nativos, host real y ADR sucesor de ADR-087 |
| RR-03 | Deuda M6 de las 5 tools analyzer `preview`: `SANDBOX_DENIED` mezcla rechazo permanente y capacidad transitoria; diagnósticos solo de sintaxis; assists no deterministas; precisión de `admitted` en el audit tras denegación post-grant; apply no verificado por compilación; e2e `analyzer_runtime.rs` desgateado; ausencia de procesos por muestreo ([matriz M6 §Deuda](../validation/M6/matrix.md)) | Media | `rust.analyzer.*` | Clase `preview` ([ADR-086](ADR-086-deprecation-and-freeze-policy.md)); 12 cortes nativos M6; writer M2; `ACTION_STALE` | Antes de promover cualquiera de las cinco a `stable`, o de habilitar diagnósticos semánticos (Opción B de ADR-084) |
| RR-04 | `rust.dependencies.audit` degrada (no bloquea) ante snapshot RustSec stale o de edad desconocida ([ADR-088](ADR-088-migration-rollback-policy.md) §5) | Media | Audit y gates que lo componen | `AuditIssue::SnapshotStale`/`SnapshotUnknownAge` explícitos; freshness y provenance en el resultado | Siguiente major que permita cambiar el contrato M1, o evidencia de un cliente calificado que trate un audit stale como limpio |
| RR-05 | Kernel LinuxKit, runc, Docker Desktop y el daemon están en la TCB; un 0-day de esas piezas escaparía del guest. Deltas de seccomp aceptados: `socketpair` AF_UNIX stream (quality), `socket` AF_INET stream para loopback con `--network=none` (fix), `perf_event_open` en una fase (profiling) | Media | Toda ejecución de código del proyecto | Seccomp deny-default verificado por fase, sin red, `--cap-drop=ALL`, `no-new-privileges`, rootfs read-only, límites de PIDs/memoria/CPU, cuarentena | Advisory que afecte a runc, seccomp, Docker Desktop o el kernel del guest; cambio de imagen aprobada o de perfil (exige recalibración) |
| RR-06 | Deadlines cooperativos y sin límite duro de RSS/CPU en el proceso host (catálogo, ORT, RustSec, parsers, encoding) | Baja | Proceso servidor | Caps de bytes/entradas/resultados, workers unidos, admisión de 16 | Transporte remoto o multi-tenant; medición M8-05 o soak M8-09 fuera de budget |
| RR-07 | Sin detección universal de secretos: el source concedido y sus secretos se retienen en artifacts privados, logs y diffs; `assert_no_credentials` detecta por nombre de archivo, no por contenido; no hay secret scanning en CI | Media | Artifacts, logs, evidencia de validación | Redacción literal, normalización M4, canarios, `.gitignore`, eventos sin paths | Publicar evidencia generada sobre repositorios de terceros; habilitar remoto o telemetría |
| RR-08 | Otros procesos del mismo uid y un host malicioso están fuera de la frontera; ACLs no inspeccionadas; el dueño que restaura o borra todo el estado reinicia el floor | Media | State root, trust, artifacts, checkout | `0700`/`0600`, uid efectivo, `nlink == 1`, binding owner, floor separado | M7 (multi-tenant/remoto) o cambio del modelo de permisos/ACL de macOS |
| RR-09 | Mutación `local_coordinated` ([ADR-050](ADR-050-local-coordinated-mutation.md)): sin CAS ni exclusión OS de editores, sin atomicidad multiarchivo visible, power loss no demostrado (ENOSPC inyectado); un journal corrupto bloquea el store compartido | Media | 6 tools de escritura | Journal versionado, revalidación de identidad y bytes, recovery explícito, `doctor.mutation_journals` | Nuevo adapter de host, pérdida de datos reportada o cambio de semántica de APFS |
| RR-10 | Una dependencia comprometida sin advisory publicado no se detecta; los `build.rs` de dependencias corren en CI y en el host de desarrollo; `paste 1.0.15` unmaintained | Media | Build, CI y binario | `cargo audit`/`deny` (CI con fetch), pins `=`, `--locked`, vendor por SHA-256, imágenes por digest, CODEOWNERS | Tarea post-M8 de paquetería; cualquier advisory RUSTSEC que afecte al lock; cambio de `deny.toml` |
| RR-11 | La firma Ed25519 autentica al publisher que eligió el host, no la corrección de los facts; no hay trust root ni catálogo oficial; el E5 fijado no se audita | Baja | Catálogo y búsqueda semántica | Firma antes del parsing, floor, SHA-256 E5, SQLite autoritativo, LanceDB derivado | Decisiones D15/D16 de distribución de catálogo o modelo |
| RR-12 | Publicación: la attestation OIDC acredita el workflow, no reproducibilidad; branch protection observada en M1 y no re-verificada (incluye el check de Windows retirado); `SONAR_TOKEN` es un secreto de larga vida; verificación offline (D14) pendiente; provenance 0.8.x no ejecutada | Media | Artifacts y repositorio público | OIDC sin clave, `gh attestation verify` con signer exacto, permisos mínimos, acciones por SHA, guard de forks, CODEOWNERS, solo draft | M8-07/D14 antes de RC1; cualquier cambio de workflows o de protección |
| RR-13 | Resultados producidos por código del proyecto (tests, lints, benchmarks, harness) no están autenticados | Baja | Tools que ejecutan código | Clasificación conservadora, tests de forgery, tamaño solicitado vs observado | Si un contrato empezara a afirmar autenticidad de esos resultados |
| RR-14 | Límites del filesystem macOS: abrir FIFO/device node puede tener efectos, ACL conservada por `CLONE_ACL` sin comparar, hardlinks por `nlink`, captura no atómica ([ADR-024](ADR-024-project-open.md)) | Baja | Captura y writer | `NOFOLLOW_ANY | RESOLVE_BENEATH`, rechazo de links, detección de cambios | Nueva versión mayor de macOS o de APFS |
| RR-15 | Sin revocación en caliente de grants; un `kill -9` del servidor deja contenedores/volúmenes etiquetados | Baja | Grants y jobs | Reinicio, cleanup unido, cuarentena, etiquetas | Transporte remoto; residuos observados en el soak M8-09 |
| RR-16 | Retención y borrado de journals, backups y logs dependen del operador; no hay borrado seguro en disco ni en RAM | Baja | State root, backups, stderr | TTL y cuotas de artifacts, `prune` explícito, permisos privados | Requisito de cumplimiento/privacidad o transporte remoto |
| RR-17 | Brechas de oráculo en gates obligatorios: rollback con dos binarios fuera de `core`/`full`, e2e analyzer desgateado, power loss sin oráculo | Baja | Calificación | Oráculos manuales con recibo (`03-rollback.json`, cortes M6) | Cierre M8-09: dos RC consecutivos |
| RR-18 | `rmcp` 3.2.0 forma parte de la TCB del protocolo: puede citar campos del request en errores y retiene permisos de cancelaciones suprimidas hasta reconectar | Baja | stdio | Pin `=`, admisión propia, tests en cinco revisiones MCP | Cualquier subida de `rmcp` |

## Alternatives considered

- **Contratar una auditoría humana o un pentest antes de 1.0.** Descartada por
  decisión de alcance de la primera versión estable en macOS. No se descarta
  para el futuro: es la condición de reevaluación de RR-01 y pasa a ser
  obligatoria antes de anunciar otro host, remoto o un catálogo/modelo oficial.
- **No publicar 1.0** hasta cerrar todos los riesgos. Descartada: RR-02, RR-05,
  RR-08 y RR-13 son límites estructurales del alcance local y no se cierran con
  más trabajo en M8; posponer 1.0 no reduciría el riesgo de los usuarios de 0.x,
  que ya operan con esos mismos límites.
- **Tratar la deuda M6 como bloqueante.** Descartada: las cinco tools son
  `preview` bajo ADR-086, sin compromiso de estabilidad, y su containment tiene
  oráculo nativo; la deuda es de calidad y precisión, no de contención.

## Consequences

- `docs/security-model.md` gana «Riesgos residuales 1.0» con RR-01 … RR-18.
- La casilla «Registro de riesgos residuales…» del checklist 1.0 queda con
  ADR de aceptación; se marca solo cuando M8-08 complete la re-review del
  punto 3 con receipt en la matriz M8.
- La casilla «auditoría independiente sin P0/P1» se cumple, como máximo, con
  la revisión de modelo del punto 2; la documentación pública no la llamará
  auditoría humana.
- M8-07 hereda RR-12 y el defecto de
  `.github/workflows/release-candidate.yml:219` (exige 31 tools; el smoke
  publica 36).
- Anunciar un host, un transporte remoto o un catálogo/modelo oficial requiere
  un ADR que reevalúe como mínimo RR-01, RR-02, RR-05, RR-06, RR-08 y RR-11.

## Status

Accepted.

## Sources

- [`docs/validation/M8/08-threat-model.md`](../validation/M8/08-threat-model.md)
  — fronteras, controles con citas, oráculos y derivación de cada `RR-n`.
- [`docs/security-model.md`](../security-model.md).
- [`docs/roadmap/m8-stabilization.md`](../roadmap/m8-stabilization.md) §M8-08,
  §«Migración, recovery y seguridad», §«Bug bars y gates» y checklist 1.0.
- [`docs/roadmap/m2-m8.md`](../roadmap/m2-m8.md) G2/G3.
- [ADR-086](ADR-086-deprecation-and-freeze-policy.md),
  [ADR-087](ADR-087-1.0-host-scope.md),
  [ADR-088](ADR-088-migration-rollback-policy.md) §5.
- [`docs/validation/M6/matrix.md`](../validation/M6/matrix.md) §Deuda de M6.
