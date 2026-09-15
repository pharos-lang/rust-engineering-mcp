# V04 — auditoría de seguridad de cierre M8 (revisión de modelo, read-only, alcance declarado)

Modelo solicitado: Claude Opus 5 (`claude -p --model opus --effort high --tools "Read,Grep,Glob"`). Rol: auditor independiente read-only (sin Bash/Edit/Write). Orquestador: Claude Fable 5.1. **Esta es una revisión de modelo, no una auditoría humana ni un pentest** (RR-01, ADR-089); tu informe debe decirlo en su encabezado.

## Alcance declarado

Producto 0.8.0 en macOS ARM64 con gateway Docker Linux ARM64 (ADR-087). Fuentes normativas: `docs/validation/M8/08-threat-model.md` (8 fronteras B1–B8, controles con cita, RR-01…RR-18), `docs/adr/ADR-089-residual-risk-register.md`, `docs/security-model.md`, `AGENTS.md` §Seguridad, G2/G3 (`docs/roadmap/m2-m8.md`), ADR-009/024/030/031/050/077/084/085/088/090.

## Preguntas de auditoría (responde cada una con evidencia archivo:línea)

1. **Fronteras**: por cada frontera B1–B8 del threat model, ¿el control citado existe en el código y hace lo que el threat model afirma? Muestrea al menos 2 controles por frontera (≥ 16), incluyendo: `env_clear` + allowlist del gateway (`crates/execution-adapter`), seccomp/red/PIDs/memoria de los contenedores, cancel/EOF → join del árbol y cuarentena, no-follow/reparse-safe del filesystem propio (`crates/project-adapter/src/filesystem/macos`), grants de host (`host_config.rs`), writer único M2 y journal (`mutation.rs`), verificación Ed25519 + floor del catálogo (`catalog-adapter`), E5/ORT por SHA-256 (`semantic-adapter`), redacción en audit/logs, `contract`/`doctor` sin efectos.
2. **Cambios de M8**: ¿introducen superficie nueva? `contract` CLI (estático), `doctor.mutation_journals` (abre el store: ¿puede un state-root hostil provocar lectura fuera de él, pánico, o bloqueo?), listas wire (`resources/templates/list`: ¿revela algo?), `--state-root` solo en doctor, scripts de release (`release-smoke.py` verifica hashes fail-closed), rollback driver (`git worktree` con `--end-of-options`).
3. **Registro de riesgos**: ¿algún riesgo residual RR-n está mal clasificado (debería ser P1/P2 bloqueante para una primera versión estable en macOS)? ¿Falta alguno evidente?
4. **Secretos/evidencia**: ¿los recibos y transcripts bajo `docs/validation/M8/` contienen rutas de `HOME`, tokens, cabeceras o datos del usuario que no deberían publicarse? (Grep dirigido: `Authorization`, `token`, `api_key`, `/Users/`, `ghp_`, `sk-`.)
5. **Supply chain**: pins `=` en `Cargo.toml`, `cargo audit/deny` en el gate, vendor por SHA-256, workflows con acciones fijadas por SHA, permisos mínimos y OIDC — cita.

## Salida

Encabezado: «Revisión de modelo (Claude Opus 5 High), read-only, alcance declarado; no es auditoría humana ni pentest». Findings por archivo con severidad P0–P3, evidencia y acción; tabla de controles muestreados (frontera, control, archivo:línea, veredicto); veredicto global `Approve` / `Approve con findings` / `Block`. **P0/P1 bloquean la readiness**; P2 de seguridad bloquea readiness salvo aceptación explícita con ADR. No inventes rutas: si no puedes abrir algo, dilo.
