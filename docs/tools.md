# Tools

`rust.project.open`, `rust.project.inspect`, `rust.toolchain.inspect`, `rust.check`, `rust.fmt.check`, `rust.clippy`, `rust.test`, `rust.dependencies.audit`, `rust.diagnostics.explain`, `rust.quality.gate`, `rust.catalog.status`, `rust.crate.search` y `rust.crate.inspect` están implementadas en este checkout; los gates de [M1-11](validation/M1-11.md) y [M1-12](validation/M1-12.md) están registrados; M1-13 tiene [gate aprobado](validation/M1-13.md). rmcp 3.2.0 gestiona discovery,
negociación y dispatch; `tools/list` devuelve trece definiciones sin cursor.

## rust.project.open

Input estricto: `{ "path": "/ruta/fisica/raiz-del-workspace" }`. El path es un
selector dentro de roots que el host ya autorizó por CLI, nunca una concesión de
permisos. No se aceptan campos adicionales, flags, roots del peer ni project_ref
aportado por el cliente. La longitud máxima es 4096 caracteres en schema y 4096
bytes en aplicación; paths ambiguos, relativos o con symlinks se rechazan.

El éxito devuelve el envelope con `status: "passed"`, errores null, diagnostics
vacío, truncation sin recorte, evidence local y data:

```json
{
  "project_ref": "prj_0123456789abcdef0123456789abcdef",
  "workspace_root": "/ruta/fisica/raiz-del-workspace",
  "fingerprint": "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  "validation": "structural"
}
```

Los valores del ejemplo son ilustrativos. Se leen manifests, se comprueban targets
existentes y un grafo acotado de miembros/path dependencies; no se ejecuta Cargo,
no se resuelven dependencias de registry ni se certifica compilación. El caller
selecciona explícitamente la raíz; no hay descubrimiento de workspaces ancestros.
Se rechazan globs, workspaces anidados, aliases antiguos `[project]`,
`dev_dependencies`/`build_dependencies` y formas fuera del subconjunto de ADR-024.
La referencia vive en este proceso, tiene TTL idle (default 1800 s), y exige
revalidación antes de cada uso futuro. Abrir dos veces genera referencias diferentes
con el mismo fingerprint si la identidad y manifests no cambiaron. Máximo 64
referencias vivas: al llenarse, nuevos opens fallan hasta que expiren entradas; no
se expulsan referencias vigentes. Reiniciar el proceso revoca todas sus referencias.

Input inválido produce `-32602`; tool desconocida, `-32601`. Rechazos de policy,
proyecto inválido/ausente o límites son resultados `blocked`, plataforma/FS no
soportado es `unavailable`, siempre `isError: true`. No se exponen paths de errores
internos ni contenido del manifest en los mensajes. `structuredContent` y el JSON
del bloque textual coinciden. Schemas generados desde Rust se validan en runtime.
Annotations: readOnly true, destructive false, idempotent false, openWorld false.

Ver [ADR-024](adr/ADR-024-project-open.md), [compatibilidad](compatibility.md) y
[seguridad](security-model.md). M0-07 acredita la frontera general de contratos.

## rust.project.inspect

Input cerrado: `{ "project_ref": "prj_0123456789abcdef0123456789abcdef" }`.
Requiere referencia viva y configuración explícita del host con la tupla completa
`--docker PATH --docker-socket PATH --state-root PATH --rust-image sha256:ID`.
Solo se acepta la identidad aprobada en ADR-031. Arrancar no ejecuta procesos;
el primer job admitido inicializa y calibra el gateway. No se descarga nada.
Durante bootstrap responde `blocked/SANDBOX_DENIED`: completar discovery y
reintentar con un nuevo ID. Sin runtime también falla cerrado.

El resultado contiene miembros/default-members como índices locales, packages,
edition, MSRV explícito nullable, targets, features y dependencias declaradas,
profiles del manifest raíz y configuración efectiva impuesta por el gateway.
Los orígenes de dependencias son kind/fingerprint y, para path, ruta relativa;
no publica URLs ni IDs opacos Cargo. Los profiles no se resuelven ni se afirman
features activadas. `.cargo/config*` se rechaza; el toolchain opcional debe ser1.98.1.

`semantics=latest_known`, identidad del proyecto y digest de los bytes capturados
son campos distintos. Evidence snapshot incluye provenance, integrity, timestamps,
network_used=false y freshness `captured-project-v1` (fresh60s, aging hasta300s).
La observación identifica Linux ARM64, imagen, configuración y ejecución; no es
una certificación del toolchain host ni una snapshot atómica del filesystem.

Una única operación posee captura, Cargo metadata frozen/offline, parsing y
cleanup; revalida ProjectRef antes de publicar. TTL vencido o identidad cambiada
rechaza el resultado. Deadline120s, metadata256KiB, estructura128KiB y resultado
MCP completo512KiB contando texto y structuredContent; excess se rechaza sin
publicar una estructura parcial. La aplicación solo renueva TTL tras éxito;
una cancelación posterior al éxito puede impedir el envío sin deshacer esa renovación.
Cleanup incierto produce error interno y cuarentena; nunca se oculta como cancelación.
Annotations: readOnly true, destructive false, idempotent true, openWorld false.
Ver [ADR-032](adr/ADR-032-project-inspection.md).

