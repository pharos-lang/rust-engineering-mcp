# ADR-067 — Policy de seguridad, audit compartido y perfiles nuevos (D19)

Fecha: 2026-09-07.

## Status

Accepted e implementado internamente. Deny y gate v2 tienen pruebas nativas;
la publicación de las tools depende todavía de clientes, mediciones, revisión y
gate conjunto. Los detalles de salida/exit de cargo-deny 0.19.7 están contrastados
con la imagen de ADR-066 y oráculos registrados.

## Context

`rust.dependencies.audit` ya tiene un matcher RustSec y un snapshot propio con
freshness. `rust.quality.gate` acepta únicamente `fast` y `standard`, con enums
y snapshots M1 congelados. Añadir variantes a ese contrato cambia sus respuestas
y el significado de clientes existentes. Los jobs M3 y su store privado ya
resuelven admisión, cancelación, retención y Resources.

El cargo-deny 0.19.7 fijado intenta preparar crates incluso con `--metadata-path`.
Su inferencia de licencias prefiere el campo de metadata a los archivos de texto.
Usarlo sin una integración explícita incumpliría la evidencia offline requerida
por M4. El código oficial fijado y sus hashes se conservan en
[sources.json](../validation/m4-deny-research/sources.json).

## Decision

### Contratos independientes

M4-01 añade `rust.deny`. M4-05 añade `rust.quality.gate.v2` con perfiles cerrados
`strict` y `release`. El nuevo nombre es necesario para el contrato de perfiles
que exige el plan; no sustituye ni retira `rust.quality.gate`. Se conservan
literalmente las trece tools M1, incluyendo `QualityProfile`, `QualityStage`,
defaults, anotaciones y semántica de resultados. Los snapshots preexistentes se
comparan contra la base M3. La quinta tool nueva corresponde exclusivamente a
este contrato de composición; no es una vertical adicional ni habilita M5.

Inputs de deny: `project_ref`, `execution_mode` y `timeout_seconds` acotados.
El host aporta la policy, el runtime y el vendor: el peer no puede proporcionar
paths, TOML libre, comandos, flags, exceptions de cargo-deny ni nuevas fuentes.
Los DTOs propios son cerrados y se derivan de tipos Rust cuando sea viable.

Inputs de gate.v2: project, profile, execution mode, timeout y baseline ProjectRef
obligatorio para release y rechazado para strict. Mutation es opt-in, con su
selección/budget cerrado de M3 y validación del presupuesto total antes de admitir.
No se añade un parámetro de versión a las llamadas existentes.

### Captura y motores

La aplicación captura el source actual una vez y revalida su autoridad durante
las etapas y antes de publicar. Captura también una sola fuente vendor protegida,
una policy inmutable y un snapshot RustSec. La metadata frozen completa de esa
fuente conserva un fingerprint propio, paths ligados a source/vendor e identidades
de paquetes; no se acepta metadata proporcionada por el cliente.

`DependencyAuditPort` sigue siendo el único motor de advisories. Deny recibe su
`AuditObservation` sin modificarla y ejecuta únicamente licenses/bans/sources en
cargo-deny. La composición comparte esa observación ya calculada con supply chain
y gate.v2. No llama tools MCP, no ejecuta cargo-audit/cargo-deny advisories ni
refresca bases. La prueba de composición debe contar una llamada al port y
comparar la observación con audit standalone sobre los mismos inputs/reloj.

Para cargo-deny se materializa una copia derivada y identificada de la metadata:
`packages[*].license` se establece a null para exigir la inspección de los archivos
reales. Se preserva la declaración original como hecho separado, sin tratarla
como evidencia de texto. Antes de esa derivación, cada manifest/source/license-file
se valida contra el inventario exacto de source o vendor. Una referencia fuera
del paquete/source autorizado, archivo ausente o datos corruptos impide una
calificación completa. No se modifica el source capturado ni el proyecto host.

La configuración generada por el adapter usa nombres/args cerrados: `--frozen`,
`--offline`, JSON, config y metadata en paths constantes, sin inclusión de graphs
de texto y `check licenses bans sources --disable-fetch`. Cargo home se reconstruye
con el directory source de ADR-055 y offline obligatorio; Docker aplica network
none y seccomp calificado. Ni `--disable-fetch` ni metadata sustituyen enforcement.
Se incluyen dependencias normal/build/dev, todos los miembros y la selección de
features declarada en el resultado. No se excluyen silenciosamente crates privados.

