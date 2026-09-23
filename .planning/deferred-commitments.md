# Compromisos pendientes — registro versionado

Estado: **vivo**, fecha 2026-09-23, baseline `51fa602e`.

Este registro reúne, con cita textual y permalink a `51fa602e`, los compromisos
que hoy solo viven en documentos que la limpieza de documentación retira
([`docs/implementation-status.md`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/implementation-status.md),
[`docs/validation/M8/checklist-1.0.md`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/validation/M8/checklist-1.0.md),
[`docs/roadmap/m7-remote.md`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/roadmap/m7-remote.md),
[`docs/roadmap/m7-g0-decision.md`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/roadmap/m7-g0-decision.md),
[`docs/roadmap/m8-stabilization.md`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/roadmap/m8-stabilization.md),
[`docs/adr/ADR-089-residual-risk-register.md`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-089-residual-risk-register.md),
[`docs/adr/ADR-087-1.0-host-scope.md`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-087-1.0-host-scope.md)).
No inventa obligaciones, fechas ni
decisiones; no marca nada como cerrado. Cada sección cita su origen, el estado
tal como lo registra ese origen, el criterio de cierre o condición de
reapertura tal como está escrito, y el plan pendiente que lo posee, si existe.
Los planes pendientes activos son
[`.planning/implement-1.0-runtime-portability-astra.md`](implement-1.0-runtime-portability-astra.md)
y [`.planning/implement-m7.md`](implement-m7.md); ningún otro documento vigente
sustituye a este como fuente de estas obligaciones.

Base de permalinks: `https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/`.

## 1. M7 — Remoto (0.7.x): Deferred, condiciones de reapertura

- **Origen:** [`docs/roadmap/m7-g0-decision.md`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/roadmap/m7-g0-decision.md),
  [`docs/roadmap/m7-remote.md`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/roadmap/m7-remote.md).
- **Estado actual:** *"Deferred con decisión"* (2026-09-13). *"No-go a la
  ejecución remota de M7 (0.7.x); M7 queda Deferred con decisión registrada. No
  se implementa ninguno de los cortes M7-01..06 ni se inicia diseño de
  HTTP/OAuth/tenancy/executor remoto."* Motivo registrado: no existe expediente
  con caso remoto real aprobado.
- **Criterio de reapertura (cita textual, m7-g0-decision.md §Reversibilidad):**
  *"`Deferred` no implica que el remoto sea imposible. Una reevaluación futura
  requiere el expediente M7-G0 completo (caso real, métricas medidas,
  comparativa con alternativas, SLO/cuotas/retención, IdP/executor reales) y
  una nueva aceptación explícita del owner; recién entonces se decidiría Go y
  se ejecutarían M7-01..06 con G1–G9."* El expediente exige, según
  m7-remote.md §Objetivo: owner/operador real, tarea y frecuencia, usuarios,
  datos/source y residencia, clientes/versiones, concurrencia medida, costo y
  límites, SLO propuesto, alternativa stdio/local/SSH/devcontainer/runner
  administrado comparada con razón verificable de insuficiencia, medición
  proxy y diseño prospectivo del piloto. *"Go exige todos los elementos;
  ausencia de uno mantiene Deferred."*
- **Plan que lo posee:** [`.planning/implement-m7.md`](implement-m7.md)
  (plan autosuficiente, deferred; no se ejecuta sin Go).
- **Regla adicional aplicable:** ver §7 (ADR-089) — anunciar el transporte
  remoto activa la reevaluación de RR-01/02/05/06/08/11.

## 2. Checklist 1.0 — filas abiertas (2, 6, 7, 10, 11)