## rust.toolchain.inspect

Input cerrado project_ref, misma política del host, preparación MCP y worker joined
que project.inspect. Devuelve `data.observation` con inventario instalado (versiones
rustc/Cargo, canal stable, host triple, targets y componentes), runtime/image/config
más tres fingerprints de ejecución, source_fingerprint y declared_toolchain nullable.
No consulta Internet, rustup, PATH del host ni lista de targets soportados.

El inventario procede de `rustc --version --verbose`, `cargo --version --verbose`
y el manifiesto de componentes del instalador dentro de la imagen aprobada. Los
valores deben coincidir con esa imagen inmutable. Componentes normalizados: cargo,
clippy, rust_std, rustc y rustfmt; solo rust_std tiene target. La lista instalada
no afirma que todas las herramientas hayan sido ejecutadas. El host triple es el
del guest Linux/aarch64, no el macOS que sirve MCP.

Revalida referencia al finalizar y publica latest_known con snapshot/freshness.
No retorna inventario parcial. Cada comando tiene límite16KiB y30s; job120s con
calibración lazy; respuesta MCP completa64KiB. Inventario corrupto es error interno
fijo; ausencia de componente ejecutable es unavailable, política no autorizada es
blocked. Annotations: readOnly true, destructive false, idempotent true, openWorld false.
Véase [ADR-033](adr/ADR-033-toolchain-inspection.md).

## Contrato M1 / 0.1.0

El alcance autorizado contiene exactamente estas trece tools:

| Tool | Corte principal | Estado |
| --- | --- | --- |
| `rust.project.open` | M0-04 | Implementado; validación estructural |
| `rust.project.inspect` | M1-01 | Implementado; evidencia M1-01 |
| `rust.toolchain.inspect` | M1-02 | Implementado; evidencia M1-02 |
| `rust.check` | M1-03 | Implementado; evidencia M1-03 |
| `rust.fmt.check` | M1-04 | Implementado; evidencia M1-04 |
| `rust.clippy` | M1-05 | Implementado; evidencia M1-05 |
| `rust.test` | M1-06 | Implementado; evidencia M1-06 |
| `rust.dependencies.audit` | M1-07 | Implementado; evidencia M1-07 |
| `rust.diagnostics.explain` | M1-08 | Implementado; evidencia M1-08 |
| `rust.quality.gate` | M1-09 | Implementado y validado; evidencia M1-09 |
| `rust.catalog.status` | M1-11 | Implementado; [evidencia M1-11](validation/M1-11.md) |
| `rust.crate.search` | M1-12 | Implementado; [gate/revisión registrados](validation/M1-12.md) |
| `rust.crate.inspect` | M1-13 | Implementado; [gate aprobado](validation/M1-13.md) |

`rust.dependencies.inspect` no es tool pública M1. Los contratos tipados, schemas y
resultados estructurados se implementarán según ADR-006 y ADR-015. La publicación
de una tool no sustituirá los controles de disponibilidad/policy por plataforma.
El archive core descubre estas trece definiciones, pero no promete que todas tengan
un camino positivo sin configuración del host. Ejecución requiere el gateway
aprobado; semántica requiere un build `local` source-qualified y assets aportados
por el usuario. Los resultados unavailable/blocked/degraded son parte del contrato.

## Gateway M0-05

El gateway M0 separado del servidor ejecuta probes locales confiables en Docker/Linux
arm64 con cgroups v2. No admite Cargo, programas arbitrarios ni mounts del host.
El cliente Docker, daemon/VM, imagen inmutable y rutas de control son TCB del host;
no se hereda el entorno ni el contexto Docker. Estado propio macOS/APFS no-follow;
otros hosts fallan cerrados. Los presupuestos de ejecución excluyen preparación y
cleanup, que tienen plazos propios de control. Daemon/host no disponible impide
certificar cleanup: se devuelve CleanupUncertain y se bloquea la instancia.
M0-06 acredita capabilities del fixture mediante una operación CLI explícita. La única tool MCP
continúa siendo rust.project.open. No se acredita ejecución de Rust Linux 1.98.1.

## Detección activa M0-06

`capabilities --docker PATH --docker-socket PATH --state-root PATH --probe-image sha256:ID`
produce JSON con status verified/degraded/unavailable, timestamp observado,
identidad de engine/configuración/imagen y evidencia por probe. Exit0 exige todas
las garantías; exit1 indica degradación o indisponibilidad; uso inválido es exit2.
`strict_available`/`restricted_available` están acotados a
`scope=trusted_probe_image_only`; `project_code_available=false` siempre.
No importa reportes previos ni hace descargas. Un engine diferente, capability
faltante o evidencia de otra configuración no habilita ejecución.

