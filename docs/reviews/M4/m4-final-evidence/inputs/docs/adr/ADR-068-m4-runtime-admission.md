# ADR-068 — Admisión del runtime M4 por identidad inmutable

## Status

Accepted para admisión por identidad y las fases nativas deny, scanner y Miri.
La publicación MCP, clientes y cierre M4 conservan sus gates independientes.

## Context

ADR-066 aprovisionó con autorización explícita del owner una imagen derivada del
runtime M3. La evidencia de M3 no podía autorizar automáticamente sus nuevos
bytes. La imagen conserva el toolchain estable y plugins de M3 e incorpora
cargo-deny 0.19.7 GNU ARM64 y nightly/Miri preparados sin adquisición en runtime.

## Decision

El gateway acepta, además de la identidad M3 existente, exclusivamente
`sha256:95dddeb5305f10b09b441e3cc4018ebb1a8a296d365c65106327d59f933c64e7`.
La CLI requiere escoger esa identidad exacta; no hay alias mutable ni cambio del
default documentado M3. El config digest es
`sha256:7d4e58b9e29b2045c13d71542f7892ee071a6886a1b939c4cbfc3ff7ce40dc45`.
Cada sesión conserva calibración, revalidación por fase y cuarentena de cleanup.

La base se recalificó mediante `m4_runtime_base_containment_is_requalified`,
resultado verificado en [M4-base-calibration](../validation/M4-base-calibration.json),
SHA-256 `1d410a123667e776b164ae0c773e928afb660e7c5c54e34f7d336932dc596abf`.
Las nuevas fases de deny se calificaron con texto de licencia real, manifest sin
texto, licencia no permitida, ban, vendor real y ocho casos adversos en
[M4-deny-adversarial](../validation/M4-deny-adversarial.md). Todo proceso atraviesa
el gateway tipado, sin red, con tres volúmenes propiedad de la operación; metadata
y deny no tienen mounts de escritura a source, vendor o policy. La limpieza de
contenedores y volúmenes se observa antes de devolver el resultado.

La imagen derivada final admitida adicionalmente es
`sha256:25ed3626e710081a571a86a29521eaf2e890e796afd422ba5e409e0ce1891635`.
Su helper tiene SHA-256
`af8af1a021094003cd90023a882d707f7062cc98938bb06a4c105bf75e10120b`;
[inventario y fuentes instaladas](../validation/M4-scanner-provisioning.json)
vinculan sus bytes. La [contención base final](../validation/M4-final-base-calibration.json) volvió a pasar. La
[calificación scanner](../validation/M4-scanner-native.json) pasó siete casos:
origen vendor, sintaxis/Unicode/cfg/macros, UTF-8 inválido, error de parseo, límite
de archivo, caída con continuación y presupuesto global parcial. Este último
preservó 917 archivos parseados y declaró 3175 agotados, devolviendo el reporte
con cleanup en 12.098 s dentro de 35 s. La reserva de control y cleanup es 25 s;
la continuidad después de timeout por archivo se acredita mediante IPC del helper,
sin atribuir al corpus nativo un timeout por archivo que no reproduce.

[Miri por gateway](../validation/M4-miri-native.json) pasó trece oráculos de
clasificación y siete de admisión/lifecycle, con cleanup verificado: build script,
proc macro y harness personalizado rechazados, timeout y cancelación del intérprete
unidos antes de devolver. Solo esta imagen admite scanner y Miri. Deny admite las
dos identidades M4; la imagen M3 conserva su contrato. No se admiten las imágenes
intermedias del desarrollo del helper. El servidor anuncia las cinco tools; [clientes](../validation/M4-clients.json),
[core](../validation/M4-core-gate.json), [full](../validation/M4-full-gate.json) y
[runtime 19/19](../validation/M4-runtime.json) pasaron con el mismo inventario
final. Las [300 mediciones](../validation/M4-budgets.json) conservan su binario
histórico explícito; no se atribuyen al binario final. El cierre requiere la
confirmación independiente indicada en el handoff.

### Inventario CLI pasivo

`security-runtime inventory [--json]` expone un documento separado de versión 1
con las identidades compiladas del runtime M4, deny, helper, nightly y sysroot.
No inspecciona instalaciones ni ejecuta procesos: `installation_observed=false`.
No modifica el reporte doctor v1 existente. La disponibilidad sigue exigiendo
selección explícita y calibración por gateway; el inventario no la acredita.

## Alternatives considered

- Reutilizar evidencia M3 sin recalibrar: rechazada por cambio de bytes.
- Sustituir la imagen M3 en todos los hosts: rechazada; selección explícita conserva
  rollback y contratos existentes.
- Admitir tags/versiones de plugin libres: rechazada por falta de identidad exacta.

## Consequences

La imagen es un requisito opcional local: el servidor no descarga ni construye.
Deny sobre una imagen M3 devuelve unavailable. Las 22 tools previas conservan sus
schemas. Los recibos finales anteriores acreditan gate y clientes por separado.
El rollback selecciona M3 y deshabilita las tools M4 sin
migración del store de artefactos ni retroceso de floors de catálogo.
