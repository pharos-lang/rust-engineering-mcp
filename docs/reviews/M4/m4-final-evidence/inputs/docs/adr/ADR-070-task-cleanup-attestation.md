# ADR-070 — Atestación de cleanup de Tasks durante M4

## Status

Accepted, hardening M4-06 de la composición compartida; no cambia schemas M1/M3.

## Context

La revisión de integración M4 observó que el compositor Tasks marcaba la señal de
cleanup al retornar una tool, incluso cuando el inspector había quedado en
cuarentena por limpieza incierta. Esperar el worker prueba que terminó la función,
pero no demuestra que desaparecieron todos los objetos del runtime. El gateway
ya conserva esa distinción y el JobExecutor tiene `CleanupObservation::Uncertain`.

## Decision

Después del join, el compositor consulta el inspector compartido. Si está en
cuarentena, no marca cleanup observado, no publica el resultado de la tool y
termina con `CleanupFailed` y observación `Uncertain`. El JobExecutor conserva el
permit y la fase de cleanup hasta su deadline; el watchdog cierra la sesión si
no hay evidencia. El hook de prueba de cleanup incierto usa la misma rama.

Deny usa la colección `data.artifacts`, aunque produzca un solo informe, para
reutilizar la revisión de liveness de Resources en cada poll. Expiración o
revocación no conserva un descriptor anunciado como disponible. El contenido
histórico del veredicto no se reescribe cuando caduca su artefacto.

## Alternatives considered

- Atestar por mero retorno del worker: confunde join con limpieza efectiva.
- Nuevo estado Tasks o error JSON-RPC: innecesario; el executor ya tiene el estado.
- Registry/Resource separado para seguridad: duplica autoridad, cuotas y recovery.

## Consequences

La corrección beneficia a las cinco invocaciones que comparten el compositor.
Requiere repetir los oráculos de cleanup incierto y cancelación de Tasks; el gate
M3 histórico no acredita estos bytes. La API pública conserva sus tipos y devuelve
el fallo de infraestructura que ya define para limpieza no confirmada. No se
libera capacidad para continuar efectos después de una cuarentena.

Verificación focalizada inicial tras el cambio: `cargo test -p rust-engineering-mcp --features test-hooks --test inspection_runtime tasks_runtime::tasks_eof_joins_hostile_child_and_uncertain_cleanup_fails_session --locked --offline -- --ignored --exact --nocapture` pasó 1/1 en 90.08 s; EOF unió el hijo en 346 ms y cleanup incierto falló la sesión. Es evidencia del cambio inicial; el gate final debe ligar los bytes integrados posteriores.
