# ADR-007 — Handles explícitos y autoridad de roots

Date: 2026-09-03

## Context

Aceptar cualquier path proporcionado por el caller autoautorizaría acceso al host.
MCP no ofrece sesiones implícitas, pero recomienda handles explícitos cuando un
servidor mantiene contexto.

## Decision

El host/CLI confiable configura roots permitidos antes de `project.open`; el proyecto
y el caller solo pueden restringir, nunca ampliar ese conjunto. `project.open`
canonicaliza y valida un workspace dentro de esos roots y crea un identificador
opaco, aleatorio (mínimo 128 bits), no derivado del path. El registro vive durante el
proceso, expira por inactividad configurable y se pierde al reiniciar. Cada uso
revalida root e identidad básica; handle inválido/expirado produce error recuperable.
El I/O propio abre paths relativos a handles de directorio con no-follow y comprobación
de reparse points equivalente; canonicalizar y luego abrir por nombre no es frontera
de seguridad porque permite TOCTOU. Los procesos externos quedan contenidos por el
sandbox OS incluso si el árbol cambia después de validar.

Separar `ProjectIdentityFingerprint` (manifests/root) de
`ExecutionFingerprint` (identidad + toolchain, target, features, args y policy).
Path dependencies externas requieren estar dentro de roots preautorizados.

## Alternatives considered

- Path en cada tool: aumenta traversal, tokens e inconsistencias.
- Hash del path como handle: adivinable, filtra identidad y no expresa autoridad.
- Registro persistente: conserva contexto tras restart, pero crea revocación y
  autorización durable innecesarias para M1.

## Consequences

El servidor mantiene estado explícito sin depender de una conexión MCP. Los clientes
deben reabrir tras restart/expiry. Tests cubren symlinks/junctions, intercambio
concurrente, reemplazo del directorio y handles falsificados. Si una plataforma no
ofrece el primitive necesario, no anuncia containment estricto.

## Status

Accepted.

Source: <https://modelcontextprotocol.io/specification/2026-07-28/server/tools>