- **Origen:** [`docs/validation/M8/checklist-1.0.md`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/validation/M8/checklist-1.0.md).
- **Estado actual (cita textual, resumen del propio documento):** *"Resumen: 6
  demostrados, 3 parciales, 2 pendientes → not ready hasta cerrar 6, 7, 10 y
  11."* El propio checklist añade, fechado 2026-09-18: *"la readiness sigue
  siendo not ready, que es lo esperado hasta RC2."* Esa nota del checklist da
  por hecho que RC1 (`v0.9.0-rc.1`) ya se cortó con un tag real; al
  2026-09-23 ese tag no existe en `origin` (`git ls-remote --tags origin`,
  `gh release list`: solo `v0.1.0`/`v0.3.0` — ver fila 11 abajo y §3). La
  readiness sigue **not ready** en cualquier caso; la decisión de readiness
  es del Technical Owner.

  | Fila | Criterio | Estado registrado | Qué lo cierra (cita) |
  | --- | --- | --- | --- |
  | 2 | Security model = enforcement; cada capability con oráculo nativo; auditoría independiente sin P0/P1 | **Parcial**: 6 controles sin oráculo declarados (RR); auditoría = revisión de modelo (V04, pendiente de veredicto); sin auditoría humana (RR-01) | [`08-threat-model.md`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/validation/M8/08-threat-model.md) §6, [ADR-089](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-089-residual-risk-register.md) |
  | 6 | CLI estable, exit codes/JSON/config/permissions y recuperación reproducidos por tercero | **Pendiente** (W29 en curso: reproducción por tercero) | Recibo `06-reproduction.md` completado por el tercero |
  | 7 | Guías de integración probadas por cliente/versión/perfil; docs sin texto histórico contradictorio | **Demostrado con límite declarado** (mantenido abierto por el resumen del propio checklist) | *"la composición de arneses M2–M6 no es repetible con los clientes actuales"* — cerrar exige arneses M2–M6 repetibles con los clientes vigentes, no solo recibos por tool sobre contratos byte-idénticos |
  | 10 | Signing/provenance/checksums/SBOM/notices verificados desde assets descargados | **Parcial**: ensayo local completo; *"attestations OIDC y redescarga exigen tag/release (RC1). Bloqueante de RC1 (V04 F-07): D14 verificado sobre assets reales descargados de un tag, no solo el ensayo local"* | Verificación de D14 sobre bytes descargados de un tag real (no solo ensayo local) |
  | 11 | Dos RC consecutivos con mismo contrato, full gate y soak verdes dentro de budgets, sin skips como pass | **Pendiente** (M8-09). El checklist registra RC1 (`v0.9.0-rc.1`) como cortado el 2026-09-18, pero al 2026-09-23 no existe ningún tag `v0.9.0-rc.1` ni release en `origin` (`git ls-remote --tags origin`, `gh release list` — solo `v0.1.0`/`v0.3.0`). RC1 debe re-cortarse o su ausencia explicarse antes de que las filas 10/11 puedan avanzar; *"RC2 exige autorización aparte"* | (a) soak 8h/1000 ciclos, nunca ejecutado — decisión del owner (2026-09-18): antes de 1.0.0; (b) `gate.py full` completo (hoy 31 pasadas/1 fallida/14 sin ejecutar por borrado accidental de tres imágenes Docker); (c) re-cortar o explicar RC1, y entonces RC2 autorizado y verde junto con RC1 |

- **Regla de gobierno para la fila 2 (ADR-089, Decision §3, re-review
  obligatoria en M8-08):** tras la auditoría independiente y sus
  correcciones, cada `RR-n` se confirma, se reclasifica o se cierra con
  evidencia; **cualquier P0/P1 de seguridad nuevo bloquea readiness y nunca
  se añade como riesgo aceptado**. Cualquier P2 de seguridad sigue la regla
  de bug bar del plan: **bloquea readiness salvo disposición explícita del
  owner**. Esta regla gobierna cómo se cierra la fila 2, no solo el
  registro RR-01…RR-19.
- **Plan que lo posee:** sin encargo. `AGENTS.proposed.md` registra M8
  stabilization como integrada y dice *"la preparación 1.0 (M8-09) sigue
  pendiente, con sus compromisos en .planning/deferred-commitments.md"*
  (esa cita no existe en `AGENTS.md`, cuyo alcance vigente aún no se
  actualizó — ver la propuesta pendiente de aplicación por el owner);
  ningún plan pendiente versionado asume ejecutar las filas 2/6/7/10/11 o
  RC2 sin nueva autorización del owner.

## 3. RC2 (0.9.0) — autorización pendiente

- **Origen:** [`docs/validation/M8/checklist-1.0.md`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/validation/M8/checklist-1.0.md)
  fila 11, [`docs/roadmap/m8-stabilization.md`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/roadmap/m8-stabilization.md)
  corte M8-09.
