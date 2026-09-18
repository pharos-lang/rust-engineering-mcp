# ADR-090 — Verificación offline y respuesta a compromiso de release

Date: 2026-09-14

## Context

D14 (`docs/roadmap/adr-backlog-m2-m8.md` §D14) exige, con fecha límite M8-07,
conservar el provenance OIDC source/tag/workflow/run/digest heredado de
ADR-047/ADR-048 y añadir una política de incidentes/rotación/verificación
offline verificable, sin exigir HSM ni confundir un tag sin firma con un
asset sin provenance. `docs/validation/M8/07.md` registra la decisión
vinculante del orquestador bajo autorización del owner (sesión 2026-09-14).

El artifact 0.8.0/1.0 es exactamente el core `aarch64-apple-darwin` de
[ADR-087](ADR-087-1.0-host-scope.md), la misma frontera de artifact que fijó
[ADR-048](ADR-048-0.1.0-qualification-and-artifact-boundary.md) para 0.1.0.
`.github/workflows/release-candidate.yml` ya construye, empaqueta
(inventario, SBOM SPDX, notices, manifest, checksums), instala y ejecuta el
smoke desde los bytes archivados, y produce attestations GitHub OIDC de
build-provenance sin clave de repositorio de larga duración; ninguna de esas
piezas cambia aquí. [ADR-089](ADR-089-residual-risk-register.md) RR-12
registra que M8-07 hereda dos defectos concretos: el literal `!= 31` en
`release-candidate.yml:219` (el smoke publica 36 tools desde el freeze
0.8.0) y la ausencia de una política de verificación offline/incident
response documentada.

El ensayo local de M8-07 (`docs/validation/M8/07-release-rehearsal.json`)
construyó el archive core con `scripts/release-artifact.py`, lo movió a un
directorio limpio bajo `target/` simulando una descarga, y ejecutó
`scripts/release-smoke.py` contra esos bytes exactamente como lo hace el
job `build`. El propio drill de redescarga+verificación encontró y corrigió
dos hashes de esquema mal fijados en `TOOL_SCHEMA_SHA256`
(`rust.binary.bloat`, `rust.analyzer.action.apply`): el `contract --json`
del binario y el manifiesto de freeze coincidían entre sí en
`description_sha256`/`input_schema_sha256`/`output_schema_sha256`/
`annotations`, pero dos valores combinados fijados en el smoke no
coincidían con esos mismos campos, y el smoke fallaba cerrado en lugar de
aceptar bytes divergentes del contrato congelado. Esa evidencia es la razón
de ser de este ADR: la verificación offline solo vale lo que valga su
oráculo, y el oráculo ya demostró que falla cerrado ante una divergencia
real antes de publicar nada.

No existe hoy custodia de clave organizacional adicional, procedimiento de
"dos operadores" ni bundle de attestation publicado; D14 decide
explícitamente si añadirlos.

## Decision

1. **Se conserva OIDC** (ADR-047/ADR-048): build en GitHub Actions,
   attestations de provenance (source commit, tag, workflow, run, digest) vía
   `actions/attest-build-provenance`, y checksums SHA-256 (`SHA256SUMS`). No
   se añade una clave organizacional adicional ni un procedimiento ficticio
   de "dos operadores"; el plan los descarta salvo necesidad demostrada por
   un incidente o por un requisito de cumplimiento futuro (ver Alternatives).
