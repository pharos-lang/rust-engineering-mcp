# Revisión independiente M5 — containment y capability de profiling

Revisor: Claude Opus 5, read-only, esfuerzo alto. Fecha: 2026-09-08.
Rama `ai/m5-performance`. Inputs congelados: [inputs.json](inputs.json).

## Veredicto del revisor

La dimensión de sandbox es real y está bien evidenciada: el delta seccomp es una
sola syscall en una sola fase, la capability es una concesión positiva del host
comprobada dos veces antes de que exista contenedor alguno, la verificación de la
configuración aplicada rechaza una inyección de `CAP_PERFMON` y un perfil
equivocado, el SVG no puede llevar contenido activo por construcción, y el
cleanup se une y pone en cuarentena en todas las salidas.

Lo que **no** se sostiene es la afirmación de que las cifras y las pilas que la
tool publica sean la medición *del producto*: el binario perfilado corre con el
mismo uid, en el mismo contenedor, con `/profile` montado **de lectura y
escritura**, y los dos artifacts del helper son archivos corrientes de ese
directorio escritos después de matar al hijo pero antes de destruir el
contenedor, mientras cualquier nieto que el hijo haya lanzado sigue vivo. Nada en
el host reconcilia el manifest con los stacks, y su campo `schema` nunca se
comprueba.

## Findings

| Severidad | Finding | Disposición |
| --- | --- | --- |
| P1 | El hijo perfilado puede sobrescribir los artifacts del propio perfilador; el host los acepta sin reconciliar | **Aceptado y corregido** — ver disposición |
| P2 | `verify_applied` del gateway M5 es un subconjunto estricto de la matriz `rust_applied` que usan los demás gateways | **Aceptado** |
| P2 | El oráculo obligatorio de permiso denegado de M5-03 no está en el recibo generado | **Aceptado y corregido** |
| P2 | «Revocable… cancela el trabajo en curso y hace join del árbol» no lo implementa ningún código | **Aceptado y corregido** |
| P3 | Varios: perfil provisionado incondicionalmente, `-` inicial admitido en nombres de target, una ruta `CleanupUncertain` sin cuarentena, 10 s para cinco cleanups, el README del helper razona sobre `cpu = -1`, la afirmación no medida del bloqueo M5-01, las cuatro tools inalcanzables mientras `--allow-profiling` se acepta, el puerto del adapter sin gate de capability | Ver disposición |

No hubo findings P0.

## Lo que el revisor verificó y encontró sólido

Delta seccomp comprobado estructuralmente (mismo `defaultAction`, `archMap` y
grupos, más un único grupo `SCMP_ACT_ALLOW` cuyo único nombre es
`perf_event_open`). Tres puertas independientes para la capability, ninguna
alcanzable por el peer. Rechazo de la configuración de Cargo del proyecto antes
de que exista volumen. `--config` a la precedencia más alta nombrando la fuente
que el `CARGO_HOME` ya declara, de modo que una segunda definición es error duro.
El helper solo perfila al hijo que él mismo lanza; catorce bloques `unsafe`, todos
en `linux.rs`, cada uno con su invariante; `lib.rs` sin ninguno. Ningún path puede
llegar a un artifact, y el alfabeto se reimpone en el host, que **rechaza** en vez
de reparar. El SVG neutraliza `:` y `=` y la `h` inicial de `href` como
referencias numéricas, así que los tokens prohibidos son irrepresentables. El
cleanup domina al error de trabajo y la cuarentena es incondicional.

El texto completo del revisor se conserva en [findings.md](findings.md).
