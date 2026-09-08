# M4 — handoff final

Estado: **Done local — 2026-09-08**.
Core, full, runtime y clientes pasaron sobre los inputs finales. Este documento
no autoriza release, publicación ni M5.

## Identidad del candidato

| Campo | Valor |
| --- | --- |
| Rama | `ai/m4-security` |
| Base | `c66a3704e1ad290603a3c1d10413df90d15c2b03` (`main`) |
| Commit de implementación | `07814664379628f00857feca13148b507de687b9` |
| Versión workspace | `0.3.0-dev` |
| Pull request | [#15](https://github.com/pharos-lang/rust-engineering-mcp/pull/15) |
| Publicación | Sin tag ni release |
| Plataforma calificada | Host local macOS 26.6.2 ARM64; runtime Linux ARM64 en Docker |

No se acredita ejecución en CI remota, el servicio Sonar, un host Linux ni
arquitectura x86_64. El intento local de Cross-Clippy para Linux no pudo enlazar
por ausencia de `x86_64-linux-gnu-gcc`; no se instaló el compilador y ese intento
no amplía la matriz soportada.

## Alcance implementado

Los seis cortes del [plan M4](../roadmap/m4-security.md) están integrados y calificados:

1. **M4-01, `rust.deny`:** metadata congelada y offline, cargo-deny aislado,
   policy host, audit compartido y resultados de licenses, bans y sources.
2. **M4-02, `rust.unsafe.scan`:** scanner sintáctico por archivo en helper
   aislado, atribución source/vendor, límites y resultados parciales explícitos.
3. **M4-03, `rust.miri`:** nightly y sysroot fijados, admisión por identidad,
   JUnit autenticado y clasificación acotada de UB, compilación, test y timeout.
4. **M4-04, `rust.supply_chain.inspect`:** composición de audit, deny, grafo y
   catálogo autenticado sin migrar la persistencia M3.
5. **M4-05, `rust.quality.gate.v2`:** perfiles strict/release sobre una captura,
   baseline explícito y mutation opt-in solo mediante Tasks.
6. **M4-06, hardening y cierre:** lifecycle Tasks, cleanup atestado, canarios,
   inventario, imagen alterada, rollback, clientes, presupuestos y revisiones.

El inventario MCP contiene 27 tools. Conserva las 22 anteriores y
añade al final, en este orden, `rust.deny`, `rust.unsafe.scan`,
`rust.supply_chain.inspect`, `rust.quality.gate.v2` y `rust.miri`. Los 23
snapshots anteriores permanecen preservados y se agregaron únicamente
[deny](../../crates/mcp-server/tests/snapshots/deny-tool.json),
[unsafe scan](../../crates/mcp-server/tests/snapshots/unsafe-scan-tool.json),
[supply chain](../../crates/mcp-server/tests/snapshots/supply-chain-tool.json),
[quality v2](../../crates/mcp-server/tests/snapshots/quality-v2-tool.json) y
[Miri](../../crates/mcp-server/tests/snapshots/miri-tool.json). El protocolo
44/44 y las cinco variantes de contrato M4 están incluidos en el
[core gate](M4-core-gate.json); esto preserva los contratos previos, y el full final acredita las fronteras nativas.

## Decisiones y runtime fijado

Las decisiones normativas del corte son:

- [ADR-066 — aprovisionamiento aislado](../adr/ADR-066-m4-runtime-provisioning.md);
- [ADR-067 — policy, audit y perfiles](../adr/ADR-067-security-policy-and-quality-contracts.md);
- [ADR-068 — admisión por identidad](../adr/ADR-068-m4-runtime-admission.md);
- [ADR-069 — scanner AST aislado](../adr/ADR-069-isolated-unsafe-syntax-scanner.md);
- [ADR-070 — atestación de cleanup de Tasks](../adr/ADR-070-task-cleanup-attestation.md);
- [ADR-071 — facts de supply chain](../adr/ADR-071-supply-chain-facts-without-catalog-migration.md);
- [ADR-072 — integridad de clasificación Miri](../adr/ADR-072-miri-classification-integrity.md).

La imagen M4 admitida y usada por scanner y Miri es
`sha256:25ed3626e710081a571a86a29521eaf2e890e796afd422ba5e409e0ce1891635`;
su identidad y relación con las imágenes anteriores se conservan en el
[recibo de runtime](M4-runtime-image.json). Deny admite esa imagen y la imagen
base de seguridad `sha256:95dddeb5305f10b09b441e3cc4018ebb1a8a296d365c65106327d59f933c64e7`.
El rollback usa exclusivamente la imagen M3
`sha256:384a1742ecc53cdd3a9c0bf36c6f8b66db73ddd118aeeae6e55654ea998ae36a`.
El [inventario pasivo](M4-runtime-inventory.json) comprobó los seis binarios
fijados y el sysroot completo sin ejecutar código guest ni usar red.

## Evidencia positiva disponible

| Evidencia | Resultado y alcance |
| --- | --- |
| [Core final de candidato](M4-core-gate.json) | PASS en 19 etapas: 1220 tests Rust, 1 doctest y 11 tests del helper, además de fmt, check, Clippy, arquitectura y verificaciones auxiliares. `source_inputs_unchanged` es `true`. |
| [Full final](M4-full-gate.json) | PASS 33/33 reanudado: 27 pasos aprobados conservados, seis restantes ejecutados con assets E5 locales exactos recuperados. Mismos 987 inputs que core y clientes; sin descarga ni cambios de código/oráculos. [Driver](M4-full-gate-resume-driver.py) y [recuperación](M4-e5-local-recovery.json). |
| [Runtime M4](M4-runtime.json) | PASS 19/19 sobre imagen final; [Rust security](M4-rust-security.json) 20/20, [M2](M4-m2-runtime.json) 10 selecciones y [M3](M4-m3-runtime.json) 62/62, conservando las imágenes propias de cada suite. |
| [Benchmark](M4-budgets.json) | PASS 300/300, 10 grupos por 30 observaciones, máximo 17 044 ms frente al techo síncrono de 60 s. Mide el binario congelado `sha256:d27cbc5907ce7e985f3f1c162356714bb88efb87e596619b8866faa6bcda8b5f`; su [inventario fuente](M4-budgets/m4-budgets-inputs.json) es parte del recibo. Es evidencia histórica para presupuesto, no un binding de los últimos cambios de parser/routing. |
| [Clientes, intento 4](M4-clients.json) | PASS ligado al candidato `f4e2c6d18982405d6674e9978352e1b9887df16adc729b5a43473adbd491f024` y a la imagen `25ed…`. Inspector 2.5.0 descubrió 27 tools, ejercitó las cinco M4, Resources y cancelación Tasks. Codex CLI 0.153.0 stock ejercitó las cinco de forma síncrona y el turno dirigido; declara `tasks_declared: false` y no acredita cancelación Tasks. |
| [MCP de las cinco tools](M4-tools-mcp.json) | PASS de resultados y recursos privados sobre la imagen final. |
| [Deny MCP y rollback](M4-deny-mcp.json) | Casos clean/finding/policy y revocación; al volver a M3 se retiran plugin y admisión M4, la nueva llamada queda unavailable y el mismo audit privado v1 se relee con una referencia nueva del mismo owner antes de revocar la fuente. La persistencia M3 no cambia. |
| [Imagen alterada](M4-tampered-plugin.json) | PASS: una identidad derivada no admitida se rechaza antes de ejecutar guest, los inputs quedan iguales y cleanup queda verificado. |
| [Privacidad runtime](M4-privacy-runtime.json) | PASS de control positivo y negativo. El canario host queda ausente; HTML de cobertura y diffs de mutation autorizados por el proyecto pueden conservar bytes de source, incluidos secretos presentes en esa fuente. Esos artifacts son privados y no se promete redacción universal del source autorizado. |

Los intentos cliente [2](M4-clients-before-corpus-refresh.json) y
[3](M4-clients-before-stdout-diagnostic.json) también pasaron y quedan
preservados. El intento 4 es el recibo vigente porque repite G4 después de la
actualización del corpus y liga el diagnóstico estricto de stdout sin cambiar su
oracle.

Los recibos finales [scanner native](M4-scanner-native.json) y
[Miri native](M4-miri-native.json) ya vinculan los bytes actuales: siete casos
scanner, 13 clasificaciones Miri y siete de admisión/lifecycle, estos últimos en
202.007 s entre ambas selecciones. Se conservan los recibos anteriores bajo
`M4-hardening-attempts/native-before-final/`. El [recibo de ejecución cliente](M4-client-execution.json)
y la [continuidad del benchmark](M4-benchmark-source-continuity.json) distinguen
la calificación actual del benchmark histórico.

## Archivos y reproducción

Los DTOs viven en `crates/domain/src/{security,unsafe_scan,miri,supply_chain,quality_v2}.rs`;
los casos de uso homónimos en `crates/application/src/`. Los adapters Deny,
scanner y Miri atraviesan `security_gateway.rs` y sus ports en
`crates/execution-adapter/src/`; los endpoints y Resources correspondientes están
en `crates/mcp-server/src/stdio/`. Los fixtures reales, tests de contrato,
seguridad y scripts de calificación acompañan cada corte.

[CI local](../ci.md#m4--calificación-local-completa) detalla los comandos core/full,
las variables de inputs explícitos y la ejecución de clientes. Los recibos
conservan argv, toolchain, plataforma, timestamps, exit codes y hashes. El
[índice final](M4-evidence-index.json) cruza los 987 inputs y el SHA del binario de
clientes. El [archivo de logs](M4-log-archive.json) liga 38 copias `.txt` a los
`.log` originales byte a byte para que la regla global de Git no descarte esa
evidencia en una futura integración autorizada.

La deuda M3 permanece en el [complemento](../prompts/implement-m4-complement-m3.md#6-deuda-que-m4-hereda-declarada)
y en [M3-07](M3-07.md); M4 no atribuye una causa ni una corrección a esos residuos.

## Revisión independiente y disposiciones

La [revisión final Opus](../reviews/m4-final-closure/review.md) no encontró P0,
P1 ni un P2 nuevo. Su [disposición del Technical Owner](../reviews/m4-final-closure/disposition.md)
registra la renovación nativa del P2 de freshness y estas
decisiones P3:

- conservar como backlog un guard conductual para los inputs del fingerprint,
  sin reemplazar la evidencia nativa por una copia de la lista de implementación;
- aceptar el benchmark como evidencia histórica con binario e inventario exactos,
  y calificar por separado las rutas actuales;
- actualizar documentación de cierre solo después de los receipts finales;
- aceptar que el guard de credenciales de staging reconoce nombres y no es un
  detector universal de secretos;
- mantener la frontera ADR-072: con JUnit autenticado y output guest silenciado,
  los streams externos no se convierten en verdad diagnóstica.

La calificación de imagen alterada tuvo dos intentos fallidos preservados. En el
segundo, la detección sin `--all` dejó una imagen owned sin resolver e invalidó
la afirmación original de cleanup. El
[registro de intentos](M4-hardening-attempts/README.md) conserva el fallo y la
[reparación](M4-hardening-attempts/m4-tampered-plugin-attempt-2/cleanup-repair.json)
identifica y elimina solo la imagen owned; el recibo exitoso actual es separado.

Core intento 2 observó una única falla transitoria en
`closed_stdout_exits_even_when_stdin_remains_open`; el assertion anterior
descartó la variante recibida, por lo que no se atribuye una causa. La
[disposición diagnóstica](M4-hardening-attempts/stdout-diagnostic/disposition.md)
registra 30/30 repeticiones aisladas y cinco suites completas de protocolo
(220 tests) exitosas. El oracle sigue exigiendo `Disconnected`, conserva su
deadline de 10 s y ahora solo informa variante y tiempo: no se aceptaron errores
o frames adicionales, no se amplió el timeout y no cambió producto.

Full intento 1 falló en el qualifier legado por cleanup no diagnosticado. Las
[10 reproducciones](M4-hardening-attempts/full-attempt-1/isolated-large-binary-10.json)
pasaron en ambas fases sin relajar el oracle; la [disposición](M4-hardening-attempts/full-attempt-1/disposition.md)
retiene la causa desconocida y el seguimiento de preservar `phases.repair.cleanup`
si reaparece. Full intento 2 pasó las 27 etapas iniciales y encontró vacío el
directorio temporal E5. Se verificaron tamaño/hash de cinco assets existentes
(487352503 bytes), sin adquirirlos otra vez, y se completaron las seis etapas
restantes mediante el runner original y el mismo source inventory. El recibo
fallido original permanece en [full-attempt-2](M4-hardening-attempts/full-attempt-2/receipt.json).
Los logs del primer segmento y de la reanudación son separados; 33/33 no significa
que el primer intento monolítico haya pasado.

## Cierre y parada

La [confirmación final Opus 5 High](../reviews/m4-final-evidence/review.md)
**acepta el cierre local**, sin P0/P1/P2 abiertos. La [disposición del Technical Owner](../reviews/m4-final-evidence/disposition.md)
cierra los seis cortes y G1–G9 con los recibos anteriores. La
[verificación final](M4-final-verification.json) calcula las comparaciones y
hashes reales; conserva los segmentos del full, los 23 baselines Git y los cinco
assets E5 con pins esperados/observados. Las observaciones menores quedan
resueltas por evidencia suplementaria o aceptadas con owner y límite explícitos.

M4 está **Done local** en `ai/m4-security`. La implementación está en
`07814664379628f00857feca13148b507de687b9` y se propone mediante el
[PR #15](https://github.com/pharos-lang/rust-engineering-mcp/pull/15). No hay tag,
release ni autorización para M5. El siguiente documento de trabajo es
[implement-m5.md](../prompts/implement-m5.md), solo como referencia de handoff.
La revisión de CI/Sonar remotos corresponde a una futura integración autorizada.