2. **Verificación offline.** Cada release publica, junto al archive core, el
   `SHA256SUMS`, la SBOM SPDX (`sbom.spdx.json`), las notices
   (`THIRD_PARTY_NOTICES.txt`) y el bundle de attestation Sigstore
   descargable que `actions/attest-build-provenance` adjunta al run. La guía
   pública (`docs/publication.md` §«Verificación offline») documenta dos
   rutas:
   - Con `gh` instalado: `gh attestation verify --bundle <archivo-descargado>
     --owner pharos-lang <asset>` reconstruye la cadena de confianza Sigstore
     sin red más allá de la descarga inicial (y cacheada) de las claves raíz
     públicas de Sigstore que `gh` gestiona; no depende de que el verificador
     tenga acceso a la API de GitHub para el propio repositorio.
   - Sin `gh`: verificación mínima por `sha256sum -c SHA256SUMS` seguida de
     la instalación descrita por `inventory.json` (target, versión, cierre de
     dependencias) — la misma comprobación que ejecuta
     `scripts/release-smoke.py` fail-closed. Esta ruta no autentica al
     publisher; solo confirma que los bytes instalados son los bytes
     publicados en el momento de la descarga.
   Ninguna ruta afirma reproducibilidad binaria bit a bit del ejecutable
   Mach-O (`scripts/release-artifact.py` ya documenta esa frontera:
   determinista en el toolchain Darwin arm64 calificado, no
   cross-zlib-byte-universal).
3. **Respuesta a compromiso.**
   - (a) La única credencial de publicación es el token OIDC de vida
     efímera emitido al workflow `release-candidate.yml`, acotado por sus
     `permissions:` mínimos (`id-token: write`, `attestations: write` solo en
     el job `build`; `contents: write` solo en el job `draft`) y por el hecho
     de que el dispatch exige un tag `vX.Y.Z` o `vX.Y.Z-rc.N` preexistente
     (`validate-ref`); un RC nunca es una release soportada.
     No hay secreto de larga duración de publicación que rotar; la protección
     de rama/tag y la revisión de CODEOWNERS sobre el propio workflow son el
     control de acceso.
   - (b) Si un asset publicado se compromete (binario, archive o el propio
     workflow): se retira el asset del release de GitHub, se publica un
     advisory siguiendo `SECURITY.md`, y se corta un nuevo tag desde una
     fuente limpia verificada. Las attestations previas quedan inválidas de
     hecho porque están ligadas por `subject-path` al digest exacto del
     archive comprometido: ningún digest nuevo puede reutilizar una
     attestation antigua, y una attestation antigua nunca valida un digest
     nuevo. No existe "revocación" de una attestation Sigstore aparte de
     dejar de distribuir el asset y publicar el advisory.
   - (c) La firma Ed25519 de catálogo (ADR-041) es un protocolo
     completamente separado del provenance de release: firma bundles de
     catálogo, no archivos de release; tiene su propio trust file con
     rotación y revocación por reemplazo de clave pública, sin relación con
     OIDC ni con Sigstore. `docs/publication.md` documenta esta separación
     explícitamente para que un lector no confunda ambos mecanismos.
4. **Drills de evidencia M8-07**, ya ejecutados o mapeados a evidencia
   existente, ninguno bloqueado por este ADR:
   - Redescarga + verificación: el smoke desde un directorio limpio
     (`docs/validation/M8/07-release-rehearsal.json`) simula la instalación
     desde una descarga fresca sin red; encontró y corrigió una divergencia
     real de contrato (ver Context).
   - Dependencia comprometida: simulada por `cargo audit` sobre el snapshot
     RustSec sintético del gate M4 (evidencia histórica M4, no repetida
     aquí).
   - Rollback de producto: `docs/validation/M8/03-rollback.json` (ADR-088).
   - Revocación de trust de catálogo: tests M1-10 (ADR-041), sin relación con
     el provenance de release pero parte del mismo programa de drills de
     confianza.
5. **Sin publicación en M8-07.** El ensayo local queda acotado a construir el
   archive con `scripts/release-artifact.py`, moverlo a un directorio limpio
   e instalarlo/ejercerlo con `scripts/release-smoke.py`, sin `gh release`,
   sin tag y sin push. La cadena completa tag → run → digest → attestation →
   verificación independiente (como en `docs/validation/M1/17-public-release.json`
   para 0.1.0) se demuestra en RC1 (M8-09) cuando el owner autorice el primer
   tag `v0.8.0`.

## Alternatives considered

