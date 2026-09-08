# M5 — matriz de implementación y calificación

Fecha de apertura: 2026-09-08. Rama de trabajo: `ai/m5-performance`. Base:
`c6099f27415b0be3838e84d21d25eed903c8c312`.

Estado: **In progress**. Este documento se completa por corte; ninguna fila pasa
a Done sin su recibo enlazado. Un gate anterior nunca acredita código nuevo.

## Entrada M4 verificada live

| Hecho declarado por el encargo | Comprobación | Resultado |
| --- | --- | --- |
| M4 integrado por PR #15 | `gh pr view 15` | `MERGED`, `mergedAt 2026-09-08T17:58:12Z` |
| Merge commit `90d72f2c…` | `git log -1 90d72f2c…` | existe y es ancestro de `main` |
| Head validado `e1be3a37…` | `gh pr view 15 --json headRefOid` | coincide; es ancestro de `main` |
| Checks aprobados | `statusCheckRollup` | 10/10 `SUCCESS`: `portable` ×3, `supply chain`, `SonarCloud`, `SonarCloud Code Analysis`, `CodeQL` ×4 |
| Bytes calificados == `main` | `git diff --stat e1be3a37..HEAD -- crates/ Cargo.toml Cargo.lock scripts/` | vacío |
| `main` == `origin/main` | `git rev-parse` | `c6099f27…` en ambos |

No se detectó discrepancia. El gate local 33/33 de M4 se acredita por su
[recibo](M4-full-gate.json); no se volvió a ejecutar sobre el mismo código.

## Decisiones cerradas antes de implementar

| Decisión | ADR | Evidencia |
| --- | --- | --- |
| D23 — método de benchmark, dataset y comparación | [ADR-073](../adr/ADR-073-benchmark-method-and-dataset.md) | Formato `sample.json` leído del `.crate` verificado de criterion 0.8.2; método congelado antes de medir |
| D24 — capability de profiling y containment | [ADR-074](../adr/ADR-074-profiling-capability-and-containment.md) | [Prueba de capability](M5-profiling-capability-probe.json) |
| Aprovisionamiento M5 | [ADR-075](../adr/ADR-075-m5-runtime-provisioning.md) | Autorización separada del owner sobre el [dossier](../roadmap/m5-provisioning-request.md) |
| Contratos de las cuatro tools | [ADR-076](../adr/ADR-076-m5-performance-contracts.md) | Veintisiete schemas previos bajo test de invariancia |

### D24 — el positivo es alcanzable sin privilegios

| Perfil seccomp | `perf_event_open` | Veredicto |
| --- | --- | --- |
| `seccomp-rust-quality.json` | `-1`, `errno=1` | denegado |
| `seccomp-rust-profile.json` (el mismo + una syscall) | `fd=3`, ring buffer mapeado, `data_head=560` | muestras reales |

Con `--cap-drop=ALL`, `no-new-privileges`, `--network=none`, uid 65534 y
`perf_event_paranoid=2` sin tocar. Sin `sudo`, sin contenedor privilegiado, sin
`--cap-add` y sin cambio de `sysctl`.

## Cortes

| ID | Corte | Estado | Evidencia |
| --- | --- | --- | --- |
| M5-01 | `rust.benchmark.run` | In progress | — |
| M5-02 | `rust.benchmark.compare` | In progress | — |
| M5-03 | `rust.profile.flamegraph` | In progress | — |
| M5-04 | `rust.binary.bloat` | In progress | — |
| M5-05 | Cierre, clientes y gate conjunto | Not started | — |

## Limitaciones declaradas hasta ahora

- El ratio 1,25× de la fixture de benchmarks es un ratio **de diseño**, no una
  medición validada. Tres ejecuciones en el host dieron 1,256, 1,331 y 1,302 para
  `slower_125/reference` y 1,013, 1,002 y 0,976 para `control/reference`. Ninguna
  tolerancia se deriva de esos números; la calibración pertenece al guest.
- Toda la evidencia de fixtures registrada hasta aquí es macOS ARM64. El
  comportamiento en el guest Linux ARM64 se califica por separado.
- `BenchmarkExit` y `BloatExit` declaran `CALIBRATED = false`: la tabla de exits
  es una hipótesis hasta que un recibo del guest la registre.
- La fixture de benchmarks no compila desde un checkout limpio hasta ejecutar
  `fixtures/criterion-vendor/materialize.py`; el árbol extraído es generado.
