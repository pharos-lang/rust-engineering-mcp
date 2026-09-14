# W04b — informe del worker

| Campo | Valor |
| --- | --- |
| Modelo | Claude Opus 5 (`claude -p --model opus --effort high --disallowedTools Agent Task`) |
| Objeto | Aplicar la [disposición V04](../V04-review-lsp-session-gateway/disposition.md) y las decisiones F1/F2 sobre el paquete W04, y recalibrar |
| Resultado | Las quince partidas aplicadas; **nueve de nueve cortes en verde** con recibo nuevo |
| Sin commit | El árbol queda sin `git commit`, como pedía el encargo |

## Qué cambió

Los quince puntos del encargo, en su numeración:

1. **F1+F2 — configuración fija.** `initialization_options()` pierde
   `cargo.sysrootQueryMetadata` y `cargo.autoreload`: **17 claves**. Enmienda
   fechada en [ADR-084 §3](../../../../adr/ADR-084-rust-analyzer-runtime-and-lsp-lifecycle.md)
   y en el [brief §4.5](../D25-D26-decision-brief.md). `config_digest` nuevo:
   `sha256:a2592cfc4e65af0b5ff6a36ff737bbc32ef7d5073af65b2a7ee36907d49981b4`.
2. **F2 — vocabulario.** `IncompleteReason::SysrootWarning` →
   `AnalyzerWarning` (cualquier `health: warning`, sin publicar el `message`).
   El corte de document-symbols exige ahora `health: ok` y `complete`, y **lo
   observa**: F2 queda resuelto, no mapeado.
3. **Oráculo m6-03 por argv completo.** Lista cerrada por línea de comandos
   entera; `rust-analyzer proc-macro`, `build-script-build`, `rustfmt` y `sh`
   fallan el corte. `MIN_PROGRAM_SAMPLES` intacto.
4. **Documentos de corte obsoletos.** El driver borra `cut-*.json` y
   `receipt.json` antes de la suite; cada `Cut` publica `fail` desde su `Drop`
   si no alcanzó su veredicto; cada documento lleva `run_started_at` y el driver
   exige que las claves de `cut_status` sean exactamente las selecciones que
   ejecutó y que ningún documento venga de otra ejecución.
5. **`IncompleteReason::NotUtf8File`**, empujado junto a la omisión, con test
   de gateway sobre una captura con un `.rs` inválido.
6. **Alcance de la admisión** escrito en
   [ADR-085](../../../../adr/ADR-085-m6-runtime-admission.md), sin cambio de
   código.
7. **Duraciones separadas**: `session.duration_ms` (la sesión) y
   `call.duration_ms` (la llamada entera), ambas en el recibo.
8. **`send` acotado por fase**: recibe el `until` de la fase; excederlo es
   `TimeoutInitialize`/`TimeoutQuery`, nunca `NotReady`.
9. **Evidencia de kill/reap**: `kill_error`/`reap_error` (solo el
   `io::ErrorKind`) y `SessionStop::KillUncertain` cuando el reap no confirma.
10. **Clasificación EOF determinista**: una escritura que el peer ya no acepta
    resuelve primero la lectura; si stdout llegó a EOF el fallo es `Eof`. El
    unit test que fallaba pasa sin relajar su aserción. m6-07 comprueba
    `evidence.killed`, `stop ∈ {eof, killed}` y `State.ExitCode == 137` del
    contenedor leído por `container inspect` **antes** del cleanup.
11. **`FrameTooLarge` / `MalformedHeader`** en lugar de `FramingRejected`, con
    el `Content-Length` declarado publicado y registrado por m6-08.
12. **`analyzer_configuration`** recorre también `source.directories()`.
13. **Notas de corte** m6-03 y m6-04 corregidas; el oráculo determinista de
    build scripts queda asignado a W07 (M6-03) por escrito.
14. **Python**: `OUTPUT` constante, `STEP_TIMEOUT_S` con refusal propia que
    nombra la variable, `RUST_MCP_TEST_DOCKER` respetado, y
    `scripts/test-m6-runtime-unit.py` nuevo (19 tests) en la lista de cobertura
    de SonarCloud.
15. **Recalibración completa**: las nueve selecciones en verde; recibo y schema
    copiados a `01-calibration.json` / `01-config-schema.json`;
    [01.md](../../01.md) reescrito sobre esta ejecución.

## Evidencia

Ejecución 2026-09-12T07:17:51Z → 07:18:05Z sobre
`sha256:f39a5b33…`; etapa de gate `passed` (07:16:46Z → 07:18:05Z).

| Dato | Valor |
| --- | --- |
| Recibo nativo | [`01-calibration.json`](../../01-calibration.json), `sha256:7b87268d6b04b19501b28a42f51602ac86ec6a3be638dfd8985f4d18aa467f01` |
| Schema del binario | [`01-config-schema.json`](../../01-config-schema.json), `sha256:b9029f5eeb51f7f647766641f1bdf6bb08be85bf531bba1d6f37bad6708d2bb6`, 84 726 bytes |
| `config_digest` | `sha256:a2592cfc4e65af0b5ff6a36ff737bbc32ef7d5073af65b2a7ee36907d49981b4` (antes `f2978ab4…`) |
| Claves fijas | 17 comprobadas, 0 ausentes |
| `health` al alcanzar quiescent | `ok` en las nueve sesiones; respuestas answered `complete` |
| argv observados | `/opt/analyzer/bin/rust-analyzer` (14 muestras). En otras ejecuciones del mismo corte: `cargo metadata --format-version 1 --no-deps …` y `cargo rustc -Z unstable-options --print …`, todos dentro de §7 |
| Tiempos | quiescent 353–385 ms; sesión 385–454 ms; llamada 1 039–1 139 ms; corte 0,3–2,2 s |
| Frame limit | `FrameTooLarge` con `Content-Length: 3 015 595` registrado |
| Crash | `stop: eof`, contenedor `State.ExitCode 137`, `oom_killed: false` |

El detalle está en [01.md](../../01.md), que es el documento del corte M6-01:
estado, digests, `health`, argv, tiempos, F1/F2 resueltos y riesgos residuales
R1–R5 actualizados.
