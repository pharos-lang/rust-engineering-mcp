# M4 — Security / 0.4.x

Estado: **Planned**. Entrada M3 cerrado. Fuentes: spec §27/35–47/48/81/97 M4,
[ADR-009](../adr/ADR-009-deny-by-default-security.md),
[ADR-038](../adr/ADR-038-owned-rustsec-audit.md),
[ADR-041](../adr/ADR-041-authenticated-catalog-bundles.md).
Aplican [G1–G9](m2-m8.md). Resultado: respuestas distintas para vulnerabilidad
conocida, policy de dependencias, presencia de unsafe, UB observado y facts de
supply chain, sin confundir hallazgos con certificación de seguridad.

## Contrato, ownership y antiobjetivos

Tools propuestas: `rust.deny`, `rust.unsafe.scan`, `rust.miri`,
`rust.supply_chain.inspect`. `rust.dependencies.audit` conserva su contrato y
motor RustSec. D19 asigna advisories al port existente; deny compone esa misma
observación y ejecuta licenses/bans/sources sin un segundo matcher/advisory refresh.
Supply chain combina hechos, no fabrica un score de seguridad ni una aprobación
legal. Config de proyecto solo restringe host. Sin remediation, fix automático,
descargas, MIRIFLAGS libres, policy universal de unsafe o ejecución host.

## Cortes verticales

| ID | Flujo end-to-end | Depende de | Evidencia/gate | Tamaño |
| --- | --- | --- | --- | --- |
| M4-01 | deny request→captura única→audit compartido+licenses/bans/sources→findings | M3, D19 | Advisory idéntico audit, ban/license/source, supresión expirada y dataset stale | L |
| M4-02 | source autorizado→scanner syntax→unsafe/extern spans por origen | 01, D20 | Comentario/string/cfg/macro/workspace/dependency discriminados | L |
| M4-03 | Miri request→nightly/sysroot fijado→sandbox→UB/unsupported/timeout | 01, D21 | UB real y control limpio, FFI, loop/cancel/descendiente | XL |
| M4-04 | grafo capturado→audit/deny/catalog facts→supply-chain report | 01/02 | Yanked known/unknown, git/source/checksum, duplicate/features y freshness | L |
| M4-05 | quality strict/release→una captura→etapas→veredicto completo | 01/03/04, M3, D19 | Sin audit duplicado, baseline SemVer explícito y partial no-pass | L |
| M4-06 | threat model→hardening→abuse fixtures→review→gate | 01–05, D21 | G1–G9, secrets/artifacts/poisoning/escape, recalibración runtime | XL |

Camino crítico: fuentes/policy→deny→supply chain→gates; runtime Miri y hardening
son otra rama obligatoria antes de cierre. Tamaño XL; no fechas. Cada hardening se
ata a abuso reproducible, efecto concreto y control positivo; no “sandbox mejorado”.

## Semántica y arquitectura

Domain: findings con source/rule/severity/coverage/suppression, unsafe locations,
Miri outcome y graph facts tipados. Application reutiliza DependencyAuditPort,
Task execution y captura única; ports nuevos solo para scanner/Miri/deny reales.
Execution adapter usa programas/args cerrados y runtimes inmutables. Catalog
adapter sigue autoridad SQLite, RustSec fuente independiente y freshness por input.
MCP añade schemas propios y CLI inventario/doctor de tools opcionales, sin ampliar
config implícita. No tocar schemas M1 al extender perfiles; decidir DTO/perfiles
nuevos en D19 y conservar fast/standard exactamente.

