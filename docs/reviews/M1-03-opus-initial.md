# M1-03 — Opus5 High, initial read-only review

Claude Code2.1.259; explicit claude-opus-5/high, no tools. Historical findings below;
see principal disposition and final validation before treating them as unresolved.

# Revisión independiente M1-03 — lectura estática, sin ejecución

**P0 confirmados: ninguno.** No encontré escape de contención, bypass de autorización de artefactos ni inyección de argv. La doble verificación owner/id en `artifact_access.rs:60,73` y la gramática cerrada de `domain/src/check.rs` sostienen la frontera. Lo que sigue son P1/P2 confirmados por lectura, más evidencia que no pude verificar.

---

## P1-1 — Artefactos huérfanos al expirar el proyecto: agotamiento de la cuota global

**Dónde:** `crates/application/src/artifact_access.rs:83-95` y `crates/mcp-server/src/stdio.rs` (construcción: `Resources::new(project.registry(), …)`).

**Confirmación estructural:** el `Store` es propiedad de `Resources`, creado *después* de `ProjectTool`. Ningún componente con acceso al store observa la expiración de un proyecto. `prune()` y `self.entries.remove(reference)` (línea 93) eliminan la entrada del registry sin llamar nunca a `ArtifactStore::revoke_owner`. `revoke_owner` y `cleanup` no aparecen invocados en ninguna ruta del código nuevo ni del diff.

**Trigger concreto:** crear 16 proyectos y ejecutar en cada uno 4 checks con log de 256 KiB (tope de 1 MiB/owner). Dejar expirar los ProjectRef. Los 64 artefactos siguen ocupando los 16 MiB globales hasta su propio TTL de 3600 s, ilegibles (la autorización falla) e irrecuperables. A partir de ahí todo `capture` devuelve `QuotaExceeded` → `InspectionError::OutputLimit` → **todos los checks de todos los proyectos responden Blocked/OUTPUT_LIMIT_EXCEEDED durante una hora**. No es fuga de datos; es denegación de servicio global acotada al TTL, con coste de ataque bajo.

**Fix mínimo:** hacer que `prune()` devuelva los `ProjectRef` desalojados y, en `read_artifact` y `ProjectRegistry::check` (que ya tienen `&mut store`), llamar `store.revoke_owner(ref)` para cada uno; añadir además `store.revoke_owner(reference)` en la rama de desalojo de `artifact_access.rs:93`.

---

## P1-2 — El truncado del log descarta stderr íntegro

**Dónde:** `crates/application/src/check.rs:91-94` (composición) frente a `check.rs:12` (`MAX_STREAM = 256 KiB` **por stream**) y el límite del store de 256 KiB **por artefacto**.

**Trigger concreto:** un check cuyo stdout JSON alcance 256 KiB (fácil: muchos diagnósticos con `rendered`, o un proc-macro verboso). El log se compone como `"=== stdout ===\n{stdout}\n=== stderr ===\n{stderr}"`; el store trunca por el final, de modo que el artefacto retenido contiene sólo stdout y **ni siquiera aparece el marcador `=== stderr ===`**. Se pierde por completo la única copia del texto legible de Cargo/rustc, que es lo que el stdout JSON ya duplica de forma normalizada. `metadata.truncated=true` es la única señal, y no distingue "se perdió la cola de stdout" de "se perdió todo stderr".

**Fix mínimo:** invertir el orden (stderr primero) o, mejor, presupuestar por stream antes de concatenar (p. ej. 128 KiB cada uno con marca explícita de recorte por sección), de forma que ambas secciones sobrevivan siempre.

---

## P1-3 — `rust_version`/`cargo_version` afirmados sin observación

**Dónde:** diff de `crates/execution-adapter/src/project_inspection.rs`, construcción de `RuntimeIdentity`: `rust_version: "1.98.1".into(), cargo_version: "1.98.1".into()`.

**Problema:** `rust.check` publica una identidad de runtime que el cliente leerá como observada, pero es una constante literal en el sitio de uso. M1-02 sí observa el toolchain a través del runtime calibrado; aquí se degrada a aserción.

**Trigger concreto:** actualizar `APPROVED_RUST_IMAGE` (`rust_gateway.rs:7`) a una imagen con otra versión sin tocar estas dos cadenas. El servidor reporta con alta confianza una versión falsa, y el `configuration_fingerprint` no lo delata porque el literal vive en `project_inspection.rs`, que no está en `implementation_fingerprint`.

**Fix mínimo:** declarar `pub(super) const APPROVED_RUST_VERSION: &str` junto a `APPROVED_RUST_IMAGE`, usarlo en ambos sitios e incluir `project_inspection.rs` entre los `include_bytes!` del fingerprint de implementación —o reutilizar la observación calibrada de M1-02.

---