Los controles positivos solo crean sockets (sin tráfico) y escriben un canario
sintético de la imagen. Ninguno tiene mounts del host. El reporte prueba denegación
de socket IPv4/IPv6 TCP/UDP usada por DNS/loopback y UNIX/NETLINK; no afirma haber realizado
consultas DNS o conexiones externas. La ausencia del canario host es observación
auxiliar: la frontera es el namespace de mounts verificado sin binds/volúmenes,
rootfs read-only y seccomp sin mount/unshare/setns. La carrera de symlinks prueba
protección del canario read-only dentro del guest, no transferencia de proyectos.
macOS/APFS + Docker Linux arm64 es la única combinación validada. El camino
Rust aprobado se valida separadamente mediante ADR-031; no amplía la autoridad
del reporte de probes M0.

M0-07 centraliza validación de contratos en `stdio::contract`: inputs y outputs
con schemas cerrados, validación Serde adicional y errores fijos sin payloads.
El snapshot de `rust.project.open` y las cinco versiones MCP se conservan.

M0-09 añade búsqueda híbrida interna con E5/LanceDB y fallback explícito; todavía
no anuncia `rust.crate.search` ni otras tools nuevas. La incorporación M1-01 de `rust.project.inspect` no publica las tools de catálogo.

ArtifactStore M0-10a es interno: no añade tools ni anuncia Resources MCP. M1 debe
vincular logs/diffs, ProjectRef vivo y retrieval autenticado antes de exponerlos.

## Límite de sesión M1-01

Las tools comparten admisión acotada ADR-030. Exceso de concurrencia, IDs pendientes
duplicados o agotamiento de16 slots retenidos por cancelaciones suprimidas cierran
la sesión; el cliente reconecta y reabre ProjectRefs. Esto no amplía la lista de
tools por sí mismo. ADR-032 conecta project.inspect al gateway Rust calibrado.

## rust.check y Resources (M1-03)

Input: project_ref requerido; package y target opcionales, workspace/all_features/
no_default_features/all_targets booleanos false, features array vacío por defecto.
Solo target aarch64-unknown-linux-gnu instalado. Nombres ASCII alfanuméricos, guion
y underscore, 1..128 bytes sin guion inicial; cada feature admite package/feature,
máximo32, sin duplicados. package/workspace y features/all_features son excluyentes.
No flags arbitrarios. frozen/offline, jobs1, network deny y source read-only siempre.

Check puede ejecutar build.rs/proc macros dentro del gateway calibrado. Exit0 solo
es passed con build-finished exitoso y evidencia completa. Compiler failure devuelve
failed/isError=false. Evidencia incompleta devuelve failed y validation_complete=false;
timeout devuelve blocked/COMMAND_TIMEOUT con evidencia parcial si pudo retenerse.
El error exacto de startup frozen/lockfile de Cargo1.98.1 con exit101 y stdout vacío
se clasifica LOCKFILE_UPDATE_REQUIRED, conserva log y validation_complete=false.
Esta clasificación de salida no concede autoridad ni prueba autenticidad del texto.

Data incluye opciones efectivas, runtime/source/identity fingerprints, latest_known,
termination/exit_code/validation_complete y log. Diagnósticos acotados a128 y128KiB,
spans relativos a fuente capturada y posiciones Unicode comprobadas; rendered=null.
Logs stdout/stderr etiquetados comparten un artifact de hasta256KiB; cada stream
reserva (256KiB-128)/2 bytes y conserva su encabezado y marcador de recorte.
Hash/tamaño representan bytes retenidos; flags de streams incluyen recortes del
gateway y del log, y log.truncated propaga toda pérdida.

resources/list devuelve vacío. resources/read acepta solo URI canónica
rust-artifact://prj_<32hex>/art_<32hex>. Devuelve blob base64 application/octet-stream
y metadata sha256/size_bytes/truncated/retention_remaining_seconds; caché private,
ttlMs0. Cada read revalida ProjectRef vivo, propietario y retención. Ausente/expirado/
propietario distinto devuelve el mismo Resource not found. Leer no renueva artifact
TTL3600s. Reinicio elimina todo; ProjectRef puede caducar antes. Cuotas por owner1MiB
y64artifacts, global16MiB/256; se rechaza capacidad insuficiente sin expulsar logs.
Una desconexión posterior a la publicación autorizada puede dejar un log sin URI
entregada, sujeto a esas mismas cuotas/TTL; no se garantiza entrega de respuesta.

El sandbox no hereda secretos del host. La configuración actual de redacción literal
es vacía, explícitamente; no se afirma detectar secretos escritos en el proyecto.
Los logs y diagnósticos normalizados provienen de salida que el proyecto puede
escribir; normalización no autentica su origen. Cargo tiene deadline30s y el
worker120s incluye preparación/calibración/cleanup. Una lectura autorizada cuenta
como actividad para TTL idle del proyecto, sin renovar retención del artifact. Véase ADR-034.

El perfil actual crea CARGO_HOME vacío por job: compila std y dependencias path
contenidas en la captura. No incluye cache registry/git, vendor config ni source
externo; esas dependencias pueden impedir la validación offline. El log explica
la indisponibilidad y nunca se habilita red para resolverla.

Con cuota de retención agotada, el check sigue devolviendo la validación y
diagnósticos: data.log=null y log_unavailable_reason=retention_capacity. Se marca
la pérdida de logs, sin expulsar artifacts previos ni convertir la validación en
OUTPUT_LIMIT_EXCEEDED cuando el reporte parcial es seguro. La cuota no bloquea
la iteración normal; las lecturas de logs existentes siguen disponibles.

