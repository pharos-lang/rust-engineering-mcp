# W24 — M8-07: ensayo de distribución 0.8.0, workflow RC y ADR-090 (D14)

## Task

Cerrar D14 con ADR-090; arreglar el literal `!= 31` de
`release-candidate.yml:219` para que use el conteo de tools del manifiesto de
freeze; ejecutar un ensayo local (sin red, sin tag, sin publicación) del
archive core, inventario/SBOM/notices y `release-smoke.py` desde un
directorio limpio simulando una descarga; documentar verificación offline y
respuesta a compromiso en `docs/publication.md`; registrar el receipt del
ensayo.

## Result

`status: passed`. Los 36 tools, la SBOM, el inventario y el smoke están
correctos, y el ensayo mismo encontró y corrigió un defecto real que llevaba
sin detectar hasta ahora: dos hashes mal fijados en
`TOOL_SCHEMA_SHA256` de `scripts/release-smoke.py`
(`rust.binary.bloat`, `rust.analyzer.action.apply`) que no coincidían con el
contrato congelado, aunque el contrato en sí (descripción, schemas de
entrada/salida, anotaciones) nunca estuvo mal — coincidía exactamente entre
`contract --json` del binario y `docs/validation/M8/freeze-0.8.0.json`. El
verificador falló cerrado como debía; se corrigió y se repitió con éxito.

El literal `31` del workflow quedó sustituido por el `tool_count` (36) leído
del manifiesto de freeze ya checked-out en el job `build`, propagado al job
`draft` vía output entre jobs — sin tocar la política OIDC/attestations ni
checkout adicional en `draft`. ADR-090 registra los 5 puntos de D14
(OIDC conservado, verificación offline con y sin `gh`, respuesta a
compromiso, drills, no-publicación en M8-07) con Alternatives y
Consequences. `docs/publication.md` gana «Verificación offline» y
«Respuesta a compromiso»; el backlog D14 pasa a Accepted; el índice ADR
lista ADR-090.

## Files changed

- `.github/workflows/release-candidate.yml` — el conteo esperado de tools ya
  no es un literal `31`: se lee `docs/validation/M8/freeze-0.8.0.json`
  (`tool_count`) en el job `build` (fuente elegida porque ese job ya tiene el
  source checked-out; el binario recién construido también podría haberse
  usado vía `contract --json`, pero el manifiesto de freeze evita invocar el
  binario una vez más solo para contar tools) y se expone como output
  `tools-count`, consumido por el job `draft` en vez del literal. Revisado el
  resto del archivo: no quedaba ningún otro literal `0.3.0`/`31`/`v0.3`; la
  comprobación de versión ya usaba `cargo metadata` (no un literal) y no se
  tocó.
- `scripts/release-smoke.py` — corregidos los dos valores de
  `TOOL_SCHEMA_SHA256` mal fijados (`rust.binary.bloat`,
  `rust.analyzer.action.apply`), encontrados por el propio ensayo (ver abajo).
  `release-artifact.py` y `release-inventory.py` no se tocaron: el primero ya
  genera inventario/SBOM/notices con 36 tools correctamente (no dependía de
  ningún literal de conteo), y el segundo es el inventario de candidatos del
  perfil `local` (ORT/E5), sin relación con el conteo de tools del core.
- `docs/adr/ADR-090-offline-verification-and-incident-response.md` (nuevo) —
  Accepted, los 5 puntos de D14, Alternatives (clave organizacional
  adicional, dos operadores, no publicar bundles) y Consequences.
- `docs/adr/README.md` — viñeta ADR-090.
- `docs/roadmap/adr-backlog-m2-m8.md` §D14 — Proposed → Accepted, con
  referencia a ADR-090 y a la evidencia del ensayo.
- `docs/publication.md` — nuevas secciones «Offline verification» y
  «Incident response» (en inglés, consistente con el resto del archivo).