Licencias se evalúan por texto con confianza fija 0.95, sin clarifications ni
exceptions libres. Las licencias aceptadas proceden de la policy del host; la
ausencia de esa lista nunca selecciona una policy legal por conveniencia.
Unknown/unlicensed y cobertura incompleta permanecen visibles. La combinación de
licencias detectadas usa la semántica conservadora del plugin, documentada como
limitación; no constituye una aprobación legal ni certificación de seguridad.

### Policy y suppressions

El host proporciona un documento cerrado versionado y SHA-256 esperado. Su lectura
usa el adapter de snapshots no-follow; el digest vincula los bytes que se parsean,
sin una reapertura posterior. El tamaño máximo de policy es 64 KiB, con hasta
128 licencias, 128 bans y 128 suppressions. El runtime no importa `deny.toml` ni
exceptions del proyecto; el proyecto no puede reducir las restricciones del host.

La versión fijada busca automáticamente `deny.exceptions.toml`,
`.deny.exceptions.toml` y `.cargo/deny.exceptions.toml` en ancestros del manifest,
incluso usando `--config`. Antes de ejecutar se rechazan esos nombres en todo el
source capturado; la imagen calificada tampoco puede contenerlos en los ancestros
de `/source`. El fixture adverso debe probar esta búsqueda real del plugin.

Source, vendor y datos generados ocupan tres volúmenes efímeros distintos. Los
tres se escriben únicamente en su fase de ingestión y quedan read-only durante
metadata y deny. El tercero contiene policy, metadata derivada y Cargo home con
config offline/directory source generada por el producto. Metadata frozen se
ejecuta antes de ingerir su copia derivada. Los fingerprints vinculan por separado
source original, vendor, policy y metadata original/derivada. Se reutiliza el
Execution Gateway y sus helpers de lifecycle; no se usa la resolución mutante M2
ni se permite crear/modificar Cargo.lock para hacer pasar una inspección.

Reglas: lista explícita de licencias, bans por paquete/rango, y tratamiento cerrado
de duplicate versions/wildcards. Sources desconocidas y Git no aprobado se deniegan.
Una ampliación posterior de fuentes exige su propia evidencia; el catálogo no
actúa como registry ni permite resolver por red.

Cada suppression contiene un ID, motor/source, regla exacta, paquete exacto,
rango de versiones no universal, reason, owner y expiry UTC. Además vincula el
digest de las reglas base. Para evitar un digest circular, `rules_digest` se
calcula sobre la representación canónica versionada de las reglas sin suppressions;
el SHA esperado por el host cubre el documento entero, incluidas las suppressions.
Los límites de strings y rangos se validan antes de efectos. ID repetido, rango
inválido/universal, owner/reason vacíos, regla global, digest distinto o
`expiry <= now` rechazan la policy completa. Una suppression de advisory solo
coincide con su ID concreto y nunca altera `AuditObservation` M1.

El resultado conserva el finding original, su severidad y su disposición
`active`/`suppressed`, más ID/owner/reason/expiry/digest de la suppression aplicada.
No elimina findings ni omisiones. Puede indicar policy satisfecha con suppressions,
pero nunca denomina ese estado "clean". Suppressions no convierten faltas de datos,
integridad, errores de parser, stale o timeout en éxito.

### Resultados, límites y oráculos

Se distinguen ejecución, cobertura y policy. Cada subcheck tiene origen, versión,
fingerprint de config/dataset, fecha y estado. Findings llevan regla, motor,
paquete/origen, severidad y disposición; como máximo 128 visibles y 512 KiB para
la respuesta MCP completa. Conteos y omisiones corresponden al conjunto completo,
y los datos retenidos usan las Resources privadas M3 después de redacción.

El parser exige una única summary JSON final con exactamente licenses/bans/sources,
sin advisories; rechaza duplicados, formato desconocido, truncación y discrepancia
entre conteos/errores/exit bitset. Los logs del plugin son entrada hostil. No se
confía en `exit=0` solo ni se acepta un summary falso después de un error de datos.
Solo los códigos y shapes contrastados con 0.19.7 habilitan parse completo.

