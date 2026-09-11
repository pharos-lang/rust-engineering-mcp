# Complemento M4 — lo que M3 dejó construido, decidido y aprendido

Este documento acompaña al [encargo base M4](implement-m4.md) y al
[plan M4](../roadmap/m4-security.md); no los sustituye. Su único propósito es que
quien ejecute M4 no repita el trabajo, las decisiones ni los errores de M3.
Todo lo afirmado aquí tiene recibo enlazado; contrástalo con el árbol real antes
de construir sobre ello, porque un documento no acredita bytes.

## 1. Punto de partida real

M3 está **cerrado e integrado en `main`**: merge `57c40373` (PR #14), rama
`ai/m3-quality` conservada en `93991c4c`, y encima los commits de recibo,
del tablero y del bump de versión (`e78a3b8`). El checkout se identifica como
`0.3.0-dev`. No hay tag, release ni publicación: la release estable sigue en `0.1.0`.

El contrato público tiene **22 tools**: las trece M1, las cinco M2 y las cuatro
M3 (`rust.test.nextest`, `rust.coverage`, `rust.semver.check`,
`rust.mutation.test`). Los 23 snapshots del directorio —los 22 contratos de
tools y `doctor-report.json`— siguen byte a byte idénticos; el snapshot de
mutation es uno de los cuatro snapshots nuevos de M3, no una modificación de un
snapshot previo.

Evidencia sobre los bytes de `main`, toda reproducible:
[core 14/14](../validation/M3/core-gate.json),
[full 25/25](../validation/M3/full-gate.json),
[runtime Docker 62/62](../validation/M3/runtime.json),
[seguridad Rust 20/20](../validation/M3/rust-security.json),
[rollback 10/10](../validation/M3/06-rollback.md) y el
[handoff](../validation/M3/07.md). El inventario del gate son 810 inputs y su
hash se registra en cada recibo: si tocas un byte calificado, ese hash cambia y
la evidencia anterior deja de describir lo que vas a integrar.

## 2. Lo que M4 hereda y debe reutilizar, no reinventar

**Ejecución de jobs con tareas MCP negociadas ([ADR-060](../adr/ADR-060-bounded-job-execution-and-mcp-tasks.md)).**
`domain::job` define identidad, estado, fase y presupuestos; `application::job`
tiene el `JobExecutor`, el registro ligado al owner y el watchdog. `JobKind` es
un enum cerrado con cuatro variantes: **añadir `rust.miri` o `rust.deny` como job
significa añadir su variante y su brazo, no crear un segundo ejecutor**. Miri es
justamente la operación larga para la que existe este camino; el plan M4 propone
300 s por defecto y 1800 s de máximo, dentro del techo de job de 3600 s.

Reglas que no puedes relajar: el permiso de job **es** el permiso único de worker
de ADR-030; `tasks/get` y `tasks/cancel` nunca lo toman; `Cancelled` solo se
publica tras cleanup observado; el TTL es fijo y el poll no lo desliza; los IDs
desconocidos, ajenos, expirados y revocados devuelven el mismo `-32602`
enmascarado. El anuncio de Tasks está **encendido** y exige declaración mutua del
peer; un peer que no la declara sigue el camino síncrono acotado.

**Store durable y privado de artifacts ([ADR-061](../adr/ADR-061-private-quality-artifact-store.md)).**
Vive bajo el state-root del host, es macOS ARM64/APFS positivo y falla cerrado en
el resto. Publica por miembro con degradación explícita, reclama lo expirado,
tiene marca de reloj, cuarentena y los comandos `quality-artifacts recover|prune`.
Los findings de M4, los reportes de deny y las salidas de Miri **son artifacts de
este store**, con su esquema `rust-quality-artifact://` de índice y trozos y su
tope de 512 KiB por respuesta. La frontera de privacidad ya está documentada:
mismo uid, mismo state root y misma raíz concedida pueden releer evidencia
retenida. Eso importa para los canarios de secretos que exige M4-06.

**Egreso por bytes desde rutas fijas.** Ningún archivo sale por una ruta que
elija el guest. Los reportes multiarchivo cruzan como **un** `ArchiveBundle` USTAR
validado con el perfil cerrado compartido (`validated_closed_ustar`), que
comprueba `prefix`, `linkname`, uid/gid y rechaza enlaces, `..` y miembros de más.
Cuando M4 tenga que sacar el árbol de `cargo-deny` o de Miri, ese es el mecanismo.

**Fases del gateway.** `Phase` ya modela ingest, guardianes que mantienen vivos
volúmenes entre contenedores, exportadores de argv fijo y una fase de listado que
acota el trabajo antes de ejecutar código del proyecto. SemVer añadió un segundo
volumen de solo lectura (`/baseline`). Si M4 necesita un runtime nuevo, extiende
esta gramática cerrada; no introduzcas un gateway paralelo ni argv libre.

**Perfil seccomp de calidad ([ADR-064](../adr/ADR-064-quality-job-seccomp-profile.md)).**
`seccomp-rust-quality.json` difiere del base en **exactamente una regla**: un
`socketpair` AF_UNIX de flujo anónimo. Lo comparten las cinco fases de calidad y
el verificador compara el perfil aplicado con el declarado por la fase, así que
una fase que reciba un perfil más ancho falla cerrado. Miri correrá bajo un
runtime nightly distinto: **la calibración vieja no lo autoriza**, y cualquier
syscall adicional necesita su propio ADR con controles negativos, igual que este.

**Volumen ejecutable de cobertura ([ADR-065](../adr/ADR-065-coverage-target-volume.md)).**
Es el único montaje del producto que permite ejecutar lo recién construido, está
acotado por job, ausente de exportadores y de toda fase no-cobertura, y su matriz
de acceso está fijada por expectativas literales por fase. Si Miri necesita algo
parecido, ese es el precedente y el listón: decisión propia, matriz literal y
controles negativos que prueben que nadie más lo obtiene.

**Aprovisionamiento del guest ([ADR-063](../adr/ADR-063-m3-guest-plugin-provisioning.md)).**
Imagen `sha256:384a1742…` con Rust 1.98.1 más cinco plugins fijados por hash,
verificada 47/47 y con
[recibo](../validation/M3/provisioning.json). `fixtures/rust-runtime/` sabe
descargar por hash, verificar y construir. Los binarios de `cargo-deny` y el
toolchain nightly de Miri entran por ahí, con versión, digest, licencia y notices,
nunca en tiempo de ejecución. Que una herramienta esté en el host o en CI **no**
la acredita en el guest: esa confusión ya costó un corte en M3.

## 3. Decisiones vigentes que condicionan M4

- **D19 sigue abierta.** M3 no la tocó. Cuando decidas perfiles `strict`/`release`,
  recuerda que M3 ya introdujo `execution_mode` en las tools nuevas y que
  `fast`/`standard` deben quedar exactamente como están.
- **Presupuestos ya medidos**, no propuestos: ADR-060 tiene la tabla con 30
  muestras en frío y 30 en caliente por operación, en
  [M3-02-budgets.json](../validation/M3/02-budgets.json). Hereda esa forma de
  medir: muestras crudas, p50/p95/p99, y sustituir el número propuesto por el
  medido en el propio ADR.
- **Cobertura y SemVer** ([ADR-062](../adr/ADR-062-coverage-accounting-and-semver-baselines.md))
  fijaron la regla de denominador cero, el dedupe de archivos compartidos y la
  taxonomía de resultados incompletos. M4 hereda el criterio: parcial, ausente o
  desconocido nunca es aprobado.
- `rust.dependencies.audit` conserva su contrato y su motor. Deny **compone** esa
  observación; no añade un segundo matcher ni refresca advisories.

## 4. Errores de M3 que no conviene repetir

Cada uno costó tiempo real y está documentado con su evidencia.

**El gate se dio por bueno antes de existir.** El primer intento de calificación
declaró cortes listos cuando la ejecución Docker no había corrido. Norma que
funcionó después: ningún corte es «Done» sin recibo con conteos, y un skip nunca
cuenta como aprobado.

**Trabajadores en paralelo sobre archivos compartidos.** Varios agentes editaron
`rust_execution.rs`, `rust_gateway.rs` y `stdio.rs` a la vez; hubo compilaciones
rotas y un agente llegó a pausar creyendo que otro invadía su paquete. Lo que
resolvió: cada worker construye primero sus archivos propios y solo al final
añade los brazos mínimos a los compartidos, releyendo el archivo justo antes de
cada parche.

**Dos defectos reales que solo aparecieron bajo concurrencia**, ambos por copia de
la tabla de descriptores en `fork`: un `flock` de un hilo hermano quedaba vivo
dentro del hijo hasta el `exec`, y el hermano recibía `Busy` al reintentar. Uno se
manifestó en el store de calidad y otro en los tests M2 solo en el runner de CI.
Si tus tests lanzan procesos, sácalos de toda ventana de lock.

**Un test intermitente por resolución de reloj.** El instante tenía resolución de
un segundo, así que un TTL «de un segundo» dejaba una ventana aleatoria menor a
ese segundo para varios `fsync`. Se corrigió inyectando el reloj, no aflojando la
aserción.

**Afirmaciones que no podían fallar.** Un campo del contrato declaraba haber
verificado que el código fuente del host no cambió, y era estructuralmente siempre
verdadero. Se eliminó del esquema. Cuando publiques una garantía, pregunta qué
tendría que ocurrir para que ese campo fuera `false`.

**Documentación que promete lo que el binario no cumple.** Ocho documentos decían
`0.3.0-dev` mientras `Cargo.toml` seguía en `0.2.0-dev`. Se subió la versión y se
re-calificó, porque la versión es un byte del inventario del gate.

## 5. Realidades operativas del entorno

- **Docker exige propietario único.** Nunca dos gates a la vez. El gate rechaza
  arrancar si quedan objetos etiquetados `org.rust-mcp.execution=true`, así que
  un proceso matado a mitad deja residuo que hay que retirar a mano.
- **Nunca dejes un gate corriendo sin vigilancia.** Un `full` tarda unos cuarenta
  minutos; una sesión que termina su turno a mitad deja el proceso huérfano y el
  recibo en `running`. `scripts/test-m3-runtime.py` ya acota cada selección y
  registra el residuo Docker al matar.
- **CODEOWNERS exige revisión del propietario, que es también el autor del PR y el
  único colaborador**, así que GitHub nunca aceptará una aprobación. El merge de
  M3 requirió bypass de administrador autorizado por el owner. Cuenta con lo mismo
  o pide un segundo revisor antes de empezar.
- **CI compila en Linux y Windows lo que aquí solo se prueba en macOS.** M3
  encontró ocho defectos reales de portabilidad la primera vez que M2 llegó a CI.
  Si tu código es específico de plataforma, comprueba pronto con
  `cargo clippy --target x86_64-unknown-linux-gnu`.
- **SonarCloud es un check obligatorio** con umbrales sobre código nuevo:
  cobertura, duplicación y seguridad. Súbelos con pruebas reales; las exclusiones
  solo valen para lo que el escáner portable no puede medir, y cada una se
  justifica en `docs/ci.md` con el recibo que sí prueba ese comportamiento.
- **CodeQL no es obligatorio** y hoy reporta 33 alertas evaluadas como falsos
  positivos: la palabra `nonce` es la etiqueta de propiedad de contenedores
  Docker. La recomendación registrada es descartarlas y renombrar la etiqueta en
  un corte propio con su gate; si M4 toca esas rutas, es el momento natural.
- **La evidencia de clientes no debe arrastrar credenciales.** Un harness copió el
  home de un cliente, con su `auth.json`, dentro del árbol de evidencia. Se retiró
  antes de cualquier commit y el harness ahora usa un directorio temporal fuera
  del repositorio y verifica que no queden credenciales. Revisa lo mismo en el
  tuyo.

## 6. Deuda que M4 hereda declarada

- Un fallo no reproducido de un test de CLI en Linux, registrado con su evidencia
  en el commit `93991c4`. La hipótesis obvia se midió y se descartó. Necesita
  reproducción en Linux antes de que nadie toque el oráculo.
- Las 33 alertas de CodeQL fueron evaluadas como falsos positivos; queda
  descartarlas y renombrar la etiqueta de propiedad Docker `nonce` en un corte
  propio con su gate.
- El anulador de anuncio de Tasks solo puede forzar la capability a encendida, así
  que el camino no anunciado no tiene oráculo extremo a extremo por el binario real;
  es un residuo aceptado.
- `LiveJobAuthority::revalidate` responde autorizado bajo contención del registro.
  Es correcto mientras se mantenga la regla de permiso único, y esa dependencia no
  está aseverada en ninguna prueba; es un residuo aceptado.

## 7. Cómo se organizó M3, por si sirve

Orquestador único que no implementa, con delegados por CLI real: integrador,
adaptadores, fixtures, documentación, validador con Docker y revisores
independientes. Cada paquete llevaba base Git, fuentes normativas, DoD, oráculos,
archivos permitidos y formato de entrega; cada ejecución dejó prompt, comandos,
salidas, código de salida y hashes en `docs/validation/M3/delegation/`, con los
intentos fallidos conservados. Las revisiones independientes encontraron cosas que
la implementación no vio: un bloqueo de disponibilidad que congelaba la sesión
entera mientras corría un job, una publicación que dejaba bytes inalcanzables
ocupando cuota, y un validador de archivos al que le faltaba comprobar el campo de
prefijo. Ninguna se encontró leyendo el propio código que las produjo.
