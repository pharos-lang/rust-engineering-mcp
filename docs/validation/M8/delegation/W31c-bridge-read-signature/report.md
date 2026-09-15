# W31c — puente Inspector: firma real de `readResource`

Bundle auditado: `target/m1-17-inspector/node_modules/@modelcontextprotocol/inspector/clients/cli/build/index.js`.

## Causa raíz confirmada

`scripts/m8-inspector-session.mjs:195` (antes del fix) llamaba:

```js
const resourceRead = await client.readResource({ uri: artifactUri });
```

`InspectorClient.readResource` (bundle línea 12060) tiene la firma `async readResource(uri, metadata)`, donde `uri` se coloca **tal cual** en `params.uri` (línea 12065-12068: `const params = { uri, ...(_meta) }`). Al pasar un objeto literal `{ uri: artifactUri }` como primer argumento, `params.uri` terminaba siendo ese objeto en vez del string — de ahí el `params.uri` malformado que el orquestador observó en la sesión runtime (`resources/read` con `uri` no-string), y que rmcp respondió como `-32601 Unknown method` al no reconocer el shape de params de un método por lo demás válido.

**Fix aplicado** (`scripts/m8-inspector-session.mjs:195`):

```js
const resourceRead = await client.readResource(artifactUri);
```

## Auditoría de todas las llamadas del puente contra el bundle

| Llamada en el puente | Uso en `m8-inspector-session.mjs` | Firma real en el bundle | Línea del bundle | Veredicto |
|---|---|---|---|---|
| `client.connect()` | línea 124 | — (no requiere argumentos) | — | correcto |
| `client.getProtocolEra()` | línea 125 | `getProtocolEra()` (sin argumentos) | 10765 | correcto |
| `client.listAllTools({ cacheMode })` | línea 126 | `async listAllTools(options)` | 10876 | correcto |
| `client.listAllResources()` | línea 131 | `async listAllResources(options)` | 12035 | correcto |
| `client.callTool(tool, args)` | líneas 67, 79, 94, 96, 146, 172, 213, 236, 253 | `async callTool(tool, args, generalMetadata, toolSpecificMetadata, taskOptions, options)` — primer argumento es el objeto `tool` (de `listAllTools`/`listAllResources`), segundo son los `args` | 11260 | correcto |
| `client.readResource({ uri })` → **corregido a** `client.readResource(artifactUri)` | línea 195 | `async readResource(uri, metadata)` — `uri` es un string, no `{ uri }` | 12060 | **bug encontrado y corregido** |
| `client.cancelToolCall()` | línea 215 | `cancelToolCall()` (sin argumentos, devuelve `boolean`) | 10402 | correcto |
| `client.disconnect(5_000)` | líneas 275, 284 | `async disconnect(safeDisconnectTimeout = 0)` | 9988 | correcto |
| `client.baseTransport?.close()` | línea 256 | acceso de propiedad, no un método del puente en sí | — | fuera de alcance de la auditoría de firmas |

Firmas citadas pero no usadas por el puente (verificadas por completitud, ninguna se invoca hoy):
- `listResourceTemplates(cursor, metadata)` — bundle línea 12138.
- `listTools(cursor, metadata)` (paginado de bajo nivel; el puente usa el agregador `listAllTools`) — bundle línea 10841.
- `listResources(cursor, metadata)` (idem, agregado por `listAllResources`) — bundle línea 12008.

No se encontró ninguna otra discrepancia de firma: cada llamada restante del puente pasa exactamente los parámetros posicionales que el bundle declara.

## Guardia de regresión

Añadido a `scripts/test-m8-clients-unit.py` (clase `InspectorBridgeSignatureTests`):

- `test_read_resource_does_not_receive_an_object_literal`: falla si `.readResource({` reaparece en el fuente del puente (regex sobre el texto del `.mjs`).
- `test_read_resource_is_called_with_the_bare_artifact_uri`: confirma la forma correcta `client.readResource(artifactUri)`.

## Verificación ejecutada

- `node --check scripts/m8-inspector-session.mjs` → sin errores de sintaxis.
- `python3 scripts/test-m8-clients-unit.py` → **118/118 tests OK** (116 preexistentes + 2 nuevos).
- Docker está disponible en este host (`docker info` responde). Se ejecutó `python3 scripts/test-m8-clients.py --preflight --with-runtime --docker-socket /var/run/docker.sock`: todas las precondiciones obligatorias (binario del candidato, versión 0.8.0, socket Docker, bundle de Inspector, binario Codex) están satisfechas; la única precondición insatisfecha es `gemini_version` (Gemini CLI observado en `1.2.3` contra el pin `1.2.2`, cliente no obligatorio). No se ejecutó una sesión `runtime` real de punta a punta: `--run --with-runtime` orquesta la matriz completa de clientes (Codex, Claude Code, Gemini, Inspector) vía `scripts/test-m8-clients.py`, lo cual excede el alcance de un fix puntual en el puente de Inspector y los archivos permitidos para esta delegación. La corrección se verificó de forma estática contra las firmas reales del bundle, que es la fuente primaria del bug (no un efecto de configuración de runtime).

## Archivos tocados

- `scripts/m8-inspector-session.mjs` — 1 línea (la llamada a `readResource`).
- `scripts/test-m8-clients-unit.py` — clase de test nueva + `import re`.

Sin commit, según instrucción.
