# M2 — revisión acotada del último delta de resolución

Claude Code CLI 2.1.260, `claude-opus-5`, high, safe-mode/read-only, sin tools,
MCP ni subagentes. [Inputs y hashes](M2-closure-resolution-inputs.json),
[prompt exacto](m2-closure-packages/M2-closure-resolution-opus-prompt.txt),
[resultado bruto](M2-closure-resolution-opus.json) y [ejecución CLI](M2-closure-resolution-run.json).

**Accepted; sin P0/P1/P2 en los cinco hunks posteriores al recheck de seguridad.**
El owner comprobó los bytes previos extraídos del paquete Accepted contra su SHA
`16d225d692d601d709c45263c68dfe69ad9cabf997544e14f4b94ea00ff490c7` y el archivo
final `8c54b84ca315ccee6a96038246447d6682ab09defc248285fc75671782c1e3de`.
El delta restringe la clasificación de ausencia a exit 101/terminación limpia,
agrega `mutation_gateway.rs` al fingerprint y fortalece oráculos unitarios y reales.
No cambia grants, candidatos aceptables ni el orden de cleanup.

Disposición del Technical Owner:

- P3 código 101 y cadenas Cargo: acotado a Cargo 1.98.1/imagen aprobada. Otro
  runtime exige calificación, y un diagnóstico no reconocido falla sin candidato.
- P3 binding enumerado: el fingerprint enumera cinco archivos y configuración
  efectiva; no se anuncia una clausura transitiva de todos los helpers. El full
  liga adicionalmente todos los inputs. Extender esa identidad se evalúa ante
  un cambio futuro concreto, sin alterar ahora el contrato ni recibos históricos.
- P3 rama OOM/no-clean: no tiene una inyección específica nueva; revisión del
  flujo verifica que retorna Failed dentro de `work` y luego ejecuta cleanup.
  Se conserva esta limitación sin atribuir una prueba inexistente.
- P3 versión ausente: el test unitario distingue ambos diagnósticos; el fixture
  real contiene quote 1.0.47 y solicita 9.9.9. El resultado runtime complementa
  esa composición, sin convertirlo en una garantía para otros Cargo.

Dos precisiones del dictamen no se adoptan como contratos: `Failed` por candidato
Cargo inválido conserva `isError=false`, mientras missing offline data es
operacional; ambos rechazan el candidato. Una rotación de fingerprints tampoco
invalida un receipt histórico: el journal conserva la validación aprobada y no se
compara con un runtime nuevo para autorizar otra operación. Los tests de replay
e identidad de ADR-059 y el gate completo cubren esa frontera por separado.

El reviewer no ejecutó pruebas ni recalculó hashes; lo hizo el owner al preparar
el paquete y los gates. Su campo “Files changed” enumera el delta revisado, no
ediciones realizadas por Claude. Uso auxiliar Haiku 4.5 registrado por CLI no
constituye otro reviewer. No se certifica M2 Done solo con este dictamen.
