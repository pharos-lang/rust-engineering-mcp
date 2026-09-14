# V01 — informe del revisor (Claude Sonnet 5, High, read-only sin tools; claude 2.1.268)

Invocación: `claude -p --model sonnet --effort high --tools '' --disallowedTools Agent Task --no-session-persistence --output-format json < input.md` (material inline: 01.md, ADR-086, 01-census.md y `git diff` W02+W03+W01b; input sha256 `10856561c35837c63af00c892f3ac23dc9b36de3bb1673d5df81deb71e97eed0`, 60 891 bytes). Inicio 2026-09-14T14:43:17Z, fin 2026-09-14T14:48:26Z, exit 0, 307163 ms, modelos ['claude-haiku-4-5-20251001', 'claude-sonnet-5'].

---

# V01 — Revisión independiente (read-only)

**Limitación de alcance:** no tengo acceso a `01-census.json`, `docs/M6/matrix.md#deuda-de-m6`, la spec completa (`rust-engineering-mcp-propuesta-v0.3.md`), ni al `README.md` raíz (solo `docs/adr/README.md` está en el material). Los hallazgos que dependen de esos archivos quedan marcados como no verificables directamente.

---

## `docs/adr/ADR-086-deprecation-and-freeze-policy.md`

**F-A (P1) — Taxonomía de estabilidad no implementa spec §57.**
El Context cita spec §57: "tres categorías de estabilidad (`stable`, `preview`, `experimental`)". La Decision (punto 1) define en cambio "`stable`, `preview` o `internal`". `internal` no es un sustituto de `experimental`: por diseño (§1 y confirmado en `01-census.md` §2: "internal (0)... las 36 tools SÍ se anuncian en tools/list, así que ninguna puede proponerse internal") es una clase que **ninguna tool puede tener nunca** — queda reservada a comandos CLI/formatos en disco. El resultado es que la taxonomía de tools tiene en la práctica solo dos clases utilizables (`stable`/`preview`), y no existe ningún lugar del ADR para una tool que la spec calificaría `experimental`. Un cliente exhaustivo que intente mapear spec §57 contra ADR-086 no puede hacerlo.
Acción propuesta: aclarar en ADR-086 si `experimental` se descarta deliberadamente (y por qué, con justificación explícita frente a spec §57) o si falta una cuarta clase para tools nuevas sin calificar aún.

**F-B (P3) — Punto 3 ("aditivo vs ruptura") sin procedimiento verificable.**
"medirlo contra los consumidores exhaustivos conocidos... Si un cliente exhaustivo rompe con ese cambio, es una ruptura" no define qué constituye "romper" (¿fallo de validación estricta de schema? ¿solo campos `required`?) ni qué universo exacto es "cliente exhaustivo". Ambigüedad menor, no bloqueante, pero afecta la aplicabilidad práctica en M8-02.

**F-C (P3) — §56 (capability document) con adopción condicional.**
Punto 8 trata la adopción del capability document de spec §56 como decisión abierta en M8-02 en vez de requisito. No puedo verificar en el material si spec §56 es obligatorio o recomendado; si es obligatorio, esto es un gap, no solo una decisión de scope.

---

## `docs/validation/M8/01.md`

**F-D (P1) — Clasificación `stable` de 3 tools sin consumidor real, contra ADR-086 §1 literal.**
ADR-086 §1: "Un elemento **sin consumidor real** o sin test **no puede clasificarse `stable`**." `rust.coverage`, `rust.semver.check`, `rust.mutation.test` no tienen ninguna invocación de cliente stock (`01-census.md`: "Su único consumidor es el e2e nativo... ningún recibo de cliente stock las invoca"). El orquestador las mantiene `stable` con una condición ("si fallan [en M8-04] se degradan a preview antes del freeze") que **ADR-086 no define en ningún punto** — no existe la clase "stable condicional/probatorio". Esto es exactamente "aceptar algo prohibido" por el criterio de la pregunta guía: el ADR dice que no pueden ser `stable` sin consumidor real, y lo son.
Acción propuesta: o bien clasificarlas `preview` hasta que M8-04 aporte el consumidor real que falta, o enmendar ADR-086 §1 para admitir explícitamente evidencia nativa e2e como sustituto válido de "consumidor real" (con criterio explícito de cuándo aplica).