## rust.fmt.check (M1-04)

Input contains only required project_ref. Checks all workspace members via fixed
`cargo fmt --all --check -- --color never --config disable_all_formatting=false`.
Stable project style and skip attributes are honored; this is configured formatting
coverage. Sources are captured read-only; no formatter writes reach host source.
The same workers, calibration, network deny,30s/256KiB streams and Resources apply.

Data adds affected_files (up to128 sorted captured relative files), exact omission
count, diff (whole display text only up to32KiB, else null), and diff_omitted.
Newline-only changes identify their file. This display diff must never be applied
as an edit. Passed requires exit0 and empty complete output; formatting differences
are failed/isError=false. Unknown warnings, invalid syntax and incomplete output
return failed/validation_complete=false; timeout is blocked with partial evidence.
Log quotas preserve the report with log=null/retention_capacity as in check.
See ADR-035 and [evidence](validation/M1-04.md).

## rust.clippy (M1-05)

Input: project_ref requerido, package opcional, workspace/all_targets false,
features vacío, lint_profile default. La misma gramática cerrada de package/features
que check; package y workspace excluyentes. Sin target, all_features,
no_default_features, flags ni configuración arbitraria.

Perfiles: default y project respetan la política capturada sin niveles adicionales;
strict añade -D warnings, también para warnings rustc; pedantic añade
-W clippy::pedantic como warnings opt-in. Los allows/config del proyecto aplican;
no se afirma detectar lints que el proyecto suprime. Passed significa ejecución
completa exit0/build-finished exitoso, y puede contener warnings. El perfil strict
puede convertir esos warnings en failed/isError=false. No se ejecuta --fix.

Cargo clippy frozen/JSON/jobs1 comparte el gateway calibrado, los límites30s/256KiB,
parser de diagnósticos y Resources de check. clippy:: identifica la familia de lint
y sus children/help; no autentica origen. Opciones efectivas, snapshot/latest_known,
fingerprints y logs quedan visibles; timeout, lock frozen, incompletitud y cuotas
conservan las reglas M1-03. Véase ADR-036 y evidencia M1-05.

Diagnostic family normalization uses clippy:: on the root and its descendants;
other roots, including code-less compiler messages, retain the historical rustc
label. This convention is not producer authentication and does not discard
code-less diagnostics or change their severity/completeness.


## rust.test (M1-06)

Entrada: project_ref vivo; package opcional, test_filter ASCII alfanumérico/`_`/`:`
(1..128, inicio alfanumérico/`_`), features cerradas, all_features, target instalado
`aarch64-unknown-linux-gnu`, timeout entero1..60 segundos (default30). features y
all_features son excluyentes. No flags arbitrarios ni workspace/all_targets.
Cargo test frozen/JSON/jobs1/colornever con `-- --test-threads=1 --color=never`;
la selección nativa incluye los doctests y harnesses habilitados. Un harness que
rechace esos argumentos fijos puede fallar. Passed acredita el comando elegido,
no la existencia, cantidad ni cobertura total de tests.

La respuesta conserva los cinco estados, validación completa, diagnósticos,
provenance/freshness latest_known y log Resource autorizado de check. build_succeeded
nullable representa la fase reportada por Cargo; errores posteriores de doctests
quedan en el log. Fallar compilación o tests es failed/isError=false. La cola humana
no se convierte en conteos; eventos Cargo adicionales tras build-finished hacen
ambigua la fase y fuerzan incompletitud/build_succeeded=null. Timeout conserva
parciales sin passed. Los encabezados humanos stdout/stderr son falsificables por
el proyecto; no autentican origen. El timeout incluye preflight y transferencia,
además de compilación/tests; captura inicial, calibración y cleanup tienen controles
independientes. Siempre se espera cleanup del árbol antes de reutilizar el worker.
R2 usa el runtime aprobado con fuente RO y red denegada; readOnlyHint describe esos
efectos host, no ausencia de ejecución de código. Véase ADR-037 y evidencia M1-06.

Para rust.test, una cola con marcadores Cargo (incluso malformados) o cualquier
cola posterior a un build fallido produce evidencia incompleta. La completitud
no autentica al productor. Un timeout puede conservar la fase reportada con
validation_complete=false; ese campo aislado nunca acredita éxito.


## rust.dependencies.audit (M1-07)

Entrada exclusiva: project_ref vivo. Requiere runtime aprobado para metadata
frozen/no-deps y el par host --rustsec-snapshot PATH/--rustsec-sha256 SHA256.
El archivo JSON v1 de ADR-038 se lee con handles APFS no-follow, máximo8MiB,
regular/single-link/stamps; el hash esperado se verifica antes de parsear.
No hay lookup de HOME, red, refresh ni instalación desde el runtime. El checksum
verifica integridad esperada por el host, no autenticidad editorial. CLI import,
firmas y antirollback durable tienen [evidencia separada M1-10](validation/M1-10.md); distribución
oficial y gate final siguen pendientes.

