# I01 — integración M8-01 (Claude Sonnet 5, Low; claude 2.1.268)

Invocación: `claude -p --model sonnet --effort low --permission-mode acceptEdits --disallowedTools Agent Task --allowedTools Read,Bash(git status:*),Bash(git diff:*),Bash(git add:*),Bash(git commit:*),Bash(git log:*),Bash(python3:*) --no-session-persistence --output-format json < prompt-header.md`. Inicio 2026-09-14T14:56:03Z, fin 2026-09-14T14:56:29Z, exit 0, 25444 ms.

Veredicto: **Hecho**. Pre-checks verdes (links-check 0 rotos; verify-inventories 7/0). Commits en `ai/m8-stabilization`:

| Commit | Contenido |
| --- | --- |
| `9ecd945` | D11 → ADR-086 (+ índice ADR, backlog D11, compatibility.md) |
| `2e2e75d` | `--help` con 36 tools; docs públicas con M6 integrado y `preview`; tabla de hitos m2-m8 |
| `e445aa1` | Censo M8-01, `01.md`, matriz y registro de delegación |

Árbol limpio tras los commits. Sin push (requiere autorización separada). El registro de este paquete se commitea aparte (I01b).

---

**Task:** I01 — integrate M8-01 into `ai/m8-stabilization` as three commits, integrator role only (no edits).

**Result:** Pre-checks green (`links-check`: 0 broken in living documents; `verify-inventories`: 7 inventories, 0 failures). Created exactly the three requested commits with the specified paths and messages.

**Commit hashes:**
1. `9ecd945` — docs(m8): decide D11 — ADR-086 deprecation and freeze policy
2. `2e2e75d` — fix(m8): --help lists all 36 tools; public docs describe the integrated M6 state
3. `e445aa1` — docs(m8): M8-01 census, surface gate decisions and delegation register

**`git status --short`:** empty (working tree clean).

**Open issues:** None. No push performed.
