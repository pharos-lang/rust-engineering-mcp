# Tools

`rust.project.open`, `rust.project.inspect`, `rust.toolchain.inspect`, `rust.check`, `rust.fmt.check`, `rust.clippy`, `rust.test`, `rust.dependencies.audit`, `rust.diagnostics.explain`, `rust.quality.gate`, `rust.catalog.status`, `rust.crate.search` y `rust.crate.inspect` están implementadas en este checkout; los gates de [M1-11](validation/M1/11.md) y [M1-12](validation/M1/12.md) están registrados; M1-13 tiene [gate aprobado](validation/M1/13.md). rmcp 3.2.0 gestiona discovery, negociación y dispatch. La release `0.1.0`
devuelve trece definiciones sin cursor. El checkout `0.3.0` devuelve 31:
añade cinco tools M2, cuatro M3, cinco M4 y cuatro M5. Las cinco M4 están
implementadas y calificadas localmente; el hito espera la confirmación final de
evidencia. Las cuatro M5 están implementadas y **calificadas localmente**; su
estado por corte está en los [contratos M5](#contratos-m5--medición-de-rendimiento).

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
| `rust.catalog.status` | M1-11 | Implementado; [evidencia M1-11](validation/M1/11.md) |
| `rust.crate.search` | M1-12 | Implementado; [gate/revisión registrados](validation/M1/12.md) |
| `rust.crate.inspect` | M1-13 | Implementado; [gate aprobado](validation/M1/13.md) |

`rust.dependencies.inspect` no es tool pública M1. Los contratos tipados, schemas y
resultados estructurados se implementarán según ADR-006 y ADR-015. La publicación
de una tool no sustituirá los controles de disponibilidad/policy por plataforma.
El archive core descubre estas trece definiciones, pero no promete que todas tengan
un camino positivo sin configuración del host. Ejecución requiere el gateway
aprobado; semántica requiere un build `local` source-qualified y assets aportados
por el usuario. Los resultados unavailable/blocked/degraded son parte del contrato.

## Vertical M3-01 en el checkout de desarrollo

`rust.test.nextest` se incorporó como la tool número 19; el checkout integrado
conserva las cuatro definiciones M3 dentro del inventario, hoy de 31 tools.
Selecciona
package/features/target y un filtro cerrado, usa siempre el perfil `rust-mcp`, no
ejecuta doctests y obtiene
counts, attempts, retries, flaky/leak/timeout sólo del JUnit acotado. JUnit,
stdout y stderr se publican en el store durable Stage 1 cuando existe un state
root calificado, con URIs privadas de índice/chunk vinculadas al ProjectRef vivo.
Sin state root, o si attach devuelve `UnsupportedStateRoot`/`Busy`, usa el store
efímero M1 Stage 0. Truncación, expiración o ausencia de artifacts queda explícita
y no puede promover el resultado a pass.

MCP Tasks está anunciado tras la calificación M3-02. Sin declaración del peer,
`execution_mode=auto|synchronous` sólo califica con perfil default y
`timeout_seconds <= 60`; `auto` mayor devuelve `TASKS_REQUIRED` antes del worker y
`task` produce `-32602`. Con declaración mutua, `auto|task` crea el job con default
300 s y máximo 3.600 s. La ejecución Docker está calificada con el perfil
nextest-only de ADR-064; M1/M2 conservan sus perfiles byte-identical. Passing=0,
failing/timeout=100, build-error=101 y no-tests=4 se derivan de observación real;
cancelación del gateway no se inventa como exit code.

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
See ADR-035 and [evidence](validation/M1/04.md).

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
firmas y antirollback durable tienen [evidencia separada M1-10](validation/M1/10.md); distribución
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

M1-11 implementado y validado; [evidencia](validation/M1/11.md). Input cerrado `{}`; no requiere
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

M1-12 implementado; [gate/revisión registrados](validation/M1/12.md). Input cerrado:

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

M1-13 implementado; [gate aprobado](validation/M1/13.md). Input cerrado:

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

La release `0.1.0` conserva exactamente las trece tools M1 anteriores. El checkout
`0.3.0` añade cinco definiciones [calificadas localmente](validation/M2/07.md): `rust.manifest.patch`,
`rust.fmt.apply`, `rust.fix.apply`, `rust.dependency.add` y
`rust.dependency.remove`. Todas usan input cerrado, un worker joined con deadline
de 240 s, respuesta MCP completa de 512 KiB y el ciclo `preview` → `commit` →
`receipt`. Sus annotations son readOnly false, destructive true, idempotent false
y openWorld false; el peer debe tratar commit y recovery como operaciones
destructivas dentro del grant.

Cada tool exige un permiso host distinto sobre la raíz exacta del workspace:

| Tool | Grant |
| --- | --- |
| `rust.manifest.patch` | `--allow-manifest-write WORKSPACE_ROOT` |
| `rust.fmt.apply` | `--allow-fmt-write WORKSPACE_ROOT` |
| `rust.fix.apply` | `--allow-fix-write WORKSPACE_ROOT` |
| `rust.dependency.add` | `--allow-dependency-add WORKSPACE_ROOT` |
| `rust.dependency.remove` | `--allow-dependency-remove WORKSPACE_ROOT` |

Un grant, `project_ref`, plan o receipt no autoriza otra clase de operación. La
raíz debe estar dentro de una `--root` de lectura, el runtime Docker completo debe
estar configurado y `--state-root` debe quedar fuera de todas las roots. Los planes
vencen a los 600 s y comparten un máximo de cuatro entradas/64 MiB. El diff tiene
un límite de 128 KiB; una respuesta que no cabe se rechaza antes de retener el plan.
Source y candidato admiten 16 MiB totales, 4096 entradas, 1 MiB por archivo, paths
de 100 bytes y profundidad 32. El store admite 128 journals/256 MiB, con 48 MiB
por journal y reserva para recovery; al llenarse rechaza trabajo nuevo.

### Ciclo común preview/commit/receipt

Preview recibe la identidad devuelta por `rust.project.open`, captura y valida un
candidato completo, y no modifica el host. Por ejemplo, commit consume exactamente
el plan y digest recibidos. `idempotency_key` admite 1–64 caracteres ASCII
alfanuméricos, guion o underscore:

```json
{
  "project_ref": "prj_0123456789abcdef0123456789abcdef",
  "action": {
    "mode": "commit",
    "plan_id": "mut_0123456789abcdef0123456789abcdef",
    "plan_digest": "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    "idempotency_key": "agent-step-17"
  }
}
```

Tras commit se debe reabrir el proyecto y usar el nuevo `data.project_ref` en
TODAS las llamadas posteriores, incluidos receipt y recovery del `operation_id`. `recover: true` solicita recuperación conservadora; el ID por sí mismo
no concede acceso:

```json
{
  "project_ref": "prj_fedcba9876543210fedcba9876543210",
  "action": {
    "mode": "receipt",
    "operation_id": "mut_0123456789abcdef0123456789abcdef",
    "recover": false
  }
}
```

Preview devuelve `kind: "preview"`, plan, expiración, lista de archivos, diff y
validación. Receipt devuelve estado `committed`, `no_change`, `aborted` o
`recovery_required`, fingerprints de efecto y la validación ligada al plan. La
semántica de evidencia es `latest_known`: un receipt no es una lectura actual del
filesystem. Un candidato rechazado por Cargo es un resultado `failed`; conflictos,
permisos, plan vencido, recovery y datos offline ausentes son resultados
operacionales con `isError: true`. La ausencia de dataset/crate offline es `unavailable` con `offline_data_missing`; datos adulterados son `blocked` con `offline_data_invalid`.

El modo es `local_coordinated`. El commit vuelve a comprobar proyecto y source y
publica los bytes aprobados mediante handles no-follow y journal. Los locks
coordinan instancias que comparten state root; no excluyen al IDE, Git u otros
writers. No hay CAS ni atomicidad visible multiarchivo. Los cambios externos pueden
causar `conflict` o `recovery_required`.

La CLI `mutation list/prune` administra journals terminales, pero el checkout no
incluye un updater o downgrade gestionado. Los journals pendientes deben
reconciliarse antes de ejecutar un binario anterior; `0.1.0` no conoce su formato.

### `rust.manifest.patch`

Solo edita el `Cargo.toml` raíz mediante una operación semántica. Ejemplo de profile:

```json
{
  "project_ref": "prj_0123456789abcdef0123456789abcdef",
  "action": {
    "mode": "preview",
    "expected_project_fingerprint": "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    "edit": {
      "operation": "profile_set",
      "profile": "release",
      "setting": { "name": "lto", "value": "thin" }
    }
  }
}
```

Las familias cerradas son:

- `lint_set/remove`: scope `package|workspace`, tool `rust|clippy`, nombre y, para
  set, level `allow|warn|deny|forbid` y `priority` opcional;
- `feature_set/remove`: nombre y, para set, hasta 128 valores Cargo;
- `profile_set/remove`: profile `dev|release|test|bench`; settings `opt-level`,
  `debug`, `strip`, `debug-assertions`, `overflow-checks`, `lto`, `panic`,
  `incremental` y `codegen-units`, con valores tipados por schema;
- `workspace_dependency_set/remove`: dependencia crates.io con requirement
  explícito, package alias opcional, features, `default_features` y `optional=false`.

Lints y profiles usan metadata frozen no-deps. Features y workspace dependencies
cambian resolución y requieren el dataset vendor aprobado descrito abajo. No se
aceptan punteros TOML/JSON, path/git/registry alternativo, tablas patch/replace ni
flags Cargo. El manifest se limita a 256 KiB y un no-op conserva sus bytes.
`opt-level` acepta 0–3, `s` o `z`; `debug` acepta `none`, `limited`, `full`,
`line-tables-only` o `line-directives-only`; `strip` acepta `none`, `debuginfo` o
`symbols`; `lto`, boolean o `off|thin|fat`; `panic`, `unwind|abort`;
`debug-assertions`, `overflow-checks` e `incremental` son booleanos, y
`codegen-units` es un entero mayor o igual a uno.

### `rust.fmt.apply`

Preview no acepta paths, flags ni configuración suministrada por el peer:

```json
{
  "project_ref": "prj_0123456789abcdef0123456789abcdef",
  "action": {
    "mode": "preview",
    "expected_project_fingerprint": "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
  }
}
```

El gateway ejecuta rustfmt sobre staging y `fmt.check` sobre el candidato completo
en otro job. Solo puede reemplazar hasta 128 `.rs` existentes; no crea/elimina
paths ni modifica manifests, lock o directorios. La configuración rustfmt ya
capturada puede influir en el resultado; la configuración Cargo continúa prohibida.
La validación conserva los fingerprints de ambas ejecuciones.

### `rust.fix.apply`

Usa la misma forma preview/commit/receipt que fmt. El comando fijo selecciona
workspace, todos los targets y features por defecto, en modo frozen/offline; no
acepta edition migration, broken-code, selección arbitraria ni flags. Requiere un
Cargo.lock existente. Tras fix, un `cargo check` frozen independiente valida el
candidato completo. Build scripts y proc macros pueden influir en cualquier `.rs`
permitido: el caller debe revisar el diff exacto.

El dataset vendor de resolución no se proporciona a fix. El camino calificado no
promete workspaces arbitrarios con dependencias externas; si el input frozen del
runtime no basta, la operación falla cerrada sin candidato.

El perfil Docker dedicado conserva `network=none`, pero permite TCP loopback dentro
del namespace para la coordinación interna de Cargo. Esa excepción no se extiende
a M1, fmt, ingest, export ni resolución, y no debe describirse como denegación
absoluta de sockets.

### `rust.dependency.add` y `rust.dependency.remove`

Add recibe requirement explícito y opciones tipadas. Este ejemplo añade un alias a
un package miembro:

```json
{
  "project_ref": "prj_0123456789abcdef0123456789abcdef",
  "action": {
    "mode": "preview",
    "expected_project_fingerprint": "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    "manifest_path": "crates/app/Cargo.toml",
    "dependency_kind": "normal",
    "target": null,
    "name": "unicode",
    "requirement": "=1.0.24",
    "package": "unicode-ident",
    "features": [],
    "optional": false,
    "default_features": true
  }
}
```

Remove usa el mismo selector sin spec:

```json
{
  "project_ref": "prj_0123456789abcdef0123456789abcdef",
  "action": {
    "mode": "preview",
    "expected_project_fingerprint": "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    "manifest_path": "crates/app/Cargo.toml",
    "dependency_kind": "dev",
    "target": "cfg(unix)",
    "name": "test-helper"
  }
}
```

`manifest_path` por defecto es `Cargo.toml`, pero Cargo debe corroborarlo como
package miembro; un workspace virtual exige seleccionar un manifest miembro.
Kind es `normal|dev|build`. Target es una clave TOML validada, no un argumento
Cargo. Add no reemplaza silenciosamente una definición diferente. Remove elimina
solo la clave seleccionada, incluida una forma dotted `renamed.workspace = true` o
una tabla propia de esa entrada, sin cambiar la definición global. Contenedores
inline/dotted que exigirían reescribir campos vecinos se rechazan como layout no
soportado. Ambas tools siempre requieren resolución offline aprobada.

### Datos Cargo offline y policy de lock

El operador prepara los datos fuera del servidor y después fija su identidad:

```text
cargo vendor --locked --versioned-dirs /ruta/privada/vendor
rust-engineering-mcp cargo-vendor inspect --directory /ruta/privada/vendor --json
```

La primera orden usa el Cargo del operador y puede requerir que este haya preparado
sus datos; nunca la ejecuta una tool MCP. Configura juntos:

```text
--cargo-vendor-dir /ruta/privada/vendor
--cargo-vendor-tree-sha256 sha256:DIGEST
```

La ruta debe ser absoluta, quedar separada de las project roots y acompañarse del
fingerprint exacto producido por inspect. Es una fuente selectiva crates.io de hasta
16 MiB, 4096 entradas y 1 MiB por archivo. El servidor la captura una vez mediante
handles no-follow durante el preview, verifica checksums/layout y usa esos bytes
inmutables en un tmpfs separado read-only. No importa CARGO_HOME, configuración,
cachés, credenciales ni registries alternativos; tampoco instala o descarga durante
runtime.

Cargo ejecuta metadata offline sobre staging escribible y después metadata frozen
sobre la resolución. La validación informa `cargo_metadata_offline_then_frozen`,
dataset, ejecución de resolución, lock resuelto, manifest seleccionado y
`lock_policy: "preserve_presence"`. Un lock existente se actualiza en el mismo
plan (`updated_existing`). Sin lock, se valida con uno transitorio y se excluye del
candidato (`transient_unpublished`). Datos ausentes/corruptos o resolución fallida
no producen un candidato manifest-only.

La CLI `cargo-vendor inspect --directory PATH [--json]` solo inspecciona: no ejecuta
Cargo, no modifica el árbol y no descarga. Su JSON format_version1 incluye estado,
fingerprint, conteos y paquetes. Éxito sale 0, rechazo operacional 1 y sintaxis
inválida 2; salida máxima 512 KiB. La captura comparte los límites anteriores,
deadline cooperativo de 30 s y soporte positivo macOS ARM64/APFS.

El límite de cuatro planes aplica a propuestas pendientes: los planes terminales
dejan capacidad para nuevas propuestas en la siguiente admisión. Un commit con
plan ausente/expirado solo puede repetir un journal existente con ID, digest y key
exactos, bajo grant vivo e identidad física original. No inicia efectos nuevos sin
preview vigente. Prune retira ese replay; un receipt terminal describe historia,
no el source actual. Véase [ADR-059](adr/ADR-059-terminal-plan-retirement-and-durable-replay.md).

## `rust.coverage`

`rust.coverage` reserva una selección Cargo cerrada (`package` o `workspace`,
features, target y un límite de tiempo) para una captura de cobertura LLVM. El
contrato no acepta flags, rutas de salida, regex de exclusión, doctests, branch ni
MC/DC. La ejecución calificada hace una sola fase `cargo llvm-cov --no-report` y
deriva JSON, LCOV y un `ArchiveBundle` HTML de ese mismo profdata. JSON completo,
LCOV y HTML son artifacts; la respuesta solo puede contener el resumen paginado.

Un porcentaje solo existe si su denominador es distinto de cero; archivos o scopes
sin líneas, regiones o funciones instrumentadas no se reportan como 0% ni 100%.
Las rutas compartidas se deduplican en el agregado. JSON y LCOV salen únicamente
desde nombres fijos en `/work/coverage`; el directorio HTML sale como un único
tar USTAR validado, sin preview ni interpretación de HTML. En especial, enlaces,
devices y rutas que escapen del bundle son rechazados. La publicación Stage 1 usa
el store durable cuando el host lo configuró y Stage 0 queda como fallback
acotado; ambas referencias se devuelven como Resources privados. Cada miembro
durable admite hasta 32 MiB. El HTML validado se conserva como un solo
`ArchiveBundle` y cuenta una vez contra los 128 miembros del job; sus entradas USTAR
siguen limitadas aparte. Si falla un miembro se marca omitido y se publican los
demás, sin dejar una reserva inaccesible.

Antes de ejecutar se comprueban la identidad fijada de `cargo-llvm-cov` y
`llvm-tools-preview`; ausencia o mismatch devuelve `Unavailable`. Sin Tasks
declarado, `auto` y `synchronous` solo califican para `timeout_seconds <= 60`; un
`auto` mayor devuelve `Blocked`/`TASKS_REQUIRED` y `task` se rechaza con `-32602`
sin `data`. Con declaración mutua, `auto|task` usa el presupuesto job de 300 s por
defecto y 3.600 s máximo, igual que las otras tools M3. Los límites de
stdout/stderr y cada artifact son 512 KiB y 8 MiB respectivamente. Una captura
truncada, timeout o parse incompleto es `Blocked`, nunca una cobertura limpia.

**Calificación vigente (2026-09-06):** cargo-llvm-cov 0.9.0 completa una sola
captura instrumentada y deriva JSON, LCOV y HTML desde el mismo profdata. El target
privado ADR-065 es read-write en run/report porque el plugin materializa allí su
lista profraw y profdata combinado; el keeper es read-only y los exporters no ven
ese target. Los modos ejecutables no sobreviven el ingest de source, el report
sigue `noexec`, la red sigue denegada y cleanup elimina los tres volúmenes. El gate
completo pasó 62/62 (nextest 19, Tasks 7, coverage 8, SemVer 18 y
mutation 10). El fixture calibrado
fija líneas 4/4, regiones 8/9 y funciones 2/2; los aliases de un archivo compartido
se deduplican tras normalización confinada a `/source`. Un crate sin código
instrumentable produce `no coverage data found` y queda incompleto sin porcentaje
fabricado. Véase [`docs/validation/M3/03.md`](validation/M3/03.md).

## `rust.semver.check`

`rust.semver.check` compara dos `project_ref` locales y vivos. Captura primero el
baseline y después el candidato, revalida cada captura y vuelve a validar ambos
grants antes de publicar. Cada lado conserva evidencia `SnapshotEvidence`
independiente; la respuesta no afirma que dos roots formen un snapshot atómico.

La entrada expone una sola selección cerrada (`package`, `features`,
`all_features`, `no_default_features`, `target`) aplicada de forma idéntica a
ambos lados. No acepta flags, revisiones Git, versiones de registry, URLs ni
selecciones asimétricas. Un target `lib` ausente se detecta en la estructura ya
capturada y devuelve `unavailable` antes de ejecutar semver. El gateway monta el
candidato read-only en `/source` y el baseline read-only en `/baseline`, usa
`--color never`, `GIT_DIR=/nonexistent`, `GIT_CEILING_DIRECTORIES=/`, `NO_COLOR=1`,
red deshabilitada y el perfil seccomp quality de ADR-064.

La respuesta repite `baseline_selection` y `candidate_selection`; ambas se
construyen del mismo valor validado y permiten comprobar la simetría efectiva sin
inferirla del argv ni del texto del plugin.

El plugin 0.50.0 no ofrece findings machine-readable. Exit code y conteos
deny/warn forman el resultado coarse; los campos por finding son best-effort y
siempre `partial`. Un formato no reconocido, salida truncada o exit 100 sin ningún
finding deny se degrada a `incomplete`/`blocked`, nunca a «sin ruptura». La salida
cruda no coloreada se captura por el stream supervisado fijo (el plugin no ofrece
ruta de reporte) y se conserva como Resource privado. Con state root calificado,
Stage 1 es el default durable; su descriptor queda ligado a un digest
domain-separated del par ordenado baseline/candidate y al owner candidato. Sin
state root, o si el attach devuelve `UnsupportedStateRoot`/`Busy`, usa el
ArtifactStore M1 Stage 0. Ambos caminos tienen truncación/omisión explícita. La
respuesta incluye como máximo 16 findings y
mantiene los conteos/`findings_omitted`, incluso con texto hostil máximamente
escapado, para permanecer bajo 512 KiB contando el mirror estructurado/textual.

Tasks está anunciado. Sin declaración del peer, `auto` y `synchronous` califican
solo cuando `timeout_seconds <= 60`; un `auto` mayor devuelve `TASKS_REQUIRED`
antes de admisión y `task` devuelve `-32602`. Con declaración mutua, `auto|task`
usa 300 s por defecto y 3.600 s máximo. No se instala ni descarga nada. La
calibración Docker está registrada en
[M3-04-semver-calibration](validation/M3/04-semver-calibration.md).

## `rust.mutation.test`

`rust.mutation.test` ejecuta `cargo mutants` sobre un `project_ref` vivo. La
prueba baseline es obligatoria (`--baseline run`): un baseline que falla es un
resultado `failed` con su propia evidencia, nunca un informe de mutación limpio.
El binario fijado 27.1.0 solo acepta `run` y `skip` en `--baseline`, de modo que
el corte expresa la intención «baseline automático» como la ejecución explícita
no omitible.

La entrada es una selección cerrada (`package`, `features`, `all_features`,
`no_default_features`, `target`) más `max_mutants` (1..=100, por defecto 100) y
`mutant_timeout_seconds` (1..=60, por defecto 60). No acepta sharding,
`--in-place`, override de baseline, ruta de salida ni flags libres de cargo. El
target se reenvía como `--cargo-arg=--target=<triple>` y solo admite el único
triple instalado.

Contención: los mutantes se aplican únicamente en una copia privada del sandbox.
`/source` se monta read-only en todas las fases de mutación, la copia vive en un
tmpfs propio del contenedor (`/mutants-scratch`, `0700`, uid/gid 65534, `nosuid`,
`nodev`) que se destruye con el contenedor y que ningún exportador monta, y no
existe ninguna ruta de decodificación que devuelva fuente mutada al host. Solo
salen bytes de informe, por tres exportadores `tar` de argv fija que leen
`/mutants/mutants.out` montado read-only: `outcomes.json`, un `ArchiveBundle`
USTAR validado con `diff/`, `logs/` y las listas por clase, y `lock.json`.
`docker cp` no se usa. `mutants.out/lock.json` registra usuario y host desde
dentro del guest: se comprueba que sean los valores del sandbox y se redacta
cualquier forma de host; el archivo nunca se publica ni se incluye en el bundle.

La respuesta no publica una afirmación `source_unchanged`: el proceso no dispone de
una segunda lectura independiente del host capaz de hacerla falsa. La inmutabilidad
se impone por el mount `/source` read-only y su verificación aplicada, y se evidencia
en el gate mediante el canary host-side antes/después de cada familia de fixtures.

El veredicto proviene solo de `outcomes.json` y de las listas
`caught/missed/timeout/unviable`, leídas por un parser acotado en tamaño,
profundidad y número de elementos. El texto legible de la herramienta nunca es
oráculo. Las tres fuentes —el denominador de la pasada de listado, las clases de
`outcomes.json` y los totales de las listas— deben coincidir; cualquier
desacuerdo degrada la evidencia a inválida. `missed >= 1` es `failed`;
`timeout`, `unviable` e incompleto nunca acreditan limpio, y cada conteo publica
su denominador explícito (`generated`, `tested`, `viable`).

Costo: el conjunto de mutantes se genera primero en una pasada de listado que no
compila ni ejecuta nada; si supera `max_mutants` el trabajo se rechaza con
`MUTANT_LIMIT_EXCEEDED` antes de construir nada. El timeout por mutante es
`<= 60 s`, el timeout de build es una función fija del anterior acotada a
`[60, 300] s`, y el presupuesto total del job es
`clamp(build_timeout + mutant_timeout × max_mutants, 300, 3600)` segundos.

Tasks está anunciado y ninguna selección de mutación entra en el presupuesto
síncrono de 60 s. Sin declaración del peer, `auto` devuelve `TASKS_REQUIRED` y
`task` se rechaza; con declaración mutua, `auto|task` crea el job. No se instala ni
descarga nada. La calibración Docker de exits, `outcomes.json` y conteos está en
[M3-05-mutation-calibration](validation/M3/05-mutation-calibration.md).

## MCP Tasks for M3 quality jobs (M3-02)

The four M3 quality tools share one negotiated execution rule. `execution_mode`
is closed to `auto`, `task`, and `synchronous`. With the Tasks extension mutually
declared, `auto` always creates an owner-bound task; it is never silently reduced
to a synchronous call. Explicit `task` without the peer capability is fixed
JSON-RPC `-32602` with no data. Without Tasks, `auto` uses synchronous execution
only for the already qualified at-most-60-second selection; otherwise the normal
tool result is `isError:true` with `TASKS_REQUIRED`, before worker admission.

A created task has a fixed 7,200,000 ms TTL and 1,000 ms poll hint. `tasks/get`
returns `working` until joined cleanup, then an inline `completed`, `failed`, or
`cancelled` payload. An ordinary tool result remains `completed` even when its
own `isError` is true. `tasks/cancel` records cooperative intent; it does not claim
`cancelled` until gateway cleanup is observed. `tasks/update` always returns the
fixed authorized error `-32602 task does not accept input`. Malformed, unknown,
expired, revoked and foreign identifiers all return byte-identical `-32602 task
unavailable` without data.

One task owns the existing ADR-030 worker permit through execution, collection,
publication and cleanup. A second tool call or worker-backed Resource read returns
the existing busy/`SANDBOX_DENIED` result; bounded `tasks/get|cancel|update` remain
responsive. Task records retain only validated bounded structured output; polling
reconstructs its JSON text mirror and rechecks artifact-member liveness.
An `unavailable` member observed during polling may be a transient projection of
registry/store contention rather than expiry; the stored record is unchanged, so
polling again after the active job completes can make the live member visible.

`TASKS_ADVERTISEMENT_READY` is enabled after the Docker lifecycle, five-version
declared/undeclared matrix, 30+30 budget series and stock-client gate passed.
Inspector 2.5.0 declared the extension and completed the task lifecycle. Codex
app-server 0.153.0 did not declare it and completed the supported synchronous path;
it therefore cannot create, poll or cancel M3 tasks. See
[M3-02 validation](validation/M3/02.md).

## Contratos M4 calificados localmente

El checkout añade cinco definiciones a `tools/list`, después de las 22
tools M1–M3 y en este orden: `rust.deny`, `rust.unsafe.scan`,
`rust.supply_chain.inspect`, `rust.quality.gate.v2` y `rust.miri`. El
[core de 19 etapas](validation/M4/core-gate.json), el
[full de 33 etapas](validation/M4/full-gate.json), el
[runtime de 19 selecciones](validation/M4/runtime.json) y los
[clientes](validation/M4/clients.json) pasaron localmente. Los 23 snapshots
anteriores se preservaron y se agregaron cinco nuevos. La confirmación final de
evidencia del hito sigue pendiente; no cambia la release `0.1.0`.

El core pasó sus 19 etapas con 1220 tests Rust, un doctest y 11 tests del helper.
El full pasó 33 etapas con el mismo inventario fuente: conservó 27 etapas ya
aprobadas y ejecutó seis frescas después de que un directorio E5 temporal vacío
se corrigiera seleccionando los assets locales existentes y reverificados. No
hubo descarga ni cambio de código; el [intento fallido](validation/M4/history/hardening-attempts/full-attempt-2/receipt.json)
permanece preservado. El runtime final pasó 19/19 sobre `25ed…`, incluidos los
[siete casos scanner](validation/M4/scanner-native.json) y las
[trece clasificaciones más siete admisiones Miri](validation/M4/miri-native.json).

Los cinco inputs rechazan campos desconocidos. `project_ref` es siempre una
referencia viva producida por `rust.project.open`; `execution_mode` conserva el
vocabulario cerrado `auto|task|synchronous`. Con Tasks negociado, `auto` o `task`
puede materializar el job. Sin esa extensión, `auto` usa el camino síncrono
calificado cuando `timeout_seconds <= 60`; con el timeout por defecto devuelve
`TASKS_REQUIRED`. `task` sin negociación y `synchronous` por encima de 60 s son
inválidos. Gate v2 solo admite sincronía para `strict` sin mutation: `release` y
mutation requieren Tasks.

| Tool | Input adicional y timeout | Evidencia y límite declarado |
| --- | --- | --- |
| `rust.deny` | Sin selección del cliente; 120 s por defecto y máximo. | Comparte una auditoría RustSec y ejecuta cargo-deny 0.19.7 solo para licenses, bans y sources sobre metadata frozen/offline. Requiere vendor, policy, snapshot RustSec, runtime y store durable autenticados por el host. Retiene hasta 128 findings y limita el resultado MCP completo a 512 KiB. |
| `rust.unsafe.scan` | Sin selección del cliente; 120 s por defecto y máximo. | Examina como máximo 4096 archivos `.rs` capturados del workspace y de los paquetes vendor presentes en metadata; conserva hasta 128 findings. Reporta spans de sintaxis `unsafe`/`extern`, origen y omisiones. No expande macros, evalúa `cfg` ni incluye código generado. |
| `rust.supply_chain.inspect` | Sin selección del cliente; 120 s por defecto y máximo. | Compone lock/metadata, checksums, duplicados, features declaradas/activas, una auditoría RustSec, deny cuando están sus inputs y yanked exact-version desde una generación autenticada del catálogo. Máximo 4096 paquetes, 128 filas visibles y 128 consultas yanked. |
| `rust.quality.gate.v2` | `profile=strict|release`; `baseline_project_ref` obligatorio solo para `release`; `mutation` opcional. 300 s por defecto, máximo 3600 s. | `strict` ejecuta format, check, Clippy, test, audit, deny y coverage. `release` añade SemVer contra el baseline. Mutation añade su etapa solo por opt-in y exige que su presupuesto derivado más 300 s quepa en el timeout global. |
| `rust.miri` | Sin selección del cliente; 300 s por defecto, máximo 1800 s. | Ejecuta tests lib e integración con nightly `2026-09-07`, sysroot fijado y Miri/nextest offline. Separa UB observada, operación no soportada, fallo ordinario, fallo de compilación e indeterminado. Rechaza grafos con proc macros, build scripts o harnesses personalizados. |

### Inputs del host y runtime M4

`rust.unsafe.scan` y `rust.miri` requieren el vendor Cargo offline autenticado.
`rust.deny` requiere además la policy de seguridad y RustSec; supply chain y gate
v2 consumen esos inputs cuando están configurados y conservan su ausencia en las
etapas correspondientes. Deny, supply chain y gate v2 reutilizan un único snapshot
RustSec por composición. Supply chain puede conservar hechos parciales cuando
falte catálogo, vendor o deny, pero lo declara como `unknown`, `partial`,
`unavailable` o `not_configured`; no fabrica un pass ni consulta registries o Git.

El host configura cada recurso con su par cerrado:

```text
--cargo-vendor-dir PATH --cargo-vendor-tree-sha256 sha256:<64-hex>
--security-policy PATH --security-policy-sha256 sha256:<64-hex>
--rustsec-snapshot PATH --rustsec-sha256 sha256:<64-hex>
```

Los paths son absolutos. Vendor y policy no pueden solaparse con una root de
proyecto; el vendor también exige el grupo Docker completo. La policy es JSON
cerrado, máximo 64 KiB, aportado por el operador: el cliente MCP no puede enviar
TOML de cargo-deny, excepciones, paths, flags ni comandos. Archivos de excepciones
de cargo-deny en el proyecto hacen fallar la evaluación.

Para completar los hechos de catálogo de supply chain, configura también la
generación local autenticada:

```text
--catalog-store PATH --catalog-trust PATH
```

El catálogo es read-only durante `serve`; import, sync y rebuild pertenecen a la
CLI explícita y nunca ocurren como efecto de una tool MCP.

La configuración unificada de las 27 tools M1–M4 usa la imagen M4 exacta
`sha256:25ed3626e710081a571a86a29521eaf2e890e796afd422ba5e409e0ce1891635`.
Scanner y Miri exigen esa identidad; las tools M1–M3 conservan su imagen aprobada.
Las cuatro tools M5 exigen en cambio el digest M5 y devuelven `unavailable` sobre
esta imagen, que es el resultado correcto y declarado
([ADR-077](adr/ADR-077-m5-runtime-admission.md)).
El runtime no adquiere ni construye imágenes durante `serve --stdio`.

El operador puede consultar el inventario compilado sin probar instalaciones:

```text
rust-engineering-mcp security-runtime inventory --json
```

El reporte identifica imagen, cargo-deny, scanner helper, nightly y sysroot y
declara `installation_observed=false`.

### Semántica conservadora y artifacts

Cada tool publica una proyección JSON normalizada en el store durable M3 y devuelve
un descriptor ligado al `project_ref`. Los resultados MCP completos tienen un
máximo de 512 KiB; al retirar filas para respetarlo se incrementan los contadores
de omisión y la completitud pasa a parcial. Streams crudos, texto fuente y locators
de dependencias no forman parte de los Resources M4.

Esta normalización no promete redacción universal del source autorizado. Los HTML
de coverage y diffs de mutation pueden contener bytes del proyecto, incluidos
secretos presentes en esos archivos; el store los trata como artifacts privados y
cada lectura vuelve a comprobar owner y `project_ref`.

Una ejecución solo puede pasar cuando toda la evidencia requerida está completa,
la autoridad sigue viva y el gateway confirmó cleanup. Timeout, cancelación,
límite de output, metadata inválida, datos offline ausentes, artifact no durable o
cleanup incierto no se reinterpretan como un resultado limpio. En particular:

- una licencia declarada en `Cargo.toml` no sustituye el texto de licencia que
  cargo-deny debe encontrar en el dataset capturado;
- cero findings del scanner solo describe la sintaxis seleccionada y nunca prueba
  ausencia de UB o memory safety;
- Miri observa las pruebas seleccionadas con una configuración concreta, no todas
  las ejecuciones posibles ni carreras entre tests;
- supply chain devuelve facts con provenance y freshness, no un score, una
  certificación de seguridad ni aprobación legal;
- gate v2 conserva una fila por etapa requerida; `skipped`, `unknown`, `partial`,
  `unavailable` o timeout nunca equivalen a `passed`.

Los contratos y sus fronteras están fijados en [ADR-067](adr/ADR-067-security-policy-and-quality-contracts.md),
[ADR-069](adr/ADR-069-isolated-unsafe-syntax-scanner.md),
[ADR-071](adr/ADR-071-supply-chain-facts-without-catalog-migration.md) y
[ADR-072](adr/ADR-072-miri-classification-integrity.md). La atestación de cleanup
compartida se describe en [ADR-070](adr/ADR-070-task-cleanup-attestation.md).

## Contratos M5 — medición de rendimiento

El checkout añade cuatro definiciones a `tools/list`, después de las 27 tools
M1–M4 y en este orden: `rust.benchmark.run`, `rust.benchmark.compare`,
`rust.profile.flamegraph` y `rust.binary.bloat` (`stdio.rs`, `list_tools`). Los
27 snapshots anteriores se conservan byte a byte bajo un test de invariancia y se
añaden cuatro nuevos. Las cuatro son `read_only(true)`, `destructive(false)`,
`idempotent(false)` y `open_world(false)`: ninguna escribe en el checkout. Eso no
significa que lo medido sea inocuo —tres de ellas compilan y ejecutan código del
proyecto dentro del sandbox, igual que `rust.test` o `rust.miri`—; solo
`rust.benchmark.compare` no ejecuta nada.
Véase [ADR-076](adr/ADR-076-m5-performance-contracts.md).

**Estado.** M5 está calificado localmente: suite nativa
([gate nativo](validation/M5/native-gate.json)), clientes
([recibo](validation/M5/clients.json)) y gates `core`/`full`
([full](validation/M5/full-gate.json)) sobre los contratos finales de captura,
logs, bloat y método. La [matriz M5](validation/M5/matrix.md) enumera recibos y
límites. Nada de esto acredita una release, un tag, una integración remota ni
un cambio de versión.

### Runtime, inputs del host y modo de ejecución M5

Las tres tools que ejecutan un proceso exigen la imagen guest M5
`sha256:e0a5ca1661b3e49d0a3d68ee3cc0963453078d08eb7fc43c30538c16b7998aac` **y
solo esa**: cualquier otro digest devuelve `unavailable` antes de crear
contenedor alguno ([ADR-077](adr/ADR-077-m5-runtime-admission.md)). Exigen
también datos offline autenticados por el host. Profile y bloat usan el
`CargoVendorSnapshot` configurado con `--cargo-vendor-dir` y
`--cargo-vendor-tree-sha256`; sin él el resultado es `MISSING_OFFLINE_DATA` y no
hay descarga, sustitución ni medida degradada. Para Criterion,
`rust.benchmark.run` acepta además, y prefiere, una captura de vendor
([ADR-078](adr/ADR-078-offline-vendor-capture.md)): el par
`--vendor-capture PATH --vendor-capture-tree-sha256 sha256:<64-hex>` nombra un
artifact inmutable y el digest que ese artifact debe volver a producir. El
servidor lo relee y lo recalcula de forma incremental antes de que el guest vea
un byte, y lo rechaza si el digest no es el declarado; ese digest es el
`vendor_fingerprint` que viaja en la provenance de la medición. La captura se
aprovisiona aparte con `cargo-vendor capture`, nunca como efecto secundario de
una medición, y el guest sigue montando de solo lectura un volumen construido a
partir de esos bytes, jamás el directorio original del host. `rust.profile.flamegraph`
exige además la capability del host `--allow-profiling user-space-sampling`
([ADR-074](adr/ADR-074-profiling-capability-and-containment.md) §2).
`rust.benchmark.compare` no necesita ninguno de los tres: es cálculo puro sobre
bytes ya autorizados del store privado.

Una fuente capturada que traiga su propio `.cargo/config.toml` o `.cargo/config`
en cualquier punto del árbol se rechaza antes de que exista ningún volumen: ese
archivo decidiría qué sources, linker y rustflags usaría la medición.

`execution_mode` conserva el vocabulario cerrado `auto|task|synchronous`, pero
M5 no posee ningún `JobKind`: `task` devuelve `blocked`/`TASKS_REQUIRED` como
resultado declarado, y `auto` y `synchronous` ejecutan dentro del timeout que la
entrada ya acota. No hay camino MCP Tasks para estas cuatro tools.

Presupuestos y techos ([ADR-076](adr/ADR-076-m5-performance-contracts.md) §7):
`run` 900 s, `profile` 300 s con 60 s de ventana de muestreo máxima, `bloat`
300 s y `compare` 30 s; SVG ≤ 8 MiB, JSON de bloat ≤ 4 MiB, muestras ≤ 32 MiB y
resultado MCP completo ≤ 512 KiB. La cuota se comprueba al publicar los
artifacts, después de ejecutar; no reserva admisión antes de iniciar el trabajo.
Los artifacts se publican en el store durable privado de ADR-061, ligados al
`project_ref` y a su owner, y se leen como Resources privados.

### `rust.benchmark.run`

`rust.benchmark.run` **mide los benchmarks que el proyecto ya tiene y no genera
ninguno**: no escribe un bench, no lo infiere del nombre de un target y no
sustituye un harness ausente. El único harness que sabe medir es
Criterion 0.8.2, verificado por una fase de identidad antes de medir; cualquier
otro es un resultado observado y no una medida degradada
([ADR-073](adr/ADR-073-benchmark-method-and-dataset.md) §1).

Entrada cerrada, sin campos desconocidos, sin flags libres y sin rutas:
`project_ref` (obligatorio, `^prj_[0-9a-f]{32}$`), `package` y `bench_target`
(`null` por defecto, 1..64 caracteres `^[A-Za-z0-9_-]{1,64}$`), `features`
(hasta 16, el mismo patrón, vacío por defecto), `all_features` y
`no_default_features` (`false`), `run_count` (1..=3, por defecto 3),
`timeout_seconds` (1..=900, por defecto 900) y `execution_mode`.

**Los parámetros del harness los fija el servidor y el proyecto no los alcanza**:
warmup 3 s, tiempo de medición 5 s y `--sample-size 30`, más `--noplot` y color
`never`, se pasan como argv cerrado. Un `criterion.toml` o un
`Criterion::default().sample_size(..)` del proyecto sigue siendo código del
proyecto: su efecto queda registrado como el tamaño **solicitado** frente a las
muestras **observadas**, y la comparación lo trata como incompatibilidad de
método. Los tres valores viajan en la provenance de cada dataset
(ADR-073 §2).

La respuesta publica identidad y exit de la ejecución
(`passed|benchmark_failed|compilation_failed|uncalibrated|incomplete`),
`exit_code`, `termination` (`exited|timed_out|cancelled|output_limit`),
`exit_run_index` —la repetición a la que pertenecen esos tres campos, que es la
última que se ejecutó—, repeticiones solicitadas y completadas,
`logs` (por repetición: bytes retenidos y recorte de cada stream),
selección, identidad de runtime, si se
publicó dataset y, si no, por qué (`harness_unrecognized`, `harness_unapproved`,
`execution_failed`, `output_missing`, `output_unparsable`, `output_too_large`,
`cancelled`), y hasta 128 resúmenes por benchmark con clave, identidad criterion,
número de muestras, tamaño solicitado, mediana, mínimo, máximo, desviación
absoluta mediana, outliers contados con vallas de Tukey **sin eliminarlos**,
`sampling_mode`, warmup, tiempo de medición y completeness. La provenance incluye
`source_fingerprint`, harness y versión, `rust_version`, `cargo_version`,
toolchain declarado, digest de imagen, plataforma, los tres fingerprints de
configuración/ejecución, selección y hardware con sus cuotas; un campo de
hardware que el runtime no puede observar se serializa ausente y bloquea la
comparación, nunca se rellena.

**Las muestras crudas no viajan en la respuesta.** Se publican como artifact
`benchmark_dataset` en formato `rust-engineering-mcp.benchmark-dataset.v2`
(`format_version = 2`; cada muestra lleva el `run_index` de la ejecución que la
produjo), junto al árbol de salida del harness como `criterion_archive`. Un
lector que no reconozca exactamente ese identificador y esa versión falla cerrado
y nunca migra medidas (ADR-073 §3).

**Los logs del harness se publican como artifacts propios, uno por repetición y
por stream** (ADR-080): `harness_stdout` y `harness_stderr`, privados,
owner-bound, con TTL y sensibilidad `source_derived` como el resto de la
evidencia M5. No se concatenan entre repeticiones y nunca salen por el `stdout`
del servidor, que es el transporte del protocolo. Cada entrada de `artifacts`
lleva el `run_index` de la repetición de la que procede; solo `benchmark_dataset`
lo lleva a `null`, porque agrupa todas las repeticiones y el índice viaja en cada
muestra. Una repetición que no escribió nada en un stream no publica ese member:
un artifact de cero bytes sería una ausencia disfrazada de evidencia.

Cada stream se acota en 256 KiB por repetición y **el recorte se declara**: el
member sale con `completeness: truncated`, su `size_bytes` es lo que sobrevivió
—nunca lo que el harness escribió— y `observation.logs` publica, por repetición,
`stdout_bytes`/`stderr_bytes` retenidos y `stdout_truncated`/`stderr_truncated`.
Un log recortado no se publica jamás como completo. Los dos booleanos sueltos
`observation.stdout_truncated`/`stderr_truncated` son el resumen: `true` si
alguna repetición fue recortada. El payload se hace UTF-8 válido sustituyendo
bytes inválidos; `stdout_replaced` y `stderr_replaced` declaran esa
sustitución por stream y repetición, sin confundirla con el recorte. Una respuesta
con hasta tres repeticiones puede llevar hasta ocho artifacts: dataset, árbol y
dos logs por repetición.

`criterion_archive` es el árbol de **una** repetición: la última que **exportó**
uno, y su `run_index` dice cuál. No tiene por qué ser la repetición cuyo exit
reporta la respuesta: si la tercera falla sin exportar y las dos primeras
exportaron, el árbol es el de la segunda mientras `exit`, `exit_code` y
`termination` describen la tercera. Por eso la respuesta publica los dos índices
—`observation.exit_run_index` y el `run_index` del artifact— y el lector los
compara en vez de suponer que coinciden. No es una fusión ni una concatenación
de las tres, porque cada repetición escribe su propio `CRITERION_HOME` y sus
rutas colisionan; el dataset publicado al lado sí agrupa todas y cada muestra
lleva su `run_index`. Su `completeness` es `complete` solo cuando se pidió una
única repetición y esa repetición terminó; con varias repeticiones el árbol
publicado es evidencia parcial de la ejecución y se declara `partial`. Un árbol
que exceda el techo de 32 MiB de ADR-076 §7 no se recorta —un tar cortado no es
un tar—: se declara como omisión (`output_too_large`) y la ejecución publica el
resto de members sin él.

Una ejecución sin dataset **sí publica sus logs**. `harness_unrecognized`,
`harness_unapproved` y un fallo de compilación observado siguen siendo resultados
declarados sin dataset, pero su evidencia —el texto del compilador o del
harness— es exactamente lo que hace accionable el `OBSERVED_FAILURE`, y viaja en
`harness_stderr`/`harness_stdout`. Solo una ejecución que no produjo byte alguno
publica cero artifacts.

Códigos de error: `TASKS_REQUIRED` (no se admite como MCP Task), `SANDBOX_DENIED`
(capacidad o discovery del sandbox), `MISSING_OFFLINE_DATA` (vendor autenticado
ausente o inválido), `ARTIFACT_UNAVAILABLE` (store durable no disponible),
`TOOL_NOT_INSTALLED` (runtime aprobado ausente), `INVALID_PROJECT` (entradas o
evidencia capturadas que no validan), `PROJECT_NOT_FOUND` (autoridad ausente o
expirada), `COMMAND_TIMEOUT`, `OUTPUT_LIMIT_EXCEEDED`, `EVIDENCE_INCOMPLETE`
(evidencia parcial), `OBSERVED_FAILURE` (la ejecución reportó fallo),
`HARNESS_UNRECOGNIZED` (no se resolvió ningún harness reconocido; sin dataset) y
`HARNESS_UNAPPROVED` (la versión de criterion resuelta no es la aprobada; sin
dataset).

Límites declarados: el positivo **está bloqueado**. El cierre de criterion 0.8.2
son 6 014 archivos y 156 267 469 bytes, con cuatro archivos por encima del límite
de 1 MiB por archivo, y un `SourceBundle` admite 4 096 entradas, 16 MiB en total
y 1 MiB por archivo; los límites **no se subieron** porque pertenecen al contrato
de datos offline calificado en M2/M4 ([M5-01-blocker.json](validation/M5/01-blocker.json)).
Lo calificado en el guest son `unrecognised-harness`,
`project-cargo-configuration-refused` y `cancellation-mid-run`
([recibo](validation/M5/01-runtime.json)). `BenchmarkExit` conserva
`CALIBRATED = false`: solo se observaron los exits 0 y 1. La herramienta tampoco
alterna el orden de baseline y candidate: ejecuta en el orden de declaración del
harness y lo registra; alternar es un protocolo del operador, no un control del
producto.

### `rust.benchmark.compare`

`rust.benchmark.compare` compara dos datasets que el mismo proyecto ya publicó.
No ejecuta procesos, no crea contenedor, no lee fuente del proyecto y por eso no
tiene `execution_mode`. Entrada cerrada: `project_ref`,
`baseline_artifact_id` y `candidate_artifact_id` (`^qart_[0-9a-f]{32}$`, opacos,
emitidos por el store y no componibles por el peer) y `timeout_seconds`
(1..=30, por defecto 30). Un artifact de otro owner no existe para esta llamada.

El método está congelado antes de medir y se publica entero en cada informe
(ADR-073 §4), bajo el identificador
`rust-engineering-mcp.benchmark-comparison.v2`:

| Elemento | Valor |
| --- | --- |
| Estadístico | mediana del tiempo por iteración (`median_per_iteration_nanoseconds`) |
| Intervalo | bootstrap percentil **por conglomerados**, 10 000 remuestreos: se remuestrean las ejecuciones y, dentro de cada una, sus muestras |
| Semilla | fija, derivada de una constante del producto mezclada con la clave del benchmark |
| Confianza | 0,95 **nominal**: el nivel que el método pide, no la cobertura que entrega (ver el aviso más abajo); con familia de más de una comparación, Bonferroni `1 - (1 - 0,95)/n` |
| Multiplicidad | `none` o `bonferroni`, con `family_size` y `adjusted_confidence_level` emitidos |
| Umbral material | 0,05 |
| Outliers | vallas de Tukey; política `reported_not_removed`, se cuentan y **no** se eliminan |
| Muestras mínimas | 10 por lado para reclamar cualquier intervalo |
| Ejecuciones mínimas | 3 por lado para reclamar cualquier dirección: las que el protocolo ejecuta por defecto |

El **minimum detectable ratio** es
`MDR = (z_{1-α/2 ajustado} + z_{0,80}) · SE`, con `SE` la desviación típica de la
distribución bootstrap del ratio: el menor ratio verdadero que ese tamaño
muestral y esa dispersión podrían detectar con 80 % de potencia al nivel
ajustado. **El MDR no se iguala al umbral del 5 %**; se emite por comparación.

El veredicto se decide en este orden: muestra ausente o truncada, menos de diez
muestras o mediana de baseline no positiva ⇒ `inconclusive`; **menos de tres
ejecuciones distintas en cualquiera de los dos lados ⇒ `inconclusive` por
`insufficient_executions`**, antes de mirar el intervalo, porque con una sola
ejecución por lado nada distingue un cambio en el código de un cambio en la
máquina y con dos la estimación de la deriva es la que este bootstrap más
subestima —el `SE` queda corto por `sqrt(k/(k−1))`, 1,41× con `k = 2`—; tres es
el `run_count` por defecto, así que la puerta pide que el protocolo se haya
seguido, no una captura extra; **dispersión degenerada (error estándar cero) ⇒
`inconclusive` por `degenerate_dispersion`**, porque una dispersión observada de cero es ausencia de
información sobre la dispersión y no precisión infinita; `MDR` mayor que el
**familia mayor que 25 ⇒ `inconclusive` por `family_beyond_resolution`**, sin
correr bootstrap y sin afirmar intervalo, porque con Bonferroni el extremo que
pediría a 10 000 remuestreos sería un estadístico de orden extremo y no una
interpolación; **campo de hardware no observable en los dos lados ⇒
`inconclusive` por `unobservable_hardware`**, porque el parámetro ambiental que
pudo causar la diferencia nunca se observó; `MDR` mayor que el umbral ⇒
`inconclusive` por precisión insuficiente; intervalo completamente por encima de
`+5 %` ⇒ `regression`; completamente por debajo de `-5 %` ⇒ `improvement`;
completamente dentro de `±5 %` ⇒ `no_material_change`; en cualquier otro caso
`inconclusive` porque el intervalo cruza el umbral. Las razones se enumeran
(`insufficient_samples`, `precision_below_threshold`, `interval_spans_threshold`,
`zero_or_negative_baseline`, `missing_measurement`, `truncated_measurement`,
`insufficient_executions`, `degenerate_dispersion`, `family_beyond_resolution`,
`unobservable_hardware`).

> [!IMPORTANT]
> **En el runtime M5, `rust.benchmark.compare` no emite ninguna dirección.**
> `METHOD_QUALIFIED_FOR_DIRECTION=false` cierra de manera independiente
> `regression`, `improvement` y `no_material_change`; `inconclusive` es el único
> veredicto direccionalmente seguro hasta la recalificación del método.
>
> El gateway observa exclusivamente el guest Linux: descubre todos los IDs de
> CPU visibles y lee el governor de cada uno mediante fases tipadas y acotadas.
> Solo publica `cpu_governor` cuando cada CPU observada responde y todos los
> valores coinciden. Sysfs ausente, un exit no cero limpio, una CPU faltante o
> governors heterogéneos producen `None`; timeout, cancelación, límite de salida
> o truncamiento producen el error operativo unido del gateway. `cpu_model` se
> publica solo con valores guest válidos y unánimes. No hay afirmación sobre el
> host físico ni macOS.
>
> La observación uniforme permite comprobar el governor; si falta, la comparación
> conserva la razón de entorno desconocido correspondiente.
> No califica el método ni habilita dirección: eso exige recalificación
> estadística separada, sin cambiar umbrales por esta observación.
>
> La tool **sí** mide y publica el efecto: sobre las capturas reales, un cambio de
> fuente del +25 % se mide como `effect_ratio` +0,2433. Lo que no hace es llamarlo
> regresión.

> [!IMPORTANT]
> **`confidence_level: 0.95` es el nivel nominal, no la cobertura entregada.** El
> bootstrap por conglomerados sortea `k` ejecuciones con reemplazo de las `k` que
> ese lado ejecutó, y su varianza tiene esperanza `((k − 1)/k)·σ²_entre`: el `SE`
> publicado queda corto por `sqrt(k/(k−1))` —1,22× con las tres ejecuciones
> exigidas— y los extremos percentiles se toman sin ensanchamiento `t_{k−1}`. Las
> dos aproximaciones empujan en el mismo sentido: **el intervalo sale más estrecho,
> es decir más confiado, que el 0,95 que declara**, nunca más ancho. Medida bajo un
> nulo gaussiano de efectos aleatorios, una re-revisión independiente situó la
> cobertura real en 0,84–0,89 con tres ejecuciones por lado. La **magnitud** de esa
> brecha depende del modelo de deriva con el que se mida; el **mecanismo** no, y no
> desaparece en ningún `run_count` que la tool acepte. El valor publicado sigue
> siendo 0,95 a propósito: es lo que el método congelado pide, y sustituirlo por un
> número «efectivo» de un solo modelo publicaría los supuestos de ese modelo como
> si fueran los del método (ADR-073 §4).

Por comparación se publican clave, veredicto, `effect_ratio`
(`candidate_median_ns / baseline_median_ns - 1`; positivo significa que el
candidato es la medición más lenta), intervalo, ambas medianas, ambos tamaños
muestrales, **ambos conteos de ejecuciones independientes agrupadas**
(`baseline_executions` y `candidate_executions`, junto a los tamaños muestrales:
son lo que decide si se admite dirección, de modo que dos informes iguales en
todo lo demás pero distintos ahí no tenían derecho al mismo veredicto), ambos
conteos de outliers, el MDR y las razones. El informe añade
`compared`, hasta 512 comparaciones con su contador de omisión, y las claves
presentes en un solo lado (`baseline_only` y `candidate_only`, hasta 256 cada
una, con sus contadores).

**Un par incompatible es un resultado, no un error de infraestructura**: la
respuesta es `status = failed` con `error_code = INCOMPATIBLE_DATASETS`,
`isError` en `false` y la lista completa y ordenada de razones, de entre
`format_version`, `unit`, `harness`, `harness_version`, `benchmark_identity`,
`rust_version`, `cargo_version`, `runtime_image`, `platform`, `configuration`,
`architecture`, `cpu_model`, `cpu_cores`, `os_kernel`, `cpu_governor`,
`virtualization`, `quotas`, `selection`, `sampling_mode`, `unknown_hardware` y
`same_artifact`. La respuesta de un par incompatible lleva además las **dos
provenances comparadas** —formato y versión, unidad, harness y versión, toolchain,
digest de imagen, plataforma, `configuration_fingerprint`,
`execution_fingerprint`, selección y hardware—, de modo que un llamador al que se
le dice `cpu_model` puede ver *qué dos* CPUs, en lugar de tener que pedirlas a
otra tool que no existe. El `source_fingerprint` **puede** diferir —esa es la razón de
comparar—, pero comparar un artifact consigo mismo se rechaza como
`same_artifact`; comparar dos ejecuciones independientes del mismo código es un
control legítimo.

Otros códigos: `SANDBOX_DENIED` (no hay capacidad de comparación en este host),
`ARTIFACT_UNAVAILABLE` (store durable no disponible), `ARTIFACT_NOT_FOUND` (un
identificador no nombra un artifact de este proyecto), `ARTIFACT_UNREADABLE`
(los bytes almacenados no se pudieron leer enteros), `ARTIFACT_TOO_LARGE` (un
dataset supera el techo de lectura de 32 MiB), `NOT_A_DATASET` (el identificador
nombra un artifact que no es un dataset de benchmark), `INVALID_DATASET` (no es
el contrato v2 que este lector implementa), `NO_COMMON_BENCHMARK` (los dos
datasets no comparten ninguna clave), `COMMAND_TIMEOUT`,
`OUTPUT_LIMIT_EXCEEDED` y `EVIDENCE_INCOMPLETE`.

**El resultado reporta una medición, nunca una causa.** Describe dos ejecuciones
en un host concreto: no atribuye causa, no generaliza a otro hardware ni a otro
proyecto y no emite ninguna recomendación de optimización (ADR-073 §6). Las
muestras las produce el harness del proyecto y se describen como observaciones de
origen no autenticado. `compare` depende de un `run` previo del mismo proyecto:
sin él no hay nada que comparar, y eso es deliberado. El método está probado
sobre tres capturas reales del guest, con control de auto-comparación, regresión
de dirección conocida y rechazo de `same_artifact`
([calibración](validation/M5/01-benchmark-calibration.json)).

### `rust.profile.flamegraph`

`rust.profile.flamegraph` muestrea en CPU un binario del proyecto dentro del
sandbox de profiling y publica un flame graph saneado y los stacks colapsados.

**Exige una capability positiva del host.** El peer, el proyecto, la URI de un
Resource y las annotations de la tool no la conceden y no permiten inferirla. Sin
`--allow-profiling user-space-sampling` la llamada responde `blocked` con
`PROFILING_NOT_AUTHORIZED` **antes** de discovery, del vendor, del worker y de
crear ningún contenedor: no hay build, no hay ejecución y no hay artifact. La
capability es por servidor y se retira quitando la bandera y reiniciando; no hay
revocación en caliente (ADR-074 §2).

Entrada cerrada: `project_ref`, `binary_target` (nombre de target cargo, **nunca
una ruta**, 1..64 caracteres `^[A-Za-z0-9_-]{1,64}$`; el hijo no recibe ningún
argumento del peer), `frequency_hz` (1..=999, por defecto 99),
`duration_seconds` (1..=60, por defecto 10), `timeout_seconds` (1..=300, por
defecto 120) y `execution_mode`.

El alcance del muestreo es estrecho y fijo: solo eventos de espacio de usuario
(`exclude_kernel`, `exclude_hv`) del tipo software de reloj de CPU, solo el
proceso hijo que lanza el helper y sus hilos, nunca un pid ajeno ni todo el
sistema. El perfil seccomp de profiling es el de calidad **más una syscall**,
`perf_event_open`; se conservan `--cap-drop=ALL`, `no-new-privileges`,
`--network=none`, rootfs read-only, uid/gid 65534 y el `/source` read-only, y no
se añade `CAP_PERFMON`, no se usa contenedor privilegiado, no se ejecuta `sudo`
y no se toca `perf_event_paranoid` (ADR-074 §3 y §4).

La respuesta declara backend, target, resultado del build
(`built|compilation_failed|target_not_found`) y su exit, exit o señal del hijo,
frecuencia, duración solicitada frente a la observada, muestras recogidas y
perdidas, stacks escritos y truncados, frames totales y no resueltos, módulos
vistos, la profundidad de pila que el kernel aplicó realmente, el errno cuando
`perf_event_open` fue rechazado, `status`
(`complete|sample_limit|duration_limit|child_exited|profiler_unavailable`),
`completeness` (`complete|lost_samples|no_samples|truncated|unavailable`) y un
ranking acotado de hasta 1 024 frames con muestras propias y totales, con su
contador de omisión. Los nombres de frame se sanean a un alfabeto cerrado; nunca
se emite una ruta del sistema de archivos ni el módulo, y un frame no resuelto se
reporta como `[unknown]` y se cuenta. El SVG lo genera el producto y no contiene
`<script>`, `on*`, `href`, `xlink:href`, `<foreignObject>`, `<image>`, entidades
externas ni ninguna URL (ADR-074 §5). Los artifacts son `flamegraph_svg`
(≤ 8 MiB) y `collapsed_stacks`.

**Cero muestras es un resultado válido y declarado**, no un fallo: se reporta
como `no_samples` con sus contadores, sin rellenar nada. Una pérdida de muestras
o de símbolos se declara igual.

Códigos de error: `PROFILING_NOT_AUTHORIZED` (el host no concedió la
capability), `TASKS_REQUIRED`, `SANDBOX_DENIED`, `MISSING_OFFLINE_DATA`,
`ARTIFACT_UNAVAILABLE`, `TOOL_NOT_INSTALLED`, `INVALID_PROJECT`,
`PROJECT_NOT_FOUND`, `COMMAND_TIMEOUT`, `OUTPUT_LIMIT_EXCEEDED`,
`PROFILER_UNAVAILABLE` (el sampler no pudo abrir el evento de rendimiento; el
errno reportado es la negativa), `OBSERVED_FAILURE` (el target no compiló o no
existe en el proyecto) y `EVIDENCE_INCOMPLETE` (evidencia parcial: muestras
perdidas o stacks truncados declarados).

Límites: presupuesto 300 s con 60 s de ventana de muestreo máxima. El unwinding
depende de frame pointers; un binario sin ellos produce stacks poco profundos y
eso se declara. El positivo se limita al guest Linux ARM64 con la imagen M5:
Mach-O y PE no quedan calificados. M5-03 está calificado nativamente
([runtime](validation/M5/03-runtime.json)) y por clientes
([recibo](validation/M5/clients.json)); el cierre conjunto sigue el estado de la
[matriz M5](validation/M5/matrix.md).

### `rust.binary.bloat`

`rust.binary.bloat` mide el tamaño exacto de un binario del proyecto dentro del
sandbox y después ejecuta el analizador fijado `cargo-bloat 0.12.1` sobre él.
Entrada cerrada: `project_ref`, `binary_target` (nombre de target cargo, nunca
una ruta), `package` (`null` por defecto), `profile` (`release` o `release_lto`,
por defecto `release`), `timeout_seconds` (1..=300, por defecto 120) y
`execution_mode`. Sin flags libres y sin `--symbols-section` configurable.

**La respuesta separa dos cosas que no deben leerse como una.** `measured` son
hechos que este producto verificó por sí mismo dentro del guest: el tamaño exacto
en bytes del archivo, su `sha256` y su formato
(`elf64_aarch64|other_elf|mach_o|pe|wasm|unknown`). `attribution` es la
estimación de `cargo-bloat` sobre a dónde fue ese tamaño: lleva `estimated`
siempre en `true` e incluye `text_section_size_bytes`, el tamaño de archivo que
el propio analizador reporta y los rankings por función y por crate. El bucket
`[Unknown]` del analizador se conserva literal.

**El ranking está acotado, y la respuesta declara por separado cada límite que
actuó** ([ADR-079](adr/ADR-079-bloat-result-semantics.md) §1). `ranking_cap`
nombra el tope propio del producto (`max_rows`, hoy 256 filas por vista) y
cuántas filas de función y de crate dejó fuera; `response_trim` nombra el
presupuesto de respuesta (`budget_bytes`) y cuántas filas se quitaron **además**
para caber en él. Las filas descartadas son siempre las más pequeñas. Ninguno de
los dos contadores es evidencia incompleta ni decide el `status`: un ranking
acotado por un tope que el producto eligió y declara es la atribución que el
contrato promete.

**El archivo medido es un build de análisis.** El analizador fuerza
incondicionalmente `CARGO_PROFILE_<PERFIL>_STRIP=false` porque necesita la tabla
de símbolos, y se comprobó que `CARGO_PROFILE_RELEASE_STRIP=symbols` no tiene
efecto alguno sobre el archivo producido
([calibración](validation/M5/04-bloat-calibration.json)). El DTO lo declara en
`analysis_build_symbols_forced`, siempre `true`. El tamaño sigue siendo exacto
*para ese archivo*, pero **no** es byte a byte el que enviaría un proyecto que
pide stripping, y un reporte de binario stripped es inalcanzable con este
analizador. `release_lto` tampoco se pide con `--profile`: es `--release` más la
variable de entorno propiedad del producto `CARGO_PROFILE_RELEASE_LTO=fat`, y
ambos perfiles dejan el binario bajo `<target-dir>/release/`.

**`passed` significa «análisis ejecutado y validado», y nada más**
([ADR-079](adr/ADR-079-bloat-result-semantics.md) §2). No afirma que el binario
esté optimizado, ni que la atribución sea exhaustiva, ni que el ranking describa
todo el archivo. El DTO lo publica como `analysis_validated`, y exige que el
analizador saliera limpio, que la completeness sea `complete`, que esté el
**tamaño exacto medido** y que coincida con el que reportó el analizador, y que
se haya publicado el artifact que sostiene la atribución.

Si el tamaño exacto que midió el producto y el `file-size` que reporta
`cargo-bloat` no coinciden, la completeness es `size_mismatch` y la atribución
**no** se publica como descripción del archivo medido. La completeness es
validez de la medición y solo eso: sus valores son `complete`, `size_mismatch`,
`unsupported_format` y `unavailable` —no hay un `truncated`, porque el tope del
producto es cobertura y se declara en `ranking_cap`—; el exit es
`passed|analysis_failed|compilation_failed|uncalibrated|incomplete`. El artifact
es como máximo uno, `bloat_json` (≤ 4 MiB), con el reporte crudo del analizador;
un fallo de build o de análisis no publica ninguno.

Códigos de error: `TASKS_REQUIRED`, `SANDBOX_DENIED`, `MISSING_OFFLINE_DATA`,
`ARTIFACT_UNAVAILABLE`, `TOOL_NOT_INSTALLED`, `INVALID_PROJECT`,
`PROJECT_NOT_FOUND`, `COMMAND_TIMEOUT`, `OUTPUT_LIMIT_EXCEEDED`,
`OBSERVED_FAILURE` (el target no compiló, no existe o el analizador rechazó la
petición), `ANALYZER_UNAVAILABLE` (el analizador aprobado no está disponible o no
es el aprobado; no se produjo medida), `UNSUPPORTED_FORMAT` (el formato del
binario no lo soporta el analizador fijado), `SIZE_MISMATCH` y
`EVIDENCE_INCOMPLETE` (la salida del analizador no se observó como una ejecución
limpia, o no se publicó el artifact que sostiene la atribución).

Límites: WASM se rechaza porque el backend no lo soporta, y Mach-O y PE no quedan
calificados por el positivo ELF. `BloatExit` conserva `CALIBRATED = false`: solo
se observaron los exits 0 y 1.

El corte está **In progress otra vez**, no calificado.
[ADR-079](adr/ADR-079-bloat-result-semantics.md) sustituye la semántica de
resultado que la calificación anterior midió, así que su
[recibo](validation/M5/04-runtime.json) —`release-positive`, `release-lto` y
`missing-binary-target`— acredita el contrato viejo y no este. Se recalifica
sobre bytes finales, con revisión independiente de por medio.
