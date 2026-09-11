# Disposición — revisión independiente del harness de clientes M5 con Claude Code

Fecha: 2026-09-10. Objeto: el delta de `scripts/test-m5-clients.py` y
`scripts/test-m5-clients-unit.py` respecto a `a88ce53` que sustituye Codex por
Claude Code 2.1.267 (`claude-sonnet-5`) como cliente agentic de la matriz M5.
Decisión del owner del mismo día: sin Codex en este cierre; revisiones con
agentes Claude y Gemini 3.8 (`agy`) para investigación/revisión.

Entradas revisadas: [prompt-header.md](prompt-header.md) más el diff, con los
digests en [inputs.sha256](inputs.sha256). Ambas revisiones son read-only y sin
herramientas: solo texto.

## Gemini 3.8 Flash (High), vía `agy` 1.1.28

Veredicto: **Block**. [Texto íntegro](gemini-3.8-flash-high-review.md).

| Hallazgo | Disposición |
| --- | --- |
| **P2** — `validate_claude_session` aceptaba un fallback de modelo: solo exigía que el modelo fijado apareciera en `modelUsage`, donde también figura legítimamente el modelo auxiliar del cliente | **Aceptado y corregido.** El criterio ahora es el campo `model` de cada mensaje `assistant` del transcript: todos deben ser `claude-sonnet-5` y debe haber al menos uno. Comprobado sobre tres transcripts reales (9, 11 y 11 mensajes, todos del modelo fijado). Test: fallback a mitad de turno con el modelo fijado presente en `modelUsage` → rechazado |
| **P2** — `list_mcp_resources` permitido sin contar, sin argumentos ni estado en el flujo docker-free | **Aceptado y corregido.** El flujo docker-free ya no admite discovery: cualquier llamada fuera de `rust.project.open` y las cuatro M5 es «another MCP capability». Test añadido |
| P3 — orden entre roots y rechazos | **Aceptado en parte.** Se exige que todos los `open` precedan a todo rechazo. No se exige orden relativo entre los cuatro rechazos: son resultados declarados independientes, cada uno ligado a su fila del plan y a su root, y el cliente real los emitió en lote (run/profile/bloat en paralelo y después compare); rechazar eso no protegería ningún hecho del producto. Tests para ambos casos |
| P3 — rutas de host fijas (`CLAUDE`, `PATH`) | **Aceptado como convención, no cambiado.** `NODE`, `DOCKER` e `INSPECTOR` ya están fijados del mismo modo y M2 fijó el mismo ejecutable de Claude; el gate M5 está calibrado en este host y el preflight lo declara |
| P3 — `JSONDecodeError` sin estructurar al parsear stdout | **Aceptado y corregido.** Una línea no JSON produce `RuntimeError("Claude <mode> transcript line N is not JSON")` |
| P3 — huecos de tests (fallback coexistente, discovery repetida, orden docker-free) | **Aceptado y corregido** con los tests anteriores |

Verificaciones del revisor que se conservan como evidencia: credenciales leídas
en sitio sin copia; entorno mínimo y `TMPDIR`/`cwd` privados; `--restricted`,
`--setting-sources ""`, `--strict-mcp-config`, `--tools` solo Resources,
`--allowedTools` solo el servidor configurado; `init.tools` sin built-ins
ajenos; `permission_denials` vacío; flujo runtime con siete llamadas exactas,
orden estricto, IDs de dataset capturados de las dos mediciones vivas,
comparación negativa contra un artifact real de otro tipo y lectura de esa
misma URI; sin filas de conversión fabricadas para Claude; `killpg` en timeout y
limpieza del directorio privado.

Hallazgo propio, anterior a las revisiones y fuera de su diff: Claude Code
devuelve un Resource binario con `blob` vacío, `text` con una nota del cliente y
`blobSavedTo`. El oráculo exigía solo «text o blob no vacío» y habría aceptado
la nota como contenido. Corregido: la evidencia es el blob decodificado, el
archivo guardado existente o texto de tipo no binario, y su longitud debe ser
la del chunk de la URI; se publica `resource_content` (kind, bytes, sha256).

## Claude Sonnet 5 (`claude-sonnet-5`, Claude Code 2.1.267, `claude -p`, sin tools)

