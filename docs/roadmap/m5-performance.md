# M5 — Performance / 0.5.x

Estado: **Planned**. Entrada M4 cerrado. Fuentes: spec §28/29/50.4/82/92/93/97 M5,
[ADR-008](../adr/ADR-008-execution-gateway.md),
[ADR-048](../adr/ADR-048-0.1.0-qualification-and-artifact-boundary.md).
Aplican [G1–G9](m2-m8.md). Objetivo: medir benchmarks existentes, comparar datasets
compatibles, perfilar y atribuir tamaño con límites de interpretación explícitos.

## Contrato y cortes

Tools propuestas: `rust.benchmark.run`, `rust.benchmark.compare`,
`rust.profile.flamegraph`, `rust.binary.bloat`. No generar benchmarks ni optimizar
código automáticamente. No heurística suggest_optimizations, flags libres, sudo,
privileged containers, cambios sysctl o activar permisos de profiling desde MCP.

| ID | Flujo end-to-end | Dependencias | Oráculo/gate | Tamaño |
| --- | --- | --- | --- | --- |
| M5-01 | Benchmark existente→job→dataset versionado+samples+provenance | M4, D23 | Harness exacto/control determinista/ruido/incomplete | L |
| M5-02 | Dos artifacts autorizados→compatibility→comparación→veredicto | 01 | Ratios conocidos, self-compare, mismatch, ruido mayor que efecto | L |
| M5-03 | Host concede profiling→job→stacks/SVG→Resource privado | 01, D24 | Workload/símbolos conocidos y permiso negado/active cancel | XL |
| M5-04 | Build cerrado→cargo-bloat→size/attribution→report | 01, D23 | Binario conocido, LTO/stripped, formato inválido/WASM | L |
| M5-05 | Método+cuatro tools→clientes/gate→budgets MCP→handoff | 01–04 | G1–G9 y review independiente de estadística/permisos | L |

Camino crítico: protocolo/identidad→dataset→compare; profiling positivo D24 es
prerrequisito independiente de cierre. Tamaño total L/XL según host de profiling.
No fechar ni asumir que Docker guest equivale a perfilador nativo calificado.

Domain modela dataset/units/samples/comparison; application verifica autorización,
compatibilidad, budgets y tareas M3. Execution adapter añade RustCommand cerrado,
parser/plugin exacto; compare es cálculo sobre bytes autorizados sin procesos.
MCP mantiene schemas y Resources ricos M3; CLI/doctor reporta profiler ausente o
permiso negado por el host. No nuevo gateway ni lectura directa de paths de artifacts.

## Método congelado antes de medir