## P1-4 — Los diagnósticos normalizados son falsificables por el proyecto bajo prueba

**Dónde:** `crates/execution-adapter/src/cargo_diagnostics.rs:312-328`.

**Mecanismo:** Cargo reenvía el stdout de rustc a su propio stdout. Un proc-macro (o un build script, cuyo stdout Cargo también procesa) puede emitir líneas `{"reason":"compiler-message","message":{…}}` sintácticamente válidas. El parser no distingue origen: las normaliza y las publica junto a las auténticas.

**Trigger concreto:** proc-macro que hace `println!` de un `compiler-message` fabricado y no interfiere con el resto del stream. Cargo emite después su `build-finished` real con `success:true` y sale con 0 → `complete=true`, `validation_complete=true`, **`status:"passed"` con diagnósticos fabricados**. El texto del mensaje llega íntegro al cliente como evidencia estructurada del compilador, con mucha más credibilidad que el log crudo que el ADR sí marca como no confiable.

Las defensas existentes (romper ante JSON inválido, ante duplicado de `build-finished`, ante línea posterior a `build-finished`) no cubren este caso porque la inyección es JSON válido y previo.

**Fix mínimo:** no es eliminable con `--message-format=json` (no hay canal separado). Fix realista: (a) extender la advertencia del ADR §"Logs are project output" a los diagnósticos, y (b) reflejarlo en la superficie MCP — la descripción de la herramienta (`check.rs:418`) y el schema de `diagnostics` deben decir explícitamente que proceden de un stream que el código analizado puede escribir. Sin esto, el ADR afirma implícitamente una autenticidad que el canal no proporciona.

---

## P2 confirmados

| # | Ubicación | Problema y fix |
|---|---|---|
| 1 | `project_inspection.rs` diff: `ExecutionLimits::new(30_000, …)` vs `check.rs:30` `DEADLINE = 120s` | El presupuesto efectivo es 30 s, no 120. Un `cargo check` real que compile dependencias lo excede con facilidad → `TimedOut` → Blocked/COMMAND_TIMEOUT sistemático. La jerarquía (gateway < worker) es correcta; el valor probablemente se heredó de los comandos triviales de M1-02. Elevar a ~100 s o justificar 30 s. |
| 2 | `check.rs:46-47` (MCP) | `features` sólo declara `length(max=32)`; la gramática real (`identifier` + un único `/`, ≤128 bytes, sin duplicados) sólo vive en el dominio. El schema publicado es más laxo que lo aceptado → clientes generan peticiones válidas por schema y reciben `invalid_params`. Añadir `inner(regex(...), length(max=128))`. |
| 3 | `resources.rs:156` | `run_joined` → `Err(WorkerError::Busy)` se aplana a `internal()`. Con un worker único y checks largos, "ocupado" es la condición esperable, no un fallo interno. Distinguir `Busy` con un `ErrorData` propio. |
| 4 | `resources.rs:136-138` | El chequeo de `ready` cortocircuita **fuera** del worker; el ADR:57 dice que los Resources corren por el worker compartido "including readiness during bootstrap". Hoy es inocuo (en bootstrap no existen artefactos), pero ADR e implementación divergen: alinear uno de los dos. |
| 5 | `check.rs:521` | La ruta de respaldo por `MAX_RESULT` devuelve Blocked/OUTPUT_LIMIT sin `data`, perdiendo la URI de un log **ya publicado y autorizado**: artefacto inalcanzable ocupando cuota 3600 s. Alcanzable sólo si el `Data` mínimo excede 512 KiB (improbable), pero es el mismo patrón que el ADR documenta para cancelación. Conservar `data` en esa rama. |
| 6 | `check.rs:312-319` | `CheckOutcome::Incomplete` se emite como `status:"failed"`. Un cliente que sólo lea `status` concluye "no compila" cuando en realidad la validación no pudo confirmarse. `validation_complete:false` y el summary lo corrigen, pero el campo primario miente. Mapear a `Blocked` con `data: Some(...)`. |
| 7 | `check.rs:118-119` (application) | Ninguna redacción se aplica a `observation.diagnostics`: sólo el log pasa por `MemoryArtifactStore`. Hoy inocuo (política literal vacía), pero incumple ADR:66 "apply any configured redaction before diagnostics **and** logs escape". En cuanto se configure un literal, los diagnósticos lo filtran. |
| 8 | `check.rs:110-112` (application) | Si el rollback falla, se devuelve `Internal` y se pierde la causa real (`access_error(error)`). Registrar/propagar el error original. |
| 9 | `artifact_access.rs:8` y `resources.rs:24` | `MAX_CONTENT = 256 KiB` duplicado en dos sitios más `ArtifactLimits::default()`. Subir el límite del store sin tocar ambos convierte lecturas legítimas en `Internal` silencioso. Derivar de una única constante. |
| 10 | `artifact_access.rs:96` | Cada lectura de Resource renueva `entry.last_used` del proyecto. El TTL del artefacto no se renueva (correcto), pero el del proyecto sí: un cliente mantiene vivo un ProjectRef indefinidamente leyendo el log. Es defendible como "uso", pero contradice la premisa del ADR de que la expiración del proyecto revoca el acceso. Decidir y documentar. |
| 11 | `stdio.rs` diff, `enable_resources()` | No se implementa `list_resources`; el ADR:62 afirma "List resources remains empty". Si el default de rmcp responde `MethodNotFound`, los clientes que enumeran al conectar ven error. Implementar el listado vacío explícito. |

