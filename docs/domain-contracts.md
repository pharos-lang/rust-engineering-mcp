# Contratos del dominio M0-02

La API está en `rust-engineering-domain` y concretada por
[ADR-022](adr/ADR-022-domain-contracts.md). Todavía no es el contrato wire MCP.

## Valores e invariantes

| Tipo | Garantía |
| --- | --- |
| `ProjectRef` | `prj_` + 32 hex minúsculos; sintaxis, no autoridad/entropía |
| `ProjectIdentityFingerprint` / `ExecutionFingerprint` | Tipos incompatibles; digest `sha256:` + 64 hex; sin calcular preimágenes |
| `NonEmptyText` | Conserva Unicode/texto original, rechaza vacío o solo whitespace |
| `SourceSpan` / `ByteRange` | Coordenadas no invertidas, extremos exclusivos, inserciones vacías permitidas |
| `Suggestion` | Una o varias ediciones; reemplazo vacío significa eliminación |
| `OutputEnvelope<T>` | Status/errores coherentes; data tipado; campos de error requeridos y nullable |
| `SnapshotEvidence` | Provenance/freshness inseparables y coherentes con clock/policy declarados |

Los paths de spans son evidencia textual, no autorización ni verificación de
existencia. Bytes y posiciones se validan de forma independiente, sin leer source.
Los campos opcionales de diagnósticos pueden omitirse; error_code/error_message,
created_at/observed_at y age_seconds exigen presencia aunque su valor sea null.

## Resultado base

`Report<T>` agrupa summary, duration_ms, diagnostics, truncation, data y evidence.
`OutputEnvelope::passed`, `failed`, `cancelled` y `operational_error` crean estados
válidos. Un error de compilación con E0502 usa `failed` y ambos campos de error
nulos. `is_operational_error()` es falso para ese caso; el adapter MCP decidirá
posteriormente cómo transportar el resultado.

| Caso | Status | error_code / error_message |
| --- | --- | --- |
| Validación correcta | `passed` | null / null |
| Fallo del proyecto | `failed` | null / null |
| Tool ausente / plataforma no soportada | `unavailable` | código / mensaje |
| Otros errores operativos de §69, incluido timeout | `blocked` | código / mensaje |
| Cancelación | `cancelled` | null / null; sigue siendo operacional |

Truncation conserva flags de streams y cantidad de diagnósticos omitidos; no cambia
el resultado de la ejecución. Un resultado parcial no se convierte por sí solo en
`OUTPUT_LIMIT_EXCEEDED`.

## Evidencia y tiempo

`Evidence` usa `{"kind":"local"}` o
`{"kind":"snapshot","details":{"provenance":...,"freshness":...}}`.
`SnapshotEvidence::assess` recibe provenance, una policy validada y un Clock; no
consulta red, filesystem ni reloj real. Los segundos UTC son enteros u64.

La edad se calcula desde created_at, nunca desde la fecha de importación. Los límites
fresh/aging son inclusivos; la fecha ausente o futura da unknown. Network usado al
crear la fuente no convierte una consulta snapshot en live. Integridad y freshness
son dimensiones distintas: metadata fresh puede tener integridad no verificada.

La deserialización comprueba coherencia con assessed_at y policy persistidos; no
autentica datos ni los vuelve actuales. La aplicación reevaluará antes de una
decisión actual y aplicará la policy de cada tool (por ejemplo audit).

## Evidencia reproducible

```text
cargo test -p rust-engineering-domain --locked --offline
cargo test -p rust-engineering-domain --doc --locked --offline
cargo tree -p rust-engineering-domain --edges normal --locked --offline
```

Los tests externos prueban la API pública como unidades de dominio; el doctest
compile-fail detecta el intercambio de tipos de fingerprint. En M0-07 se añadirán
schemas y snapshots wire a partir de tipos Rust, sin mantener un protocolo paralelo.

`ArtifactId` valida art_ seguido de128 bits hex canónicos; ArtifactMetadata de salida
contiene owner, tamaño, SHA256 de bytes redactados/truncados y tiempo monotónico
local de creación/expiry. No es un formato persistido ni un timestamp UTC.