Deny: [checks oficiales](https://embarkstudios.github.io/cargo-deny/checks/index.html).
Cada subcheck tiene fuente/fecha/versión/config y cobertura. Licencias requieren
source bytes offline verificados (D05), no inferir licencia de código de un string
del manifest. Unknown no es licencia permitida. Suppression propuesta: rule/source/
package version range/reason/owner/expiry/policy digest. Sin suppression global
silenciosa, ni ignorar todas las vulnerabilidades. Expirada/malformada se rechaza;
se informa hallazgo original y disposición, manteniendo audit M1 intacto.

Unsafe: ubicar unsafe blocks/fn/impl y extern, separar workspace/dependencies;
presencia no implica defecto ni ausencia implica seguridad. D20 compara AST/parser
propio limitado con scanner externo exacto; regex sola no distingue strings/macros.
Declarar cfg, expansión de macros y generado no cubierto. Sin ejecución para
expandir macros salvo permiso/sandbox separado. Fixture con falso positivo en
comentario y unsafe real de mismo texto discrimina semántica.

Miri: [repo oficial](https://github.com/rust-lang/miri), versión nightly/commit/sysroot
y digest decididos en D21. No setup automático. Clasificar UB/test failure/compile
failure/unsupported operation/timeout; modo limpio no prueba ausencia universal
de UB. No permitir flags que quiten isolation. Test fixtures use-after-free,
uninitialized/aliasing/race, limpio, FFI unsupported y cfg(miri), con resultado
esperado fijado contra el binario real. No correr ejemplos hostiles en el host.

Supply chain: captured lock graph+metadata, sources/checksums/duplicados/git,
audit y deny, yanked del catálogo con fuente/freshness, features declaradas vs
activas. ADR-044 no guarda package source/checksum/doc URLs completos: unknown
permanece unknown o D22 introduce schema/adquisición aparte. No convertir advisory
IDs de catálogo en auditoría. Ausencia de catálogo no borra findings útiles ni
produce “clean”. URL con credenciales se redacta, sin resolverla por red.

Gates propuestos en D19: strict=standard+deny+coverage; release=strict+semver con
baseline ProjectRef explícito. Mutation solo opt-in con presupuesto, nunca default.
Captura actual única para todas etapas y baseline separado identificado; audit se
reutiliza una vez. Composición application no llama tools MCP entre sí.

## Threat model, operación y distribución

Amenazas: dependencia/plugin comprometidos, malicious build.rs/proc macro,
catálogo/model poisoning, source confusion, suppression amplia/expirada, parser
hostil, secretos en artifacts y escape/quota. Mantener no-follow y red deny real,
env reconstruido, process tree cleanup y auditoría G2/G3. Runtimes nuevos exigen
calibración nueva: evidencia vieja del image M1 no autoriza Miri.

Hardening mínimo M4-06: secret canaries en logs/diagnósticos/HTML/diffs antes de
publicar; firma/hash/sequence/source mismatch de catálogo; binario plugin digest
alterado; attempts socket/cloud metadata/namespace/mount; orphan/fork/disk/output
bombs. Documentar limitación de secret scanning, allocator nativo y ACL/privileged
host; cerrar claims que no tengan enforcement, no parchear el threat model con
palabras. Revocación de tool/source bloquea nueva admisión y conserva auditoría.

Budgets: heredar M3 para artifacts/jobs, M1 para captura/grafo; defaults propuestos
scan/deny 120 s, Miri 300 s/máximo 1800 s, 128 findings visibles, 512 KiB result.
Omisiones/coverage quedan explícitas, paging owner-bound si existe artifact.
SLI: findings por fuente, stale/unknown, suppression vencida, incomplete,
timeouts/cleanup y scan/redaction bytes. No telemetría externa ni “cero findings”
como SLO de seguridad. Riesgo de falsos positivos se mitiga con fixture/origen y
supresión auditable, no desactivando controles.

Tests unit/contract/protocol/integration/security/native/performance y clientes G4;
fixtures reales RustSec/SQLite, scanner/plugin/Miri. Linux/Windows portable solo;
macOS/APFS y guest de runtime nuevo se califican positivamente. Pin/lock de nuevas
dependencias requiere auditoría, licencias/notices, SBOM/provenance y smoke de
bytes distribuidos. Security bundle opcional solo por demanda/ADR. No claves ni
catálogo oficial. Rollback deshabilita plugin/source comprometido, cuarentena,
vuelve a formatos compatibles sin retroceder floor y retestea antes de admisión.

## DoR, DoD y aceptación

DoR: M3 cerrado; D19–D22 preparados/decididos según efecto, datasets frescos con
origen, plugins y nightly exactos aprovisionados, suppressions/fixtures/budgets
cerrados. DoD: seis cortes y G1–G9, Sonnet por contrato, Opus High threat/sandbox,
sin P0/P1 ni P2 de seguridad/evidencia obligatoria. No cierre solo con Miri denied.

- [ ] Cada pregunta tiene una tool/motor dueño y deny no duplica vulnerabilidades
  ni refresh de audit. Fuente: spec §27, ADR-038; M4-01/04, D19.
- [ ] Unsafe se reporta por origen/cobertura; Miri distingue UB/unsupported y pasa
  casos reales adversos/limpios. Fuente: spec §27.2/27.3; M4-02/03.
- [ ] Suppressions, freshness, fuentes faltantes y datos desconocidos son visibles,
  sin clean parcial. Fuente: ADR-020/038/044; M4-01/04/05.
- [ ] Threat review, secret-canary y poisoning/containment tienen controles reales
  y cleanup probado. Fuente: spec §36/81, ADR-009/041; M4-06.
- [ ] Full gate, clientes, inventario/license/SBOM/provenance y distribución por
  target coinciden con claims. Fuente: G4/G5/G7/G8 y ADR-048.

Handoff: gates y source hashes, fuentes/policy/suppressions, runtime Miri, findings
dispuestos y límites de inferencia; detener antes de M5.
