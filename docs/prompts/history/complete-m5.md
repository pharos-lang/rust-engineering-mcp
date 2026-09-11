# Completar M5 — sesión limpia

Eres el Technical Owner, arquitecto, implementador e integrador de M5 en
`/Users/cburgosro/Projects/rust-mcp`. Continúas un trabajo avanzado; **no lo
empieces de cero y no rehagas lo que ya tiene recibo**.

## Autorización y frontera

Autorizado: implementar lo que falta de M5 sobre la rama existente
`ai/m5-performance`, con commits locales coherentes.

**No autorizado**: publicar PR, hacer push o merge, publicar tags o releases, ni
avanzar a M6. Detente antes de M6.

## Estado al recibir

Rama `ai/m5-performance`, 62 commits sobre `main`, que sigue intacto en
`c6099f27` y sin push. HEAD al escribir esto: `bdfd785`.

**El árbol tiene cambios sin commitear que son del owner, no tuyos**: subidas de
versión en `Cargo.toml`, `Cargo.lock` y `crates/mcp-server/Cargo.toml` (lancedb
0.38.0, fastembed 6.0.3, jsonschema 0.55.1, tokio-rustls 0.26.5). **Presérvalos y
no los incluyas en ningún commit tuyo.** Nota: en la sesión anterior acabaron
enmendados dentro del commit `a3cb48c`, cuyo mensaje solo habla de revisiones;
sepáralos solo si el owner lo pide.

Corre los gates en un **worktree limpio** en tu HEAD, no en el árbol principal,
por esa razón: un recibo no debe acreditar bytes que quien lo firma no controla.

## Decisiones vinculantes: léelas antes de tocar nada

Son del owner y no se renegocian. Si crees que alguna es imposible, **detente y
repórtalo** en vez de elegir una interpretación.

| ADR | Qué fija |
| --- | --- |
| [ADR-078](../../adr/ADR-078-offline-vendor-capture.md) | Captura de vendor offline separada de `SourceBundle`, con sus límites, su alfabeto y **tres correcciones fechadas** |
| [ADR-079](../../adr/ADR-079-bloat-result-semantics.md) | `passed` de bloat = «análisis ejecutado y validado», nada más |
| [ADR-080](../../adr/ADR-080-harness-logs-as-artifacts.md) | Logs del harness como artifacts por repetición |
| [ADR-081](../../adr/ADR-081-benchmark-statistical-requalification.md) | Criterios estadísticos congelados **antes** de medir, con dos correcciones |
| [ADR-073](../../adr/ADR-073-benchmark-method-and-dataset.md) · [074](../../adr/ADR-074-profiling-capability-and-containment.md) · [076](../../adr/ADR-076-m5-performance-contracts.md) · [077](../../adr/ADR-077-m5-runtime-admission.md) | Método, profiling, contratos y admisión de imagen |

Estado por corte: [matriz](../../validation/M5/matrix.md) ·
[handoff](../../validation/M5/handoff.md). Revisiones:
`docs/reviews/M5/m5-security/`, `docs/reviews/M5/m5-method/`,
`docs/reviews/M5/m5-rereview.md`, `docs/validation/M5/delegation/`.

## Lo que falta, en orden de dependencia

### 1. Ruta crítica — desbloquear el positivo de M5-01

`rust.benchmark.run` nunca ha medido un benchmark de criterion de extremo a
extremo. La captura funciona en el host; **la ingesta en el guest falla**, y la
causa está medida:

- El volumen del vendor se crea con `mutation_gateway::VOLUME_OPTIONS` —tmpfs de
  64 MiB, 8 192 inodos— mientras ADR-078 fija el contrato en 512 MiB y 32 768
  entradas. Ingerir el cierre real produce **2 717** líneas de `No space left on
  device` y `tar` sale con 2, que el gateway mapea a `Infrastructure`.
- Segundo defecto en el mismo camino: `cleanup_until` valida el volumen contra
  `VOLUME_OPTIONS`, así que un volumen dimensionado de otra forma se rechaza al
  limpiar. Se observó `CleanupUncertain` **y un volumen superviviente**.

**Qué hacer.** Dimensionar el volumen del vendor desde los límites de ADR-078
—`size` al límite de bytes, `nr_inodes` al de entradas— dejando uid, gid, mode,
`nosuid`, `nodev` y `noexec` idénticos, y usar `cleanup_until_with_options`, que
ya existe para esta misma razón en el volumen de target. Hay que llevar las
opciones también al `fingerprint_volume`.

Referencia, **no aplicable con `git apply`** porque sus rutas son de un
scratchpad: [`vendor-volume-sizing.patch`](../../validation/M5/history/gate-attempts/vendor-volume-sizing.patch).
Deriva el cambio tú y compruébalo.

**Esto mueve todas las huellas de ejecución M5**, así que exige recalificar los
cinco cortes. Es esperado, no un problema.

**Evidencia de que detrás funciona** (probado con un parche local que se
revirtió, así que **no cuenta como calificación**): ingesta de 161 MB en ~1 s,
`cargo bench` compila los 52 paquetes en 24,4 s y mide en ~30 s bajo
`--cpus=1 --memory=1g`; el corte pasó completo en 149 s con dataset real —tres
benchmarks, 30 muestras cada uno, `sampling_mode: Linear`— y el digest de la
captura `sha256:8e1c814b…` llegó a `vendor_fingerprint`. Una captura provisionada
está en `fixtures/criterion-vendor/capture/` (gitignorada), así que el corte es
ejecutable de inmediato.

