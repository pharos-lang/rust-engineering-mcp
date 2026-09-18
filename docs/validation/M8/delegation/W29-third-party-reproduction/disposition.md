# W29 — disposición del orquestador (2026-09-15)

Intentos: W29/W29b (cuota del CLI agotada), W29c (allowlist: rutas absolutas/`./`),
**W29d** (105 turnos): entregable [06-reproduction.md](../../06-reproduction.md).
Veredicto del tercero: **parcialmente reproducible** (1 P1, 1 P2, 2 P3) más dos
limitaciones del sandbox de la sesión (clientes interactivos bloqueados;
tubería de una pasada que cancela llamadas con worker), declaradas como no
imputables a la documentación.

| ID | Sev | Disposición | Cierre |
| --- | --- | --- | --- |
| F-1 README «Instalar la release» no completable sin release publicada | P1 (docs) | Aceptado: el README indica explícitamente que sin release publicada para la versión del checkout se usa «Compilar desde el código fuente», y que la release 0.8.x llegará con RC1 (M8-09); vía de compilación reproducida íntegramente | W35 |
| F-2 `mutation list --state-root <vacío>` → `blocked/io` mientras `doctor` lo ve limpio | P2 (producto/CLI) | Aceptado: un store nunca inicializado debe listarse como vacío (`passed`, 0 registros) o declararse `not_initialized` sin `io`; coherente con `doctor.mutation_journals`; test de CLI | W34 (tras W33) |
| F-3 CHANGELOG «Calificación nativa pendiente del orquestador» (M6) | P3 | Aceptado: anotar «calificada en el gate `full` de M6 (`69a0be14…`)» | W35 |
| F-4 `--help` «development server» sin explicación pública | P3 | Aceptado: docs explican el literal y su relación release/checkout (sin cambiar el literal: forma parte de la salida ya probada por `cli.rs`) | W35 |
| Limitaciones de sesión (clientes interactivos; tubería de una pasada) | — | No imputables a la documentación; la calificación de clientes reales la acredita [clients.json](../../clients.json) (Inspector/Codex/Claude Code) | — |
