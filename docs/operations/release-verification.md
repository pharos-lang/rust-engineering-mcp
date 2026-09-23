# Verificación de releases

Cómo se construye y verifica el artifact de release, qué hay publicado hoy,
y qué hacer ante un asset comprometido. Esta página no promete ninguna
publicación futura — describe únicamente lo que el mecanismo actual hace.

## Qué hay publicado

| Tag | Tools | Perfil |
| --- | --- | --- |
| `v0.1.0` | 13, todas `stable` | Archive `core`, único host `aarch64-apple-darwin` |
| `v0.3.0` | 31, todas `stable` | Archive `core`, mismo host |

El checkout de desarrollo actual anuncia 36 tools (31 `stable` + 5
`preview`) y **no corresponde a ninguna versión publicada** — es
checkout-only (ver [`../reference/compatibility.md`](../reference/compatibility.md#versión-y-contrato)).
Cada release publicada es un único archive `core` para
`aarch64-apple-darwin`: no incluye modelo, ORT, LanceDB, catálogo, trust,
fixtures, Docker ni toolchain — el perfil `local` completo se califica desde
fuente, no se distribuye como binario.

## Cómo se construye

`.github/workflows/release-candidate.yml` es **solo dispatch manual**, y
exige un tag `vX.Y.Z` o `vX.Y.Z-rc.N` ya existente antes de correr. Tres
jobs:

1. **`validate-ref`** (`ubuntu-latest`): confirma que el tag coincide con la
   versión del workspace y con el formato esperado.
2. **`build`** (`macos-26`, permisos `id-token: write` + `attestations: write`
   **solo en este job**): construye el archive `core` — que empaqueta dentro
   de sí `inventory.json`, el SBOM SPDX (`sbom.spdx.json`) y las notices de
   terceros (`THIRD_PARTY_NOTICES.txt`; ver `scripts/release-artifact.py:568-570`) —
   además de `SHA256SUMS` y `release-smoke-receipt.json`, instala y ejecuta el
   archive producido (`version`, `doctor` pasivo, discovery, las tools
   esperadas, denegaciones estructuradas) y crea la atestación de
   build-provenance de GitHub OIDC (`actions/attest-build-provenance`) sobre
   el archive, `SHA256SUMS` y el recibo de smoke, antes de publicar nada.
   Ese mismo job compila `build.rs` de las dependencias (Cargo lo exige) antes
   de atestar — ver RR-10 en
   [`execution-and-security.md`](../architecture/execution-and-security.md#registro-de-riesgos-residuales-de-10).
3. **`draft`** (`ubuntu-latest`, permisos `contents: write` **solo aquí**):
   publica un **prerelease en borrador** — nunca una release publicada
   automáticamente. Promoverlo a release pública es una acción manual
   separada del operador, después de verificar de forma independiente la
   descarga, los hashes y las atestaciones.

Esta separación de permisos por job es deliberada: **la credencial de
publicación no es un secreto de larga duración que rotar** — es el token
OIDC efímero emitido a ese workflow, gateado por la existencia previa del
tag. Ese alcance es solo el de publicar; no cubre toda la cadena de
suministro del build: `SONAR_TOKEN` (CI, no este workflow) sigue siendo un
secreto de terceros de larga vida, y el mismo job que atesta compila
`build.rs` de dependencias antes de firmar. Ver RR-10 y RR-12 en
[`execution-and-security.md`](../architecture/execution-and-security.md#registro-de-riesgos-residuales-de-10).

Scripts que implementan cada paso: `scripts/release-artifact.py` (build +
inventario + SBOM + notices + checksums) y `scripts/release-smoke.py`
(instalación + ejecución del archive). Ambos tienen su propia suite unitaria
(`scripts/test-release-artifact.py`, `scripts/test-release-smoke.py`),
ejecutada en `scripts/gate.py core` — ver
[`../development/testing.md`](../development/testing.md).

## Cómo verificar un artifact descargado

Cada release publica exactamente tres assets junto al tag: el archive
`.tar.gz`, `SHA256SUMS` y `release-smoke-receipt.json` — confirmado contra
`gh release view v0.3.0 --json assets` y el conjunto `expected` que exige el
job `draft` de `release-candidate.yml`. **No hay ningún asset de SBOM,
notices ni bundle de atestación publicado por separado.** El SBOM
(`sbom.spdx.json`), las notices (`THIRD_PARTY_NOTICES.txt`) y `inventory.json`
existen únicamente **dentro** del archive extraído
(`scripts/release-artifact.py:568-570`); la atestación de build-provenance no
se publica como asset descargable — vive en el registro de atestaciones de
GitHub para el repositorio y se consulta en línea con `gh attestation verify`.

Dos rutas de verificación, ninguna afirma reproducibilidad bit-a-bit del
binario:

**Con `gh` (autentica al publisher; requiere red — consulta la API de
GitHub, no solo claves cacheadas):**

```sh
gh attestation verify rust-engineering-mcp-v0.3.0-aarch64-apple-darwin.tar.gz \
  --repo pharos-lang/rust-engineering-mcp \
  --signer-workflow pharos-lang/rust-engineering-mcp/.github/workflows/release-candidate.yml
```

Este es el mismo comando (con el mismo flag `--signer-workflow`) que el propio
job `build` ejecuta sobre sus tres artifacts antes de transferirlos a
`draft` (`release-candidate.yml:149-165`). Es reproducible localmente contra
`v0.3.0`: sin `--signer-workflow`, `gh attestation verify --repo --format json`
sobre el mismo archive confirma que `subjectAlternativeName` referencia
exactamente ese workflow, lo que respalda la equivalencia. Sin
`--signer-workflow`, `gh attestation verify --repo` acepta la atestación de
**cualquier** workflow del repositorio que haya publicado una — no fija el
workflow firmante exacto; con el flag, la verificación falla si el firmante
no es `release-candidate.yml`. `gh attestation verify` **hace una llamada de
red a la API de GitHub** para listar las atestaciones del subject; no es una
operación offline. Repite el comando para `SHA256SUMS` y
`release-smoke-receipt.json` si quieres verificar esos dos assets también.

**Verificación offline (bundle descargado): no está documentada como
ejercida hoy.** `gh attestation download` seguido de
`gh attestation verify --bundle <bundle-local>` es una ruta que `gh` soporta
en general, pero no se ha probado en este repositorio; no la presentes como
un procedimiento verificado hasta ejercitarla. ADR-090 §2 describe esta ruta
como parte de la política; su estado real es "no implementada/no ejercida"
(ver `decisions.md`).

**Sin `gh` (solo integridad de bytes, sin autenticar al publisher):**

```sh
shasum -a 256 -c SHA256SUMS
```

seguido de inspeccionar `inventory.json` dentro del archive extraído para
confirmar target y versión. Esta ruta confirma que los bytes instalados
coinciden con los publicados en el momento de la descarga — **no** autentica
quién los publicó, a diferencia de `gh attestation verify`.

Ningún camino afirma reproducibilidad bit-a-bit del binario Mach-O universal;
solo hay determinismo sobre el toolchain Darwin ARM64 qualified del propio
workflow.

## Reproducción local del build de release

El mismo camino que ejecuta el workflow se reproduce en local con los dos
scripts de release, sobre un binario `aarch64-apple-darwin` compilado desde el
commit del tag:

```bash
python3 scripts/release-artifact.py --binary target/release/rust-engineering-mcp \
  --target aarch64-apple-darwin --tag vX.Y.Z --output-dir dist
python3 scripts/release-smoke.py --archive dist/rust-engineering-mcp-vX.Y.Z-aarch64-apple-darwin.tar.gz \
  --sha256sums dist/SHA256SUMS --tag vX.Y.Z --target aarch64-apple-darwin \
  --output-receipt dist/release-smoke-receipt.json
```

`--tag` es `vX.Y.Z` con `X.Y.Z` igual a la versión del workspace. Los scripts
de reproducción de los candidatos locales de `0.1.0` se retiraron por no aplicar
a la cadena de release actual; siguen en el historial de Git en
[`docs/release/reproduction/`](https://github.com/pharos-lang/rust-engineering-mcp/tree/51fa602e/docs/release/reproduction).

## Licencias de dependencias upstream

`licenses/upstream/` (movido desde
[`docs/release/upstream-licenses/`](https://github.com/pharos-lang/rust-engineering-mcp/tree/51fa602e/docs/release/upstream-licenses)
por la misma decisión D2, bytes idénticos) contiene los textos de licencia de cada
dependencia empaquetada en el archive (por ejemplo `google--flatbuffers`,
`microsoft--onnxruntime`, `modelcontextprotocol--rust-sdk`,
`intfloat--multilingual-e5-small`), con un `README.md` y `receipt.json`
propios que documentan la procedencia de cada texto.

## Respuesta a un asset comprometido

1. **Credencial de publicación**: la única es el token OIDC efímero del
   workflow, con scope mínimo (`id-token`/`attestations: write` solo en
   `build`, `contents: write` solo en `draft`), gateado por la existencia
   previa de un tag `vX.Y.Z`/`vX.Y.Z-rc.N`. Para *esa* credencial no hay
   secreto de larga duración que rotar. Esto no cubre toda la superficie:
   `SONAR_TOKEN` sí es un secreto de terceros de larga vida (RR-12), y el
   job que atesta compila `build.rs` de dependencias antes de firmar (RR-10)
   — ver [`execution-and-security.md`](../architecture/execution-and-security.md#registro-de-riesgos-residuales-de-10).
2. **Si un asset publicado está comprometido**: se retira de la GitHub
   Release, se publica un advisory según [`../../SECURITY.md`](../../SECURITY.md),
   y se corta un tag nuevo desde source limpio. Las atestaciones existentes
   están ligadas por `subject-path` al digest exacto del archive
   comprometido — ningún digest nuevo puede reutilizar esa atestación, y esa
   atestación nunca valida un digest nuevo. No existe otro mecanismo de
   "revocación" para Sigstore más allá de dejar de distribuir el asset y
   publicar el advisory.
3. **Separación de la firma de catálogo**: la firma Ed25519 de bundles de
   catálogo (ver [`catalog-maintenance.md`](catalog-maintenance.md) y
   [`../reference/data-formats.md`](../reference/data-formats.md)) es un
   protocolo completamente distinto — firma bundles de catálogo, no assets
   de release — con su propio archivo de confianza y su propia rotación por
   reemplazo de clave pública, independiente de GitHub OIDC/Sigstore.

## Limitaciones documentadas

- **Verificación de release sobre un tag real, parcialmente pendiente.** El
  ensayo local de build/checksum/SBOM/notices/smoke está completo y pasó,
  pero el ciclo íntegro
  `tag → run → digest → attestation → verificación independiente` sobre un
  tag publicado real todavía no se ha ejecutado de punta a punta en este
  corte.
- **`cargo-semver-checks` no forma parte de ningún pipeline de release.** La
  estabilidad de contrato SemVer se rastrea manualmente vía el
  diff de freeze (ver [`../reference/compatibility.md`](../reference/compatibility.md#congelación-de-contrato-080-y-verificación)),
  no con un chequeo automático de la API pública del binario en CI.
- **Sin firma de código ni notarización de Apple.** La única provenance es
  `SHA256SUMS` + la atestación de build OIDC; no hay certificado de firma de
  código adicional sobre el binario.
- El workflow `.github/workflows/release-candidate.yml` produce siempre un
  **prerelease draft**, nunca una publicación automática — cualquier
  promoción a release pública es una decisión manual explícita del
  operador, fuera de este workflow.
