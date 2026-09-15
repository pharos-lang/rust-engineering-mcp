# V04 — disposición del orquestador (2026-09-15)

Veredicto del auditor: **Approve con findings** — 0 P0, 0 P1, 0 P2 de
seguridad; 7 P3. 27 controles muestreados (≥ 2 por frontera B1–B8) existen y
se comportan como afirma el threat model; supply chain verificada (pins `=`,
`--locked`, audit/deny en gate y CI, vendor por SHA-256, acciones por SHA,
permisos mínimos, OIDC).

| ID | Sev | Disposición | Cierre |
| --- | --- | --- | --- |
| F-01 citas de `env_clear` a código de test | P3 | Aceptado: citas → `lib.rs:333-346` + call sites | W32 (threat model) |
| F-02 citas B8 desfasadas; issue §9.1 ya resuelto | P3 | Aceptado | W32 |
| F-03 `doctor` crea el lock del store y lo retiene brevemente | P3 | Aceptado: documentar (ADR-088 §3, `docs/tools.md` ya lo dice tras W30) | W32 |
| F-04 rutas `/Users/<user>` en recibos versionados | P3 | Verificado por el orquestador: `*.stdout`, `*.stderr`, `*-events.jsonl` y `state-*` **no** están versionados (`git ls-files`); sí lo están 8 `gemini-events.json` (respuesta JSON de `agy`: estado, `denied_actions`, uso de tokens — sin rutas de HOME ni credenciales, verificado por grep), que son la evidencia de la denegación headless; las rutas de HOME en recibos se relativizan al publicar con `public-export.py` (`<LOCAL_HOME>`); se anota en `08.md` | orquestador |
| F-05 token OIDC visible durante `cargo build` | P3 | Aceptado parcialmente: `persist-credentials: false` en todos los checkouts (cambio seguro); el split del job de attestation queda como RR-10 ampliado con condición de reevaluación (cambiar el workflow sin poder ejecutarlo antes de RC1 es más riesgo que beneficio) | W33 (workflow) + W32 (ADR-089) |
| F-06 `--rustsec-snapshot`/`--catalog-*` no se rechazan dentro de roots | P3 | Aceptado: rechazo consistente en `host_config.rs` + test | W33 (Rust) |
| F-07 RR-12 mezcla riesgos y precondiciones | P3 | Aceptado: branch protection re-observada, provenance 0.8.x y D14 pasan al checklist RC1 como ítems bloqueantes | W32 + `checklist-1.0.md` |
| RR-19 sin firma de código/notarización macOS | — | Aceptado como riesgo residual nuevo (Baja) con condición de reevaluación (Gatekeeper para distribución fuera de archive/attestation) | W32 |
