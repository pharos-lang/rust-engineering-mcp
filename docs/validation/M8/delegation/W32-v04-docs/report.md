# W32 — informe (correcciones documentales V04)

Modelo: Claude Sonnet 5, `--effort medium`. Rol: worker de documentación, sin
subagentes, sin segundo plano, sin commit. Archivos tocados: `docs/validation/M8/08-threat-model.md`,
`docs/adr/ADR-089-residual-risk-register.md`, `docs/adr/ADR-088-migration-rollback-policy.md`
(§3, una frase), `docs/security-model.md` (lista RR), `docs/validation/M8/checklist-1.0.md`
(filas 10 y 11).

## F-01 — citas de `env_clear` a código de test

Verificado con Grep antes de escribir: `crates/execution-adapter/src/lib.rs:333-346`
es el único `Command::new`/`env_clear` de producción; call sites confirmados en
`rust_gateway.rs:1832`, `mutation_gateway.rs:369,412`, `analyzer_gateway.rs:551`.
Corregidas dos citas en `08-threat-model.md`: la fila B3 "Peer LSP hostil" (antes
`lsp_session.rs:879`, que es `#[cfg(test)]`) y §4.2 "Secretos en source y en
evidencia" (antes citaba también `supervisor.rs:465` y `analyzer_gateway.rs:1645`,
ambos de test).

## F-02 — citas B8 desfasadas; issue §9.1 ya resuelto

Verificado con Grep: `permissions` del job `build` en `:41-44`, bloque
`gh attestation verify` en `:147-164`, `contents: write` del job draft en
`:178-179`, pin de checkout en `:47`. Corregida la fila B8 "Robo de clave de
firma" y "Acción de terceros comprometida" en `08-threat-model.md`.

El issue §9.1 (`release-candidate.yml:219` exigía `tools == 31` mientras el
smoke publica 36) está cerrado: la comparación toma `tool_count` de
`docs/validation/M8/freeze-0.8.0.json` (`:128-136`, `:200`, comparado en
`:228`), verificado leyendo el workflow completo. Cerrado en la fila B8
correspondiente, en §9 issue 1, y en la consecuencia de ADR-089 (antes citaba
el defecto como abierto).

## F-03 — `doctor` retiene el lock del store

`docs/tools.md` ya documenta el comportamiento desde W30 (línea 609: "crea el
lock del store si no existe"). Añadida una frase a ADR-088 §3 (Decision, punto
3) señalando que el preflight no es puramente de lectura: abre el store, crea
`mutation-store.lock` si falta y toma un `flock` no bloqueante durante el
escaneo, con posible `Busy` ante contención — sin cambio de comportamiento,
solo documentación.

## F-07 — RR-12 dividido

En ADR-089, threat model §7 y security-model.md: RR-12 queda con los riesgos
estructurales aceptados (OIDC ≠ reproducibilidad, `SONAR_TOKEN` de larga
vida). Las tres precondiciones (re-observar branch protection, ejecutar
provenance 0.8.x, verificar D14 sobre assets reales) se sacan de RR-12 y
pasan a `checklist-1.0.md` como ítems bloqueantes de RC1 en las filas 10 y 11.

## RR-10 ampliado (token OIDC, F-05)

RR-10 (ADR-089, threat model §7 y §4.1, security-model.md) ahora incluye la
exposición del token OIDC al job `build` (`release-candidate.yml:41-44`, que
compila `build.rs` de dependencias antes de atestar). Mitigación:
`persist-credentials: false` (W33, cambio seguro ya aplicado en los
checkouts). El split del job de attestation queda como condición de
reevaluación, no como acción de este corte: cambiar el workflow sin poder
ejecutarlo antes de RC1 es más riesgo que beneficio.

## RR-19 (nuevo)

Añadido en ADR-089 (registro y Decision punto 1), threat model §7 y §8
(conteo actualizado a 19), y `docs/security-model.md` (lista de riesgos
residuales): sin firma de código ni notarización macOS; integridad por
`SHA256SUMS` + attestation OIDC; condición de reevaluación: distribución por
un canal que active Gatekeeper (Homebrew, instalador) fuera del
archive/attestation actual.

## Verificación

`python3 -B scripts/docs-hygiene.py links-check`: 1 roto en documentos vivos,
`docs/validation/M8/checklist-1.0.md:13 -> 06-reproduction.md`, preexistente
y esperado hasta W29 (ignorado por instrucción). Ningún enlace nuevo roto por
estos cambios.

No se hizo commit.
