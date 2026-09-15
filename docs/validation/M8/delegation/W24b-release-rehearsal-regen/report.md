# W24b — repetición del ensayo de distribución 0.8.0 sobre bytes commiteados

## Task

Repetir exactamente el ensayo de W24 (`docs/validation/M8/delegation/W24-release-rehearsal/report.md`
§pasos) sobre el árbol commiteado actual (HEAD `86a5007`), regenerando
`docs/validation/M8/07-release-rehearsal.json` y solo la sección «Resultado
del ensayo» de `docs/validation/M8/07.md`. Sin commit, sin publicación, sin
tocar scripts ni workflows.

## Discrepancia con el encargo: árbol no limpio

El encargo suponía `git status --short` vacío. No lo estaba al arrancar el
ensayo:

- `docs/validation/M8/05-budgets.json`, `05-measurement.json`, `05.md`
  (modificados) — recalibración M8-05 de presupuestos de
  latencia/RSS/tamaño de binario, fechada 2026-09-15, sin relación con este
  ensayo.
- `docs/validation/M8/clients/attempt-14/` (sin trackear) — evidencia de
  matriz de clientes, sin relación.
- Durante el ensayo apareció además `docs/validation/M8/03-rollback.json`
  modificado (regeneración M8-03 de otro worker concurrente) — tampoco
  tocado por mí, fuera de mi alcance de archivos permitidos.

Verifiqué que ninguno de estos archivos afecta al ensayo: confirmé que
`archive_size_core_bytes` (el único valor de `05-budgets.json` que este
ensayo consume) sigue en 12 700 000 B tras la recalibración, y que todos
los inputs de empaquetado/build (`Cargo.toml`, `Cargo.lock`, `src/`,
`scripts/release-artifact.py`, `scripts/release-smoke.py`,
`scripts/contract-freeze.py`, `docs/validation/M8/freeze-0.8.0.json`)
coincidían exactamente con `86a5007`. Documenté la discrepancia con detalle
en `tree_dirty_detail` del receipt y en `07.md`, y seguí adelante dentro de
mi alcance de archivos (`07-release-rehearsal.json`, `07.md` §Resultado del
ensayo) en vez de detenerme, porque la desviación no afecta a nada que este
ensayo mida o escriba.

## Result

`status: passed`. El árbol commiteado ya incorpora el fix de W24
(`TOOL_SCHEMA_SHA256` de `rust.binary.bloat` y `rust.analyzer.action.apply`
en `scripts/release-smoke.py`), así que `release-smoke.py` pasó **al primer
intento** desde el directorio limpio — no hubo fallo cerrado que corregir
esta vez. Los 36 tools, la SBOM (563 relaciones), el inventario
(221 paquetes) y el smoke están correctos. El archive core midió
10 423 020 B, **dentro** del presupuesto `archive_size_core_bytes`
(12 700 000 B, 82,07 %) — ligeramente mayor que en W24 (10 392 940 B,
81,83 %) por deriva normal de compilación entre commits, sin relevancia.

Confirmé además `resources/templates/list` en **forma RFC 6570**
(`{?offset,length}`) para `rust-quality-artifact://...`, resolviendo V03b
P2-3: la evidencia anterior citada por el encargo reproducía la forma
antigua `?offset={n}&length={n}`, pero el servidor real ya devuelve la
forma RFC 6570 correcta.

## Files changed

- `docs/validation/M8/07-release-rehearsal.json` (regenerado) — receipt
  completo: `format_version 1`, `generated_utc`, `head_commit` `86a5007...`,
  `tree_dirty: true` con `tree_dirty_detail` explicando la discrepancia
  anterior, 11 pasos con comando/exit/extracto (uno menos que W24: sin el
  paso de hallazgo/fix, ya no aplica), presupuesto de tamaño, hashes de
  cada asset, `found_and_fixed: []`, y los mismos tres items `unavailable`
  con motivo y remisión a RC1.
- `docs/validation/M8/07.md` — reemplazada solo la sección «Resultado del
  ensayo», ahora titulada «Resultado del ensayo (W24b — repetición sobre
  bytes commiteados)»; el resto del archivo (la decisión D14, §1-5) no se
  tocó.

No se tocó ningún otro archivo. Sin commit.

## Pasos del ensayo (ver detalle y comandos exactos en `07-release-rehearsal.json`)

| # | Paso | Resultado |
| --- | --- | --- |
| 1 | Confirmar `Cargo.toml` = 0.8.0 | passed |
| 2 | `cargo build --release --locked --offline -p rust-engineering-mcp` (ya al día) | passed |
| 3 | `contract-freeze.py verify freeze-0.8.0.json --strict` | passed |
| 4 | `release-artifact.py` → archive + `SHA256SUMS` + inventory + SBOM + notices | passed |
| 5 | Tamaño de archive vs presupuesto | **within** (10 423 020 / 12 700 000 B, 82,07 %) |
| 6 | Copiar archive+`SHA256SUMS` a directorio limpio bajo `target/` (descarga simulada) | passed |
| 7 | `release-smoke.py` desde el directorio limpio | **passed al primer intento** (36 tools, doctor pasivo, version 0.8.0, 4 tools/call representativas, proceso limpio) |
| 8 | `test-release-smoke.py` | passed, 9/9 |
| 9 | `contract --json` manual contra el binario extraído vs `freeze-0.8.0.json` | passed — 36 tools, hashes/anotaciones por tool idénticos, 0 discrepancias |
| 10 | Sesión `serve --stdio` manual: `tools/list` (36) y `resources/templates/list` (2, forma RFC 6570 confirmada) | passed |
| 11 | Cleanup del directorio de ensayo bajo `target/` | passed |

## Tamaño del archive vs presupuesto

10 423 020 B frente al presupuesto `archive_size_core_bytes` de
`docs/validation/M8/05-budgets.json` (12 700 000 B, sin cambios pese a la
recalibración M8-05 de otros magnitudes) → **within**, 82,07 % del
presupuesto.

## Lo que queda para RC1 (M8-09)

Sin cambios respecto a W24: attestations OIDC reales, redescarga
independiente desde un GitHub Release publicado, y la reconciliación
tag↔versión que hace el propio workflow — ninguna posible sin tag real, red
y un runner de GitHub Actions.

## Tests

```
python3 -B scripts/test-release-artifact.py   → OK, 11 tests
python3 -B scripts/test-release-smoke.py      → OK, 9 tests
```
