# Validación de la planificación M2–M8

Estado: planificación aceptada para integración documental; no acredita ejecución M2.
Fecha: 2026-09-05. Base: `aa61bce44834df3154441f75bfbab726097ad0ff`.

## Lectura y baseline

Se leyeron AGENTS, la spec completa, documentación pública/operacional, los 48
ADRs, status, matrices M0/M1/M1-17, recibo de publicación y review final Opus.
Se inspeccionaron árbol real, manifests/lock, registry/snapshots, tests y workflows.
La [baseline](baseline-2026-09-05.md) conserva commits/runs históricos y live,
protección de main, hashes de archive/binario y verificación de tres attestations.
No se ejecutó el binario descargado ni se repitió el gate de implementación M1.

## Paquetes independientes

Workers GPT-5.6 Sol: High para M2/M6 y M3/M4/M5; Medium para M7/M8. Todos fueron
read-only respecto del repositorio y entregaron propuestas, no implementación.
El Technical Owner integró los cortes y descartó: estimaciones de calendario sin
capacidad conocida; HSM/doble releaser sin requisito; Tasks obligatorias antes de
verificar rmcp; preflight/hash/flock descritos como CAS o exclusión real; interfaces
vacías anteriores a una vertical. Las referencias de workers se contrastaron con
los nombres reales de los ADRs.

Claude Code `2.1.260`: Sonnet 5 High (`claude-sonnet-5`) y Opus 5 High
(`claude-opus-5`), print con tools vacías, configuración MCP estricta sin servidores,
sin persistencia de sesión y sin expansión de comandos. Disponibilidad Sonnet
verificada antes del paquete; la respuesta de cada review conserva modelo real.

`agy` lista `gemini-3.8-flash-high` como Gemini 3.8 Flash (High); ese es el modelo
seleccionado, sin sustitución por Gemini 3.1 Pro. Help no ofrece `--version`;
changelog empieza en 1.1.27 y el recibo de herramientas conserva el hash del CLI,
sin inferir que la primera nota pruebe la versión del ejecutable. La primera
invocación falló por `-p` sin argumento y no hizo review; el segundo intento terminó.
Se solicitó sandbox/read-only y no usar tools. `agy` avisó que `--mode plan` no tiene
efecto con expansión deshabilitada: no se presenta ese flag como enforcement de
solo lectura. El resultado informa una sola vuelta y no reporta acciones; el diff
posterior comprueba que no cambió producto. La respuesta declara revisión sin tools.

Los manifests siguientes fijan exactamente archivos y SHA-256 entregados a cada
revisor. Los hashes son anteriores a cualquier corrección posterior documentada;
no implican que el revisor leyera otros archivos o inspeccionara el runtime.

| Revisor | Scope | Evidencia |
| --- | --- | --- |
| Sonnet 5 High | Planes M2–M8, prompts, trazabilidad y decisiones | [manifest](evidence/reviews/sonnet-manifest.json), [respuesta original](evidence/reviews/sonnet-review.json) |
| Opus 5 High | Escritura/estado/lifecycle/M7/readiness y ADRs de frontera | [manifest](evidence/reviews/opus-manifest.json), [respuesta original](evidence/reviews/opus-review.json) |
| Gemini 3.8 Flash High | Spec completa, matriz y maestro; no planes individuales | [manifest](evidence/reviews/gemini-manifest.json), [respuesta original](evidence/reviews/gemini-review.json) |

## Disposición de findings

Sonnet P1-1/P1-2 pidió incluir este documento y el diff exacto del tablero:
ambos se entregaron en [re-review aceptado](evidence/reviews/sonnet-recheck.json),
con [manifest exacto](evidence/reviews/sonnet-recheck-manifest.json).
Sonnet P2-2 se corrigió: D11 antes de M8-01, alcance D13 antes de M8-02 y
calificación D13 antes de M8-07. P2-1 motivó puerta D02 Go/No-go y contingencia
explícita; se rechaza su ejemplo de lock solo del servidor como alternativa que
satisfaga exclusión frente a editores externos. Reducir esa garantía requiere
decisión del owner, no fallback silencioso. P3-1/P3-2 agregaron censo de superficie
antes del freeze y motivo de `--locked --offline`; P3-3 se atiende incluyendo ADRs
de frontera en el paquete focalizado. La respuesta dice esfuerzo por defecto;
el comando realmente solicitó `--effort high`, y metadata confirma Sonnet 5.

Opus emitió changes required: un P0 de precedencia y ocho P1 de diseño, además de
P2/P3. Disposición del Technical Owner, confirmada por [Opus re-review aceptado](evidence/reviews/opus-recheck.json) con [manifest](evidence/reviews/opus-recheck-manifest.json):

