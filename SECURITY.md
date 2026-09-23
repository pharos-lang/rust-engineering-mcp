# Seguridad

## Estado actual

El checkout de desarrollo es `0.9.0-rc.1` (release candidate de desarrollo,
sin tag ni release publicada) y anuncia 36 tools: 31 de clase `stable` y 5
`rust.analyzer.*` de clase `preview`. La release binaria publicada más
reciente es `v0.3.0` y expone 31 tools estables (sin las 5 `preview`); solo
publica el core para macOS ARM64/APFS. El binario publicado **no tiene firma
de código ni notarización macOS**; su única provenance es `SHA256SUMS` más la
attestation de build OIDC de GitHub (`gh attestation verify`). Ejecutar
código del proyecto (tests, build scripts, proc macros, lints, benchmarks)
requiere el gateway Docker/Linux ARM64 aprobado por el host; sin ese gateway
el runtime no ejecuta nada del proyecto.

La referencia vigente y completa del modelo de seguridad, el Execution
Gateway, el aislamiento por fase y el threat model (fronteras, controles y
oráculos) es
[`docs/architecture/execution-and-security.md`](docs/architecture/execution-and-security.md).
Este documento resume el estado y los riesgos residuales; no repite el
detalle técnico ni la evidencia por hito, que vive en ese capítulo y, para
hitos ya cerrados, en el historial de Git a partir del commit `51fa602e`.

## Requisitos para habilitar ejecución

Roots confiables explícitos por el host, I/O relativo no-follow/reparse-safe,
un único Execution Gateway (nunca `sh -c`/`bash -c`/shell arbitrario),
entorno reconstruido con allowlist, aislamiento real del gateway Docker
(seccomp deny-default, sin red salvo excepción documentada, `--cap-drop=ALL`,
rootfs read-only, límites de PIDs/memoria/CPU) y control del árbol de
procesos con timeout/cancelación. `cargo check`, Clippy, tests, `build.rs` y
proc macros pueden ejecutar código no confiable del proyecto; no se
describen como seguros por ser herramientas de validación.

No se anuncia una garantía de aislamiento antes de demostrarla con pruebas en
la plataforma correspondiente; una opción offline de Cargo no demuestra por
sí sola aislamiento de red.

**No usar el servidor con repositorios no confiables en esta versión de
desarrollo.** El audit de seguridad es una revisión de modelo, no un
pentest (RR-01); el kernel, `runc` y Docker Desktop siguen en el TCB (RR-05);
1.0 no está calificado como cierre. Autoriza únicamente las roots que
necesitas y usa rutas absolutas.

## Riesgos residuales

`docs/architecture/execution-and-security.md` mantiene la tabla completa de
diecinueve riesgos aceptados por 1.0 (RR-01…RR-19, seis columnas: riesgo,
severidad, alcance, mitigación, condición de reapertura) y el threat model
M8-08 (8 fronteras de confianza, 53 controles, oráculos por frontera). Dos
limitaciones deben quedar explícitas aquí, no solo en el capítulo:

- **Sin firma de código ni notarización macOS** (RR-19, riesgo aceptado): la
  integridad del archive de release se verifica por `SHA256SUMS` y por la
  attestation OIDC de build de GitHub, nunca por Gatekeeper. Un canal de
  distribución que active Gatekeeper (por ejemplo Homebrew o un instalador)
  necesitaría resolver esto antes de usarse.
- **El store privado de artifacts de quality jobs (ADR-061/ADR-062) no usa un
  binding secreto.** `owner_binding = uid + state-root + granted-root`, sin
  ningún token o secreto asociado — es una garantía deliberadamente más
  débil que un binding basado en secreto: cualquier proceso con el mismo uid
  y un grant vivo sobre la misma root puede releer la evidencia. No es una
  fuga de aislamiento por sesión ni por peer inexistente; es un límite de
  diseño documentado.

Ningún otro riesgo residual se repite aquí para evitar que este resumen y el
capítulo de arquitectura diverjan; ante cualquier duda, el capítulo es la
fuente vigente.

## Reportar un problema

Reportar vulnerabilidades mediante
[GitHub private vulnerability reporting](https://github.com/pharos-lang/rust-engineering-mcp/security/advisories/new),
con versión o commit, plataforma, pasos de reproducción e impacto. No abrir
una incidencia pública antes de coordinar la corrección y no incluir
secretos reales en fixtures o logs. Las releases binarias publicadas
contienen solo el core macOS ARM64; el soporte a otro host requiere una
nueva decisión registrada (ver `.planning/deferred-commitments.md`) que
reevalúe los riesgos residuales relevantes.

Sin detección universal de secretos: el source concedido y sus secretos
propios se retienen en artifacts, logs y evidencia de validación tal como el
peer los entregó; no hay secret scanning de contenido en CI. No enviar
evidencia que contenga secretos reales.