Metadata y lock consumen el mismo SourceBundle. Solo lock v4 acotado, identidades
no ambiguas y nodos alcanzables desde miembros del workspace. Esto no certifica
sincronización general manifest/lock ni resolución de features actualmente activas.
SQLite conserva/selecciona records autoritativos; RustSec0.32.0 compara versiones,
patched/unaffected y severidad. No se inventa una versión corregida si solo existe
un requisito o si patched está vacío. Se informa un camino más corto representativo
por cada raíz (máximo8,32paquetes), no todas las rutas alternativas.

La respuesta incluye source/lock/snapshot fingerprints, runtime, coverage, findings,
informational y evidence project/RustSec con latest_known. Solo origen crates.io
canónico se compara como tal; miembros locales se excluyen explícitamente y otros
sources producen coverage incompleta. Fresh<=24h con ambos timestamps conocidos y
no futuros permite passed limpio; aging/stale/unknown no pasan. Findings históricos
se retienen bajo unavailable. Vulnerabilidades con evidencia usable son failed,
los informativos solos pueden pasar. Ausencia es unavailable; integridad/path/lock
inválidos y budgets son blocked; fallos válidos conservan isError=false. Todo usa
el worker unido y revalida ProjectRef antes de publicar; esta tool no crea logs.

M1-07 review refinement: audit observations expose snapshot_record_count and
snapshot_sequence. Empty datasets are rejected; positive counts scope claims to the
host-selected records, without asserting global RustSec completeness or publisher
authentication. Sequence visibility is not durable antirollback. Snapshots are read
and checked again on each request; no implicit cache activation or refresh. Missing
configured files are unavailable; containment rejection is SANDBOX_DENIED and
elapsed deadlines remain COMMAND_TIMEOUT. A fresh vulnerable result is failed even
with incomplete coverage; stale evidence remains unavailable with retained findings.

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
source edits, global catalog import or new platform support. At the M1-09 baseline, M1-10..17 remained pending; current M1-10 evidence is below.

For quality, capture created_at/observed_at precede execution and freshness is
assessed at final publication. Passed applies to that captured generation, even
when its age is Aging/Stale; it is never proof of live file equality. All stage
runtime configurations must agree; command-specific execution fingerprints differ.
The final control check before lease renewal commits publication. Later cancellation
may suppress delivery in rmcp but cannot turn that committed result into a timeout;
retained logs remain bounded by owner authorization, TTL and quotas.

## CLI de catálogo M1-10 (no tool MCP)

`catalog status`, `catalog import SNAPSHOT`, `catalog sync --source SNAPSHOT`,
`catalog sync --url HTTPS_URL --allow-host HOST` y `catalog rebuild-index` requieren
`--store` y `--trust` absolutos. `--json` produce Report v1. Rebuild necesita
`--model-dir` y `--index-store`; importar semantic.index necesita modelo verificado
y feature local. Status permite modelo para índice embebido, o modelo+index-store
para derivado externo. Consulte [flags, reportes y errores](catalog-bundle-format.md).

Estos comandos administrativos tienen efectos locales; solo sync remoto intenta
red y lo declara incluso si falla. Es administración CLI, distinta de la nueva
tools de lectura `rust.catalog.status`, `rust.crate.search` y `rust.crate.inspect`.

## rust.catalog.status

M1-11 implementado y validado; [evidencia](validation/M1-11.md). Input cerrado `{}`; no requiere
ProjectRef y rechaza paths, refresh o download del peer. El host configura estos
flags de `serve --stdio` (paths absolutos y protegidos):

| Flags del host | Relación |
| --- | --- |
| `--catalog-store PATH --catalog-trust PATH` | Par obligatorio para configurar catálogo |
| `--catalog-model-dir PATH` | Opcional junto al par de catálogo |
| `--catalog-index-store PATH` | Opcional; requiere modelo; selecciona índice externo |

Tras bootstrap, la primera llamada admitida carga una generación read-only, sin
lease administrativa ni limpieza de staging. Catálogo/modelo/índice y fallos de carga
se conservan por sesión; `lifecycle=session_generation_restart_to_reload` indica que
imports o rebuilds posteriores requieren reiniciar. Sin índice externo se considera
el índice incluido en el bundle. `local` debe estar compilada para cargar E5/LanceDB;
core informa `feature_disabled` cuando se configura ese camino, sin perder SQLite.

`data.context` contiene catálogo, reserva, modelo, índice semántico y RustSec. Cada
componente es `available` con identidad/evidencia validada o `unavailable` con razón
fija; un resultado `passed` puede contener componentes indisponibles. El catálogo
incluye publisher/channel, fingerprints, secuencia, schema, conteo y presencia del
payload RustSec bundled; la reserva declara `pending` si no coincide con active.
La reserva sigue observable aunque falte active. Modelo e índice exigen validación
nativa, identidad común y cobertura completa de nombres; su fallo conserva SQLite.

Snapshots usan `latest_known` y freshness reevaluada con el reloj actual. RustSec
refleja `--rustsec-snapshot`/hash utilizados por audit, releídos en cada llamada;
el payload bundled no sustituye esa fuente. Network informa
`acquisition_allowed=false`, `enforcement=runtime_api_disabled`, sin claim de sandbox
OS global. Deadline120s cooperativo en el worker joined compartido; cancelación o
timeout retienen admisión hasta finalizar trabajo nativo y descartan éxito tardío.
El resultado MCP completo, incluidas representaciones textual/estructurada, tiene
cap128KiB. [ADR-042](adr/ADR-042-catalog-runtime-status.md).
0.1.0 no incluye catálogo, trust ni fixture oficial; cualquier configuración procede
del host. La clave de fixture nunca identifica a IUMotion Labs ni a una release.