- `docs/validation/M8/07-release-rehearsal.json` (nuevo) — receipt completo
  del ensayo: `format_version 1`, `generated_utc`, `head_commit`
  `6fa1ef1f69b461b5cf2b3782c13ef81353a63e8e`, `tree_dirty: true`, 13 pasos con
  comando/exit/extracto, presupuesto de tamaño, hashes de cada asset, el
  hallazgo/corrección de `release-smoke.py`, y tres items `unavailable` con
  motivo y remisión a RC1.
- `docs/validation/M8/07.md` — añadida solo la sección «Resultado del
  ensayo (W24)» (el resto del archivo, la decisión D14 en sí, no se tocó).

No se tocó ningún otro archivo. Sin commit.

## Pasos del ensayo (ver detalle y comandos exactos en `07-release-rehearsal.json`)

| # | Paso | Resultado |
| --- | --- | --- |
| 1 | Confirmar `Cargo.toml` = 0.8.0 | passed |
| 2 | `cargo build --release --locked --offline -p rust-engineering-mcp` (ya al día) | passed |
| 3 | `contract-freeze.py verify freeze-0.8.0.json --strict` | passed |
| 4 | `release-artifact.py` → archive + `SHA256SUMS` + inventory + SBOM + notices | passed |
| 5 | Tamaño de archive vs presupuesto | **within** (10 392 940 / 12 700 000 B, 81,8 %) |
| 6 | Copiar archive+`SHA256SUMS` a directorio limpio bajo `target/` (descarga simulada) | passed |
| 7 | `release-smoke.py` desde el directorio limpio (primer intento) | **failed closed** — halló el defecto de dos hashes |
| 8 | Corrección de `TOOL_SCHEMA_SHA256` en `release-smoke.py` | aplicada |
| 9 | `release-smoke.py` desde el directorio limpio (segundo intento) | **passed** (36 tools, doctor pasivo, version 0.8.0, 4 tools/call representativas, proceso limpio) |
| 10 | `test-release-smoke.py` (regresión tras el fix) | passed, 9/9 |
| 11 | `contract --json` manual contra el binario extraído vs `freeze-0.8.0.json` | passed — 36 tools, hashes/anotaciones por tool idénticos |
| 12 | Sesión `serve --stdio` manual: `tools/list` (36) y `resources/templates/list` (2) | passed |
| 13 | Cleanup del directorio de ensayo bajo `target/` | passed |

## Tamaño del archive vs presupuesto

10 392 940 B frente al presupuesto `archive_size_core_bytes` de
`docs/validation/M8/05-budgets.json` (12 700 000 B) → **within**, 81,8 % del
presupuesto.

## Lo que queda para RC1 (M8-09)

- Attestations OIDC reales (`actions/attest-build-provenance` +
  `gh attestation verify`): exigen un runner de GitHub Actions y un tag real;
  imposible sin red ni tag en este ensayo local.
- Redescarga independiente desde un GitHub Release publicado (patrón
  `docs/validation/M1/17-public-release.json` de 0.1.0): exige un release
  publicado; no se publicó nada en M8-07 por diseño.
- La reconciliación tag↔versión de workspace que hace el propio workflow
  (`github.ref_name` contra `cargo metadata`) no se ejecutó; el ensayo
  confirmó `Cargo.toml` directamente y pasó `--tag v0.8.0` como argumento
  local.

Ninguno de los tres bloquea D14: la política de verificación offline y
respuesta a compromiso de ADR-090 no depende de haberlos ejecutado, solo de
que el mecanismo (OIDC, `gh attestation verify`, SHA-256+inventario) exista y
esté documentado.

## Tests

```
python3 -B scripts/test-release-artifact.py   → OK, 11 tests
python3 -B scripts/test-release-smoke.py      → OK, 9 tests
python3 -B scripts/test-gate-reporting.py     → OK, 13 tests
python3 -B scripts/docs-hygiene.py links-check → 0 broken in living documents
                                                  (459 broken in frozen records
                                                  y 5 excluidos por .gitignore,
                                                  ambos preexistentes, sin
                                                  relación con este cambio)
```

YAML del workflow validado con `python3 -c "import yaml"` (parseo completo) y
los tres bloques `python3 - <<'PY' ... PY` extraídos y compilados con
`compile()` para confirmar sintaxis Python válida.
