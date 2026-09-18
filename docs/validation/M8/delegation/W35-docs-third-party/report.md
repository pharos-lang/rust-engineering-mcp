# W35 — correcciones documentales de la reproducción por tercero (F-1, F-3, F-4)

Worker: Claude Sonnet 5, `--effort medium`. Sin subagentes, sin segundo plano,
sin commit. Archivos tocados: `README.md`, `CHANGELOG.md`.
`docs/client-configuration.md` no requirió cambios (F-4 se resolvió en
README, junto al resto del recorrido de compilación desde fuente que ya
cubría F-1).

## F-1 (P1) — README §Instalar la release macOS ARM64

Añadido, inmediatamente antes del párrafo de descarga: un aviso que indica
que si no existe una release publicada para la versión del checkout (p. ej.
`0.8.0` hasta RC1/M8-09) hay que usar «Compilar desde el código fuente»
(enlace de ancla `#compilar-desde-el-código-fuente`), y que las releases
publicadas hoy son `v0.1.0` y `v0.3.0`.

Confirmado que la sección «Compilar desde el código fuente» ya documenta,
en este orden, exactamente lo que el tercero reprodujo con éxito: `git
clone`, `cargo build --release --locked -p rust-engineering-mcp`, la
ubicación del binario (`target/release/rust-engineering-mcp`) y la
comprobación con `version --json` / `doctor --json`. No se modificó esa
sección porque ya cumplía D-5 de la disposición.

## F-3 (P3) — CHANGELOG 0.8.0, texto histórico desactualizado

Sustituida la frase «Calificación nativa pendiente del orquestador.» en la
entrada M6-04/M6-05 por «Calificación nativa cerrada en el gate `full` de M6
(`docs/validation/M6/M6-full-gate.json`,
`sha256:69a0be14c1e2ae0cce07014daeba1818c49fa115aa3b67313bb0baffe07f34d0`).»
(hash completo verificado contra `docs/validation/M6/matrix.md` y
`docs/validation/M8/delegation/README.md`, que citan el mismo recibo).

Revisada el resto de la sección 0.8.0 por frases «pendiente»/«en desarrollo»
ya resueltas por `docs/security-model.md` §M6 (que registra los doce cortes
nativos M6 verdes en el mismo gate `full`, incluidos M6-01/M6-02/M6-03). Se
encontraron y corrigieron dos entradas más con el mismo texto histórico:

- **M6-02/M6-03**: «M6 sigue en desarrollo local, sin integración remota, PR
  ni release; calificación nativa pendiente del orquestador.» → «Calificación
  nativa cerrada en el gate `full` de M6 (...).»
- **M6-01**: «M6 sigue en desarrollo local, sin integración remota, PR ni
  release.» → «Calificación nativa cerrada en el gate `full` de M6 (...).»

No se tocó la cabecera `0.8.0 — freeze de contratos (sin publicar; RC en
M8-09)`: sigue siendo cierta (RC1 no se ha cortado todavía) y no es el
hallazgo F-3.

## F-4 (P3) — `--help` "development server"

Añadida una frase en README, tras la explicación de `doctor`/`--help` tras
la sección de compilación desde fuente: la cabecera de `--help` se
autodescribe como «Rust Engineering MCP — development server» porque el
binario corre desde un checkout de desarrollo; la release publicada muestra
el mismo texto literal y no cambia el contrato del CLI ni de las tools (el
literal en sí no se toca: está probado por `crates/mcp-server/tests/cli.rs`).

## Verificación

```
python3 -B scripts/docs-hygiene.py links-check
```

Resultado: `2905 links resolved; 0 broken in living documents; 5 point at
evidence excluded by .gitignore; 459 broken in frozen records` — 0 rotos en
documentos vivos, incluido el nuevo enlace de ancla del README y las
referencias a `06-reproduction.md`.

Sin commit, según instrucción.
