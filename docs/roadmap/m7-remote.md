# M7 — Remote / 0.7.x condicional

Estado: **Conditional**; ejecución y release **Deferred** hasta Go. No existe en
la evidencia revisada un caso remoto aprobado. No-go actual por falta de evidencia;
no implica que remoto sea imposible. Fuentes: spec §8.1, §97 M7,
[ADR-003](../adr/ADR-003-stdio-first.md), [ADR-023](../adr/ADR-023-mcp-stdio-bootstrap.md).
Aplican íntegros [G1–G9](m2-m8.md). No se inicia diseño detallado de HTTP antes de G0;
lo siguiente define decisiones y criterios que deberán resolverse si se aprueba.

## Objetivo, puerta y antiobjetivos

Resultado observable condicional: un actor identificado ejecuta una tarea remota
necesaria con autorización y aislamiento demostrados, sin ampliar permisos locales.
La alternativa válida es un acta Deferred y estabilización local en M8.

M7-G0 requiere un expediente con owner/operador real, tarea y frecuencia, usuarios,
datos/source y residencia, clientes/versiones, concurrencia medida, costo y límites,
SLO propuesto, alternativa stdio/local/SSH/devcontainer/runner administrado comparada,
razón verificable por la que esas alternativas no satisfacen la tarea, medición
proxy sobre stdio/SSH/devcontainer existente y diseño prospectivo del piloto con
éxito/refutación, sin exigir HTTP construido. El piloto HTTP se ejecuta después
del Go y antes de release M7-06; si las alternativas satisfacen la tarea o el
piloto no cumple, abortar promoción. Se exige aceptación explícita del owner.
Go exige todos los elementos; ausencia de uno mantiene Deferred. No inventar usuarios,
presupuesto, IdP, infraestructura ni fechas. M7-G0 tamaño M; no se estima ejecución
remota hasta delimitar el caso. M8 depende de la decisión, no de un Go.

Fuera: shell remoto, clones arbitrarios, credenciales del peer en procesos, plataforma
general de CI/CD, Authorization Server propio, HTTP activado por defecto y cualquier
requisito de disponibilidad no respaldado por operador/infraestructura.

## Cortes verticales si Go

| ID | Flujo end-to-end | Depende de | Evidencia y gate | Tamaño |
| --- | --- | --- | --- | --- |
| M7-G0 | Expediente→comparativa→decisión firmada por owner→Go o Deferred | Cierre M6 | Acta con métricas/fuentes y responsable; no código HTTP | M |
| M7-01 | Cliente autenticado de prueba→rmcp Streamable HTTP→discovery acotado | G0 Go, D07 | Wire real, TLS/Origin/Host/header/body adversarial; no tool con proyecto aún | L |
| M7-02 | Identidad→grant→proyecto preaprovisionado→project.open/inspect | 01, D08 | Dos tenants, issuer/audience/grant mismatch, revocación; proyecto ajeno invisible | XL |
| M7-03 | rust.check remoto→gateway aislado→diagnóstico/Resource tenant-bound | 02, D09 | Código hostil, egress, quotas, active cancellation y cleanup real | XL |
| M7-04 | Dos proyectos/tenants→admisión justa→jobs concurrentes y cancel | 03, D10 | Saturación, starvation, 429/retry, capacidad retenida hasta cleanup | L |
| M7-05 | Operación→auditoría/redacción→revocación/borrado→recuperación | 02–04 | Incident drill, fallo IdP/storage/executor, restore y evidencia privada | L |
| M7-06 | Deployment candidato→clientes reales→gates→rollback operativo | 01–05 | G1–G9, matriz HTTP/stdio, SBOM/notices/provenance y review Opus | L |

## Contrato, fronteras y decisiones previas

No tools nuevas obligatorias por transporte. Los contratos existentes permanecen;
la paridad se verifica por versión/operación/efectos. Autoridad remota requiere un
contexto tipado tenant/principal/client/grant/policy generation en application;
su introducción y revocación son D08, no una modificación implícita de ProjectRef M1.
Domain no conoce OAuth/HTTP/JWT. Adapter rmcp valida protocolo negociado; adapters
identity/authorization y executor remoto representan fronteras reales.
CLI `serve --http` y configuración host solo se diseñan tras Go y ADR; stdio continúa.
No transportar scopes como permisos irrestrictos del dominio.

Elegir operador-preprovisionado o ingreso autenticado con bytes verificados; nunca
path/URL del peer como autoridad. Staging/import conserva M2 no-follow y límites.
M7 excluye mutación remota de source/manifest/lock: solo consultas y validaciones
con source RO; los artifacts privados de ejecución sí se escriben en sandbox. No
anunciar paridad universal de tools mutables locales. Mutaciones remotas requieren
un alcance nuevo explícito, D13 con adapter positivo del target remoto y reuse de
la transacción/autoridad M2 recalificada allí; no basta portable CI.