## rust.crate.search

M1-12 implementado; [gate/revisión registrados](validation/M1-12.md). Input cerrado:

```json
{
  "query": "serialización JSON",
  "mode": "hybrid",
  "limit": 10,
  "filters": {
    "msrv_lte": "1.80",
    "allow_yanked": false,
    "include_prerelease": false
  }
}
```

Solo `query` es obligatorio: hasta256 bytes UTF-8,16 términos y sin caracteres de
control. Defaults: `mode=hybrid`, `limit=10` (1..50), filters vacío, yanked/prerelease
false y MSRV sin restricción. `msrv_lte` acepta major.minor[.patch] decimal canónico,
sin ceros iniciales, sufijos ni espacios; comparación normaliza patch omitido a0.
Una versión sin MSRV canónico comprobable queda excluida cuando se solicita ese filtro.
No se aceptan SQL/FTS personalizados, paths, modelo, refresh ni download.

| Modo solicitado | Ranking cuando sus canales están disponibles |
| --- | --- |
| `lexical` | FTS5 con términos literales escapados unidos por AND; BM25 menor primero |
| `semantic` | Solo candidatos E5/Lance; squared-L2 menor primero |
| `hybrid` | Unión por RRF: suma de `1/(60+rank)` por canal, ranks desde1, mayor primero |

Los empates se resuelven por nombre. Se conservan rank/score de cada canal; BM25 y
L2 no se equiparan ni miden calidad/seguridad. Modelo/índice ausente, deshabilitado,
inválido o incompatible, o fallo de inferencia/índice, produce `effective_mode=lexical`
y `fallback` explícito con los mismos filtros. Cancelación/deadline no degrada a éxito.

SQLite selecciona la mayor SemVer conocida que cumpla filtros antes del limit final,
entre hasta64 versiones por crate. `latest_known_stable` se calcula independientemente
de esos filtros y conserva `yanked`; stable significa sin prerelease. `selected_version`
puede ser anterior e incluye licencia/MSRV nullable, publicación e IDs de advisories
listados. Lista vacía no acredita seguridad ni cobertura completa de RustSec. Nombre,
descripción, repository y versions proceden de SQLite; índice solo aporta candidatos.

La ventana es50 candidatos por canal, unión hasta100. `window` informa candidatos,
examined/filtered_out/eligible/returned, `limit_truncated` y `omitted_by_output`;
no afirma `has_more` ni completitud global. `coverage=candidate_window_only` y
`advisory_interpretation=snapshot_listed_ids_only` hacen explícitos esos límites.
La evidencia usa `latest_known` y freshness del reloj de consulta; modelo/índice solo
aparecen cuando el canal semántico ha funcionado.

Search comparte la instancia/provider/generación retenida de status, sin otra carga
ni adquisición. Se admite tras bootstrap en el mismo worker joined, con120s
cooperativos que incluyen validación JSON y encoding. El cap512KiB del CallToolResult
completo incluye texto y structuredContent; se eliminan resultados del final del ranking,
conservando prefijo y facts íntegros y actualizando los conteos. Si ni metadata cabe,
se devuelve OUTPUT_LIMIT_EXCEEDED. La admisión permanece ocupada hasta el retorno
real y un éxito tardío se descarta. [ADR-043](adr/ADR-043-catalog-search-modes.md).

## rust.crate.inspect

M1-13 implementado; [gate aprobado](validation/M1-13.md). Input cerrado:

```json
{
  "name": "serde",
  "section": "versions",
  "limit": 20,
  "offset": 0
}
```

Solo `name` es obligatorio:1..64 caracteres ASCII alfanuméricos, `_` o `-`.
Defaults: section=`overview`, limit=20 (1..50), offset=0 (0..128), version y
snapshot_fingerprint ausentes. `version` es SemVer exacta de hasta128 bytes,
validada antes de consultar SQLite. No acepta paths, SQL, refresh ni download.

| Section | Version | Datos |
| --- | --- | --- |
| `overview` | Opcional; offset debe ser0 | Escalares de crate y selected_version nullable |
| `versions` | Prohibida | Versiones con yanked, MSRV/licencia/publicación nullable y counts |
| `features` | Obligatoria | Versión exacta y nombres de features; sin expansiones |
| `dependencies` | Obligatoria | Versión exacta y name/requirement/kind/optional registrados |
| `advisories` | Obligatoria | Versión exacta e IDs listados; sin auditoría RustSec completa |

Cada resultado exitoso incluye name, snapshot_fingerprint, sequence y evidencia
con provenance/freshness reevaluada, bajo semántica `latest_known`.
`lookup.kind` distingue `crate_not_found`, `version_not_found` y `found` con página.
Una colección vacía sigue siendo found. Catálogo no disponible es `unavailable`;
fingerprint distinto produce `blocked`/`SNAPSHOT_MISMATCH` antes de leer facts.

