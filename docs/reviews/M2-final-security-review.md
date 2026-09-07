# M2 — revisión independiente final de seguridad de ejecución

Fecha: 2026-09-05. Revisor: Claude Code CLI 2.1.260, `claude-opus-5`, effort high,
read-only, safe-mode, sin tools ni MCP. No implementación ni comandos del revisor.
[Inputs iniciales](M2-final-security-inputs.json), [respuesta inicial](M2-final-security-opus.json),
[inputs recheck](M2-final-security-recheck-inputs.json), [recheck](M2-final-security-recheck-opus.json).
Los manifests identifican bytes revisados; las pruebas runtime las ejecutaron owner/workers.

**Veredicto independiente: Accepted, sin P0/P1 pendientes en el alcance.**
No equivale al cierre M2, al gate conjunto, al cliente real ni a la revisión del
writer nativo. El revisor declara límites y verificaciones fuera del paquete.

| Finding | Disposición del Technical Owner |
| --- | --- |
| P0-1 original, socket supuestamente imposible | Refutado y retirado por el revisor: probe del filtro aplicado y libseccomp real admiten STREAM con 1/15. Se conserva el error original de revisión. |
| Precisión residual del socket | Corregida: 15/1 conserva STREAM/CLOEXEC y rechaza RAW antes del kernel; prueba semántica distingue el orden. [Evidencia](../validation/M2-fix-socket-mask.json). |
| P1-1 metadata/dataset | Corregido: cada manifest acreditado por captura o inventario vendor, grafo coherente offline/frozen y SHA de ambos documentos reales en provenance. [D05](../validation/M2-D05-hardening-gate.json). |
| P1-2 grants | Cerrado: parser y wiring distintos por operación, publisher kind-specific y digest por dominio. Sonnet revisó la integración; no se confunde un archivo omitido del paquete con código ausente. |
| P2-1 decoder fingerprint | Corregido: decoder incluido. |
| P2-2/4 cardinalidad y texto | Frontera aceptada ADR-057: aplicación controla una operación/editor preserva texto; publisher repite allowlist semántica sobre bytes exactos aprobados. No se afirma segunda prueba de procedencia textual. |
| P2-3 datos offline | Corregido matcher de ausencia; follow-up añade versión ausente contra Cargo real. Missing data es unavailable; corrupto blocked; candidato inválido failed. |
| P2-5 coste vendor | Deuda acotada aceptada: dataset host aprobado y 4096 entradas/16 MiB; no se promete coste constante ni resistencia DoS universal. |
| P2-6 límites | 30 s / 1 MiB son límites deliberados y publicados, con fallo cerrado. No se promete resolución de todo workspace. |
| P2-7 zip de scope | Corregido length/directories antes de zip, pruebas adversas en ambas direcciones. |
| P2-N1 helpers fingerprint | [Follow-up](../validation/M2-D05-hardening-followup.json) agrega mutation_gateway.rs al fingerprint de resolución; no cambia schema ni autoridad. |
| P2-N2 clasificación de scope | Residuo aceptado: rechazo de bytes guest fuera de scope se muestra como toolchain_unavailable, sin plan ni efecto. Tests hostiles prueban la detección; no debe interpretarse como diagnóstico preciso de instalación. Refinar taxonomía requiere un cambio posterior explícito. |
| P2-N3 versión offline | Follow-up agrega quote=9.9.9 contra vendor1.0.47 y comprueba clasificación real. |
| P2-N4 deadline entre fases | Residuo aceptado: agotamiento previo a dispatch puede aparecer como infraestructura; no se ejecuta la fase ni se omite cleanup. La cancelación/timeout con descendientes activos tiene evidencia separada. |
| P2-C1 tree_fingerprint duplicado | Deuda de mantenibilidad aceptada. Fixtures end-to-end verifican acuerdo de ambos adapters; una divergencia falla cerrada. No se añade una dependencia hash al dominio solo para cerrar esta sugerencia. |

Verificaciones: SourceBundle ordena y exige paths únicos; state path incluye un subdirectorio aleatorio por sesión bajo el state-root autorizado; los fingerprints describen esa configuración exacta y no prometen igualdad entre sesiones. Send/recv proceden del perfil M1 previo. Source
rechaza `.cargo/config` y `.cargo/config.toml`, HOME/CARGO_HOME son tmpfs guest
fijos. La imagen exacta tiene pruebas positivas reales y [ausencia comprobada de configuración Cargo raíz](../validation/M2-image-config.json); no se generaliza a otras imágenes. El runtime no importa configuraciones ni credenciales host.

La nota final del revisor que equipara cambiar el fingerprint a invalidar receipts
anteriores **no se adopta**: un receipt conserva su validación histórica ligada al
plan; no afirma ejecución con la configuración actual ni autoriza otro commit.
La actualización de perfil no reescribe journals ni permite tratar un receipt
antiguo como aprobación nueva. Los fixtures anteriores siguen siendo históricos.

El veredicto y los P2 aceptados deben leerse junto a los gates y review nativo;
ninguna limitación se convierte en garantía por quedar fuera del paquete.

Los metadatos de Claude Code registran uso auxiliar de Haiku 4.5 además del modelo de revisión explícito. Se conserva el JSON íntegro; no se presenta ese uso auxiliar como una segunda revisión ni una sustitución del revisor solicitado.
