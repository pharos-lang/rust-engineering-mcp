# Prompt para cerrar M0/M1 y entregar el handoff de planificación M2–M8

Copia el bloque siguiente en una sesión nueva de Codex abierta en
`<LOCAL_HOME>/Projects/rust-mcp`.

```text
Asume el rol de Technical Owner, arquitecto, integrador y revisor final de Rust
Engineering MCP en <LOCAL_HOME>/Projects/rust-mcp. Tu objetivo es cerrar de
forma verificable todo M0 y M1/0.1.0, publicar la release 0.1.0 únicamente cuando
cumpla sus gates y dejar el repositorio limpio para comenzar M2. No implementes
ninguna feature de M2 o posterior en esta sesión.

Como último entregable obligatorio, después de cerrar M0/M1, crea y entrégame otro
prompt autosuficiente para una sesión nueva que genere la planificación completa de
M2 a M8. Guarda ese prompt en docs/prompts/plan-m2-m8.md y también inclúyelo completo,
en un bloque copiable, en tu respuesta final. Esta segunda sesión será solo de
planificación: no debe implementar M2–M8.

ESTADO DE PARTIDA QUE DEBES VERIFICAR, NO ASUMIR

- Checkout local esperado: main limpio en e8a2336c48cede8139edc28f07b563fe624a4be8.
- Snapshot público esperado: d2192037e55362e2834969db627844c2f734a50f.
- Repositorio público: https://github.com/pharos-lang/rust-engineering-mcp.
- Última CI pública live conocida: run 33928952807, verde en Linux x86_64, macOS
  ARM64, Windows x86_64 y supply chain. El recibo repo-visible anterior referencia
  correctamente el run 33928437393; no confundas el recibo histórico con el último
  run sobre el commit público documental final.
- main público está protegido para administradores, con actualización estricta,
  esos cuatro checks obligatorios y sin force-push ni borrado.
- M0 figura Done. M1-01..14 y M1-16 figuran Done. M1-15 y M1-17 figuran Blocked.
- El contrato público M1 contiene exactamente trece tools. No añadas
  rust.dependencies.inspect ni amplíes el contrato.
- La fuente está bajo MIT OR Apache-2.0 y el copyright pertenece a IUMotion Labs.
- GitHub es el canal de fuente; GitHub Releases es el canal binario inicial;
  crates.io permanece deshabilitado.
- No hay release ni tag 0.1.0 conocidos.

Si cualquiera de esos datos cambió, usa el estado live y registra la diferencia.
No confíes en hashes, rutas temporales, versiones instaladas o resultados anteriores
sin verificarlos.

INICIO OBLIGATORIO

1. Lee completamente AGENTS.md y la especificación
   docs/spec/rust-engineering-mcp-propuesta-v0.3.md.
2. Lee docs/implementation-status.md, docs/ci.md, docs/publication.md,
   docs/validation/M1-17.md, docs/validation/M1-17-matrix.md,
   docs/validation/M1-17-review-disposition.md, docs/release/preparation.md,
   docs/release/offline-candidates.md y todos los ADR relevantes, especialmente
   ADR-009, 019, 029, 031, 038, 041 y 047.
3. Inspecciona git status, ramas, árbol, manifests, Cargo.lock, tests, CI pública,
   branch protection, releases, tags, alertas y artifacts reales.
4. Construye una matriz repo-visible que trace cada entregable y Definition of Done
   de M0/M1 contra evidencia actual. Distingue implementación, evidencia,
   decisión del owner y dependencia externa. No reabras trabajo cerrado sin una
   contradicción concreta; tampoco conserves un Done que la evidencia invalide.
5. Trabaja desde la primera brecha real. No rediseñes desde cero.

ALCANCE Y CRITERIO DE CIERRE

No declares M0/M1 cerrados porque el workspace compile. El cierre exige, como mínimo:

- ninguna fila M0/M1 requerida en Blocked, In progress o Not started;
- ninguna decisión pendiente que sea requisito para los artifacts 0.1.0 elegidos;
- las trece tools y sus schemas/structuredContent/Resources verificadas;
- full gate sobre los bytes finales del candidato y gates focalizados afectados;
- CI pública core/protocolo/catálogo en la matriz de OS soportada;
- security tests reales para cada capability anunciada y fail-closed verificable
  donde la plataforma no pueda garantizarla;
- instalación, ejecución y doctor del artifact final en cada target publicado;
- inventario de dependencias por artifact, licencias/notices/SBOM y hashes;
- provenance/attestations verificables y ningún secreto en fuente, logs o artifacts;
- MCP Inspector y un flujo stock Codex dirigido por modelo que use el servidor real;
- revisión independiente final sin P0/P1 abierto y disposición explícita de cada
  finding restante;
- README, CHANGELOG, SECURITY, arquitectura, tools, compatibilidad, ADR, estado y
  recibos sincronizados;
- repositorios local y público limpios, CI final verde, tag/release coherentes y
  evidencia enlazada desde docs/implementation-status.md.

La CI portable ya observada no acredita por sí sola sandbox, filesystem protegido,
ORT, LanceDB ni distribución nativa. Un skip no es un pass. La especificación permite
bloquear tools donde falten garantías, pero cualquier reducción de plataformas,
features o artifacts anunciados requiere decisión explícita, ADR y documentación
antes de cambiar el gate. No sacrifiques seguridad para obtener una matriz verde.

BRECHAS CONOCIDAS QUE DEBES RESOLVER O DISPONER FORMALMENTE

1. Reconciliar la CI pública actual con la matriz M1-17, que todavía describe como
   ausentes varios recibos nativos. Determina qué demuestra cada runner y qué falta.
2. Linux/Windows/x86: completar adapters, harnesses y pruebas nativas necesarias para
   las capabilities que se anuncien. Windows requiere semántica no-follow/reparse-safe
   antes de habilitar acceso protegido; si no existe, debe fallar cerrado y el
   soporte publicado debe decirlo con precisión. Aprovecha runners GitHub hospedados
   por el OS real para ejecutar harnesses nativos dentro de sus límites; reemplaza
   skips portables por pruebas discriminantes cuando exista implementación. No llames
   "native qualified" a un job que solo compiló o verificó comportamiento fail-closed.
3. Resolver la evidencia de licencia/redistribución de Kanaria 0.2.0, el modelo E5 y
   la atribución estática de ORT. Usa el Cargo.lock, revisiones exactas y fuentes
   oficiales. No inventes textos ni trates un campo license del manifest como si
   fuera el archivo exigido para redistribuir.
4. Generar notices e inventario finales por cada artifact/target. Separa claramente
   el binario core, features locales, modelo y catálogo. Si un componente no tiene
   evidencia suficiente, exclúyelo del artifact afectado y documenta la consecuencia;
   no lo distribuyas por conveniencia. El workflow actual empaqueta las licencias del
   código original pero no genera todavía un SBOM ni el inventario final de terceros:
   impleméntalos y comprueba el contenido de cada archive antes de release. Excluir
   E5/ORT/LanceDB del artifact inicial exige reconciliar explícitamente el criterio
   vigente de que un build sin feature local no califica M1; cualquier descope requiere
   ADR/spec/status y evidencia de que el contrato restante sigue cumpliéndose.
5. Decidir y documentar el alcance de catálogos 0.1.0. Si IUMotion Labs distribuye
   un catálogo firmado, define y prueba generación, custodia segura, rotación y
   revocación de la clave Ed25519. La clave pública va en trust config; la privada
   nunca entra al repositorio, prompts, logs ni Actions sin un mecanismo aprobado.
   La fixture seed42 está prohibida. Si 0.1.0 no distribuye catálogo oficial, fija
   formalmente esa frontera en ADR/spec/status y demuestra que el verifier/importer
   M1 sigue completo sin fingir que existe una publicación.
6. Completar el benchmark/gate del modelo aplicable a la distribución: calidad ES/EN,
   CPU, RAM, startup y limitaciones por target. Conserva la conclusión honesta del
   piloto M1-16: el endpoint quedó saturado y no probó equivalencia ni valor causal.
7. Repetir el uso stock Codex dirigido por modelo con un escenario candidato-bound:
   el modelo debe descubrir y llamar el servidor real, abrir/inspeccionar un fixture,
   ejecutar check, observar un diagnóstico estructurado, editar con sus capacidades
   host autorizadas, repetir check o quality.gate hasta verde y observar al menos un
   error estructurado de runtime ausente. Registra transcript, hashes, configuración,
   tools realmente llamadas y cleanup. No sustituyas un intento fallido por llamadas
   directas del controlador.
8. Corregir la deuda de scripts/gate.py para registrar inicio, fin y conteos de forma
   directa en futuros recibos; no reescribas timestamps históricos.
9. Confirmar que M0 sigue íntegro, en especial M0-04/M0-06/M0-11. Si una limitación
   fail-closed es compatible con su contrato, documéntala; si contradice el DoD,
   impleméntala y vuelve a calificarla.

MODELO DE TRABAJO Y POOL DE AGENTES

Codex/GPT es el Technical Owner. Conserva arquitectura, alcance, contratos públicos,
security model, decisiones, integración Git, publicación y cierre. Usa subagentes
Codex solo para subtareas independientes y acotadas, con archivos o evidencia
disjuntos. Mantén como máximo una oleada que quepa en los slots disponibles; evita
que dos workers editen las mismas interfaces centrales. Una asignación inicial útil
es: (a) auditoría de matriz nativa y harnesses, (b) licencias/notices/artifacts, y
(c) trazabilidad documental y release. Usa High para (a) y (b), y Medium para (c),
salvo que una dificultad concreta justifique cambiarlo. El principal integra y
verifica todo.

Claude Code es reviewer externo read-only:

- Claude Sonnet 5 (`claude-sonnet-5`), effort medium o high: revisiones habituales
  de cortes, contratos, tests, documentación, notices y diffs focalizados.
- Claude Opus 5 (`claude-opus-5`), effort high: arquitectura, sandbox, filesystem,
  threat model, supply chain y revisión final de cierre. Usa medium solo para un
  follow-up pequeño ya delimitado.
- Verifica `claude --version`, `claude --help` y disponibilidad del modelo antes de
  cada campaña. La versión observada al redactar este prompt fue 2.1.260.
- Invoca modelo explícito, sin fallback, sin herramientas, sin MCP y sin persistencia,
  mediante las opciones actuales equivalentes a `--print`, `--model`, `--effort`,
  `--tools ""`, `--strict-mcp-config`, `--permission-mode dontAsk`,
  `--permission-prompts none`, `--no-session-persistence` y `--output-format json`.
- Entrega paquetes pequeños que indiquen reglas, archivos, hashes y preguntas.
  Conserva output/receipt, confirma modelUsage real y realiza disposición principal.

Por instrucción explícita del owner en este prompt, Gemini 3.8 se incorpora como
asistente adicional read-only mediante la CLI Antigravity `agy`. Su uso complementa
la política de reviewers de AGENTS.md; no reemplaza la revisión Claude requerida ni
recibe autoridad arquitectónica, legal o de cierre:

- `gemini-3.8-flash-medium`: lectura amplia del repositorio, inventarios y comparación
  mecánica entre spec, ADR, tablero, evidencia y workflows.
- `gemini-3.8-flash-high`: investigación oficial por revisión exacta, comparación de
  alternativas y búsqueda adversarial de contradicciones en matriz/licencias/notices.
- Verifica `agy --version`, `agy --help` y `agy models`. La versión observada fue
  1.1.26 y los IDs anteriores estaban disponibles. Usa `--model` explícito, esfuerzo
  proporcional, `--mode plan`, `--sandbox` y output JSON cuando aplique.
- Gemini no decide arquitectura ni licencia y no edita. Toda conclusión se valida
  contra la fuente primaria y la versión fijada en Cargo.lock.

No dupliques la misma tarea en tres modelos por rutina. Usa Gemini para cobertura
amplia e investigación/comparación, Sonnet para revisión precisa de cortes, Opus
para riesgos sistémicos y cierre, y Codex para implementación e integración. Sí usa
dos revisiones independientes cuando una decisión de seguridad, licencia o release
lo justifique. Ejecuta investigaciones y reviews independientes en paralelo cuando
no compartan estado; los gates Docker, las mutaciones Git y la publicación son
secuenciales. Registra quién revisó qué, modelo/versión/effort, alcance, hashes y
limitaciones. Ninguna salida de modelo reemplaza evidencia ejecutable.

Uso mínimo obligatorio del pool en esta sesión: una auditoría inicial de trazabilidad
con Gemini 3.8 High, revisión Sonnet 5 de cada corte material o paquete final de cortes
afines, una revisión Opus 5 de seguridad/arquitectura para cambios nativos y una
revisión Opus 5 del candidato final. Evita convocar un modelo si no existe un paquete
concreto y verificable que revisar.

IMPLEMENTACIÓN, GIT Y VALIDACIÓN

- Respeta arquitectura hexagonal, deny-by-default, Execution Gateway único, I/O
  handle-relative no-follow, env limpio, límites y kill-tree. No uses shell con input
  del usuario ni describas check/test como seguros.
- Usa Rust/Cargo 1.98.1, lockfile y versiones reales. Consulta documentación oficial
  actual cuando MCP, rmcp, Cargo, GitHub Actions, SQLite, LanceDB, RustSec, licencias
  o APIs puedan haber cambiado.
- Por cada corte: tipos/contrato, pruebas discriminantes, camino completo, gate
  focalizado, revisión principal, review externo proporcional, ADR/docs/status y
  gate post-merge.
- En el repositorio local usa una rama `ai/` por unidad desde main limpio, commits
  coherentes, merge `--no-ff` y smoke post-merge. Preserva cambios ajenos.
- El historial local privado no se publica. Para GitHub usa scripts/public-export.py,
  verifica `PUBLICATION-SNAPSHOT.json`, cero credenciales/rutas privadas y sincroniza
  un clone público separado. Como main público está protegido, publica una rama,
  abre PR, espera todos los checks y mergea sin bypass. No empujes el historial local.
- El gate normal es fmt, check, Clippy -D warnings y tests workspace/all-targets.
  Ejecuta además contract/protocol/integration/security/native/full según impacto.
  El cierre usa `python3 scripts/gate.py full` con prerequisitos explícitos y assets
  exactos. No instales ni refresques dependencias del producto silenciosamente.
- Queda autorizada la instalación de herramientas de calificación oficiales en una
  carpeta local aislada, con versión e integridad verificadas, sin scripts de
  instalación ni cambios globales. MCP Inspector oficial 2.5.0 ya fue autorizado
  bajo esas condiciones. Registra cada instalación. No extiendas esta autorización
  a runtimes/modelos redistribuibles ni a secretos.

PUBLICACIÓN 0.1.0

Cuando y solo cuando la matriz de cierre esté satisfecha sobre el candidato final:

1. Produce el snapshot público saneado mediante el exporter y un PR contra main.
2. Espera CI verde, revisa el diff público y mergea respetando branch protection.
3. Crea y empuja el tag versionado coherente con la política SemVer del repo.
4. Ejecuta el workflow manual de release candidate desde ese tag.
5. Verifica archives por target, instalación/doctor, hashes, LICENSE/NOTICE/SBOM,
   attestations OIDC y correspondencia source→artifact.
6. Si todo pasa, publica la GitHub Release 0.1.0. Este prompt autoriza el tag, el PR,
   el merge y la publicación de esa release exclusivamente después de satisfacer y
   registrar los gates. crates.io sigue deshabilitado. No publiques modelos o
   catálogos cuya autorización/evidencia siga incompleta.

Si una limitación externa hace imposible cerrar un requisito, no marques Done ni
fabriques evidencia. Completa todo lo demás, demuestra exactamente el bloqueo y pide
al usuario solo la decisión o credencial mínima que no puedas resolver. No te detengas
por decisiones reversibles o por trabajo difícil. Continúa hasta cierre real.

HANDOFF OBLIGATORIO PARA PLANEAR M2–M8

Después de que M0/M1 estén realmente cerrados, crea
docs/prompts/plan-m2-m8.md. Ese segundo prompt debe partir de los commits, tag,
release, ADRs y gates finales reales, no de los hashes históricos de este prompt.
Debe instruir una sesión nueva para:

- planificar M2, M3, M4, M5, M6, M7 y M8, sin implementar;
- mantener YouTrack fuera salvo que el repo lo habilite explícitamente;
- producir un roadmap repo-visible con cortes verticales pequeños, dependencias,
  contratos, threat model, ADRs requeridos, pruebas, gates, evidencias y DoD;
- definir por milestone objetivos, fuera de alcance, riesgos, migration/compatibility,
  distribución, observabilidad, performance y criterios verificables por terceros;
- tratar M7 remoto como condicional a un caso de uso real;
- incluir la ruta de estabilización 0.8–0.9 y readiness para 1.0.0;
- asignar nuevamente el pool Codex/Claude/Gemini según sus fortalezas;
- detectar contradicciones entre el roadmap de la spec y la evidencia final M1;
- terminar con una secuencia ejecutable de prompts/handoffs por milestone.

Tu respuesta final de esta sesión de cierre debe indicar: estado M0/M1, commits
local/público, tag/release, CI y artifacts; decisiones tomadas; gates y reviews;
cualquier limitación que no afecte el cierre; enlaces a evidencia; worktrees limpios;
y el bloque completo del nuevo prompt M2–M8. No presentes trabajo M2 implementado.
```