La página incluye overview con description, repository declarado no verificado,
updated_at, version_count y latest_known_stable independiente de la versión
seleccionada: mayor SemVer sin prerelease, conservando yanked; null si no existe.
Documentation y source son `{ "status": "unknown", "reason": "not_recorded_in_snapshot" }`;
source de paquete no se infiere de la provenance del catálogo. Versiones ordenan
por SemVer descendente; features/advisories por nombre ascendente y dependencias
por nombre y kind. Hasta64 versiones y128 elementos por colección.

`pagination` expone offset/total/returned/next_offset/omitted_by_output. Para continuar,
repetir name/section/version, usar next_offset y el snapshot_fingerprint recibido;
el fingerprint es obligatorio para offset>0. Cada combinación es una consulta
explícita, no un cursor opaco ni una credencial. Offset==total permite página vacía;
offset>total es input inválido. Una generación distinta requiere reiniciar paginación.

Comparte el provider SQLite retenido de status/search y no requiere E5/Lance.
El worker joined conserva admisión hasta completar I/O, validación de facts/schema
y encoding con120s cooperativos. El cap512KiB cubre CallToolResult completo,
incluidos texto y structuredContent. Si hace falta, elimina entradas enteras del
final de la página, conserva prefijo y recalcula next_offset=offset+returned;
no recorta facts ni salta registros. Overview es indivisible y nunca se vacía una
colección para producir continuación sin progreso: si el resultado irreducible
no cabe, devuelve OUTPUT_LIMIT_EXCEEDED. Cancelación/deadline descartan éxito tardío.
[ADR-044](adr/ADR-044-paged-crate-inspection.md).

## CLI y doctor M1-14

```text
rust-engineering-mcp version [--json]
rust-engineering-mcp doctor [--active] [--json] [host flags]
rust-engineering-mcp capabilities [--json | --human] --docker PATH --docker-socket PATH --state-root PATH --probe-image sha256:ID
```

Son comandos CLI, no tools MCP; se conservan las trece tools. Version mantiene su
línea humana y añade JSON format_version1 con package/version, compiled_local,
target_os y target_arch. No demuestra capabilities de ese target. Capabilities
mantiene sus probes activos y JSON por defecto; --human representa el mismo resultado.

Doctor comparte los flags cerrados de serve: --root (hasta16), --project-ttl-secs
(1..86400), --catalog-store/--catalog-trust, --catalog-model-dir y
--catalog-index-store (este último requiere modelo); --rustsec-snapshot junto con
--rustsec-sha256; --docker/--docker-socket/--state-root/--rust-image juntos y con la
imagen Rust aprobada. No descubre configuración del proyecto ni ejecutables en PATH.
Los flags de catálogo de doctor usan el prefijo --catalog-, a diferencia de la CLI
administrativa catalog. No se admiten flags duplicados salvo --root repetible.

Pasivo abre archivos configurados mediante los adapters seguros; puede cargar el
modelo/índice nativos, pero no ejecuta subprocesses ni adquiere la lease del store.
Runtime y herramientas del host quedan not_checked o not_configured. --active,
con runtime configurado, autoriza calibración y las observaciones fijas de
rustc/cargo/componentes en la imagen aprobada. Usa un source en memoria del producto,
no una root del usuario. cargo-audit figura not_used: el motor es la biblioteca RustSec.

El JSON format_version1 contiene operation, mode, status, duration_ms, checks,
catalog y runtime. Cada check tiene id/scope/status/reason/component_reason/action/
severity finitos. La salida humana deriva del mismo reporte. Passed y warning salen0;
failed sale1, incluida una dependencia configurada inválida; errores de sintaxis salen2.
Servicios opcionales no configurados y freshness aging/stale/unknown son warnings.
Las acciones son recomendaciones: nunca se sincroniza, instala o repara automáticamente.

Límite128KiB incluyendo terminador; deadlines cooperativos120s pasivo/900s activo.
SIGINT/SIGTERM/SIGHUP cancelan y esperan el worker y cleanup; la finalización puede superar
el deadline durante cleanup o cómputo nativo. El resultado describe el diagnóstico,
no readiness universal. [ADR-045](adr/ADR-045-cli-doctor.md).

Una salida bloqueada por el consumidor vence a los5s o por señal, después de
terminar la observación y cleanup; sale1 y puede no entregar un JSON completo.

## Perfil del artifact 0.1.0

El único archive previsto es core para `aarch64-apple-darwin`. Debe pasar desde sus
bytes empaquetados `version`, doctor pasivo, discovery, inventario exacto de trece
tools y los caminos estructurados degraded/unavailable esperados. Conserva SQLite
lexical cuando el host aporta un catálogo válido, pero no incluye modelo, ORT,
LanceDB, catálogo, trust, fixtures, Docker ni toolchain. El perfil `local` completo
continúa siendo M1 y se califica separadamente desde fuente según ADR-048.

## Escritura local M2 en desarrollo

