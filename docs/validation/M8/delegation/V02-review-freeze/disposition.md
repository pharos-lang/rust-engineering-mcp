# V02 — disposición del orquestador (2026-09-14)

Veredicto del revisor: **Block** (0 P0/P1, 4 P2 de contrato/gate, P3 varios).
Todos los P2 reproducidos por el orquestador en el código; se aceptan.

| ID | Sev | Disposición | Dónde se cierra |
| --- | --- | --- | --- |
| P2-1 `executes_project_code` falso en 8 tools | P2 contrato | **Aceptado con semántica fijada**: `executes_project_code` = «la tool puede ejecutar build scripts, proc macros, tests o binarios del proyecto en el guest» → `true` para check, clippy, test, test.nextest, quality.gate, quality.gate.v2, coverage, semver.check (cargo semver-checks compila), mutation.test, miri, benchmark.run, profile.flamegraph, binary.bloat, fix.apply; **`false`** para fmt.check, fmt.apply (solo rustfmt), benchmark.compare (sin proceso), las cinco `rust.analyzer.*` (build scripts/proc macros/check-on-save deshabilitados: solo textDocument/*) y el resto ya en `false`. Test que fije los valores; campo añadido al censo con motivo | W09 (Rust) + W10 (censo) |
| P2-2 falso pass por reclasificación | P2 gate | **Aceptado**: `verify` clasifica con la clase **registrada** en el manifiesto; cualquier cambio de clase (stable↔preview) o de conteo = fallo salvo regeneración deliberada; test | W10 |
| P2-3 etapa `contract-freeze` omitible | P2 gate | **Aceptado**: etapa siempre presente; manifiesto ausente = fallo | W10 |
| P2-4 clase del subcomando `contract` y de su formato; docs incompletas; cadena de verificación mal descrita | P2 contrato | **Aceptado**: `contract` y su documento (`format_version: 1`) son **`stable`** desde 0.8.0 (oráculo de RC); docs listan todos los campos; CHANGELOG describe la cadena real (protocol tests: servidor vivo == snapshots; `cli.rs`: CLI == snapshots; `contract-freeze`: snapshots == manifiesto) | W10 |
| P3 procedencia `head_commit` con árbol sucio | P3 | Aceptado: `generate`/`diff` registran `tree_dirty`; el orquestador regenera ambos JSON tras el commit del código y los commitea aparte | W10 + orquestador |
| P3 `git show` con `text=True`; `--base` sin `rev-parse --verify`/`--end-of-options` | P3 | Aceptado | W10 |
| P3 «byte-idénticos» solo probado para 13 M1 | P3 | Aceptado: `diff` compara también `snapshot_sha256`; CHANGELOG dice lo que se probó | W10 |
| P3 huecos de tests del script; vector no-ASCII | P3 | Aceptado | W10 |
| P3 literales de protocolo/SDK/plantillas solo por longitud; `cli.rs` sin comparar annotations; fallo silencioso sin stderr | P3 | Aceptado | W09 |
| P3 listas duplicadas (orden de tools ×2, nombres preview ×3) | P3 | Aceptado como deuda trazada: la cadena está cubierta de extremo a extremo (protocol.rs ↔ snapshots ↔ cli.rs ↔ manifiesto); unificar cuando se toque el registro de tools | matriz §Deuda |
| P3 promesa «31 stable» sin la condición M8-04; redacción `--rust-image` | P3 | Aceptado | W10 |
| Diseño: un manifiesto regenerado en el mismo diff pasa siempre | — | Conocido: en M8-09 RC2 se verifica contra el manifiesto de RC1 (`--strict`), no contra uno regenerado | M8-09 |

Re-revisión V02b (Opus, diff-only) tras W09/W10; luego gate `core` sobre bytes finales.
