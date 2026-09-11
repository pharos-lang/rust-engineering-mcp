# ADR-079 — Qué significa que `rust.binary.bloat` tenga éxito

Fecha: 2026-09-09.

## Status

Accepted. Sustituye la semántica de resultado que ADR-076 §6 dejó implícita.
No concede ninguna calificación: la recalificación nativa y por clientes se hace
después de implementar esto, sobre bytes finales.

## Context

Una revisión independiente y después la matriz de clientes mostraron lo mismo
desde dos lados: `rust.binary.bloat` **no puede devolver `passed` para ningún
binario que enlace `std`**. El tope propio del producto es
`BLOAT_MAX_ROWS = 256`; el positivo nativo produjo 634 funciones, de las que se
omiten 378 —23 304 de 235 016 bytes atribuidos, un 9,9 %—, así que la
completeness sale `Truncated` y `outcome()` convierte todo lo que no sea
`Complete` en `blocked` / `EVIDENCE_INCOMPLETE`
(`crates/mcp-server/src/stdio/bloat.rs`).

Los datos viajan completos —tamaño exacto incluido—, así que no se pierde
información: lo que está mal es la palabra. Y el camino de éxito de la tool es
estructuralmente inalcanzable, lo que hace que su `passed` no signifique nada.

El defecto se encontró construyendo la matriz de clientes, es decir en el momento
en que corregirlo pone una fila en verde. Por eso se registró sin corregir y se
pidió decisión explícita. La decisión es corregirlo, y el orden importa: **primero
la semántica, después las pruebas discriminantes, después la revisión
independiente, y solo entonces la recalificación.** Cambiarlo en el otro orden
sería ajustar el producto al resultado.

## Decision

### 1. Tres conceptos que hoy están mezclados

| Concepto | Qué significa | Dónde vive |
| --- | --- | --- |
| **Validez de la medición** | El analizador corrió, produjo un informe que el producto pudo parsear, y el tamaño que reporta coincide con el que el producto midió por su cuenta | Decide el `status` |
| **Cobertura del ranking** | Cuántas filas de atribución entran en el tope del producto y cuántas quedan fuera | Se **declara**, no decide el `status` |
| **Recorte de la respuesta** | Cuántas filas se quitaron además para caber en el presupuesto de 512 KiB | Se **declara**, no decide el `status` |

Hoy los tres colapsan en un único `complete: bool`, y el `outcome` lee ese
booleano. Esa es la raíz del defecto.

### 2. `passed` significa «análisis ejecutado y validado»

Y **nada más**. En particular no afirma que el binario esté optimizado, ni que la
atribución sea exhaustiva, ni que el ranking describa todo el archivo. Un análisis
válido puede terminar correctamente mostrando las 256 funciones principales,
siempre que:

- declare cuántas omitió y por qué límite (tope del producto o presupuesto de
  respuesta, distinguibles), y
- conserve el **tamaño exacto del archivo analizado**, que es una medición del
  producto y no una estimación del analizador.

### 3. Lo que sigue impidiendo el éxito

No se relaja nada de esto:

- **`SizeMismatch`**: el tamaño que reporta el analizador y el que mide el
  producto no coinciden. La atribución entonces no describe el archivo medido y
  no se publica como si lo hiciera.
- **`Unavailable` / `UnsupportedFormat`**: no hubo medición, o el formato no es
  uno que el analizador pinnado soporte.
- **`CompilationFailed` / `AnalysisFailed`**: fallo observado, que ya es `failed`
  con `OBSERVED_FAILURE` y sigue siéndolo.
- **Pérdida de evidencia necesaria**: si el artifact que sostiene la atribución no
  se pudo publicar, no hay éxito que declarar.

### 4. Consecuencia sobre el DTO

`complete` deja de ser un booleano que mezcla causas. La respuesta declara por
separado el tope que actuó y el recorte que actuó, con sus conteos, de modo que
un lector distinga «el producto acotó el ranking a 256» de «la respuesta no cabía»
de «la evidencia no es válida». La `completeness` del artifact sigue siendo del
artifact.

## Alternatives considered

- **Subir `BLOAT_MAX_ROWS`.** Descartado: mueve el umbral sin arreglar la
  semántica, y cualquier binario más grande vuelve a caer. El defecto no es el
  valor del tope, es que un tope declarado se lea como evidencia incompleta.
- **Dejarlo como está y documentarlo.** Descartado por el owner: una tool cuyo
  camino de éxito es inalcanzable no es una tool documentada, es una tool rota.
- **Emitir `passed` con `complete: false` sin distinguir causas.** Descartado:
  reproduce el problema un nivel más abajo, porque el recorte por presupuesto y
  el tope del producto no significan lo mismo para el lector.

## Consequences

La recalificación nativa y la matriz de clientes de `rust.binary.bloat` se
rehacen **después** de este cambio, sobre bytes finales, con revisión
independiente de por medio. Los recibos actuales quedan como historia de la
semántica anterior y se archivan, no se editan. Hasta que eso ocurra, el estado
publicado de M5-04 vuelve a **In progress**: la calificación anterior midió un
contrato que esta decisión sustituye.
