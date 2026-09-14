# Disposición — revisión independiente V08 del camino de escritura del analyzer (W08)

Fecha: 2026-09-12. Revisor: Claude Opus 5 (read-only, diff por stdin, 414 s),
distinto del worker. [Texto íntegro](claude-opus-5-review.md); hashes en
[inputs.sha256](inputs.sha256). Veredicto: **Approve con P3** — sin P0-P2. Sin
segundo writer/journal, sin bypass de authorize/generation/idempotency; el plan
no se puede commitear tras cambiar la fuente; el refactor de audit M2 preserva
comportamiento y los 5 schemas M2 intactos; round-trip de `validation` sólido;
sin fuga de texto del peer; e2e nativo no vacuo.

Cierres en **W08b** (materiales del borde de escritura):

| P3 | Disposición |
| --- | --- |
| Comprobación de kind DESPUÉS de `plans.resolve` (podría ligar la idempotency key a un plan ajeno antes de PERMISSION_DENIED → interferencia cruzada) | Aceptado → W08b comprueba el kind ANTES de `resolve` |
| Grant comprobado DENTRO del worker (una llamada sin grant puede salir LOCK_BUSY/CANCELLED/TIMEOUT en vez de SANDBOX_DENIED) | Aceptado → W08b mueve el grant antes de `run_joined` |
| Paridad listado↔preview (el listado marca `applicable` lo que preview rechaza: no-op, bundle-limit, no-`.rs`) — item V07 no cerrado del todo | Aceptado → W08b aplica las mismas comprobaciones estructurales en el listado |
| `bounded_title` solo reemplaza Cc; bidi/zero-width/U+2028-2029 cruzan el wire | Aceptado → W08b amplía el saneo (bidi U+202A-202E/U+2066-2069, zero-width, separadores de línea/párrafo) en títulos aplicables y rechazados |
| Dirección inversa (plan analyzer commiteado por una tool M2) sin test; image_id no atado por test no-ignorado; grant de otra raíz sin test de tool | Aceptado → W08b añade los tres tests |
| Timeout de commit/receipt con mensaje de "analyzer budget" | Aceptado → W08b: mensaje que remite al receipt |
| Vocabulario de audit ampliado (nuevos reason/tool) sin bump de schema | Aceptado → W08b sube la versión del evento de audit o documenta el enum abierto |
| Un mismo estado → varios códigos (ref stale: PROJECT_NOT_FOUND/PERMISSION_DENIED/ACTION_STALE/CONFLICT según dónde se detecte) | Aceptado en parte → W08b normaliza donde sea barato o lo documenta en tools.md |
| Overflow de apply = `blocked/LIMIT_EXCEEDED` (ADR-083 §3 lista RESULT_LIMIT) | Aceptado → W08b alinea el código con ADR-083 o documenta la diferencia (apply rechaza, no trunca) |
| Audit `admitted=false` en PERMISSION_DENIED tras grant presente | **Deuda M6-06** (precisión de audit; no afecta seguridad) |
| Prominencia de archivos tocados solo textual (sin flag build.rs/vendored en los datos) | **Deuda M6-06**: bajo Opción A el diff exacto es la superficie de revisión; un flag por archivo es mejora |
| Amplificación de CPU bajo el lock (32 acciones copian el bundle) | Aceptado como acotado; W08b puede recortar `LineIndex` reusado si es barato, si no → deuda |

Verificado por el orquestador: los tres e2e de apply pasan contra la imagen
real (el writer cambia disco, ACTION_STALE protege). W07+W08 se commitean
juntos como el vertical M6-04/05 tras W08b + recalibración.
