# Disposición del orquestador — R01 (Gemini 3.8 Flash High, `agy` 1.2.0, `--sandbox`, web)

Fecha: 2026-09-11. Informe: [report.md](report.md) (14 preguntas, 24 fuentes
citadas: manifest 1.98.1, fuentes de rust-analyzer en `rust-lang/rust@48a229cea`,
LSP 3.17, libro de RA).

## Contraste independiente antes de aceptar nada

| Afirmación del informe | Contraste del orquestador | Veredicto |
| --- | --- | --- |
| Q1: hash xz de `rust-analyzer-1.98.1-aarch64-unknown-linux-gnu.tar.xz` = `fa58b688…`; `rust-src-1.98.1.tar.xz` = `e605d336…`, «citando» el manifest | `curl` del manifest publicado (sha256 `a7c8774a…`, `Last-Modified: 03 Sep 2026`, idéntico al registrado en `fixtures/rust-runtime/sources.json`): `xz_hash` RA = **`a0fd960a…`**, rust-src = **`5c846ebc…`**, exactamente los del dossier. El informe además presenta la entrada como si solo tuviera `url`/`hash`, cuando el manifest lleva `url`/`hash`/`xz_url`/`xz_hash` | **P1 — hashes fabricados presentados como cita textual.** Rechazado. Ningún hash del informe se usa; los del aprovisionamiento son los del manifest publicado, que W01 vuelve a verificar por red antes de descargar |
| Q1: `rust-analyzer --version` → `rust-analyzer 1.98.1 (48a229cea 2026-09-01)`; commit RA sincronizado `104b3c29…` | No verificado localmente (no hay binario 1.98.1 todavía) | **Pendiente**: W01 captura la salida real de `--version` en el recibo; la calibración W04 la fija como constante |
| Q2: negociación `positionEncodings` con preferencia `utf-8` (`capabilities.rs`) | Coherente con la API pública de `lsp_types`; el fragmento citado es verosímil | **Aceptado condicionalmente**: la calibración nativa comprueba `capabilities.positionEncoding == "utf-8"` en el `initialize` real |
| Q3: `experimental/serverStatus` gated por `experimental.serverStatusNotification`, payload `{health, quiescent, message}`, `is_fully_ready = is_quiescent && !prime_caches` | Coherente con la documentación pública de RA (`lsp-extensions.md`) | **Aceptado condicionalmente**: oráculo de readiness de D26; calibración exige un transcript real con `quiescent: true` |
| Q4: sin `rust-src`, `Sysroot::error` + `unresolved-import`; `cargo.sysrootQueryMetadata=false` evita `cargo metadata` del sysroot | Confirma la necesidad del Ítem B ya autorizado | Aceptado; `sysrootQueryMetadata=false` entra en la config fija |
| Q5: procesos externos con la config fija: `rustc --print sysroot`, `cargo locate-project`, `cargo --version`, `rustc -vV`, `cargo rustc -Z unstable-options --print cfg` (fallback `rustc --print cfg -O`), `--print target-spec-json`, `cargo metadata --no-deps`; `--no-deps` no escribe `Cargo.lock` | Todos son `execve` de binarios del guest ya presentes; ninguno ejecuta código del proyecto | Aceptado; la calibración observa `container top` durante initialize y confirma ausencia de `build-script-build`/proc-macro-srv |
| Q6: `rust-analyzer.toml` del workspace **puede** fijar `cargo.buildScripts.enable/overrideCommand`, `check.overrideCommand`, `runnables.command`, `rustfmt.overrideCommand`, `cargo.extraEnv`; precedencia por encima de `initializationOptions`; no hay opción para desactivar su carga; `initializationOptions` anidado, claves sin prefijo, **claves desconocidas ignoradas en silencio** | Confirma la decisión del brief: rechazo en captura de todo `rust-analyzer.toml` (y `.rust-analyzer.toml`) | Aceptado. Consecuencia nueva: la calibración debe validar cada clave de la config fija contra `--print-config-schema` del binario real (un typo dejaría el default en silencio) |
| Q7–Q10: documentSymbol jerárquico, `workspace/symbol` (`scope`/`kind`/`limit`, default 128 y `only_types`), `includeDeclaration` honrado, referencias externas posibles, pull diagnostics `textDocument/diagnostic` con solo diagnósticos nativos, `workspace/diagnostic` no soportado, `command` solo si el cliente anuncia `experimental.commands`, snippets recortados si no se anuncia `snippetTextEdit`, resource ops solo si se anuncian | Coherentes con la documentación pública | Aceptados condicionalmente; cada uno tiene su prueba nativa en M6-01..04 |
| Q11: `shutdown`→`exit` código 0; `exit` sin `shutdown` código 1; EOF de stdin termina; `-32800`/`-32801`; cero peticiones servidor→cliente con `window.workDoneProgress=false`, `workspace.configuration=false`, `didChangeWatchedFiles.dynamicRegistration=false` | Coherente con LSP 3.17 | Aceptado; el conjunto mínimo de capabilities entra en D26 |
| Q12 (no preguntado; overlays VFS) | Informativo | Sin efecto |
| Q13: RSS no documentado | Honesto | Se mide en calibración (cgroup `memory.peak`) |
| Q14: `--print-config-schema`, `RA_LOG`, `--log-file` | Coherente con `--help` del 1.97.1 local | Aceptado |

## Decisión

El informe se usa como **mapa de verificación**, no como evidencia: por la
fabricación del Q1 ningún dato numérico ni fragmento «citado» se acepta sin
un oráculo local (binario real, transcript real o schema real). Los puntos
`[R01]` del [brief D25/D26](../D25-D26-decision-brief.md) se cierran en su §4
con esta disposición. La lección queda registrada para el resto de M6: un
informe de modelo con web es evidencia auxiliar (G8), incluso cuando cita
fuentes.
