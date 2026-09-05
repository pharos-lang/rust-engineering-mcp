# M2-02 native writer — recheck externo acotado

Fecha: 2026-09-05  
Snapshot revisado: `331d163`  
Packet SHA-256: `f6e83fd1616be2f46c5e4b910e9d7c4d7890b94e6a30f6913d47053e93b4c6da`

## Alcance y ejecución

El recheck cubre exclusivamente el publisher nativo M2-02 contenido en el packet
de 243.047 bytes. Los catorce hashes registrados en
`M2-02-native-recheck-inputs.json` coinciden exactamente con los archivos de
`331d163`. No cubre extensiones posteriores, incluido `FixApply`, ni declara M2
Done.

Se verificó Claude Code `2.1.260` antes de invocarlo. La revisión se ejecutó con
modelo explícito `claude-opus-5`, esfuerzo `high`, tools vacías, MCP estricto y
vacío, `dontAsk`, sin persistencia de sesión y sin ejecutar comandos ni leer otros
archivos. El resultado bruto registra 418.666 ms, `stop_reason=end_turn`, cero
permission denials y un único modelo en `modelUsage`: `claude-opus-5`. No apareció
modelo auxiliar Haiku ni subagente.

## Veredicto

**Accepted with tracked P2. No blocker.**

No se encontró P0 ni P1. Los dos P1 que motivaron el `Revise` inicial quedaron
resueltos: F1 fue descartado con el orden canónico de `SourceBundle` y una prueba
real de layout modular; F2 ya no tiene un trigger propio del writer porque la
admisión calcula y valida el journal de peor caso antes de crear journal o temporal.

## Disposición de findings originales

| Finding | Severidad original | Disposición del recheck |
| --- | --- | --- |
| F1, orden DFS frente a orden lexicográfico | P1 | **Disproved.** `SourceBundle::with_directories` ordena los archivos por path. La prueba `src/parser.rs` + `src/parser/ast.rs` confirma el orden y completa un commit real. |
| F2, recovery determinísticamente irrecuperable | P1 | **Fixed para triggers propios; residual aceptado para corrupción externa.** El cálculo de peor caso usa todos los staged nodes máximos, `u64::MAX` y `RecoveryRequired`, y se valida antes del primer write. No se añadió raw force-prune porque borraría la evidencia. |
| F3, headroom fijo insuficiente | P2 | **Fixed.** Se reserva `2 × worst_case_record_len`; queda el caso distinto N1 descrito abajo. |
| F4, receipt/recover divergentes con nombre reservado ocupado | P2 | **Fixed.** Ambos retornan receipt `RecoveryRequired` y preservan los bytes. |
| F5, temp ausente en Applying retornaba error sin evidencia | P2 | **Fixed.** Retorna receipt con before/after y `effect_after=None`. |
| F6, borde format sin semántica Rust | P2 | **Wedge disproved; residual de scope aceptado.** `SourceFile` limita 1 MiB y `SourceBundle` 16 MiB. El writer solo garantiza reemplazos existentes `.rs`; provenance rustfmt pertenece a la aplicación. |
| F7, bits rename sin evidencia | P2 | **Fixed mediante fixture negativa; runtime probe diferido.** El swap protegido rechaza un parent symlink y `EXCHANGE` sin flags sigue el mismo link y efectúa el swap. |
| F8, gaps de crash multi-file | P2 | **Fixed.** SIGKILL real cubre prefijo de un clone, prefijos publicados, `Published` y prefijo de un cleanup; recovery usa store nuevo y verifica topología y bytes. |
| F9, ausencia de latencia end-to-end | P2 | **Fixed mediante medición; batching no requerido.** Commit APFS de 128 archivos/16 MiB: 5.687 s; replay: 173 ms; recovery: 103 ms. Es una observación, no un benchmark ni gate temporal. |
| F10, fases terminales con igual rank | P2 | **Residual aceptado, sin ruta productiva.** Rangos totales distintos romperían transiciones terminales válidas; un predecessor explícito sería el hardening viable. |
| F11, hashes incompletos | P2 | **Fixed.** La evidencia incluye `domain/source.rs`, `macos.rs`, ADR-054, soporte externo de tests y métricas. |

## Nuevos findings

### N1 — P2: ceiling del store durante recovery de registros heterogéneos

La reserva se comprueba al admitir cada registro. En un store casi lleno por
registros pequeños, recuperar después un registro grande puede crear
transitoriamente una staging copy que exceda 256 MiB. Si el proceso cae en esa
ventana, `list_records` y commits nuevos fallan por límite, aunque receipt/recover
por ID siguen disponibles y pueden promover el staging. No hay pérdida de bytes ni
P1, pero el ID puede requerir inspección del nombre del journal. Debe seguirse en el
gate combinado junto con el procedimiento de reconciliación operativa.

### N2 — P2: pico de memoria no medido

La admisión materializa el body, clones para encoding y el record de peor caso. El
límite de 48 MiB acota el archivo, no el RSS. La medición demuestra que el host
usado completa la operación, pero no mide el pico ni establece un límite formal.

### N3 — P2: precisión de la afirmación sobre tests fuera de `src`

El subprocess harness se movió físicamente a `tests/support`, lo que satisface el
architecture checker. Sigue compilándose dentro del target `--lib` mediante
`#[cfg(test)]` para acceder a internals privados. La evidencia debe describirlo como
separación del árbol productivo y ausencia del binario de producción, no como otro
compilation unit.

## Residuales a seguir

- procedimiento explícito para records externos corruptos o indescifrables;
- N1, ceiling temporal durante recovery heterogéneo;
- N2, pico de memoria de admisión;
- runtime probe opcional para flags de rename;
- latencia como snapshot de un host, no como regression gate;
- pin opcional de ADR-050/052 dentro del artifact de qualification;
- precisión del conteo, que incluye helpers environment-gated.

El resultado completo y su metadata están en
`M2-02-native-recheck-opus.json`; el inventario y hashes exactos están en
`M2-02-native-recheck-inputs.json`.