| Finding | Disposición |
| --- | --- |
| P0-1 AGENTS prohíbe M2 | No se acepta modificar AGENTS en fase documental: la instrucción final del owner prevalece. Se programa actualizar su scope al iniciar implementación M2 y sincronizarlo en DoD; prompts ya explican precedencia. |
| P1-2 D02 | Dos propiedades separadas, candidatos explícitos y Go/No-go/contingencias. No se acepta afirmar imposibilidad universal desde review sin prueba; exclusividad meramente declarada requiere owner y ADR, nunca fallback automático. |
| P1-3 locks/lecturas | Lecturas M1 no adquieren writer lock, conservan admisión/captura/error model; regresión de trece tools bajo commit y cache invalidation añadidas. No se promete atomicidad de snapshot externa. |
| P1-4 TOML | Patch rechaza path/git/registry/registry-index tipados, candidatos no capturables; fixtures escape/absoluto/link con canarios. |
| P1-5 receipts | Predicado grant vigente ∧ mismo principal host ∧ policy; postidentity/reopen, revocación y B≠A explícitos. |
| P1-6 Cargo config | Config administrativo generado desde allowlist, hash cada job; sin wrappers/linkers/runners/net/http/credentials; threat asset y fixture nuevos. |
| P1-7 formatos | Versionado/unknown-version desde M2-01; D12 adelantado para journal/receipt. Downgrade gestionado se bloquea con journal pendiente. No se finge que 0.1.0 puede detectar un formato futuro. |
| P1-8 edits M6 | Payload interno de bytes no implica tool pública genérica; D01 decide antes del writer, D25 versiona extensiones. C22 registra tensión. |
| P1-9 remoto | M7 source RO excluye mutaciones; nueva ampliación requiere D13 positivo del target. C24 lo registra. |
| P2-10/11 | G0 usa proxy existente y plan de piloto; piloto HTTP post-Go. DoR M2 escalonado por corte. |
| P2-12/13 | Fuentes §52/69/70, cache/availability y controles positivos de ataques explícitos. |
| P2-14/15 | No-op rustfmt solo observado; nuevo candidato necesita preview distinto. Plantilla Cargo fix cerrada y flags en receipt. |
| P2-16/17 | Registro riesgos residuales en checklist 1.0 y publicación positiva de límites en docs M2. |
| P3-18/19 | Modelos/effort exigidos por owner y CLI disponible en tooling receipt; no cambiar ese encargo por inferencia del revisor. Erratas e inventario D25 corregidos. |

El packet de re-review incluye las correcciones, diff del tablero, este índice y
las instrucciones finales exactas del owner. Sonnet re-review P2-3 pide cerrar esta
disposición Opus antes de M2; su P3-4 detectó un `+` tipográfico, ya corregido.

Gemini H-01/H-02 (P1) identifican prerrequisitos
de implementación ya explícitos en D02/D05, no defectos ejecutables de M1 ni
bloqueos a integrar el plan. Se mantiene el bloqueo a commit de mutaciones sin
frontera demostrada y a resolución sin datos offline. No se acepta su sugerencia
de usar inode/dev preflight como control suficiente ni de declarar atomicidad
multiarchivo; tampoco sus nombres de error improvisados o `isError` universal:
D01/G1 exigen DTOs propios y preservar el error model M1. H-03..05 (P2) y H-06..07
(P3) ratifican D13, scope M6, M7 condicional, trece tools y deuda documental.
La afirmación del revisor de cobertura 100% se limita al registro de encabezados;
la responsabilidad sobre los requisitos dentro de cada sección sigue siendo del
Technical Owner.

Opus re-review retiró el P0 y confirmó los ocho P1 resueltos. Su P2 residual
(frase de mutación remota en tests) y nueve P3 se corrigieron: tabla C22 continua,
razón toolchain_unavailable, latencia dentro de presupuestos M1, atomicidad también
para lecturas M1, puerta D02 visible, modelo Gemini exacto, contraste fallback
L09/L10, registro final, AGENTS como primera acción y DoD unido por referencia.
Son correcciones documentales focalizadas, revisadas por el Technical Owner; no
se atribuye al revisor lectura de bytes posteriores a su manifest.

La incertidumbre excepcional D02 motivó consulta adicional a **Claude Fable 5.1
High**, CLI 2.1.260, modelo explícito `claude-fable-5-1`: [disponibilidad](evidence/reviews/fable-availability.json),
[manifest](evidence/reviews/fable-d02-manifest.json), [respuesta](evidence/reviews/fable-d02.json).
Confirma que no hay exclusión fuerte acreditada con el UID actual y propone swap
seguido de validación como contrato alternativo. El Technical Owner no acepta
su afirmación de que eso ya cumpla el DoD fuerte: antes de detectar conflicto ya
publicó el candidato, y un segundo swap de rollback también compite con escritores.
Retener el inode desplazado no significa que la ruta no haya perdido una actualización
ni demuestra validación linealizable. Se traslada a experimento nativo y eventual
decisión explícita del owner; no se incorpora como garantía aceptada.

La [investigación read-only de prerrequisitos](m2-d02-host-preconditions.md) registra
SDK/Apple, alternativas y experimento a ejecutar después de integrar la planificación.
Ningún revisor sustituye el gate positivo de escritura ni autoriza reducir scope.
Conclusión del Technical Owner: no quedan findings documentales bloqueantes sin
disposición; D02/D05 y los demás Dxx siguen Proposed con sus puertas de implementación.

## Validación y cierre

El [recibo documental](evidence/document-validation.json) acredita links/anchors,
215 encabezados/124 secciones, 26 decisiones, 46 cortes y orden acíclico condicionado
por M7. Scope documental y status fuera del backlog son idénticos a la base.
`python3 -B scripts/check-architecture.py` y `git diff --check` pasaron. Los hashes
del recibo excluyen este índice para evitar autorreferencia. Cualquier corrección
posterior exige actualizar el recibo. No se usan tests Rust históricos como gate de este diff.
El commit e integración documental preceden a la implementación autorizada de M2;
esta última tendrá rama, decisiones, pruebas y estado separados.
