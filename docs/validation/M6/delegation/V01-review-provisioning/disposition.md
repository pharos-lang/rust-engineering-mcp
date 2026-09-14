# Disposición — revisión independiente V01 del aprovisionamiento M6 (W01)

Fecha: 2026-09-11. Objeto: el diff de W01 (`fixtures/rust-runtime/m6/**`,
`scripts/build-m6-runtime.py`, `scripts/test-m6-provisioning.py`,
`scripts/gate.py`, `.github/workflows/sonarcloud.yml`, `docs/ci.md`,
ADR-082), sha256 del diff en [inputs.sha256](inputs.sha256). Revisor: Claude
Sonnet 5 (Claude Code 2.1.268, `claude -p --model sonnet --effort high --tools
"" --restricted`, diff por stdin, 288 s), distinto del worker que implementó.
[Texto íntegro](claude-sonnet-5-review.md). Veredicto del revisor: **Block**.

| Hallazgo | Disposición del orquestador | Corrección (W01b) |
| --- | --- | --- |
| **P2** — `provision.py --output` llega sin sanear a `shutil.rmtree` (S2083/S8707) | Aceptado | `OUTPUT_DEFAULT` + `beside_default`; test de traversal |
| **P2** — la guarda del tar no tenía tests discriminantes para hardlink/dev/fifo/duplicados | Aceptado | Tests para `LNKTYPE`, `CHRTYPE`, `BLKTYPE`, `FIFOTYPE`, miembro duplicado y backslash bajo raíz correcta; 18 → 28 tests |
| P3 — sin cota de tamaño (bomba de descompresión) | Aceptado | `MAX_MEMBER_BYTES` 64 MiB, `MAX_ARCHIVE_BYTES` 512 MiB, con tests |
| P3 — self-check de `PATH` en `build.sh` cubría 2 de 6 directorios | Aceptado (el oráculo autoritativo sigue siendo `command -v` en el contenedor real) | Bucle sobre los seis directorios del `PATH` final + `/opt/rust/bin` |
| P3 — glob sin guarda antes de `ln -s` | Aceptado | `test -e` por objeto y aserción posterior de al menos un `librustc_driver-*.so` |
| P3 — ADR-082 sugería que el script lee el hash de `sources.json` | Aceptado | Redacción: la constante *es igual* al valor registrado |
| P3 — `NEW_COMPONENTS` nombraba solo el binario | Aceptado | Renombrado `ANALYZER_BINARIES` |
| P3 — sin test de directorio extraño en `validate_context` | Aceptado | Test añadido |

Hallazgos del orquestador, fuera del alcance del diff que vio el revisor:

| Hallazgo | Corrección |
| --- | --- |
| `docs/ci.md` heredaba una aritmética desfasada (omitía `m5-helper-guest-clippy`; los recibos M5 acreditan 23/38). Con las dos etapas M6: **25 core / 40 full**, no 24/38 | Corregido en W01b; el propio worker señala que «14 etapas nativas» sigue siendo un subconteo previo (15 con `m5-runtime`) — deuda documental de M5, fuera de este paquete |
| `scripts/build-m6-runtime.py` es host/Docker-only y su `main()` no es cubrible en CI | Añadido a `sonar.coverage.exclusions` (precedente `build-m5-runtime.py`) |

## Estado tras W01b

Sin P0–P2 abiertos. Recibo regenerado
[`provisioning.json`](../../provisioning.json): `status: passed`, imagen
`rust-engineering-runtime:1.98.1-arm64-m6` =
`sha256:f39a5b33ee7d54243664162eb635f8ec223d512042beb7cd18ecf071046b310c`
(sustituye a `64b2e614…` del primer build aprobado, que ya no existe como
referencia de calificación), base observada `e0a5ca16…`, red usada solo para
manifest + dos tarballs, `docker build --network=none`,
`rust-analyzer 1.98.1 (48a229c 2026-09-01)` capturado del guest, fuera de
`PATH`, rust-src presente, binarios M3/M4/M5 intactos, contexto sin residuo.
Verificado por el orquestador tras W01b: 28 + 16 + 13 tests Python, links-check
0 rotos, inventarios 0 fallos.

La imagen **no está admitida** en el gateway: la admisión es ADR separado con
calificación nativa (W04).