- **Clave organizacional adicional** (HSM o secreto de firma propio para
  releases). Rechazada: duplicaría el problema de custodia/rotación que OIDC
  ya elimina, sin ganancia de seguridad sobre un token OIDC efímero ligado al
  workflow exacto; queda como opción solo si un incidente futuro demuestra
  que OIDC + branch protection es insuficiente.
- **Procedimiento de "dos operadores"** para cada publicación. Rechazada
  para un equipo de este tamaño: no hay hoy separación de roles real que
  hacer cumplir, y un procedimiento no forzado por tooling es teatro de
  seguridad, no un control. Se reevalúa si el equipo de publicación crece.
- **No publicar bundles de attestation Sigstore** y limitarse a
  `SHA256SUMS`. Rechazada: `actions/attest-build-provenance` ya los produce
  sin coste adicional, y omitirlos degradaría la verificación offline a
  solo-integridad (sin autenticación de origen) para todos los usuarios, no
  solo para los que carecen de `gh`.

## Consequences

- `docs/publication.md` gana las secciones «Verificación offline» y
  «Respuesta a compromiso» con los comandos exactos citados arriba.
- `docs/roadmap/adr-backlog-m2-m8.md` §D14 pasa de `Proposed` a `Accepted`.
- `.github/workflows/release-candidate.yml:219` deja de exigir un literal
  `31`: el conteo esperado de tools se deriva en el job `build` del
  manifiesto de freeze del repo ya checked-out
  (`docs/validation/M8/freeze-0.8.0.json`, campo `tool_count`) y se propaga
  al job `draft` como output entre jobs, cerrando el defecto que
  [ADR-089](ADR-089-residual-risk-register.md) RR-12 le atribuía a M8-07.
- `scripts/release-smoke.py` corrige los dos valores de
  `TOOL_SCHEMA_SHA256` que no coincidían con el contrato congelado
  (`rust.binary.bloat`, `rust.analyzer.action.apply`); ambos coincidían ya en
  `contract --json` y en `docs/validation/M8/freeze-0.8.0.json`, así que el
  contrato en sí nunca estuvo mal — el defecto era solo del pin del
  verificador, y el propio verificador lo detectó fallando cerrado.
- RR-12 permanece parcialmente vigente hasta RC1: la cadena tag/run/digest y
  la verificación de attestations no se ejecutan sin un tag real; este ADR
  no la cierra, solo fija la política que RC1 ejecuta.
- Ningún ADR nuevo autoriza publicación; `gh release create --draft
  --prerelease` sigue siendo el único paso que produce un artifact visible, y
  sigue exigiendo un tag `vX.Y.Z` o `vX.Y.Z-rc.N` preexistente más la
  autorización separada del owner para crearlo.

## Status

Accepted (orquestador bajo autorización del owner, sesión 2026-09-14).

## Sources

- `docs/validation/M8/07.md` — decisión D14 vinculante y alcance ADR-087.
- `docs/validation/M8/07-release-rehearsal.json` — ensayo local, hallazgo y
  corrección de `TOOL_SCHEMA_SHA256`, presupuesto de tamaño de archive.
- [ADR-047](ADR-047-publication-license-and-delivery.md),
  [ADR-048](ADR-048-0.1.0-qualification-and-artifact-boundary.md) — OIDC,
  frontera de artifact y licencia que este ADR no sustituye.
- [ADR-087](ADR-087-1.0-host-scope.md) — alcance de host y artifact para
  0.8.0/1.0.
- [ADR-089](ADR-089-residual-risk-register.md) RR-12 — defecto heredado que
  este ADR y su implementación cierran.
- [ADR-041](ADR-041-authenticated-catalog-bundles.md) — protocolo de firma de
  catálogo separado del provenance de release.
- `docs/validation/M1/17-public-release.json` — cómo se calificó y verificó
  independientemente el release 0.1.0; patrón que RC1 repite para 0.8.0.
- `.github/workflows/release-candidate.yml`,
  `scripts/release-artifact.py`, `scripts/release-smoke.py`.
- `docs/roadmap/adr-backlog-m2-m8.md` §D14.