**F-E (P3) — "objetivo (~35) se cumple" con 36 y cero consolidaciones.**
§3: "El objetivo del plan (~35) se cumple" cuando el resultado real es 36 y ninguna consolidación fue aceptada. Es una afirmación retórica más que demostrada; sería más preciso decir que el objetivo no se alcanza exactamente y justificar por qué el exceso de 1 es aceptable.

**F-F (P3) — Fila 4 de la tabla de consolidaciones mezcla justificaciones.**
La fila que agrupa `fmt.check↔apply`, `test↔test.nextest`, `toolchain.inspect↔project.inspect` da como motivo conjunto "Pares lectura/escritura o tools M1 congeladas" sin mapear qué motivo aplica a cada par (`toolchain.inspect↔project.inspect` no es un par lectura/escritura). Trazabilidad débil, no bloqueante.

---

## `docs/validation/M8/01-census.md`

**F-G (P2) — "36/36 real_consumers[] no vacío" es engañoso para 3 tools.**
§2 afirma sin matiz "36/36 tienen `real_consumers[]` no vacío", pero el mismo documento aclara después que para 3 de ellas el único elemento de ese campo es un test e2e nativo, no un cliente real. Usar el mismo campo/etiqueta para dos tipos de evidencia distintos (cliente stock vs. test nativo interno) es la raíz de F-D: si `real_consumers[]` se hubiera poblado solo con invocaciones de cliente stock, la contradicción con ADR-086 §1 habría sido visible de inmediato en la tabla resumen.
Acción propuesta: separar el campo en `real_consumers_stock[]` / `native_e2e_only[]` en el JSON y reflejarlo en la narrativa.

**F-H (P3) — §3.2, metodología de "ningún outlier > 5%" es asimétrica.**
El porcentaje de cada tool se calcula como `schema_bytes_de_esa_tool / total_wire_bytes_de_tools_list`, no como `bytes_totales_de_esa_tool_(incluyendo_description/annotations) / total`. El denominador incluye descripciones y anotaciones de las 36 tools, pero el numerador de cada tool las excluye — esto subestima sistemáticamente el porcentaje real de cada tool individual. El margen actual (top ~4.5%) probablemente no cambia la conclusión, pero el método declarado no mide lo que dice medir.

**F-I (P3) — §8 "huérfanos = 0" no verificable con el material provisto.**
Se remite a `01-census.json.orphans_note`, no incluido en esta revisión. Dentro de lo visible, la afirmación descansa en la columna "Consumidor real: sí" de §2, que F-G muestra que es más débil de lo que aparenta para 3 filas. No es un defecto confirmado, es una limitación de esta revisión.

---

## Diff W03

**F-J (P1) — `README.md:21` (raíz) no fue corregido pese a estar dentro del alcance declarado.**
`01.md` §1 afirma explícitamente: "F1 es más amplio que lo reportado: **README.md:21**, architecture.md:291/333, client-configuration.md:316/418 y tools.md:138/1645 también describen el checkout con 31", y §4 dispone F1 como "Aceptado y ampliado... Dónde se cierra: **W03 (ahora)**". El `git status` de esta sesión no incluye `README.md` (raíz) entre los archivos modificados — solo `docs/adr/README.md`, que es un archivo distinto. El conteo obsoleto "31" en el README raíz **sigue sin corregirse**, contradiciendo la propia disposición del orquestador.
Acción propuesta: corregir `README.md:21` o retirar la afirmación de F1 como cerrado en W03 hasta que se haga.