- **Estado actual:** el checklist registra RC1 (`v0.9.0-rc.1`) como cortado
  el 2026-09-18, pero al 2026-09-23 no existe ningún tag `v0.9.0-rc.1` ni
  release en `origin` (verificado con `git ls-remote --tags origin` y
  `gh release list`: solo `v0.1.0` y `v0.3.0`). RC1 debe re-cortarse o su
  ausencia explicarse antes de progresar. *"RC2 exige autorización aparte"*;
  no está autorizado en ningún caso. No confundir con las RC de
  `1.0.0-rc.*` del plan de runtime/portabilidad (§5), que son un programa
  distinto (`RUP-1.0`) sobre una numeración diferente.
- **Criterio de cierre:** autorización explícita del owner para RC2; entonces
  ejecutar soak 8h/1000 ciclos y `gate.py full` completo (ver fila 11 en §2);
  *"Dos RC consecutivos con mismo contrato, full gate y soak verdes dentro de
  budgets, sin skips como pass"* (m8-stabilization.md, checklist ítem
  correspondiente).
- **Plan que lo posee:** sin encargo.

## 4. Actualización de paquetería post-M8 (incl. `lancedb`)

- **Origen:** [`docs/roadmap/m8-stabilization.md`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/roadmap/m8-stabilization.md)
  §«Tarea post-M8 — actualización de paquetería (decisión del owner,
  2026-09-10)».
- **Estado actual (cita textual):** *"Cuándo: después de cerrar M8; no antes,
  y nunca dentro de una ventana de cierre de milestone. Qué: evaluar y, donde
  sea posible, subir las versiones de la paquetería del workspace, incluida
  `lancedb`, con un ADR sucesor de ADR-027 cuando toque la capa semántica."*
  `lancedb` permanece pineado en `=0.31.0` (Lance 8) tras revertir la subida a
  `0.38.0` durante el cierre de M5 por incompatibilidades documentadas
  (`job.rs`/`Error::Http`, `LocalSpillStore` eager de `lance-io ≥ 11`
  incompatible con `memory://`, patch vendor y `tinyvec` 1.12.0). Otros
  paquetes con implicaciones documentadas: `arrow-array`/`arrow-schema`
  (`=58.4.0`, deben coincidir con lo que exija `lancedb`/`fastembed`),
  `fastembed`+`ort` (`=6.0.3`/`=2.0.0-rc.13`), `rmcp` (`=3.2.0`), `jsonschema`
  (`=0.55.1`), `tokio`/`tokio-util`/`tokio-rustls`/`reqwest`/`ring`/`rustix`,
  `rusqlite` (`=0.40.2`), `rustsec`/`cargo-lock` (`=0.32.0`/`=11.0.1`), y
  varios pins `=` de serialización (`serde`, `schemars`, `toml_edit`, etc.).
- **Criterio de cierre (cita textual, Definition of Done):** *"ADR con la lista
  de subidas aceptadas y rechazadas y sus motivos; `cargo audit`/`cargo deny`
  sin nuevas advertencias; `core` y `full` aprobados sobre el nuevo lock;
  matrices de clientes repetidas para lo que toque protocolo; documentación
  pública sincronizada."*
- **Plan que lo posee:** sin encargo.

## 5. RR-12 — cierre parcial (provenance de publicación)

- **Origen:** [`docs/adr/ADR-089-residual-risk-register.md`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-089-residual-risk-register.md)
  registro RR-12, cuyo propio texto (citado abajo) registra que solo está
  **parcialmente** cerrado.
- **Estado actual (cita textual, RR-12):** *"Publicación: la attestation OIDC
  acredita el workflow, no reproducibilidad; `SONAR_TOKEN` es un secreto de
  larga vida de un servicio de terceros. (F-07, V04: re-observar branch
  protection, ejecutar la provenance de 0.8.x y verificar D14 sobre assets
  reales dejan de ser riesgo aceptado y pasan a ítems bloqueantes de RC1 en
  checklist-1.0.md filas 10 y 11)"*. Alcance: artifacts y repositorio público.
  Condición de reevaluación registrada: *"Cualquier cambio de workflows o de
  protección."*
- **Criterio de cierre:** el mismo que las filas 10 y 11 del checklist 1.0
  (§2 de este registro) — D14 verificado sobre assets reales descargados de un
  tag, y RC2 verde consecutivo con RC1 una vez RC1 exista realmente en
  `origin` (hoy no hay tag ni release `v0.9.0-rc.1`; ver §2 fila 11 y §3).
- **Plan que lo posee:** sin encargo (ligado a los mismos ítems de §2/§3).

## 6. Decisiones pendientes del tablero histórico

