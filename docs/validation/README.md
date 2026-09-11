# Evidencia de calificación — layout y política de historia

Cada milestone cerrado tiene **un solo paquete de evidencia** con la misma
forma. Los recibos acreditan bytes por SHA-256; las matrices, handoffs, ADRs,
README y el [tablero](../implementation-status.md) los enlazan por ruta.

## Layout por milestone

```text
docs/validation/M<n>/
  matrix.md                 matriz de cortes y evidencia (M1: 17-matrix.md y m0-m1-closure-matrix.md)
  handoff.md                handoff de cierre (M4, M5)
  core-gate.json            gate `core` vigente
  full-gate.json            gate `full` vigente
  runtime.json              suite Docker nativa vigente
  clients.json              matriz de clientes vigente
  native-gate.json, <corte>-*.json, <corte>.md   recibos y registros por corte
  <paquete>/                clients/, delegation/, provisioning/, runtime/, …
  history/                  intentos fallidos y recibos superados que se conservan
    inventory.json          ruta original, SHA-256, bytes y motivo de cada entrada
```

Dentro de `M<n>/` los nombres pierden el prefijo `M<n>-` que llevaban en la
raíz (`M5-core-gate.json` → `M5/core-gate.json`); los archivos dentro de un
paquete conservan su nombre. `M0` y `M1` no tienen `history/`: ningún recibo
suyo fue superado.

## Política de historia y de lo que no se versiona

Visión: en la versión 1.0.0 el árbol contiene lo que es útil, válido y está
alineado con la versión; lo que ya no afecta al desarrollo ni a la ejecución
queda en el historial de Git, localizable por hash.

- **El recibo aceptado acredita el milestone.** Los recibos superados, los
  intentos fallidos y sus logs no se versionan: `M<n>/history/inventory.json`
  registra para cada uno la ruta, el SHA-256, los bytes, el motivo y el último
  commit que contiene los bytes (`retired`), con un hash agregado por grupo
  (`retired_groups`). Las narrativas de diagnóstico (`README.md`,
  `disposition.md`) se conservan porque son lecciones, no bytes medidos.
- **Un intento de clientes es su `receipt.json` y su `protocol.jsonl`.** Las
  salidas crudas del Inspector y del turno dirigido por modelo (`*.stdout`,
  `*.stderr`, `*-events.jsonl`, `harness-stderr.txt`) y el estado privado del
  store (`state-*/`) no se versionan; `.gitignore` los excluye y los harnesses
  no los leen.
- **Salidas crudas detrás de un recibo** (`*.log`, junit, transcripciones de
  gate) no se versionan: el recibo ya registra su hash y su resultado. Se
  conservan las capturas de ayuda de CLI de `M3/provisioning/help/` porque el
  adapter documenta con ellas el contrato que parsea.
- **Paquetes de revisión**: quedan la disposición, los findings y la salida del
  revisor; las copias de entradas (`inputs/`) no se versionan porque Git ya
  tiene esos bytes en el commit revisado y `inputs.json` guarda sus hashes.
  `docs/reviews/inventory.json` registra lo retirado.
- **Transcripts de delegación** de milestones cerrados no se versionan; quedan
  los manifiestos `.sha256` y el inventario.
- Los recibos son inmutables y citan la ruta con la que se generaron; se
  resuelven con [`path-map.json`](path-map.json) y con los inventarios.
  `python3 -B scripts/docs-hygiene.py verify-inventories` comprueba que lo
  conservado coincide por hash y que lo retirado no volvió al árbol.

## Archivos anclados fuera del layout

Dos recibos se quedan en su ruta anterior porque el código Rust los incrusta
con `include_str!` y esta reordenación no toca crates:

| Ruta | Anclado por |
| --- | --- |
| `docs/validation/M2-D04-native-qualification.json` | `crates/execution-adapter/src/mutation_archive.rs` |
| `docs/validation/artifacts/M1-01-runtime-volume-feasibility.json` | `crates/execution-adapter/src/rust_applied.rs` |

Los comentarios de documentación de `crates/` y el snapshot de contrato
`crates/mcp-server/tests/snapshots/binary-bloat-tool.json` citan la evidencia
por su ruta actual; los recibos, transcripts, entradas de revisión y drivers
archivados siguen citando la ruta con la que se generaron y se resuelven con
`path-map.json`.

## Registros congelados

`scripts/docs-hygiene.py links-check` exige que todo enlace de los documentos
vivos resuelva. No reescribe ni exige nada a los registros congelados:
transcripts de agentes (`M3/delegation/**`), copias de entradas y prompts de
los paquetes de revisión (`docs/reviews/M<n>/*/inputs/**`, `prompt.md`),
recibos de medición (`docs/research/m1-16/measurement/results/**`) y el
prompt de reordenación, que describe el árbol anterior. Un enlace a un archivo
excluido a propósito por `.gitignore` (por ejemplo un `*.log` inventariado) se
clasifica como evidencia excluida, no como enlace roto.

## Cómo mover evidencia

```text
python3 -B scripts/docs-hygiene.py links-check            # antes y después
python3 -B scripts/docs-hygiene.py apply-moves < plan.json  # git mv + reescritura de enlaces
python3 -B scripts/docs-hygiene.py verify-inventories
```

`apply-moves` verifica por SHA-256 que cada archivo movido que no sea un
documento vivo llega byte a byte; solo los documentos vivos cambian, y solo en
sus enlaces. Un recibo nunca se edita.