[ADR-050](adr/ADR-050-local-coordinated-mutation.md) fija edición local
coordinada para M2: permiso de escritura del host configurado una vez, preview/diff,
precondiciones, locks entre instancias MCP, journal y recuperación conservadora.
No exige servicios privilegiados, sudo, cuentas ni cambios de ownership del proyecto.
El host mantiene estables las roots y evita editar simultáneamente los archivos
afectados durante el commit breve. El MCP no bloquea al IDE ni promete CAS o atomicidad
visible multiarchivo. Conflictos observados detienen la operación; los posteriores
pueden requerir recuperación conservando evidencia y sin pisar bytes desconocidos.
La rama de desarrollo incorpora `rust.manifest.patch` para lints del `Cargo.toml`
raíz e integra `rust.fmt.apply` para archivos Rust existentes. Ambos usan
preview/commit/receipt, con grants separados `--allow-manifest-write WORKSPACE_ROOT`
y `--allow-fmt-write WORKSPACE_ROOT`. La calificación conjunta sigue en curso;
estas capacidades no forman parte de la release `0.1.0`. Las trece tools M1 y su
sandbox se conservan. Fix, dependencias y las demás familias de patch siguen
pendientes; el tablero enlaza la evidencia sin anunciar M2 terminado.

### Contrato inicial de `rust.manifest.patch` (M2-01)

Entrada cerrada `{project_ref, action}`. `action.mode` selecciona:

- `preview`: `expected_project_fingerprint` de `project.open` y `edit` tipado.
  `edit.operation` es `lint_set` (scope package/workspace, tool rust/clippy,
  name, level allow/warn/deny/forbid, priority opcional) o `lint_remove`
  (scope/tool/name). No acepta paths, punteros JSON ni flags Cargo.
- `commit`: `plan_id`, `plan_digest` exactos del preview y `idempotency_key`
  ASCII alfanumérica, guion o underscore, de 1–64 caracteres.
- `receipt`: `operation_id` original y `recover` booleano. Requiere referencia
  viva obtenida al reabrir y grant vigente; el ID no concede acceso.

Preview devuelve el diff completo autorizado, hashes before/after, validación Cargo
y duración del plan (600 s); no publica source. Commit revalida el source completo,
no vuelve a generar el candidato aprobado. El recibo separa intended_after de
el efecto terminal registrado: committed acredita after; no_change y aborted
acreditan before; recovery_required deja effect_after desconocido. Es evidencia
histórica latest_known, con freshness unknown, no lectura actual del filesystem.
Un candidato inválido observado por Cargo es failed con isError=false; conflicto,
permiso denegado, recuperación y aborto son resultados operacionales isError=true.

Límites iniciales: cuatro planes/64 MiB agregados, Cargo.toml 256 KiB, diff128 KiB,
respuesta MCP completa512 KiB,128journals/256MiB store. Se rechaza una salida excesiva
antes de retener un plan. Solo tablas TOML estándar, package/workspace rust/clippy;
las demás familias se incorporarán en M2-06. Rechaza mixed newlines si la edición
cambia bytes y preserve los bytes exactos para un no-op. El source capturado y el
candidato se validan con el runtime aprobado y la política frozen de M1.


### `rust.fmt.apply` (M2-02 en integración)

Entrada cerrada `{project_ref, action}`. Preview exige únicamente
`expected_project_fingerprint`; no admite paths, flags, comando ni configuración
aportados por el peer. Commit/receipt tienen los mismos campos que manifest.patch,
pero los planes y receipts quedan ligados al grant de formato.

El runtime produce un candidato completo en tmpfs acotado y hace fmt.check en otro
job read-only antes de aprobarlo. La validación registra ambos fingerprints de
 ejecución y el hash del candidato. Solo se admiten reemplazos de hasta 128 `.rs`
existentes; Cargo.toml, Cargo.lock, directorios y otros archivos permanecen exactos.
La configuración rustfmt admitida ya presente forma parte del snapshot aprobado;
la configuración Cargo del proyecto continúa prohibida. No se aceptan altas/bajas.

La publicación usa journal v2 y puede ser parcialmente visible durante el commit.
Recovery de un prefijo publicado solo completa el sufijo aprobado cuando toda la
generación lógica, incluidos archivos no editados, sigue siendo la conocida. En
caso contrario conserva bytes/evidencia y devuelve recovery_required. No ejecuta
rustfmt otra vez ni recalcula el diff. Los cuatro planes y 64 MiB en memoria se
comparten entre las tools M2. El no-op también tiene un receipt verificable.

### CLI `cargo-vendor inspect`

`cargo-vendor inspect --directory PATH [--json]` inspecciona un directorio absoluto
del host, con captura protegida y checksum de cada archivo. No ejecuta Cargo,
modifica archivos ni descarga. JSON format_version1 contiene status, error_code,
message, tree_fingerprint, file_count, total_bytes y packages (nombre, versión,
checksum de paquete). Éxito sale0, rechazo operacional sale1 y sintaxis inválida
sale2. No incluye source. La salida completa está limitada a512KiB; captura tiene
deadline cooperativo30s y la entrega a un pipe bloqueado5s. Los límites de datos
son16MiB/4096entries/1MiBarchivo y paths portables100bytes. Soporte positivo requiere
el adapter macOS ARM64/APFS; en otros targets se rechaza antes del filesystem.