Veredicto: **Block**. [Texto íntegro](claude-sonnet-5-review.md). 456,8 s;
`modelUsage` con `claude-sonnet-5` y el auxiliar `claude-haiku-4-5`.

| Hallazgo | Disposición |
| --- | --- |
| **P1** — no se comprueba qué modelo emitió cada `tool_use`; dos modelos coexistentes en `modelUsage` se aceptaban | **Aceptado y corregido** (coincide con el P2 de Gemini): todo mensaje `assistant` —donde viven los `tool_use`— debe declarar `claude-sonnet-5`; test de fallback a mitad de turno |
| **P1** — la lectura de Resource solo comparaba la URI y exigía texto/blob no vacío; `resource_uri_sha256` es el hash de la URI, no del contenido | **Aceptado y corregido.** El contenido leído (blob decodificado, archivo guardado por el cliente o texto no binario) debe medir la longitud del chunk y, cuando el chunk cubre el artifact entero, su SHA-256 debe ser el `sha256` del descriptor publicado por `rust.benchmark.run`. Comprobado en el transcript real: 40960 bytes y `799487…bccf4` en ambos lados. Se publica `resource_content` con `whole_artifact` |
| **P1** — `stdout`/`stderr` crudos de la CLI se guardan como evidencia sin escaneo de contenido | **Aceptado y corregido.** `assert_no_credential_text` falla el gate si el transcript o el stderr contienen `authorization`, `access_token`, `refresh_token`, `auth.json`, `sk-ant-` o `bearer `; se suma al escaneo por nombre de archivo que `run()` ya aplica al intento |
| **P2** — `mcp_servers` comparado por igualdad exacta de dict, frágil ante metadatos | **Aceptado y corregido:** se compara la lista de pares (nombre, estado); test con `type: stdio` adicional |
| **P2** — `credentials_copied`/`session_persistence` eran literales, no hechos derivados | **Aceptado y corregido.** El directorio privado se escanea con `assert_no_credentials` antes de borrarlo; `session_persistence` se sustituye por `session_persistence_flag` más `client_home_residue`: Claude Code escribe bajo `~/.claude/projects/<slug del cwd>` los binarios de Resource que guarda aunque reciba `--no-session-persistence` (observado en la sonda). El recibo cuenta esos archivos y transcripts y el harness elimina ese directorio, que solo existe para el cwd desechable de la ejecución |
| **P2** — `HOME` real accesible por el proceso hijo, sin aislamiento equivalente al symlink de `CODEX_HOME` | **Aceptado como limitación documentada, no corregido.** El login de Claude Code se lee en sitio (keychain/HOME) y un `CLAUDE_CONFIG_DIR` privado exigiría copiar la credencial, que está prohibido. Mitigaciones verificadas en cada sesión: `--restricted`, `--setting-sources ""`, `--strict-mcp-config`, `--tools` limitado a Resources, `--allowedTools` limitado al servidor, `init.tools` sin built-ins ajenos, `permission_denials` vacío y cwd privado. Es el mismo modelo que M2 aceptó |
| P3 — `env: {}` en `mcp.json` podría vaciar el entorno del servidor | **Verificado empíricamente, no cambiado:** el proxy y el servidor arrancaron y midieron en las dos sondas con esa configuración |
| P3 — cualquier root de `FIXTURES` podía abrirse | **Aceptado y corregido:** solo los roots que referencian las filas positivas del plan (docker-free) o `benchmark` (runtime); test con `benchmark_compile_error` |
| P3 — `MCP_TOOL_TIMEOUT` de 900 s frente a 900 s de pared en docker-free | **Anotado, no cambiado:** el límite por llamada y el de pared son independientes; los rechazos docker-free tardan milisegundos |
| P3 — supuesto de que `content` no lleva prefijo `Error: ` | **Verificado en transcript real y endurecido:** el parser tolera el prefijo si apareciera |

## Estado tras la disposición

Sin P0–P2 abiertos en el harness. Limitación abierta y declarada: acceso del
proceso cliente a `HOME` (arriba). Las dos sondas reales con el harness final
—docker-free y runtime— pasan sus oráculos; la matriz completa se ejecuta
después desde el worktree limpio sobre el HEAD que contiene estos cambios.
