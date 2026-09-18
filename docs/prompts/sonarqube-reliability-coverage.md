# Encargo — SonarCloud sobre `main`: 13 issues de fiabilidad y cobertura 78,5 % → ≥ 80 %

Repositorio `/Users/cburgosro/Projects/rust-mcp`, rama base `main` (M8 mergeado en
`868b9936`, versión de workspace `0.9.0-rc.1`). Destinatarios: **Claude Code Opus**
y **Codex (gpt-5.6-sol)**, trabajando en paralelo con el reparto de §5.

Estado medido en vivo el 2026-09-18 (no lo asumas, vuelve a leerlo antes de
empezar):

```sh
curl -s "https://sonarcloud.io/api/measures/component?component=pharos-lang_rust-engineering-mcp&metricKeys=coverage,bugs,reliability_rating,uncovered_lines,lines_to_cover"
curl -s "https://sonarcloud.io/api/qualitygates/project_status?projectKey=pharos-lang_rust-engineering-mcp"
```

| Métrica | Valor |
| --- | --- |
| `bugs` | **13** (todos `rust:S9334`, todos CRITICAL) |
| `reliability_rating` | **4.0 (D)** |
| `coverage` (overall) | **78,5 %** — 63 544 / 80 821 líneas, 17 277 sin cubrir |
| Quality gate de `main` | **ERROR**, única condición roja: `new_coverage` 78,4 % < 80 |
| `security_rating` / `sqale_rating` / `code_smells` | 1.0 (A) / 1.0 (A) / 0 |

---

## 1. Los 13 issues de fiabilidad: **la sospecha por defecto es que son falsos positivos**

Los 13 son la misma regla, `rust:S9334`, con el mismo mensaje: *«Add
`#[serde(default)]` so this field defaults to `None` when missing from the
input.»*

| Archivo | Líneas |
| --- | --- |
| `crates/execution-adapter/src/project_metadata.rs` | 68, 89, 92, 97, 99, 103 |
| `crates/domain/src/evidence.rs` | 51, 53, 229 |
| `crates/domain/src/result.rs` | 120, 122 |
| `crates/catalog-adapter/src/audit.rs` | 28, 30 |

**No apliques la sugerencia de la regla sin más.** Todos los sitios señalados usan
un deserializador propio (`required_nullable` en `domain`, `nullable` en
`execution-adapter`) cuyo propósito documentado en `crates/domain/src/lib.rs:26`
es exactamente el contrario de lo que pide la regla:

> «Unlike Serde's default Option handling, this requires a field to be present
> while still accepting an explicit null. **Used for required-nullable
> contracts.**»

Es decir: `Option<T>` aquí significa **«presente pero anulable»**, no
«opcional». Añadir `#[serde(default)]` haría que un campo **ausente** pasara a
`None` en silencio, destruyendo el contrato que ese helper existe para imponer.
Varias de esas structs llevan además `#[serde(deny_unknown_fields)]`: el parseo
estricto es deliberado, no un descuido.

### Procedimiento por hallazgo (los 13, uno a uno; nada de cambios en bloque)

1. ¿El campo usa `required_nullable` / `nullable`? ¿La struct lleva
   `deny_unknown_fields`? → indicio fuerte de falso positivo.
2. ¿De dónde viene el JSON? Entrada **externa y ajena** (salida de
   `cargo metadata`, archivos de catálogo) donde un campo ausente debería
   tolerarse, o **contrato propio** donde la ausencia debe fallar.
3. ¿El tipo participa en el contrato congelado? Comprueba si tocarlo movería
   `crates/mcp-server/tests/snapshots/*-tool.json`.

**Resultado esperado**: la mayoría se marcan en SonarCloud como *false positive*
con la justificación anterior (o se ajusta el alcance de la regla para el
proyecto), y **el código no se toca**. Si encuentras alguno que sí sea un campo
genuinamente opcional, arréglalo y **dilo explícitamente**, con el razonamiento.

**Prohibido**: `#[serde(default)]` «para que baje el contador». Es cambiar el
comportamiento de deserialización de un producto cuyo hito anterior entero fue
congelar contratos.

---

## 2. Cobertura: objetivo concreto **+1 113 líneas cubiertas**

`0,80 × 80 821 = 64 657` líneas cubiertas necesarias; hay 63 544. **+1 113**.
Esa es la cifra a batir, no un porcentaje difuso.

La masa sin cubrir está en los gateways dependientes de Docker, que el runner de
análisis (Linux, sin Docker) no puede ejecutar:

| Sin cubrir | Cob. | Archivo |
| --- | --- | --- |
| 1 589 | 17,9 % | `crates/execution-adapter/src/rust_gateway.rs` |
| 1 153 | 68,0 % | `crates/execution-adapter/src/performance_gateway.rs` |
| 816 | 36,1 % | `crates/execution-adapter/src/project_inspection.rs` |
| 753 | 57,8 % | `crates/execution-adapter/src/security_gateway.rs` |
| 701 | 4,9 % | `crates/mcp-server/src/stdio/quality_artifacts.rs` |
| 681 | 34,6 % | `crates/execution-adapter/src/mutation_gateway.rs` |
| 545 | 48,9 % | `crates/execution-adapter/src/resolution_gateway.rs` |
| 442 | 4,3 % | `crates/execution-adapter/src/lsp_session.rs` |
| 324 | 4,4 % | `crates/mcp-server/src/catalog_cli.rs` |
| 271 | 13,4 % | `crates/execution-adapter/src/coverage_gateway.rs` |
| 212 | 0,0 % | `crates/execution-adapter/src/rust_calibration.rs` |

