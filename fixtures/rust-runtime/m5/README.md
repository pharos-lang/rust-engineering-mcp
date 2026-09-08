# Imagen guest M5 — `rust-engineering-runtime:1.98.1-arm64-m5`

Deriva por digest de la imagen M4 aprobada
`sha256:25ed3626e710081a571a86a29521eaf2e890e796afd422ba5e409e0ce1891635` y añade
exactamente dos binarios en `/opt/perf/bin`:

| Binario | Versión | Origen | Licencia |
| --- | --- | --- | --- |
| `cargo-bloat` | 0.12.1 | archivo publicado en crates.io, `sha256:56e2c483…` | MIT |
| `rust-mcp-profile-helper` | ver `fixtures/profile-helper` | fuente de este repositorio | MIT OR Apache-2.0 |

Nada más cambia: ni el toolchain, ni los plugins M3, ni los binarios de seguridad
M4, ni el usuario, ni el `WORKDIR`, ni el `PATH`. Ninguno de los dos binarios
queda accesible por `PATH` en un contenedor de trabajo; el gateway los invoca por
ruta absoluta.

## Procedimiento

```sh
python3 -B scripts/build-m5-runtime.py
```

Ese script es el procedimiento completo y deja el recibo en
`docs/validation/M5-provisioning.json`. Hace, en orden: comprobar que
`rust-engineering-runtime:1.98.1-arm64-m4-scanner` resuelve exactamente a
`sha256:25ed3626e710…` y abortar si no; preparar el contexto con `provision.py`;
construir con `--network=none --pull=false`; y verificar sobre la imagen
resultante que los dos binarios existen, que **ninguno** de los dos es alcanzable
por `PATH`, que los binarios M3/M4 siguen presentes, y que el contexto de
construcción no dejó residuos.

La base se nombra por tag y no por digest en el `FROM` porque BuildKit resuelve
un `FROM sha256:…` como referencia **remota** y falla con `DeadlineExceeded` bajo
`--network=none`, y el builder clásico ya no funciona en Docker 29.7.2 (se queda
colgado tras el aviso de deprecación). La garantía de digest no se pierde: se
comprueba en el script inmediatamente antes de construir y el id observado queda
en el recibo.

`provision.py` **no accede a la red**. Toma cada archivo `.crate` de la caché
local de Cargo y verifica su `sha256` contra el checksum que el propio Cargo
registró en el lockfile fijado del paquete que lo necesita: el `Cargo.lock`
publicado dentro del archivo de `cargo-bloat` y el `Cargo.lock` privado del
helper. Si falta un input, falla; no lo adquiere.

`build.sh` corre dentro de la imagen sin red: verifica `SHA256SUMS`, confirma
`rustc 1.98.1` y el host `aarch64-unknown-linux-gnu`, extrae el vendor, escribe
cada `.cargo-checksum.json`, compila ambos binarios con `--release --locked
--offline --target aarch64-unknown-linux-gnu`, comprueba que el ELF resultante es
AArch64 y que no le falta ninguna biblioteca, y deja bajo
`/usr/share/doc/rust-runtime/m5` el SBOM, el mapa de dependencias, las licencias
recolectadas, el hash de cada binario y un inventario completo.

## Rollback

La imagen M4 `sha256:25ed3626e710…` permanece intacta y sigue siendo la aprobada
mientras la M5 no esté calificada. Revocar M5 es volver a apuntar el gateway a
ese digest; no hay estado que migrar.

## Límites

El positivo se califica en Linux ARM64. `cargo-bloat` no soporta WASM y el
soporte Mach-O/PE de la herramienta no queda calificado por este trabajo.