- **Origen:** [`docs/implementation-status.md`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/implementation-status.md)
  §«Decisions Pending».

  | Decisión (cita) | Momento límite (cita) | Gate (cita) | Plan que lo posee |
  | --- | --- | --- | --- |
  | Catálogo oficial futuro | Antes de una release que lo distribuya | Nueva decisión de fuente/términos y procedimiento de custodia/rotación/revocación; no aplica a 0.1.0. | Sin encargo |
  | Soporte positivo adicional por OS | Antes de anunciar otro target | Adapter protegido y security tests nativos; CI portable no basta. | [`.planning/implement-1.0-runtime-portability-astra.md`](implement-1.0-runtime-portability-astra.md) (fases MAC/LINUX/WINDOWS) |
  | Distribución futura del perfil `local` | Antes de empaquetar E5/ORT/LanceDB | Licencias/notices y recibos nativos completos; excluido de 0.1.0. | Sin encargo |

- **Contexto del alcance de hosts (origen adicional):**
  [`docs/adr/ADR-087-1.0-host-scope.md`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-087-1.0-host-scope.md)
  fija *"1.0 se califica y publica para un único host positivo: macOS ARM64
  (macOS 26/APFS) con el gateway Docker Linux ARM64 fijado"*; Linux x86_64 y
  Windows x86_64 quedan como *"targets de portabilidad de
  fuente/protocolo/fail-closed en CI, no calificados"*. Reapertura: *"Calificar
  una familia adicional en el futuro requiere un subprograma D13 propio:
  adapter [no-follow/reparse-safe], oráculos G4 nativos, un host real para el
  gate"* y, para Windows, además corregir la regresión de stdio
  pre-`initialize` retirada del CI el 2026-09-13.

## 7. ADR-089 — regla de reevaluación ante nuevo host, transporte remoto o catálogo oficial

- **Origen:** [`docs/adr/ADR-089-residual-risk-register.md`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-089-residual-risk-register.md)
  §Decision punto 4 y §Consequences.
- **Regla (cita textual, Decision §4):** *"Una condición de reevaluación
  cumplida suspende la aceptación de ese riesgo hasta un ADR que lo reevalúe.
  Anunciar un host adicional, un transporte remoto o un catálogo/modelo
  oficial activa a la vez RR-01 y los riesgos de su alcance."*
- **Regla (cita textual, Consequences):** *"Anunciar un host, un transporte
  remoto o un catálogo/modelo oficial requiere un ADR que reevalúe como mínimo
  RR-01, RR-02, RR-05, RR-06, RR-08 y RR-11."*
- **RR-01 en detalle (cita, Registro):** la condición de reevaluación de RR-01
  es *"Antes de anunciar un host distinto de macOS ARM64, un transporte remoto
  (M7) o un catálogo/modelo oficial (D15/D16); ante un P0/P1 reportado tras la
  release; si el owner contrata una auditoría humana"* — es decir, cualquiera
  de esos tres anuncios exige además evaluar si la revisión de modelo (Opus 5
  High, read-only) sigue siendo suficiente o si corresponde una auditoría
  humana.
- **A quién aplica hoy:** esta regla es una condición de entrada para ambos
  planes pendientes de este registro:
  - [`.planning/implement-1.0-runtime-portability-astra.md`](implement-1.0-runtime-portability-astra.md)
    anuncia hosts nuevos (Linux x86_64, Windows x86_64) en sus fases LINUX y
    WINDOWS.
  - [`.planning/implement-m7.md`](implement-m7.md) anunciaría el transporte
    remoto si algún día se aprueba un Go (§1).
  Ninguno de los dos planes puede saltarse el ADR de reevaluación de
  RR-01/02/05/06/08/11 al llegar a ese punto; este registro no lo autoriza por
  sí mismo.
- **Plan que lo posee:** ambos planes citados arriba deben cumplir esta regla
  como condición de entrada a su fase correspondiente; no existe un tercer
  plan que la ejecute de forma independiente.

## Mantenimiento de este registro

Este archivo se actualiza cuando cambie el estado citado en su origen (nueva
acta, ADR sucesor, cierre de una fila del checklist, autorización de RC2,
etc.), citando siempre la fuente vigente en ese momento. No se retira mientras
exista al menos un compromiso abierto; si todos se cierran, la retirada
requiere disposición explícita del Technical Owner, igual que cualquier otro
cambio de alcance documental.