### Empieza por buscar un hueco de instrumentación, no por escribir tests

Precedente de esta misma milestone (**W39b**, ver
`docs/validation/M8/delegation/W39b-llvm-profile-passthrough/`): los helpers de
spawn de los tests de integración borraban `LLVM_PROFILE_FILE` con `env_clear()`,
así que el binario bajo prueba **no aportaba cobertura en ningún host**.
Arreglarlo subió la cobertura de nuevo código de 60,7 % a 93,5 % de una sola vez.

**Antes de escribir un solo test**, comprueba si queda un hueco equivalente: algún
arnés, harness o helper cuya ejecución no se esté contabilizando. Es mucho más
barato que escribir 1 113 líneas de cobertura a mano.

### Después, tests portables in-process (precedente W39)

Lo que sí se puede cubrir sin Docker: construcción de argumentos, parseo de
salidas, clasificación de resultados, mapeo de errores, validación de config.
Cuando la lógica esté enredada con E/S, **extrae funciones puras** —extracciones
sin cambio de comportamiento, como `classify_mutation_records` y
`build_report`/`render` en W39— y prueba esas.

### Prohibiciones

- **No excluyas ningún archivo Rust de producto de la cobertura.** `docs/ci.md`
  registra como invariante que ninguno está excluido, y
  `scripts/test-gate-reporting.py` lo hace cumplir. Las exclusiones de
  `sonar-project.properties` son solo para arneses host-only, con una frase de
  justificación cada una en `docs/ci.md`.
- **No tests que solo ejecuten código sin aseverar nada** para inflar la métrica.
  Un test que no puede fallar no es cobertura, es ruido.

---

## 3. Limitación del entorno que condiciona todo esto

Tres imágenes Docker aprobadas fueron **borradas del host por error**
(`APPROVED_RUST_IMAGE` `384a1742…`, `APPROVED_M4_IMAGE` `25ed3626…`,
`APPROVED_M5_IMAGE` `e0a5ca16…`; solo sobrevive `APPROVED_M6_IMAGE`). En
consecuencia **`gate.py full` no puede ejecutarse** y los tests dependientes de
Docker tampoco. Ver `docs/validation/M8/matrix.md` §Pruebas.

Por eso la vía portable es la única disponible ahora, y por eso conviene: la
cobertura que se gane así se cuenta en el runner de análisis, que es donde se
mide. No intentes reprovisionar las imágenes: eso está planificado aparte, antes
de 1.0.0, y exige recualificar tres constantes de identidad aprobada.

---

## 4. Verificación obligatoria antes de dar nada por hecho

```sh
cargo build --release --locked --offline
python3 -B scripts/gate.py core            # 30/30 etapas
python3 -B scripts/contract-freeze.py verify --strict   # status: passed
python3 -B scripts/docs-hygiene.py links-check
python3 -B scripts/docs-hygiene.py verify-inventories
```

Además: `git diff --stat crates/mcp-server/tests/snapshots/` debe estar **vacío**
salvo que un cambio de contrato sea deliberado, esté documentado y lleve el
manifiesto regenerado.

Criterios inamovibles del proyecto: **nada que no corrió es pass**; un skip o un
`unavailable` no es pass; los recibos son source-bound; P0/P1 bloquean.

---

## 5. Reparto entre los dos agentes

Para que no colisionen, **nunca editan el mismo archivo a la vez**:

- **Claude Code Opus** — §1 completo (los 13 hallazgos: es trabajo de criterio
  sobre contratos, donde equivocarse cuesta caro) y las **extracciones de
  funciones puras** de §2, que tocan código de producto.
- **Codex (gpt-5.6-sol)** — §2: primero la búsqueda del hueco de
  instrumentación, y luego los **tests portables** sobre los archivos de la
  tabla, empezando por los de mayor masa sin cubrir que no requieran extracción
  previa (`quality_artifacts.rs`, `catalog_cli.rs`, `lsp_session.rs`,
  `rust_calibration.rs`).

Cada uno ejecuta §4 **completo** antes de entregar. Si uno necesita que el otro
extraiga una función antes de poder probarla, lo pide explícitamente en vez de
tocar el archivo ajeno.

> Nota de política: durante M8 Codex estaba **prohibido como worker** (solo valía
> como cliente stock para M8-04, y esos recibos siguen siendo válidos). Este
> encargo es posterior al cierre de M8 y el owner lo habilita explícitamente como
> agente de trabajo. Si se vuelve a ejecutar la matriz de clientes, Codex debe
> seguir calificándose como cliente stock sin modificar.

## 6. Entrega

Resultado real, no intención: veredicto por cada uno de los 13 hallazgos con su
razonamiento; cifra de cobertura antes y después medida en SonarCloud (no
estimada); recibo del `core` verde; y lista explícita de lo que quedó sin hacer y
por qué. Si algo queda bloqueado, identifícalo de forma reproducible con sus
dependientes y la acción necesaria. No conviertas un skip en éxito.