D23 fija primera integración Criterion exacta y su formato. [Cargo bench](https://doc.rust-lang.org/cargo/commands/cargo-bench.html)
permite harnesses distintos; uno desconocido puede reportar ejecución/logs, pero no
fabricar medidas comparables. [Criterion](https://bheisler.github.io/criterion.rs/book/analysis.html)
orienta el diseño, los defaults se fijan por versión/fixture antes de medir.

Dataset v1 propuesto: benchmark identity y source digest, samples crudas, units,
warmup, número de muestras, orden/repeticiones, selección target/features/profile,
Rust/Cargo/plugin/compiler flags cerrados, runtime image/config, CPU/model/core,
OS/kernel/arch, virtualización, governor/frecuencia si observables, carga y quotas.
Unknown permanece unknown y puede impedir comparación. No identificar baseline
solo por nombre de branch. Source puede variar entre baseline/candidate; método,
benchmark identity, hardware/OS/config deben ser compatibles y diferencias visibles.

Protocolo propuesto de fixture: warmup 3 s, 30 muestras mínimo, tres ejecuciones
independientes por candidato y orden alternado; fijar semilla y método bootstrap
para CI 95%, reportar tamaño de efecto y outliers sin descartarlos a posteriori.
Umbral material inicial 5%, ruido observado por controles; minimum detectable
regression se estima desde dispersión y tamaño muestral, no se iguala sin evidencia
al 5%. Si la precisión no discrimina el umbral, resultado inconclusive. Para muchas
comparaciones declarar familia/método de corrección o resultados exploratorios.
Es una propuesta a aprobar en D23, no un claim estadístico ya validado.

Compare verifica ownership/format/plugin/units/benchmark/config/hardware antes de
estadística. DTO devuelve regression/improvement/no_material_change/inconclusive,
efecto/intervalo/samples/threshold/MDR y razones. Self-compare, datasets de ratio
conocido, ruido controlado, muestras faltantes/unidades incompatibles/carga distinta
son oráculos. No concluir causalidad ni generalizar a otro host o proyecto.

## Profiling y tamaño

[Flamegraph](https://github.com/flamegraph-rs/flamegraph) depende de backend/OS y
permisos. D24 compara Linux perf aislado y macOS xctrace autorizado, con versión
exacta, scope de procesos y capability del host. No calificar por ejecutar con
privilegios amplios. Si no hay configuración que preserve containment, unavailable
y M5 permanece abierto hasta una decisión de alcance; test denied no cumple la
entrega flamegraph. Cambiar target positivo requiere D13 y oracle nativo.

Entrada cerrada benchmark/binario existente sin args arbitrarios; salida stacks
colapsados y SVG saneado, frecuencia/duración/lost samples/unresolved frames,
profiler/kernel/toolchain y sensitivity. No scripts/URL activos en SVG, paths
privados ni símbolos de procesos ajenos. Probar workload con frame conocido y
control de muestreo cero; cancelación durante profiler con árbol observado.

[cargo-bloat](https://github.com/RazrFalcon/cargo-bloat) exacto sobre build release
cerrado. Separar bytes exactos del archivo y atribución estimada por función/crate.
Primer caso positivo ELF guest Linux ARM64; Mach-O/PE no se califican por ello.
WASM rechazado si backend no lo soporta. Fixtures con dos crates, ranking/tamaño
independiente, símbolos strip, LTO y output de versión desconocida.

## Seguridad, budgets, operación y distribución

Benchmarks/builds ejecutan código de proyecto (R2/R1), profiling añade permiso
específico. G2/G3: env reconstruido, red deny real, source RO/target-temp aislados,
CPU/RAM/PID/disco y worker hasta kill-tree/cleanup; nunca perf_event_paranoid mutable
por la tool. Artifact spoofing, source secreto en símbolos, profiler escapado,
exhaustión y benchmark que falsifica output forman el threat delta. Se describe
origen no autenticado de muestras producidas por harness del proyecto.

Presupuestos propuestos: run 900 s, profiling 60 s/default 10 s, compare 30 s,
bloat 120 s; mismos ceilings CPU/RAM/PID del runtime calificado salvo D24 explícito.
Artifacts M3 con SVG≤8 MiB, bloat≤4 MiB, samples≤32 MiB, result≤512 KiB.
Quota reservada antes de job, pérdida de muestras/símbolos declarada. SLI del MCP:
startup/dispatch/normalización/RSS y bytes; comparar overhead sin Cargo contra
baseline M1 conservada, no usar duración total Cargo como overhead del servidor.

Unit de método/parser/compatibility, contract/protocol de cuatro tools, integration
plugins/bench reales, native profiling, adversarial env/red/descendientes/quotas,
performance controlado y clientes G4. Sin benchmark hostil en host fuera del gateway.
Runtimes/plugins se aprovisionan explícitamente con digest/license/notices/SBOM/
provenance; no bundle automático. Formato dataset independiente de SemVer del server,
reader desconocido falla; migración conserva samples crudas o requiere rerun,
nunca transforma mediciones incompatibles en equivalentes. Rollback del profiler
revoca capability, cancela/join, conserva evidencia y vuelve al runtime calificado.

## DoR, DoD y aceptación

DoR: M4 cerrado, D23/D24 decididos, hardware/runtime/tooling real, método y budgets
congelados antes de medir, fixtures positivos/negativos. DoD: M5-01..05 y G1–G9,
profiling positivo con permisos mínimos, Sonnet de contrato/estadística y Opus High
del permiso de profiling. P0/P1 y P2 de método/evidencia que falsee veredicto bloquean.

- [ ] Cuatro tools entregan artifacts/medidas reales y compare rechaza identidades
  incompatibles. Fuente: spec §28/104, M5-01..04 y D23.
- [ ] Samples/warmup/variabilidad/CI/MDR son visibles y casos ruidosos dan
  inconclusive, sin claim causal. Fuente: spec §92/116, M5-01/02.
- [ ] Flamegraph tiene prueba positiva nativa y negativa de permiso; no sudo ni
  sandbox privilegiado. Fuente: ADR-009/048, M5-03 y D24.
- [ ] Artifact privacy, límites, cancelación activa y cleanup pasan con code
  hostil; size exacto se distingue de atribución. Fuente: spec §36–45/77; M5-03/04.
- [ ] Full gate y cliente sobre bytes finales, inventory/SBOM/provenance y
  budgets MCP conservan la frontera target real. Fuente: G4–G8, M5-05.

Handoff: datasets/método/hardware, receipts/reviews, capability profiling y límites;
detener antes de M6.