Deny tiene deadline predeterminado 120 s y máximo 120 s. Auto/synchronous/task
reutilizan la selección de M3: sin Tasks negociadas solo se admite una selección
síncrona calificada de hasta 60 s. JobExecutor y el permit único de ADR-030 se
mantienen; poll/cancel no consumen el worker y Cancelled espera cleanup/join.
Los límites de source/vendor y artifacts son los ya establecidos en ADR-031/055/061.

Fixtures mínimos antes de Done: limpio con LICENSE real; manifest MIT sin texto
no-pasa; texto no permitido; ban por versión; source no aprobado; duplicate;
advisory real igual a audit standalone; stale/unknown snapshot; suppression exacta,
expirada/malformada; license-file escapado; metadata/summary corrupta; overflow;
cancel/timeout y revocación con cleanup. Se usan goldens del guest y mutaciones
que hagan fallar el oráculo. Las latencias se miden 30 cold + 30 warm con muestras
crudas y percentiles; los límites anteriores no son predicciones de rendimiento.

### Strict y release

`strict` ejecuta format/check/clippy/test/audit/deny/coverage sobre la captura
actual única. Audit se evalúa una vez; la etapa deny referencia su observación.
`release` añade SemVer con baseline separado, autorizado/capturado y revalidado
como en ADR-062. Mutation no se ejecuta salvo opt-in y con baseline obligatorio
de M3. Todos los motores estándar se ejecutan con el estable de la misma imagen
M4 calificada; Miri usa su nightly separado y conserva su tool independiente.

El resultado original de la etapa audit conserva su semántica M1 también en
strict/release: una suppression de deny no convierte un audit fallido en pasado.
La policy usa el parser puro `semver` ya fijado en Cargo.lock para rangos y
versiones, sin introducir una implementación alternativa ni adquirir dependencias.

Se publican todas las etapas requeridas, incluidas no ejecutadas y su razón.
Un required skip/unknown/partial/unavailable/timeout nunca equivale a passed.
Un fallo de validación es un resultado de la tool; una pérdida de authority o
cleanup incierto conserva las reglas de bloqueo/cuarentena. No se mezcla un
runtime distinto silenciosamente ni se captura otra versión del proyecto entre
etapas. El default global es 300 s, máximo 3600 s, limitado además por la suma
de presupuestos declarados y las reglas de admisión M3.

## Alternatives considered

- Ampliar el enum M1: cambia un contrato congelado y los clientes existentes.
- Invocar audit y deny standalone desde MCP para componer: recaptura el proyecto
  y duplica el motor/advisories, rompiendo la comparación de evidencia.
- TOML deny del cliente/proyecto: permite ampliar excepciones, rutas, fuentes y
  alcance sin autoridad host; la configuración se genera desde tipos validados.
- Aceptar `license = "MIT"` como evidencia suficiente: el fixture sin texto lo
  refuta y contradice el requisito de source bytes del plan M4.
- Suppression global o sin caducidad: hace invisible la pérdida de cobertura.
- Otro scheduler/store: duplica las fronteras M3 ya calificadas sin necesidad.

## Consequences

El nuevo contrato de composición añade una quinta tool M4 al discovery solo cuando
esté implementada y probada. Las cuatro tools de seguridad del plan permanecen
separadas por pregunta. La policy es configuración administrativa optativa del
host, con coste de preparar fuentes de licencia offline verificadas; no añade
instalaciones al core ni altera los defaults M1.

Rollback deshabilita la configuración/capability M4 y vuelve al binario anterior
tras EOF/cleanup, preservando artifacts compatibles, journals M2 y floors. Los
contratos nuevos se pueden retirar en el rollback sin reinterpretar los M1.
El store quality v1 conserva su formato; cualquier cambio necesario a persistencia
se decide y prueba aparte antes de modificarlo.

## Sources

