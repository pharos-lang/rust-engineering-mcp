# Revisión local independiente — configuración aplicada M5

## Task

Revisión read-only del cierre P2 que hace que cada contenedor de rendimiento M5
reutilice la matriz de autoridad, recursos y mounts aplicada por M1–M4. El
alcance exacto está congelado en `reviewed-files.sha256`.

## Result

**PASS ESTÁTICO, SUJETO A GATES.** No quedan findings P0–P2 en los bytes
revisados. La revisión comprobó que las fases ordinarias y `GovernorProbe`
alcanzan el mismo verificador compartido, y que este contrasta las dos vistas de
mounts devueltas por Docker.

## Files changed

Solo este recibo y su manifiesto de hashes fueron creados por la revisión. No
edité código de producto, ADRs, contratos ni pruebas.

## Tests executed

- `git diff --check`

No se ejecutó Cargo ni Docker. Los tests agregados fueron inspeccionados, pero
su resultado pertenece a los recibos de gates del owner.

## Evidence

### AS-01 — RESOLVED — matriz común completa

`crates/execution-adapter/src/rust_applied.rs:237-292` define la expectativa de
fase y `verify_phase`. La función reutiliza `only_created`,
`no_host_authority`, `expected_volume_mounts_ok`, `applied_limits_ok` y
`applied_profile_ok`; después exige cardinalidad exacta de tmpfs, usuario,
entrypoint, argv y entorno.

Esto incorpora a M5 los 26 controles antes omitidos: `AttachStdin`,
`StdinOnce`, `Config.Volumes`, y `AutoRemove`, `GroupAdd`, `UTSMode`,
`OomKillDisable`, `OomScoreAdj`, `DeviceCgroupRules`, `StorageOpt`,
`Annotations`, `Init`, `MaskedPaths`, `ReadonlyPaths`, `UsernsMode`,
`CgroupParent`, `Sysctls`, `Ulimits`, `PidMode`, `VolumesFrom`, `LogConfig`,
`Devices`, `DeviceRequests`, `PublishAllPorts`, `PortBindings` y
`RestartPolicy` de `HostConfig`.

### AS-02 — RESOLVED — ambas vistas de mounts

`crates/execution-adapter/src/performance_gateway.rs:990-1057` construye una
expectativa tipada por cada volumen y permiso de la fase. El verificador común
exige cardinalidad exacta en `Mounts` y `HostConfig.Mounts`; identidad, path,
driver, modo, propagación y acceso en la vista aplicada; y `NoCopy`, labels,
subpath y configuración de driver exactos en la vista solicitada. La ausencia
de un volumen que la fase declara montado falla cerrada.

### AS-03 — RESOLVED — todas las rutas y fingerprint

Las fases ordinarias pasan por `create_phase` → `verify_applied` →
`verify_applied_with_command` → `verify_phase`. `GovernorProbe`, cuyo argv se
deriva de la topología guest observada, pasa explícitamente por
`verify_applied_with_command` antes de iniciar el contenedor. El fingerprint de
ejecución incorpora `rust_applied.rs`, de modo que un cambio futuro de esta
frontera altera la identidad de ejecución M5.

Los tests del delta recorren todas las fases de benchmark, profile y bloat,
incluido `GovernorProbe`; mutan los 26 campos recuperados y cada propiedad de
ambas vistas de mounts; y rechazan mounts ausentes, extra y con campos
desconocidos. No se modificaron callers ni verificadores existentes de M1–M4.

## Risks

La conclusión es estática. Compilación, Clippy, tests focales, tests conjuntos y
la ejecución Docker deben pasar sobre estos mismos hashes antes de usar este
recibo como evidencia de cierre del gate.

## Decisions

- La autoridad y los límites comunes permanecen definidos una sola vez en
  `rust_applied.rs`.
- M5 conserva como datos propios el programa, argv, entorno, seccomp, topología
  de volúmenes y permisos de cada fase.
- El argv dinámico de `GovernorProbe` se verifica contra la lista guest que se
  usó para crear el contenedor.
- El snapshot histórico G1–G9 no se reescribe; la auditoría final debe enlazar
  este recibo y los gates que resuelvan el finding.

## Open issues

- Ejecutar y registrar los gates focales y conjuntos sobre los hashes
  congelados.
- Sincronizar matriz, handoff y tablero solo después de recibir esos resultados.