---

## Evidencia no verificada (no pude leer los archivos ni ejecutar)

1. **`SourceBundle` ordenado por path.** `cargo_diagnostics.rs:78` usa `binary_search_by(|f| f.path().cmp(relative))`. Si `SourceBundle::new` no garantiza orden lexicográfico, los spans de archivos válidos se descartan de forma no determinista (`truncated=true`, sin error). **Todos los tests del módulo usan un único archivo**, así que la búsqueda binaria multiarchivo carece de cobertura. Verificar el invariante y añadir un test con ≥3 archivos desordenados.
2. **`NonEmptyText` y caracteres de control.** `text()` (línea 125) recorta por bytes pero no filtra controles ni `\n`; para `code` sí se filtran explícitamente (línea 216). Si `NonEmptyText` no rechaza controles, `message` y `label` pueden llevar ANSI/saltos de línea al cliente. No cubierto por tests.
3. **`execution_fingerprint` sobre argv real.** La calibración sólo instancia `CheckProject(default)` (`rust_gateway.rs` diff, bloque de fases). El ADR delega la ligadura de los argumentos variables al fingerprint de ejecución; no pude confirmar que se calcule sobre `phase.arguments()` con opciones. Sí es indicio favorable que `rust_applied::verify` pasara a recibir `&Phase`.
4. **Semántica de `remove` y de `read` en `MemoryArtifactStore`** respecto a `now == expires_seconds`: `retention()` (`artifact_access.rs:46`) trata `remaining == 0` como NotFound; si el store lo considera vivo, hay una ventana de un segundo con comportamiento divergente entre `read` y la autorización.
5. **`prune()` y `resolve_inner(..., false)`**: asumí que el tercer parámetro `false` significa "no renovar el lease", conforme al comentario de `artifact_access.rs:81-82`. Si renovara, el orden cuidadoso de las líneas 79-96 sería inútil.
6. **`schemas::CheckOptions`** (fichero no aportado) debe coincidir campo a campo con el `Serialize` derivado de `CheckOptions`; `Contract::new()` probablemente lo valide, pero no lo confirmé.
7. **Gates de contención.** Los seis escenarios adversariales build.rs/proc-macro **no se han re-ejecutado** con la configuración nueva. Dado que `implementation_fingerprint` incorpora ahora `rust_execution.rs` y `check.rs`, y que se añadió una fase de calibración, la recalibración y el rerun son bloqueantes para Done. El P1-4 hace ese rerun más relevante, no menos.

---

## Aspectos que revisé y considero correctos

- Gramática cerrada de `CheckOptions`: `identifier` rechaza el `-` inicial, se prohíbe `pkg/feat/extra`, se ordenan las features (argv determinista, fingerprint estable) y se rechazan duplicados en vez de deduplicar silenciosamente.
- Construcción de argv por `--flag=valor` en un único elemento, sin shell y sin `--` de paso libre.
- `parse` falla cerrado ante: registro sin LF final, cualquier línea tras `build-finished`, `build-finished` ausente, JSON inválido y recursión excesiva del parser. `validation_complete` exige `Exited` + `(0, true) | (1.., false)` + stream y stderr no truncados.
- Verificación cruzada byte-offset ↔ línea/columna contra la fuente capturada: rechaza spans desalineados, desplazados a mitad de carácter multibyte o de rutas no capturadas; `rendered`, `text` y `expansion` nunca se publican (bien cubierto por tests).
- Agrupación multipart todo-o-nada con detección de solapamiento e inserciones duplicadas ambiguas.
- Orden de bloqueo registry→store idéntico en `check.rs:450-457` y `resources.rs:149-152`, sin `await` con guards vivos.
- Reloj monotónico compartido entre store y verificación de retención (mismo `Instant` de origen), como exige el ADR.
- Rollback acotado al artefacto nuevo tras fallo de autorización final; los logs de trabajos anteriores sobreviven.
- `cacheScope=private` + `ttlMs=0`, blob base64 (256 KiB → ~350 KiB, holgado bajo el marco de 1 MiB), URI opaca de longitud fija sin normalización, escape ni interpretación de rutas.