Fuentes oficiales deberán fijarse por revisión al decidir D07/D08: [MCP transport](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports),
[Rust SDK fijado](https://docs.rs/rmcp/3.2.0/rmcp/),
[authorization MCP](https://modelcontextprotocol.io/specification/draft/basic/authorization).
El último enlace es draft mutable, no contrato aceptado. No adoptar requisitos
draft ni actualizar rmcp automáticamente.

## Threat model y operación obligatorios

Activos: source, grants, secretos, artifacts, catálogo privado y capacidad de compute.
Actores: cliente anónimo, usuario de otro tenant, proyecto hostil, operador y proveedor
de identidad. Cada request autentica/autoriza issuer exacto, audience/resource,
firma/algoritmo permitido, tiempo, scopes, tenant/project y policy generation.
Sin tokens en URLs/logs, passthrough a Git/registries o confianza en headers proxy
arbitrarios. Metadata/JWKS solo desde issuer aprobado con HTTPS/allowlist, límites,
freshness y comportamiento fail-closed; rotación y revocación ensayadas. Decidir
tokens opacos/introspección frente a JWT, requisitos OAuth de clientes y expiración
offline en D08. Una policy revocada impide nueva lectura/ejecución antes del TTL.

TLS y proxy trust explícitos; Origin/Host/CORS cerrado, DNS rebinding, request
smuggling, slowloris y límites de headers/body/decompresión/streams. Si la revisión
wire requiere headers de método/nombre, comprobar coherencia con envelope vía SDK.
Errores 401/403/429 y Retry-After no revelan existencia de otro proyecto. Plano
administrativo separado. SLI: rechazos por razón, latencia/cola p95/p99, fairness,
cleanup pendiente, edad de claves y uso por tenant sin datos sensibles.

Registry/cache/artifacts/scheduler/audit llevan autoridad de tenant, no solo un ID
aleatorio. Verificar sustituciones cruzadas en cada frontera. Cuotas jerárquicas por
IP anónima, identidad, tenant, proyecto, operación y pool global; valores salen del
expediente medido y son DoR, no “ilimitados hasta tener usuarios”. Idempotencia se
liga a tenant/principal/tool/inputs/generation, con TTL y política de replay.

Executor: imagen inmutable, job privado, no daemon socket/host mounts/cloud metadata,
sin secretos heredados, source RO, target/temp efímeros acotados,
seccomp/namespace/cgroups o alternativa nativa demostrada. Red deny real, CPU/RAM/
PID/disco/wall/output limitados. Disconnect/cancel/timeout terminan el árbol;
cleanup incierto pone executor en cuarentena. No aislar tenants solo con prefijos.
Catálogo compartido solo si datos/policy permiten compartirlo; SQLite sigue
autoritativo e índices privados no mezclan hechos entre tenants.

## Pruebas, compatibilidad, distribución y rollback

Fixtures con dos identidades y dos tenants, token vencido/issuer falso/audience
erróneo/scope insuficiente, key rotation/revocation, SSRF JWKS, cross-project URI,
replay, slow client, body enorme, build.rs/proc macro/test hostil y descendiente
desacoplado. Oráculos de denegación incluyen controles positivos que prueben que
el intento ocurrió; ausencia de tráfico sin intento no certifica egress deny.
Probar fallos de IdP, storage, executor y restart durante una operación remota en curso (jobs/artifacts).

Unit/contract/protocol/integration/security/adversarial/performance por G4; clientes
HTTP exactos más regresión stdio de cinco versiones existentes según negociación.
No todas las versiones legacy necesitan soportar HTTP: cada celda se declara y
una ausente no pasa. Compatibilidad y schema del estado remoto/versiones de tokens
se deciden antes de migrar. Backups de datos no revierten revocación ni floor.

Distribución remota separada del core: container/runtime inventariados, imagen por
digest, SBOM/notices, provenance, scanner y promoción/rollback por operador.
No hereda la calificación de Docker guest M1. Rollback ante fuga/escape/pérdida:
cerrar admisión, revocar grants afectados, cuarentenar jobs, conservar auditoría,
restaurar solo formatos compatibles y recalificar. No prometer HA ni disaster
recovery sin topology/SLO/runbook aprobados.

## DoR, DoD y criterios de aceptación

DoR: G9, M6 cerrado y M7-G0 Go, D07–D10 decididos, fixtures/IdP/executor reales,
cuotas/SLO/retención y responsabilidades aprobadas. DoD: M7-01..06 y G1–G9 con
review Sonnet de contratos y Opus High de auth/tenancy/containment. P0/P1 y P2 de
seguridad/evidencia obligatoria bloquean. Si no Go, DoD de la **decisión** es acta
Deferred; no se marca implementado M7.

- [ ] Un tercero encuentra expediente y aprobación antes de cualquier código HTTP;
  si faltan, no existe release 0.7.x forzada. Fuente: spec §8.1/§97 M7 y M7-G0.
- [ ] El mismo caso protegido produce resultado equivalente por stdio/HTTP y acceso
  cruzado/revocado se deniega sin filtrar datos. Fuente: ADR-003/007/009, M7-01/02.
- [ ] Cancel/timeout/disconnect sobre un hijo observado concluyen con ausencia de
  objetos y cuota liberada después de cleanup. Fuente: ADR-008/030, M7-03/04.
- [ ] Quotas/SLO/privacidad, restore y firma de bytes se verifican con fixtures reales
  e inventario por target, sin skips positivos. Fuente: spec §36–45/66/80–82, M7-05/06.

Handoff: acta Go/Deferred, commit/evidencia/reviews, riesgos y configuración del
operador; detener sesión antes de M8. Dependencia saliente: solo estado final
verificado de M7-G0/M7, nunca disponibilidad remota supuesta.