- [Plan M4](../roadmap/m4-security.md), D19 y G1–G9.
- ADR-030, ADR-038, ADR-040, ADR-055, ADR-060, ADR-061, ADR-062, ADR-066.
- [Cargo-deny check](https://embarkstudios.github.io/cargo-deny/cli/check.html)
  y [config](https://embarkstudios.github.io/cargo-deny/checks/cfg.html), contrastados
  con el código del commit `759a4946dcfe93a56fb42d464e193c4c448af4e3` (0.19.7).
- `src/cargo-deny/check.rs`, `common.rs`, `stats.rs`, `src/licenses/gather.rs`
  y `src/diag/grapher.rs` en el [inventario upstream](../validation/m4-deny-research/sources.json).

### Publicación y compatibilidad del artefacto (integración M4-01)

El artefacto de deny es un informe JSON normalizado por el compositor integrado,
no una copia del stderr del plugin. Conserva SHA-256, tamaños y truncation de los
streams originales, reglas/severidades/paquetes verificados, conteos, disposition
y omisiones. Elimina todos los mensajes, labels, notes y campos libres del guest;
no se promete detectar secretos arbitrarios mediante patrones. Los datos de
identidad de paquetes y suppressions autorizadas por el host siguen siendo datos
sensibles al proyecto, accesibles solo con su autoridad viva.

El productor del artefacto usa `PluginIdentity::Builtin` versión 1 y digest del
normalizador; cargo-deny mantiene su identidad exacta independiente en el
resultado y fingerprint de ejecución. Se reutilizan ToolLog/Utf8LogV1 y el store
M3 sin añadir variantes a su formato persistido. Un binario M3 puede leer o
reconciliar esos descriptores durante rollback. Retención ausente, cuota o fallo
de publicación no se convierten en evidencia completa. Los bytes originales se
descartan después de publicar y nunca llegan al registry de Tasks.

La policy requiere flags de host `--security-policy` y
`--security-policy-sha256` juntos; MCP no acepta rutas ni overrides de policy.
La captura offline del vendor usa el par de flags M2 existente; si falta no se
anuncia una evaluación completa, también para proyectos sin dependencias. El adapter D05 vigente exige al menos
un paquete en un dataset válido; no se anuncia soporte de dataset vacío. Esta
restricción operativa es explícita y no implica que paquetes vendor no usados
entren en el grafo evaluado. La tool
permanece fuera del inventario público hasta completar su calificación y G4.

El DTO deny resume audit mediante estado, issue, completitud, provenance/freshness,
fingerprints y conteos. Los findings aparecen una vez en la lista combinada;
la `AuditObservation` completa permanece inalterada en aplicación para otras
composiciones. Este DTO nuevo no modifica el contrato audit M1. El presupuesto
512 KiB mide el `CallToolResult` serializado completo, incluido el espejo textual.
Si hay que quitar filas, incrementa omisiones y convierte el resultado en parcial;
un resultado recortado conserva el Resource autorizado y nunca pasa.

### Proyección de gate.v2

El gate nuevo conserva los defaults exactos de format/check/clippy/test de standard.
Coverage usa el workspace con features por defecto y sin doctests; SemVer aplica
la selección por defecto idéntica a ambos lados. La salida declara cada selección.
El agregado conserva estados, conteos, métricas, fingerprints y una proyección
normalizada de audit/deny. Su Resource contiene esa misma evidencia acotada, sin
streams crudos, texto de código, HTML ni diffs de las etapas. Las tools individuales
conservan sus propios artifacts de reparación y contratos existentes.

La completitud de un veredicto SemVer se decide por la clasificación M3 calificada,
independiente de su lista auxiliar de findings best-effort, que sigue declarándose
partial. No se cambia M3 ni se presenta esa lista como exhaustiva. Una clasificación
incomplete/uncalibrated sí bloquea release. Coverage sin denominadores ejecutables
no pasa el gate nuevo, aunque la tool individual pueda informar una ejecución vacía.

El timeout global abarca todas las etapas y publicación. Ninguna etapa recibe más
que el presupuesto global restante. Para mutation opt-in se exige, antes de
capturar, que su presupuesto derivado más 300 s reservados a las otras etapas
quepa en el timeout solicitado (máximo 3600 s). Sin mutation, el default sigue
siendo 300 s. Se conserva una fila requerida con razón cuando un motor no está
disponible; cleanup incierto o autoridad perdida abortan la publicación.
