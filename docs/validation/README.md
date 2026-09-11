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

## Política de historia

- **Un gate que falla es evidencia, no basura.** Los intentos fallidos y los
  recibos superados se conservan enteros en `M<n>/history/`; nada se edita,
  solo se mueve con `git mv`.
- `history/inventory.json` registra para cada entrada conservada (`retained`)
  la ruta original, el SHA-256, los bytes y el motivo de superación, y para
  cada retirada (`retired`) la lista de archivos, un hash agregado y el último
  commit que contiene los bytes. `python3 -B scripts/docs-hygiene.py
  verify-inventories` comprueba todos los inventarios.
- **El estado privado del store de cada intento de clientes no es evidencia.**
  `M<n>/clients/attempt-N/` conserva `receipt.json`, `protocol.jsonl`, los
  transcripts del turno dirigido por modelo y la traza del harness; los
  directorios `state-*/` (blobs, `store.lock`, watermark, perfiles seccomp
  copiados) fueron retirados el 2026-09-11 con hash agregado en el inventario
  y quedan excluidos por `.gitignore`. Los harnesses crean un `state-*` nuevo
  por intento y nunca leen uno anterior.
- Los inventarios y recibos anteriores a la reordenación citan rutas
  anteriores. Son inmutables: se resuelven con [`path-map.json`](path-map.json)
  (primero `file_moves`, después el prefijo más largo de `dir_moves`).
- Los logs `*.log` de gates versionados antes de la regla `*.log` de
  `.gitignore` siguen versionados y se mueven con su paquete; no se añaden
  logs nuevos sin un inventario que los cite.

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
python3 -B scripts/docs-hygiene.py apply-moves plan.json  # git mv + reescritura de enlaces
python3 -B scripts/docs-hygiene.py verify-inventories
```

`apply-moves` verifica por SHA-256 que cada archivo movido que no sea un
documento vivo llega byte a byte; solo los documentos vivos cambian, y solo en
sus enlaces. Un recibo nunca se edita.
