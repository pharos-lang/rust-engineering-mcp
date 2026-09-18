# V02 — revisión independiente read-only del freeze 0.8.0 (W05 Rust + W06 Python + W07/W08 docs)

Modelo solicitado: Claude Opus 5 (`claude -p --model opus --effort high --tools ""`). Rol: revisor read-only sin tools; material inline (diff completo y archivos clave). Orquestador: Claude Fable 5.1.

Revisa como Principal Engineer, contra `AGENTS.md` (hexagonal, stdout solo protocolo en `serve`, sin `unwrap` en rutas normales), ADR-086 (D11), ADR-087 (D13), spec §55–58 y G1/G5/G6:

1. **Contrato**: ¿los 31 snapshots `stable` quedan byte-idénticos y solo cambian las 5 descripciones `preview`? ¿El prefijo altera algo más que la `description`? ¿`contract --json` es determinista, estático (sin Docker/host/red) y su hash canónico es reproducible desde Python tal como se especifica? ¿La tabla cerrada de `stability`/`executes_project_code`/`requires_runtime` es exhaustiva y está probada contra `list_tools`?
2. **Oráculo de freeze** (`scripts/contract-freeze.py`): ¿`verify` falla ante cualquier cambio en un `stable` (nombre, schema, annotations, description) y ante cambio de conteo? ¿Puede dar falso pass (p. ej. hash de schema calculado sobre una vista incompleta, orden de claves, `ensure_ascii`)? ¿`diff --base` usa `git show` sin `shell=True`? ¿Cumple las reglas Sonar de la casa (sin `/tmp` literal, sin `type=` en argparse, rutas constantes)?
3. **Versión y migración**: ¿el bump a 0.8.0 es coherente en `Cargo.toml`/`Cargo.lock`/CHANGELOG/docs? ¿Las migration notes 0.3.0→0.8.0 son exactas (5 tools añadidas, 1 grant nuevo, `binary.bloat` solo descripción, 0 deprecaciones, 13 M1 byte-idénticas a 0.1.0)? ¿Alguna promesa nueva no calificada?
4. **D13**: ¿ADR-087 y las notas de spec §61/§97 cambian el alcance con honestidad («resuelto por cambio de alcance», no «cumplido») y sin reescribir hechos de 0.1.0/0.3.0?

Salida: findings por archivo con P0–P3 (P0/P1 bloquean; P2 de contrato/gate/seguridad bloquea readiness), evidencia (cita corta) y acción; veredicto `Approve` / `Approve con findings` / `Block`. Si un archivo que necesitas no está inline, dilo como limitación en vez de suponer.
