# M3 — matriz de validación

Fecha: 2026-09-06. Estado: **M3-01..06 calificados localmente; G1..G9
satisfechos para el alcance local, sin release**.
W6 pasó `check-architecture` (exit 0), el core **14/14** y el full **25/25** sobre
el mismo inventario de 801 inputs, con `source_inputs_unchanged: true` en ambos.
`audit-data` —el bloqueo reproducible de W5— pasó en 43.228 s fuera del sandbox
administrado. El runtime Docker M3 pasa **62/62** y rust-security **20/20**.
Los re-reviews independientes confirmaron que no quedan hallazgos bloqueantes;
los residuos aceptados permanecen registrados abajo.

| Elemento | Estado | Evidencia y límite |
| --- | --- | --- |
| M3-01 nextest y artifacts privados | Done (qualified) | Contrato nextest/five-version, runtime 19/19 y security 20/20 pasan; Stage 1 degrada por miembro y sus Resources responden sin esperar locks; fallback Stage 0 acotado; task mode queda calificado por M3-02. [Recibo y límites](M3-01.md) |
| M3-02 tasks, poll, cancel y expiry | Done (qualified) | Docker lifecycle 4/4 más 3 selecciones nuevas de materialización por tool (7 en el grupo `tasks_runtime`), matriz five-version declarada/no declarada, 30 cold + 30 warm por operación, Inspector task flow y Codex fallback pasan. Advertisement ON. Las cuatro tools acreditan `CreateTaskResult` de extremo a extremo. [Evidencia](M3-02.md) · [Runtime](M3-runtime.json) · [Clients](M3-02-clients.json) · [Budgets](M3-02-budgets.json) |
| M3-03 coverage | Done (qualified) | Coverage 8/8 dentro del runtime actual 62/62; target ADR-065 RW en run/report, keeper RO, export ausente; USTAR compartido rechaza prefix/link/uid/gid hostiles; bundle HTML de 320 KiB supera el cap antiguo; counts 4/4 líneas, 8/9 regiones, 2/2 funciones. [Calibración](M3-03.md) · [Recibo](M3-runtime.json) · [ADR-065](../adr/ADR-065-coverage-target-volume.md) |
| M3-04 SemVer | Done (qualified) | Tool 21; W3 Docker 18/18: exits 0/100/101, warn-only, parser sobre goldens reales, ambos roots read-only, target dir confirmado, git/registry/cancelación, Stage 1 degradable y fallback Stage 0. [Recibo](M3-04.md) · [Calibración](M3-04-semver-calibration.md) |
| M3-05 mutation | Done (qualified) | Tool 22; W3 Docker 10/10: exits 0/1/2/3/4, schema/listas/bundle reales, identidad guest, cap pre-build, hostil y cancelación. La inmutabilidad se acredita con mount RO/verifier y canary host, no con un campo de respuesta. [Recibo](M3-05.md) · [Calibración](M3-05-mutation-calibration.md) |
| M3-06 integración y handoff | Done (qualified) for the local scope | [Core W6](M3-core-gate.json) 14/14 en 408.568 s; [full W6](M3-full-gate.json) **25/25** en 2,530.937 s, `audit-data` incluido; [rollback G6](M3-06-rollback.md) 10/10; [handoff](M3-07.md). No hay commit, tag, push ni release. |
| D06 bounded jobs/tasks | Integrated and qualified | Lifecycle, permit compartido, token de registry, masking, quotas, TTL, liveness, child join, EOF/restart y clientes calificados; Tasks anunciado y aún gated por declaración del peer. [ADR-060](../adr/ADR-060-bounded-job-execution-and-mcp-tasks.md) · [M3-02](M3-02.md) |
| D17 private artifact store | Integrated for quality tools | Autoridad live no bloqueante, publicación durable degradable por miembro, bundles contados una vez y Resources index/chunk; fallback Stage 0 sin state root o attach no disponible. Formato v1 versionado en el nombre del directorio. [ADR-061](../adr/ADR-061-private-quality-artifact-store.md) · [Rollback](M3-06-rollback.md) |
| D18 coverage/SemVer accounting | Integrated and qualified | [ADR-062](../adr/ADR-062-coverage-accounting-and-semver-baselines.md), [SemVer 18/18](M3-04.md), [coverage 8/8](M3-03.md) |
| Provisioning explícito | Done | [ADR-063](../adr/ADR-063-m3-guest-plugin-provisioning.md), [config P02](M3-image-config.json) y [verificación 47/47](M3-provisioning.json) |
| G1 Arquitectura y contrato | Done | Arquitectura, 22 tools y los 23 snapshots byte-identical pasan el core y el full. W6 cerró el hueco de `VF-CONTRACTS`: `rust.coverage`, `rust.semver.check` y `rust.mutation.test` tienen ahora prueba product-level de que un peer que declara la extensión recibe un `CreateTaskResult` (`taskId`, `status=working`, `ttlMs=7200000`, `pollIntervalMs=1000`, sin `content`/`structuredContent`), y mutation cubre además su camino `auto`, el único alcanzable en producción. Las tres selecciones pasan dentro del runtime 62/62. |
| G2 Autoridad y threat model | Done | V-SEC fue dispuesto y rust-security pasa 20/20. El M3 orchestrator aceptó ADR-064 y ADR-065 el 2026-09-06, después de la revisión independiente de seguridad y de los gates runtime/Rust-security; la aceptación delegada está registrada en [D03-adr-acceptance](m3-delegation/D03-adr-acceptance/). La autorización separada del owner del 2026-09-05 cubría guest provisioning, no estas decisiones. |
| G3 Lifecycle, concurrencia, cuotas y auditoría | Done for M3 cuts | D06-T01..T14 pasan, incluidos cuatro casos Docker de lifecycle con child real y budgets medidos. |
| G4 Fixtures y pruebas | Done | Runtime **62/62** (nextest 19, Tasks 7, coverage 8, SemVer 18, mutation 10), Inspector 2.5.0 task flow y Codex 0.153.0 fallback. La materialización por tool ya no es un hueco separado. |
| G5 Gates y evidencia | Done | W6 core **14/14** (408.568 s) y full **25/25** (2,530.937 s) sobre 801 inputs / 45,966,311 bytes, con `source_inputs_unchanged: true`. Incluye fmt/check/clippy/test 1,072 + 1 doctest, audit/deny, Docker security 4/4, rust-security 20/20, M2 runtime, M3 runtime 62/62, `audit-data`, semantic, catalog, catalog-status, crate-search, crate-inspect y doctor. Higiene Docker 0 contenedores / 0 volúmenes. [Core](M3-core-gate.json) · [Full](M3-full-gate.json) |
| G6 Compatibilidad, migración y rollback | Done | Recibo propio de M3: versión desconocida falla cerrada y se conserva; el binario sin store M3 no lee, migra ni borra el directorio; `quality-artifacts recover|prune` se comportan como está documentado sobre objetos válidos, expirados y en quarantine; M2 y sus journals quedan intactos. 10/10 selecciones (6 nuevas + 4 reutilizadas). [Recibo](M3-06-rollback.md) · [JSON](M3-06-rollback.json) |
| G7 Operación y distribución | Done for the non-release M3 scope | Provisioning 47/47 registra imagen/config, cinco componentes, hashes, licencias, notices y SBOM guest. M3 no produce paquete ni amplía plataformas. [Provisioning](M3-provisioning.json) · [ADR-063](../adr/ADR-063-m3-guest-plugin-provisioning.md) |
| G8 Revisión independiente y bug bar | Done | V-SEC y V-CONTRACTS previos fueron dispuestos. Los re-reviews [VR-CONTRACTS](m3-delegation/VR-contracts-rereview/last-message.md) y [VR Opus](m3-delegation/VR-opus-rereview/last-message.md) confirmaron que los hallazgos bloqueantes están corregidos; los residuos aceptados quedan registrados en esta matriz. |
| G9 Definition of Ready y Done | Done for the local scope | G1..G8 están satisfechos para el alcance local. No implica commit, tag, push, release ni ampliación de plataforma. [Handoff](M3-07.md) |

## Residuos aceptados

- **`test-hooks` advertisement override:** accepted residual. El hook solo puede
  forzar la capacidad a `true`; por tanto, el camino wire no anunciado no tiene
  un oráculo end-to-end a través del binario real. Consecuencia visible para el
  owner: la matriz acredita el camino anunciado/negociado, pero no una prueba
  positiva del caso no anunciado.
- **`LiveJobAuthority::revalidate` bajo contención:** accepted residual. Puede
  responder `authorized` mientras el registro está contendiendo; es sólido solo
  mientras se mantenga la regla de un único permit y no está afirmado por una
  prueba. Consecuencia visible para el owner: esa garantía sigue siendo una
  invariante operativa, no una propiedad independiente demostrada por el gate.