**F-K (P2) — `tools.md:1645` y `client-configuration.md:316` probablemente tampoco corregidos.**
Los hunks del diff para `tools.md` solo tocan las líneas ~1-10 y ~135-138 (cubre el `:138` citado, no el `:1645`); los hunks de `client-configuration.md` solo tocan ~401-440 (cubre el `:418` citado, no el `:316`). Sin ver el archivo completo no puedo confirmarlo con certeza, pero la ausencia de hunk en esas líneas, sumada a que el propio F1 las señaló explícitamente, sugiere que también quedan sin corregir.
Acción propuesta: verificar y corregir ambas líneas antes de cerrar F1.

**F-L (P2) — `docs/roadmap/m2-m8.md`: "release v0.3.0 vía PR #18" es un hecho nuevo no corroborado.**
El diff añade "(PR #17, `6ea330d…`; release `v0.3.0` vía **PR #18**)" para M5. Ningún otro documento del material menciona PR #18: la verificación de commits en `01.md` §1 solo lista PR #14/#15/#17/#20, y tanto `01.md` como `01-census.md` afirman repetidamente que el tag `v0.3.0` **es** el mismo commit que el merge de PR #17 ("el mismo commit que el merge de PR #17"), sin mencionar una PR adicional de tagging. Esto puede ser válido (una PR de release separada que solo crea el tag sobre el mismo commit), pero se introduce sin evidencia citada en el material revisado y no pasó por la verificación del orquestador en §1 de `01.md`.
Acción propuesta: citar la evidencia de PR #18 (existe en `main`, qué cambia) o retirar la mención hasta verificarla.

**F-M (P2) — M6 descrito públicamente como "calificadas" sin mencionar `preview`/deuda de contrato.**
`client-configuration.md` y `compatibility.md` ahora dicen que las 5 tools `rust.analyzer.*` están "calificadas", en el mismo lenguaje usado para M1-M5 (que son `stable`). Ninguno de los dos documentos menciona que su clase es `preview` ni la deuda de contrato citada en `01.md` §2 (semántica de `SANDBOX_DENIED`, Opción B de diagnósticos pendiente, no-determinismo de assists) que es precisamente el motivo por el que no son `stable`. Es cierto que ADR-086 §8 deja el mecanismo visible de `preview` para M8-02, pero la ausencia total de cualquier matiz en docs públicas ya editadas en W03 puede inducir a un integrador a tratarlas como equivalentes a las 31 `stable`.
Acción propuesta: añadir una nota breve de clase `preview`/deuda conocida en `client-configuration.md` y `compatibility.md`, sin esperar al mecanismo formal de M8-02.

**Verificado sin hallazgos:** el paréntesis de `--help` (grants `--rust-image`/`--allow-analyzer-action-write`) es exacto y coherente con `client-configuration.md`; `docs/adr/README.md` indexa correctamente ADR-078/079/081/085/086 (cierra F3); la línea de estado de `m2-m8.md` y la tabla de hitos M3-M6 son coherentes con las PRs citadas y verificadas por el orquestador; los conteos en `architecture.md:291/333` y `tools.md:138` sí quedaron corregidos.

---

## Veredicto: **Block**

Tres P1 (F-A, F-D, F-J) y cinco P2 (F-G, F-K, F-L, F-M, más el gate de contrato implícito en F-D). Ninguno requiere rediseño: F-J/F-K son ediciones de texto puntuales, F-L es verificar/citar una PR, F-A y F-D requieren una decisión explícita del owner sobre la taxonomía de clases y sobre si evidencia e2e nativa puede sustituir "consumidor real" — ambas ya identificadas por el propio proceso (R01-F12) pero sin resolver en el ADR. Con esas correcciones el material es sólido: metodología transparente, hallazgos bien evidenciados, commits verificados, y las consolidaciones descartadas están razonablemente justificadas contra G1/G2/ADR-086 §9.
