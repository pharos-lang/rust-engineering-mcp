# W06 — informe del worker (Claude Sonnet 5, Medium; claude 2.1.268)

Invocación: `claude -p --model sonnet --effort medium --permission-mode acceptEdits --disallowedTools Agent Task --allowedTools <lista> --no-session-persistence --output-format json < prompt-header.md`. Inicio 2026-09-14T16:01:27Z, fin 2026-09-14T16:05:04Z, exit 0, 40 turnos, 216405 ms, permission_denials: 5.

---

## Informe W06 — M8-02 (decisiones 5–7)

**Task:** implementar `scripts/contract-freeze.py` (generate/verify/diff), sus tests, generar `docs/validation/M8/02-schema-diff.json`, y añadir la etapa `core` del gate — sin tocar `freeze-0.8.0.json` (aún no lo genera el orquestador).

**Result:** completado. Los 36 nombres de tool reales se clasifican exactamente como 31 `stable` + 5 `preview` (los cinco `rust.analyzer.*`), verificado contra el repo en vivo por un test dedicado.

**Files changed:**
- `scripts/contract-freeze.py` (nuevo) — subcomandos `generate`, `verify [--strict]`, `diff --base REF --out PATH [--only NAME,...]`.
- `scripts/test-contract-freeze.py` (nuevo) — 10 casos `unittest`.
- `scripts/gate.py` — añadida `contract-freeze-tests` (etapa `core`, justo después de `m6-runtime-unit-tests`) y `contract-freeze` condicional a `Path.exists()` sobre `docs/validation/M8/freeze-0.8.0.json`. No existe patrón previo de etapa "skipped" en `gate.py` (no hay `status: skipped` en ningún `run(...)`), así que seguí la instrucción de omitir la etapa por completo cuando el manifiesto no existe, sin registrarla en `report['steps']`. Nada más del gate cambió.
- `docs/validation/M8/02-schema-diff.json` (generado).

**Resumen del diff:**
- `since_v0.1.0_m1_only` (13 nombres M1, filtro `--only`): **13 unchanged**, 0 added/removed/changed. Confirma la decisión 7 (13 M1 byte-idénticas desde v0.1.0).
- `since_v0.3.0` (36 tools actuales vs 31 en v0.3.0): **5 added** (los `rust.analyzer.*`), **1 changed** (`rust.binary.bloat`), **30 unchanged**.
  - **Hallazgo a reportar sin maquillar:** la decisión 6 dice "solo en `description`", pero el cambio real en `rust.binary.bloat` está en `$defs/BloatProfile/description` **dentro de `inputSchema` y `outputSchema`** (una ruta de doc reescrita por docs-hygiene: `docs/validation/M5-04-bloat-calibration.json` → `docs/validation/M5/04-bloat-calibration.json`), no en el campo `description` de nivel superior de la tool, que es idéntico byte a byte. Mi script reporta esto correctamente como `input_schema_changed`/`output_schema_changed` = `true`, `keys_changed: ["inputSchema","outputSchema"]`, `annotations_changed: false`. La decisión 6 debería corregirse para reflejar que el cambio vive en el schema, no en la `description` de la tool.

**Salida de tests:**
```
python3 -B scripts/test-contract-freeze.py   → Ran 10 tests, OK
python3 -B scripts/test-gate-reporting.py    → Ran 13 tests, OK
python3 -c "...json.load(...02-schema-diff.json)" → valid json
```

**Risks:**
- El hallazgo de `binary.bloat` arriba puede requerir ajustar el texto de la decisión 6 en `02.md` (no lo edité — fuera de mis archivos permitidos).
- `verify` compara `stability`/`annotations`/hashes de schema y `description`, pero **no** usa `snapshot_sha256` como criterio de fallo (solo lo registra como evidencia extra); un cambio de bytes sin cambio semántico no dispara fallo — esto es intencional según la letra del encargo ("cualquier diferencia... en schema, annotations, description").

**Open issues:**
- No generé `docs/validation/M8/freeze-0.8.0.json` (según instrucción); la etapa `contract-freeze` del gate quedará inactiva hasta que el orquestador lo cree.
- Ningún commit realizado, como se pidió.
