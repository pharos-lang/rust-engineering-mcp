# M2-02 — Recheck externo de contrato `rust.fmt.apply`

Fecha: 2026-09-05. Estado: **Accept with P2 follow-ups** para el contrato y la
corrección revisados. Este documento no declara M2-02 ni M2 Done y no cubre el
publisher/journal APFS ni D05, revisados por separado.

## Revisor y recibos

Se verificó `/Users/cburgosro/.local/bin/claude --version`: `2.1.260 (Claude
Code)`. `--help` confirmó las opciones usadas. El recheck read-only se ejecutó
con:

```text
/Users/cburgosro/.local/bin/claude --safe-mode --print
  --model claude-sonnet-5 --effort medium --tools ''
  --strict-mcp-config --mcp-config '{"mcpServers":{}}'
  --no-session-persistence --disable-slash-commands
  --output-format json
```

El [recibo de inputs](M2-02-contract-recheck-inputs.json) fija cinco
fuentes/selecciones, 43207 bytes de paquete y SHA-256
`3270ade63fab838b8e17932e866b28437a720cff2c0a881ea8cf435cb9a158cb`.
El [recibo bruto](M2-02-contract-recheck-sonnet.json) se persistió antes de
interpretarlo. Registra resultado exitoso en una iteración, cero permission
denials, cero tools y cero búsquedas web; `claude-sonnet-5` fue el modelo principal
con 19567 output tokens y 16177 thinking tokens. El recibo también atribuye 18
output tokens auxiliares a `claude-haiku-4-5`; no sustituyó al Sonnet solicitado.
Duración CLI: 206958 ms; duración API: 208286 ms; coste reportado: USD 0.2825208.
SHA-256 del recibo bruto:
`70cb065d254183e8e9f5dd2ee0f7fa8c83dfd52a3484f88be46663f3dc3e3a93`.

El [recibo inicial](M2-02-contract-inputs.json) y su
[respuesta bruta](M2-02-contract-sonnet.json) conservan la revisión que encontró
el P1. Su paquete tenía SHA-256
`220b2dd759b150bb599ebe3d7e50c715897c492d084da1248f574008b5c0c3db` y
la respuesta bruta
`a65f8cd90befaad5f192a3b3fa4089991ced792a8db9936bb5db7fdcb0551781`.

## Disposición

### P0

Ninguno.

### P1

Ninguno abierto en este alcance. El recheck considera cerrado el P1 inicial:

- `PreviewRetention` mantiene un token monotónico compartido. Si el guard sale sin
  retener, publica `false` con `Release`; `Plan::retained` lo lee con `Acquire`.
- `MutationPlans::resolve` no devuelve planes revocados. `remember_inner` los
  elimina antes de contar cuatro entradas o sumar el límite agregado de 64 MiB.
- `MutationTool::call` mantiene el guard fuera de `run_joined`, entrega al closure
  solo el token y llama `retain()` únicamente cuando el resultado contiene un
  preview, la codificación MCP terminó y el resultado completo quedó dentro del
  límite.
- Cancelación o deadline tardíos, drop del future, error del closure, error de
  codificación y fallback por tamaño dejan caer el guard sin retener. Los receipts
  durables mantienen precedencia ante una señal tardía.

La prueba MCP fuerza la secuencia `remember -> cancel -> cierre del closure ->
Joined.interrupted=Cancelled`, comprueba output cancelado sin `data`, deja caer el
guard, obtiene `NotFound` para el plan y reutiliza los cuatro slots. La prueba de
aplicación cubre además revocación con cuatro slots vivos, dos candidatos que ocupan
exactamente 64 MiB, recuperación total de ambos presupuestos y resolución de un
preview retenido con éxito.

### P2

1. La prueba rápida cruza el `Workers::run_joined` real y el `SharedPlans` real,
   pero construye directamente el `Output::failure` que representa el resultado
   observable. Un test futuro con doubles de registry/provider/inspector podría
   invocar `MutationTool::call` completo y proteger específicamente el orden
   `encode -> size bound -> retain` contra refactors.
2. Si el transporte pierde una respuesta después de que `call()` devolvió el MCP
   ya codificado, el proceso no puede saberlo y el plan retenido permanece hasta su
   TTL. Es una propiedad inevitable de la frontera RPC, distinta de la carrera P1
   ya cerrada.
3. Permanecen los seguimientos previos sin cambio: test MCP rápido de cross-kind
   commit/receipt, composición rápida de preview no-op/receipt `no_change` y una
   eventual distinción diagnóstica entre timeout del proceso aislado y deadline
   total.

El P2 previo del parser se cerró. La prueba directa acepta la forma fmt canónica de
nueve campos base más el fingerprint terminal y rechaza versión incorrecta,
fingerprint terminal ausente, longitud declarada incorrecta, valor truncado, byte
extra y fingerprint no hexadecimal.

## Validación local del delta

```text
cargo test -p rust-engineering-application --test mutation
# 15 passed

cargo test -p rust-engineering-mcp --bin rust-engineering-mcp stdio::mutation::tests
# 6 passed

cargo clippy -p rust-engineering-application --test mutation -- -D warnings
# passed

cargo clippy -p rust-engineering-mcp --bin rust-engineering-mcp --tests -- -D warnings
# passed
```

## Límites del recheck

El paquete corto revisó la retención/revocación en aplicación, el wiring MCP, la
semántica de `run_joined`, las pruebas nuevas y la revisión anterior. No inspeccionó
el publisher/journal nativo, Docker/D04 ni D05, y no repitió pruebas runtime. El
gate conjunto y la decisión de estado del milestone corresponden al Technical
Owner.