Las cuatro selecciones ya están escritas en
`crates/execution-adapter/src/performance_native.rs`: el positivo, el rechazo por
digest declarado que no cuadra, la comprobación de solo-lectura en el guest, y la
cancelación durante la ingesta.

### 2. Entorno observable para `rust.benchmark.compare`

`hardware_profile` en `performance_port.rs` fija `cpu_governor: None`
incondicionalmente y nadie lo observa nunca, así que **cambiar de máquina no
arregla nada**. Hace falta observación verificable del entorno.

**No hay prisa y no hay riesgo de abrir nada por accidente**: existe una guarda
explícita, `METHOD_QUALIFIED_FOR_DIRECTION = false`, y `check-architecture.py`
falla si alguien la pone en `true` sin un recibo que lo respalde.

### 3. Recalificación nativa completa

Los cinco cortes sobre bytes finales, con sus recibos en `docs/validation/`.
Uno cada vez (`--exact --ignored --test-threads=1`).

### 4. Matriz de clientes final

`scripts/test-m5-clients.py --run --with-runtime`. El plan ya cubre las filas de
recuperación de logs; la matriz anterior está marcada como superada porque midió
contratos que ADR-079 y ADR-080 cambiaron.

### 5. Revisiones independientes

De la **captura de vendor**, que es código nuevo y sustancial que nadie de fuera
ha visto, y de la recalificación de bloat. Usa `codex` o `agy`: hoy las
revisiones de otra familia de modelo encontraron cuatro defectos que cuatro
revisiones internas no vieron, incluidos los dos peores del día. El patrón y las
restricciones del entorno están en
[`m5-delegation/README.md`](../../validation/M5/delegation/README.md).

### 6. Gate `core` y `full` sobre bytes finales

**Corriendo solo.** Con `RUST_MCP_TEST_SOCKET`, `RUST_MCP_E5_DIR`
(`target/m1-15-candidate/assets/model`) y `ORT_LIB_LOCATION`.

### 7. Cierre

Matriz, handoff, tablero, `implementation-status.md` y documentación pública
sincronizados con el comportamiento real.

## Lo que NO debes hacer

- **No subas los límites de `SourceBundle`.** Son la frontera calificada en M2/M4
  y la razón de que ADR-078 exista.
- **No debilites la puerta de precisión ni muevas el umbral material del 5 %.**
- **No pongas `METHOD_QUALIFIED_FOR_DIRECTION` en `true`.** El criterio de
  potencia está **declarado inalcanzable en este entorno**, medido: la puerta de
  precisión decide antes que el intervalo el 99,7–100 % de las veces, y resolver
  el 5 % exige deriva bajo 2,2 % mientras este host mide 6,1–28,7 %.
- **No reconstruyas la imagen del guest** salvo decisión explícita: un digest
  nuevo invalida todos los recibos nativos y las dos calibraciones. Digest
  admitido: `sha256:e0a5ca1661b3e49d0a3d68ee3cc0963453078d08eb7fc43c30538c16b7998aac`.
- **No marques M5 como Done** hasta que M5-01..05 y G1–G9 estén demostrados,
  incluido el positivo de profiling. Separa lo demostrado localmente de la
  integración remota pendiente.

## Reglas de operación que costaron caro aprender

- **El gate corre solo.** Hoy hubo tres flakes de planificación, uno de ellos un
  gate en rojo que causó compilar en paralelo. Ante un `TimeoutExpired` sobre un
  subproceso trivial, mira `uptime` antes de sospechar del producto, relanza a
  solas, y **no amplíes el timeout**.
- **Un recibo acredita los bytes que midió.** Cuando un contrato cambia, la
  calificación anterior deja de acreditarlo: hoy M5-04 volvió a *In progress* por
  esto. Los recibos superados se archivan, **no se editan**.
- **Fija el criterio antes de medir.** Se aplicó a los umbrales estadísticos y al
  diagnóstico de un flake, y las dos veces evitó racionalizar el resultado.
- **No publiques un número sin sus condiciones.** Un hueco declarado es
  evidencia; un número con nota al pie acaba citándose sin la nota.
- **Etiqueta la procedencia de cada límite**: medido, política o argumentado.
  Decir «lo medimos» de un número que la medición no obliga es razonamiento
  circular.

## Dos errores del owner anterior, para que no se repitan

Los dos son el mismo patrón: **un número correcto en su propio marco, fijado sin
mirar la restricción con la que interactúa.**

1. El criterio de potencia de ADR-081 era aritméticamente imposible junto al de
   cobertura, porque `regression` se emite si y solo si el extremo inferior supera
   el umbral, luego con efecto igual al umbral `potencia ≤ 1 − cobertura`.
2. Los límites de ADR-078 se fijaron desde mediciones del host sin comprobar que
   el volumen del guest pudiera recibirlos. Ninguna captura en los límites del
   contrato podía ingerirse.

Las dos las encontró la medición, no el razonamiento. Cuando fijes un número,
busca activamente con qué interactúa.
