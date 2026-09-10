# Revisión local fallback — captura vendor M5

## Task

Revisión read-only del contrato ADR-078, la captura/verificación nativa, su
replay incremental hacia el gateway y el volumen vendor. El alcance exacto está
congelado en `reviewed-files.sha256`.

## Result

**PASS WITH P3 FOLLOW-UP.** V-01 quedó corregido en los bytes congelados con hash
`088b3ce0dea5a90c38ec97ae6692ffed509b291ee95235d19d8eb14cd3f6e395`.
No quedan findings P0-P2 en este paquete.

## Files changed

Solo este recibo y su manifiesto de hashes fueron modificados por la revisión.
No edité código de producto, documentación normativa ni pruebas.

## Tests executed

- `git diff --check -- crates/project-adapter/src/vendor_capture.rs`

No se ejecutó Cargo, Docker ni gates; la revisión del fix fue read-only.

## Evidence

### V-01 — RESOLVED — replay autenticado antes del último chunk

`crates/project-adapter/src/vendor_capture.rs:81-253` introduce un replay con
stamp del descriptor, hash incremental, bytes leídos y estado sticky. Cada read
comprueba el stamp antes y después; al alcanzar `artifact_bytes` intenta un byte
extra, compara el SHA-256 con el digest declarado y solo entonces devuelve el
último chunk. Si longitud, stamp, byte extra o digest difieren, devuelve
`VendorCaptureAccess::Mutated` y toda lectura posterior conserva ese rechazo.

Esto coincide con la semántica del supervisor: este deja de pedir bytes cuando
recibe la longitud declarada, por lo que autenticar en un EOF posterior habría
sido insuficiente. La prueba `same_length_rewrite_is_refused_before_the_final_chunk_is_returned`
reescribe el mismo inode sin cambiar longitud, fuerza el caso donde el stamp
inicial ya ve la versión alterada, exige rechazo por digest antes del último
chunk y verifica el estado sticky. El camino limpio exige recibir exactamente la
longitud declarada.

### V-02 — DISPOSED — los límites del tmpfs no prometen el borde

ADR-078 fija deliberadamente `size=512m,nr_inodes=32768` desde los límites del
contrato, pero también declara que una captura cercana al techo no está
calificada y que el documento no afirma que funcione. Por tanto, el overhead de
tmpfs y el inode raíz son una limitación operativa ya declarada, no una garantía
incumplida por este delta. La captura medida de 156 MB sigue siendo el punto que
la matriz final debe recalificar.

### V-03 — P3 — reutilización por nombre depende del llamador para verificar

`crates/project-adapter/src/filesystem/macos/vendor_capture.rs:341-350` puede
reutilizar un artifact existente por nombre y devolver su identidad sin
verificarlo dentro de `capture_vendor_tree`. El llamador de producto actual,
`crates/mcp-server/src/cargo_vendor_cli.rs:148-161`, lo abre y verifica antes de
publicarlo para runtime, así que no existe bypass en M5. Estrechar la API o
verificar al reutilizar reduciría el riesgo de un llamador futuro incorrecto.

## Risks

La carrera no se ejecutó durante esta revisión. El test discriminante está en el
delta, pero debe correr con el gate focalizado y conjunto sobre los bytes finales.

## Decisions

- El descriptor abierto sigue siendo la autoridad; no se reabre el path.
- El digest se valida antes de exponer el chunk que completa la longitud, porque
  el supervisor no pide EOF.
- La modificación detectada queda sticky; ningún rewind rehabilita ese objeto.
- La limitación de capacidad cerca del techo permanece explícita en ADR-078.

## Open issues

- Ejecutar los tests Rust y la matriz runtime final.
- Considerar V-03 como hardening posterior.
- Claude Sonnet 5 fue el único modelo externo invocado: el primer intento no
  pudo autenticarse y el reintento escalado fue interrumpido sin salida. Opus 5
  no fue invocado. No se verificó disponibilidad de ninguno de los dos modelos
  ni se observó agotamiento de cuota.
