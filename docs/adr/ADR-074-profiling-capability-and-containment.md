# ADR-074 — Capability de profiling, containment y helper propio (D24)

Fecha: 2026-09-08.

## Status

Accepted como contrato D24. Cierra la decisión que el
[backlog](../roadmap/adr-backlog-m2-m8.md#d24--profiling-y-privilegios) dejaba
Proposed con fecha límite M5-03. La viabilidad está demostrada empíricamente
(§Evidencia); la calificación nativa se registra en `docs/validation/M5-03.md`.

## Context

`rust.profile.flamegraph` (spec §28.3) exige un positivo real: el plan M5 dice
que un test de permiso denegado **no** cumple la entrega, y que si no existe una
configuración que preserve el containment el resultado es `unavailable` y M5 queda
abierto. También prohíbe `sudo`, contenedores privilegiados, cambios de `sysctl` y
ampliaciones silenciosas del sandbox.

El inventario del 2026-09-08 confirmó que **ningún** perfilador está aprovisionado:
ni en el host (`~/.cargo/bin`) ni en las cuatro imágenes guest. `perf`,
`cargo-flamegraph`, `samply` e `inferno` están ausentes en todas. El host tiene
`xctrace` y `dtrace` de Xcode, pero perfilar en el host significaría ejecutar
código del proyecto —hostil por definición (spec §43 lista los benchmarks entre
las operaciones que ejecutan código no confiable)— fuera del gateway. Eso vulnera
G2 y no es una alternativa aceptable.

Dentro del guest, `/proc/sys/kernel/perf_event_paranoid` vale `2`, que permite
mediciones **de espacio de usuario** sin privilegio alguno. El perfil seccomp
`seccomp-rust-quality.json` permite 121 syscalls y **no** incluye
`perf_event_open`, con `defaultAction: SCMP_ACT_ERRNO`.

## Evidencia

Prueba ejecutada el 2026-09-08 con un programa Rust sin dependencias que invoca
`perf_event_open` por syscall cruda, compilado y ejecutado dentro de la imagen M4
aprobada (`sha256:25ed3626e710…`) con exactamente las banderas del gateway
(`--cap-drop=ALL`, `--security-opt=no-new-privileges=true`, `--network=none`,
`--pids-limit=128`, `--cpus=1`, `--memory=1g`, `--user=65534:65534`), sin `sudo`,
sin contenedor privilegiado y sin tocar `perf_event_paranoid`:

| Perfil seccomp | Resultado |
| --- | --- |
| `seccomp-rust-quality.json` actual | `perf_event_open` → `-1`, `errno=1` (EPERM). Denegado. |
| El mismo + exactamente `perf_event_open` | `fd=3`; ring buffer de 8+1 páginas mapeado; `PERF_EVENT_IOC_ENABLE` → 0; `data_head = 560` tras el workload. Muestras reales recogidas. |

El evento se abrió con `PERF_TYPE_SOFTWARE`/`PERF_COUNT_SW_CPU_CLOCK`,
`exclude_kernel = 1`, `exclude_hv = 1`. No se usó ningún contador PMU: en un guest
virtualizado ARM64 los contadores hardware no están disponibles y pedirlos habría
convertido el positivo en un fallo dependiente del hipervisor.

## Decision

### 1. Backend: helper propio del proyecto, no un perfilador de terceros

M5-03 usa `rust-mcp-profile-helper`, un binario construido desde fuente del
repositorio e instalado en la imagen guest, análogo a `rust-mcp-unsafe-helper`
(ADR-069). **No** se aprovisiona `perf`, `cargo-flamegraph`, `samply` ni
`inferno`.

Razones, en el orden de precedencia de AGENTS:

- **Seguridad y alcance mínimo.** El helper abre un único evento en modo usuario
  sobre el proceso hijo que él mismo lanza, lee el ring buffer y escribe stacks
  colapsados. No tiene motor de scripting, no lee `/proc` de terceros, no acepta
  un pid arbitrario y no abre red. `perf(1)` trae subcomandos de scripting y una
  superficie enorme que el sandbox tendría que contener sin necesitarla.
- **Control del artifact.** El SVG lo genera el producto, no una herramienta
  externa. Eso hace de la sanitización una propiedad de construcción y no una
  verificación posterior sobre bytes ajenos.
- **Provisionamiento.** No añade ningún componente de terceros al runtime, ni
  licencia, ni SBOM, ni superficie de actualización.

### 2. Capability positiva del host, no del peer

Profiling exige una capability explícita concedida por el host confiable en su
configuración. El peer, el proyecto, la URI y las annotations no la conceden. Sin
esa capability, `rust.profile.flamegraph` responde `blocked` con
`PROFILING_NOT_AUTHORIZED` **antes** de crear ningún contenedor.

La capability es por servidor y se retira quitando la bandera y reiniciando. **No
existe revocación en caliente**: la concesión se lee una vez del argv de arranque
y ningún camino la muta después. Un texto anterior de este ADR decía que su
retirada cancelaba el trabajo en curso y hacía join del árbol; eso describía una
intención, no el código, y se corrige aquí. Implementar revocación en caliente
—una bandera que la tool relea, con la operación en vuelo cancelada por el
`InspectionControl` existente y unida por el cleanup existente— es una decisión
posterior, no algo que este ADR pueda dar por hecho.

### 3. Extensión mínima y explícita del sandbox

Se añade un perfil `seccomp-rust-profile.json` que es exactamente
`seccomp-rust-quality.json` **más una syscall**: `perf_event_open`. Nada más.

Se conservan sin cambio: `--cap-drop=ALL`, `--security-opt=no-new-privileges=true`,
`--network=none`, `--read-only`, `--ipc=private`, `--cgroupns=private`,
`--pids-limit`, `--cpus`, `--memory`, uid/gid 65534 y el montaje `/source` de solo
lectura. **No** se añade `CAP_PERFMON` ni `CAP_SYS_ADMIN`, **no** se usa
`--privileged`, **no** se modifica `perf_event_paranoid` y **no** se ejecuta nada
con `sudo`. La tool no puede activar permisos de profiling: si el kernel del host
denegara `perf_event_open`, el resultado es `unavailable` con el errno observado.

El perfil aplicado se verifica contra el declarado por fase. **No es paridad con
ADR-064, y esta línea decía que lo era.** La comprobación M5 es un subconjunto
estricto de la matriz `rust_applied` que usan los demás gateways: compara
montajes, argv, entorno, usuario, entrypoint, capacidades, `Privileged` y el
seccomp aplicado, pero **no** declara `PidMode`, `UsernsMode`, `Init`, `Sysctls`,
`Devices`, `MaskedPaths`, `ReadonlyPaths`, `Ulimits` ni una docena más que M4 sí
rechaza. Lo señaló una revisión independiente, está aceptado, y no se corrige
aquí porque tocar `rust_applied.rs` —calificado en M1–M4— merece su propia
decisión y su propia recalificación.

Un caso concreto que sí queda cubierto, y no por esta comprobación: `Init` y
`PidMode` romperían la suposición de que el helper es PID 1 de su namespace, de
la que depende el vaciado de §5.1. No se deja al contraste de configuración —el
helper **mide** su pid en tiempo de ejecución y un helper que no sea PID 1 emite
`namespace_drained: false`, que el host convierte en `InvalidMetadata`. Es una
observación del hecho, no una comparación de la configuración declarada.

### 4. Alcance de la medición

Solo eventos de espacio de usuario: `exclude_kernel = 1`, `exclude_hv = 1`, y
únicamente `PERF_TYPE_SOFTWARE`/`PERF_COUNT_SW_CPU_CLOCK`. Solo el proceso hijo
lanzado por el helper y sus hilos (`inherit = 1`); nunca un pid ajeno ni todo el
sistema. Frecuencia y duración acotadas por el producto: 99 Hz por defecto, techo
de 999 Hz; duración por defecto 10 s, techo 60 s.

### 5. Artifacts, privacidad y sanitización

La salida cruda son stacks colapsados; el SVG lo renderiza el producto a partir de
ellos. Reglas de construcción:

- Los nombres de frame se saneen a un alfabeto cerrado; cualquier otro byte se
  sustituye. Nunca se emite un path del sistema de archivos ni el módulo: solo el
  símbolo. Un frame no resuelto es `[unknown]` y se cuenta.
- El SVG no contiene `<script>`, ni `on*`, ni `xlink:href`, ni `href`, ni
  `<foreignObject>`, ni `<image>`, ni entidades externas, ni ninguna URL. Todo el
  texto se escapa en XML. La ausencia se prueba con un test de contrato sobre los
  bytes generados, no por inspección.
- El artifact es privado, owner-bound, con TTL y cuota, sobre el store de ADR-061.
- Se declaran siempre `samples_collected`, `samples_lost`, `frames_unresolved`,
  `stacks_truncated`, frecuencia, duración observada y estado del hijo. Una
  pérdida de muestras o de símbolos se declara; no se rellena.

### 5.1. El host no cree lo que el manifest le cuenta (enmienda de 2026-09-09)

Una revisión independiente de containment mostró que la versión original de esta
decisión tenía un agujero: el binario perfilado corre como el mismo uid, en el
mismo contenedor, con `/profile` montado de lectura y escritura, así que podía
pre-crear o sobrescribir los dos artifacts del propio perfilador — la variante
determinista ni siquiera necesitaba ganar una carrera— y el host publicaba el
manifest que encontrase sin comprobar ni su `schema`.

El contenido de un artifact producido dentro del contenedor no es, por sí solo,
evidencia de nada. Se decide en consecuencia:

- **El helper vacía su namespace de PIDs antes de emitir.** Es PID 1 ahí, así que
  `kill(-1)` y cosechar hasta `ECHILD` alcanza a todo descendiente, no solo al
  hijo directo; el barrido se reemite en cada vuelta y está acotado en tiempo.
  Fuera de PID 1 mata solo a su hijo conocido y lo declara. El resultado viaja en
  el manifest (`descendants_reaped`, `namespace_drained`) en lugar de asumirse, y
  un `namespace_drained: false` **invalida la ejecución**: no es un éxito
  degradado. Esto cierra además un hueco anterior por el que los caminos de
  límite de duración y de muestras dejaban nietos vivos.
- **Los dos artifacts se abren con `O_EXCL`.** Un archivo pre-creado es un
  rechazo declarado, nunca una sobrescritura silenciosa.
- **El host reconcilia antes de publicar.** El manifest debe declarar su `schema`,
  llevar exactamente el conjunto de claves esperado, devolver los parámetros que
  el propio host puso en el argv, y sus contadores deben coincidir con lo que el
  parser del host encontró en los stacks. Cualquier desacuerdo es
  `InvalidMetadata`: los bytes pueden estar bien, pero el metadato que los
  describe no se puede avalar. Esto atrapa también a un helper simplemente
  defectuoso, no solo a un hijo hostil.

Las tres afirmaciones están **observadas**, no solo razonadas. Hasta el
2026-09-09 ninguna ejecución registrada había hecho trabajar al vaciado:
`rust-mcp-profile-workload` es de línea recta, su hijo ya estaba cosechado
cuando el vaciado corría, `kill(-1)` alcanzaba un namespace vacío y
`descendants_reaped` era `0` en todos los recibos del árbol. Dos selecciones
nativas del corte M5-03, sobre la imagen admitida, lo cierran
(`docs/validation/M5-03-runtime.json`):

- `profile-descendant-drained` perfila un binario que deja un nieto vivo por
  doble fork —reparentado sobre el propio helper, que es PID 1— y observa
  `descendants_reaped: 1` con `namespace_drained: true`, helper exit 0, hijo
  exit 0, manifest y artifact reconciliando (3 pilas, 195 muestras) y ninguna
  escritura del descendiente en el artifact publicado.
- `profile-precreated-artifact-refused` deja que el nieto cree
  `/profile/stacks.txt` antes de que el programa perfilado arranque su workload
  y observa el `O_EXCL`: helper exit 4, el gateway no exporta nada e
  `InvalidMetadata` en el host.

Consecuencias que se aceptan: el host y el helper quedan acoplados por versión
—viajan en una sola imagen construida de un solo árbol—, y la comprobación de
`namespace_drained` convierte la topología del contenedor (`--init=false`,
`--entrypoint`, namespace de PIDs privado) en una dependencia dura. Es la
elección honesta: si el helper no es PID 1, no puede garantizar lo que afirma.

### 6. Frontera de target

El positivo se califica en el guest **Linux ARM64** con la imagen M5 declarada.
Mach-O y PE **no** quedan calificados por ello. Cambiar el target positivo exige
D13 y un oráculo nativo nuevo, conforme al plan.

## Alternatives considered

- **`perf` de Debian dentro del guest.** Viable —la evidencia demuestra que el
  syscall basta— pero añade un componente de terceros grande, con scripting, y no
  aporta ninguna capacidad que el helper no cubra. Descartado por alcance mínimo.
- **`xctrace`/Instruments autorizado en el host macOS.** Descartado: ejecutaría
  benchmarks del proyecto —código hostil— fuera del gateway, contra G2. Además
  dejaría el positivo dependiendo de una herramienta que el producto no distribuye.
- **`--cap-add=PERFMON`.** Innecesario: `perf_event_paranoid = 2` ya permite el
  perfilado de espacio de usuario. Añadir la capability sería una ampliación sin
  necesidad demostrada.
- **Servicio host estrecho que perfile por encargo.** Descartado: introduce un
  broker privilegiado, exactamente lo que D02/ADR-050 rechazó para mutación.
- **Declarar la plataforma sin enforcement y devolver `unavailable`.** Descartado
  porque existe una configuración que preserva el containment; el plan solo admite
  esa salida cuando no la hay.

## Consequences

M5-03 tiene un positivo nativo alcanzable sin privilegios. El coste es código
propio: un decodificador de ring buffer, un símbolizador ELF y un renderizador SVG
que el producto debe probar por sí mismo, incluido el caso de cero muestras, el de
frames no resueltos y el de cancelación con el árbol observado. El unwinding
depende de frame pointers: el fixture fuerza `-C force-frame-pointers=yes` y un
binario sin ellos produce stacks poco profundos, lo que se declara y no se
disimula. Una denegación de permiso sigue siendo un caso de prueba obligatorio,
pero ya no es el único resultado posible.
