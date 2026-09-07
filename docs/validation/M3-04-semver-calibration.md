# M3-04 — calibración de `rust.semver.check`

Fecha: 2026-09-06. Estado: **calificado 18/18 en Docker**.

La calibración se ejecutó contra `cargo-semver-checks 0.50.0` en la imagen
aprobada `sha256:384a1742ecc53cdd3a9c0bf36c6f8b66db73ddd118aeeae6e55654ea998ae36a`,
siempre con `/source` y `/baseline` read-only, red deshabilitada y el perfil
`seccomp-rust-quality.json`. El recibo consolidado es
[`M3-runtime.json`](M3-runtime.json); los cuatro intentos se conservaron como
`M3-runtime-attempt1.json` a `M3-runtime-attempt4.json`.

## Resultado observado

| Caso | Exit observado | Resultado de producto |
| --- | ---: | --- |
| baseline y candidato idénticos | 0 | `Passed`, sin ruptura |
| `pub fn` eliminado | 100 | `Failed`, deny `function_missing` |
| método de trait agregado con default | 0 | `Passed` |
| método de trait agregado sin default | 100 | `Failed` |
| variante agregada a enum `#[non_exhaustive]` | 0 | `Passed` |
| variante agregada a enum exhaustivo | 100 | `Failed` |
| item feature-gated eliminado, feature habilitado | 100 | `Failed` |
| mismo cambio sin habilitar el feature | 0 | `Passed`, sin señal |
| proyecto sin target `lib` | 101 | `Unavailable` antes de acreditar compatibilidad |
| baseline roto | 101 | `Incomplete`, nunca pass |
| finding configurado como warning | 0 | `Passed` con warning visible |
| baseline que requiere registry, offline/network-none | 101 | fallo reconocible, no hang |
| cancelación/EOF con hijo activo | sin exit publicable | `Cancelled`; cleanup unido |

La política para exit 100 sin finding parseable se pincha con un oracle compuesto:
el runtime confirma que el binario real emite 100 para una ruptura y el unit test
`semver_check::tests::parser_uncertainty_and_breaking_without_a_deny_row_fail_closed`
inyecta evidencia parseada vacía y exige `Blocked/INCOMPLETE_EVIDENCE`. No se
fabricó una salida imposible del plugin para hacer pasar ese caso.

## Salida no coloreada y parser

La ayuda fijada no ofrece salida de findings machine-readable. La rama fallback de
ADR-062 §11 queda seleccionada: outcome grueso autoritativo por exit code, conteos
deny/warn y filas best-effort desde un parser acotado, `completeness: Partial`, y
stdout/stderr crudo como Resource privado.

Formas observadas y fijadas como goldens en
`crates/execution-adapter/tests/fixtures/semver-{clean,breaking,warn}.stdout`:

```text
--- failure function_missing: pub fn removed or renamed ---
223 checks: 222 pass, 1 fail, 0 warn, 31 skip
```

```text
--- warning function_missing: pub fn removed or renamed ---
223 checks: 222 pass, 0 fail, 1 warn, 31 skip
```

El caso limpio contiene `Summary no semver update required`. La opción
`--color never`, `NO_COLOR=1`, `GIT_DIR=/nonexistent` y
`GIT_CEILING_DIRECTORIES=/` produjeron bytes sin escapes y neutralizaron un `.git`
plantado. Un formato no reconocido permanece `Incomplete`; nunca se interpreta
como cero rupturas.

## Target-dir, captura y artifacts

Los pares reales completaron rustdoc/build con ambos roots read-only y
`CARGO_TARGET_DIR=/work/target`; por tanto el target dir cubre candidato y baseline
dentro del contenedor. La prueba de inmutabilidad confirma que ningún byte se
escribió bajo `/source` ni `/baseline`. Cada lado conserva su propio
`SnapshotEvidence`; no se afirma atomicidad entre roots.

Stage 1 leyó el raw output por índice y chunk. La segunda selección MCP sostuvo el
`store.lock` real durante startup y confirmó fallback Stage 0 ante `Busy`. Ambos
artifacts respetan el presupuesto de 512 KiB y las omisiones declaradas.

## Selecciones y evidencia

Pasaron 16 selecciones exactas en `semver_runtime` y dos en
`inspection_runtime` (18/18, 285.763 s en el intento final), incluyendo todas las
familias, warn-only, git plantado, registry deny, cancelación/EOF, inmutabilidad,
Resource Stage 1 y fallback Stage 0. El script usa `--exact --ignored
--test-threads=1` y exige exactamente un test ejecutado y aprobado por selección.

Inputs de provisión:

| Input | SHA-256 |
| --- | --- |
| `m3-provisioning/help/cargo-semver-checks-help.stdout` | `9f6083facba0fb4efa1055c8de564c2a4f7a1a9b76ba3b1a18cec2e74a950eeb` |
| `m3-provisioning/help/cargo-semver-checks-check-release-help.stdout` | `c6f3e08c5de346c4f02e5185938159fe94e42f0f523da4014ac1044aee1c0783` |
| `M3-image-config.json` | `3252effc88fb3f8c31fec6f4ea146e057f6ef1c19252f7c554640b35c0d4ab77` |
