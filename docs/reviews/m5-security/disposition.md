# Disposición del Technical Owner — revisión de containment M5

Fecha: 2026-09-08. Revisor: Claude Opus 5, read-only. Rama `ai/m5-performance`.

La revisión es correcta en lo esencial y encontró un defecto real que yo no
había visto. Ninguna de sus objeciones se descarta.

## P1 — el hijo perfilado puede sobrescribir los artifacts del perfilador

**Aceptado.** El hallazgo es válido y la variante determinista —el hijo
pre-crea `stacks.txt` en modo `0444` y `emit()` falla— no necesita siquiera
ganar una carrera. Las precondiciones que el revisor enumera son todas ciertas:
`/profile` es escribible en `ProfileRun`, el volumen es `mode=0700` propiedad de
65534 y el contenedor corre como 65534, el helper solo mata a su hijo directo, y
el host aceptaba cualquier manifest sin comprobar ni su `schema`.

Corrección en curso, en dos frentes porque uno solo no basta:

1. **Vaciar el namespace antes de emitir.** El helper es PID 1 en su propio PID
   namespace, así que `kill(-1, SIGKILL)` y cosechar hasta `ECHILD` elimina a
   todo descendiente, no solo al hijo. Cierra además el hueco G3 preexistente por
   el que los caminos `duration_limit` y `sample_limit` dejaban nietos vivos. El
   resultado se declara en el manifest (`descendants_reaped`, `namespace_drained`)
   en vez de asumirse, y la suposición «soy PID 1» se comprueba antes de señalar.
2. **Reconciliar en el host.** El manifest debe declarar su `schema` y sus
   contadores deben cuadrar con el artifact que el propio host parseó; si no
   cuadran, se rechaza en vez de publicar. Esto atrapa también a un helper
   simplemente defectuoso, no solo a un hijo hostil.

Los artifacts se escriben además con `create_new`, de modo que un archivo
pre-creado es un rechazo y no una sobrescritura silenciosa.

## P2 — `verify_applied` es un subconjunto de la matriz `rust_applied`

**Aceptado, no corregido en esta sesión.** El revisor tiene razón tanto en el
hecho como en dónde duele: la fase que ejecuta código de proyecto bajo un perfil
seccomp ampliado es exactamente donde no debería haber menos cobertura que M4, y
ADR-074 §3 afirma paridad con ADR-064 que no se sostiene. La corrección correcta
es exponer las matrices de `rust_applied` para una fase M5 y delegar, dejando
local solo la comparación de mounts/argv/env/seccomp propia de M5.

No se hizo aquí porque toca `rust_applied.rs`, calificado en M1–M4, y merece su
propio corte y su propio gate en vez de un cambio apresurado al final de la
sesión. Queda anotado como trabajo obligatorio antes de cerrar M5, y ADR-074 §3
no debe leerse como paridad con ADR-064 hasta entonces.

**Actualización de cierre local, 2026-09-10:** corregido en `89ec114` según
ADR-074 §3.1. M5 delega a `rust_applied` y verifica ambas vistas de mounts;
los callers M1–M4 no cambian. La
[re-review independiente](../../validation/m5-delegation/closure-applied-security/review.md)
no encuentra P0–P2 en el delta. La disposición técnica requiere todavía los
gates nativos y conjuntos registrados en la matriz M5; no se presenta la
revisión estática como calificación de ejecución.

## P2 — el oráculo de permiso denegado no está en el recibo generado

**Aceptado y corregido en la documentación; la selección nativa está en curso.**
`M5-03-profiling-native.json` era un smoke manual anterior al gateway y se citaba
junto al recibo reproducible como si ambos fueran evidencia del camino de
producto. Ya está reetiquetado, con sus divergencias enumeradas —incluida la que
el revisor detectó, que su caso de cero muestras pasa un argumento que el
producto nunca pasa— y la matriz y el handoff dicen ahora que el oráculo de
denegación sigue pendiente en el recibo generado.

## P2 — «revocable» con semántica que ningún código implementa

**Aceptado y corregido.** ADR-074 §2, `SECURITY.md` y `docs/security-model.md`
decían que retirar la capability cancelaba el trabajo en curso y hacía join del
árbol. No existe tal camino: la concesión se lee una vez del argv y nada la muta.
Los tres textos dicen ahora lo que el código hace, y nombran la revocación en
caliente como decisión posterior.

## P3

- **Afirmación no medida en el bloqueo M5-01 y opción omitida.** Corregido. Se
  retiró la estimación de «~20 MiB» por no ser el argumento que sostiene el
  bloqueo, y se añadió la cuarta opción que el revisor echaba en falta con la
  evidencia de por qué no funciona: Cargo exige todo paquete del lockfile
  independientemente del target, así que los cuatro archivos por encima del
  límite no se pueden podar, y eso basta para bloquear la ingesta.
- **README del helper razonando sobre `cpu = -1`.** Aceptado; el razonamiento
  quedó obsoleto con el cambio a eventos por CPU. Se corrige junto al P1.
- **`-` inicial admitido en nombres de target de profile y bloat.** Aceptado y
  corregido en los tres sitios, con el comentario de benchmark rectificado: decía
  que «una ruta, un argumento y un flag» fallaban la validación, y el flag no
  fallaba. Un guion **dentro** del nombre sigue aceptándose, porque es un target
  legítimo de cargo.
- **Una ruta `CleanupUncertain` sin cuarentena, y 10 s para cinco cleanups.**
  Aceptados como observaciones correctas; ambos fallan cerrados. Pendientes.
- **Perfil de profiling provisionado incondicionalmente.** Correcto; se anota
  que la presencia del archivo no es evidencia de concesión.
- **El puerto del adapter no tiene gate de capability.** Correcto para un
  embebedor de la biblioteca. Pendiente como defensa en profundidad.
- **`--allow-profiling` se aceptaba mientras las tools eran inalcanzables.**
  Resuelto: las cuatro tools ya están registradas y el inventario es de 31.

## Sobre lo que el revisor verificó

Se conserva íntegro. Su comprobación estructural del delta seccomp, de las tres
puertas de la capability, del rechazo de la configuración de Cargo del proyecto,
del alcance del helper, de la imposibilidad de contenido activo en el SVG y de
la precedencia del cleanup sobre el error de trabajo es la parte más valiosa de
la revisión y coincide con la evidencia registrada.
