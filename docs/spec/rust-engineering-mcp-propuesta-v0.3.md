# Rust Engineering MCP

## Especificación de arquitectura, producto y roadmap

**Versión del documento:** 0.3.1  
**Estado:** Baseline de producto, aclarada por los ADRs aceptados en `docs/adr/`  
**Autor original:** Cesar Burgos - Pharos Team  
**Revisión:** 2026-09-03  
**Nombre de trabajo del producto:** `rust-engineering-mcp`  

---

# 1. Resumen ejecutivo

Este documento propone el diseño de un **Model Context Protocol (MCP) server especializado en ingeniería de software con Rust**, orientado a mejorar la calidad del código producido por agentes como Claude, Gemini, ChatGPT/Codex y otros clientes compatibles con MCP.

La propuesta parte de una idea fundamental:

> El MCP no debe intentar reemplazar el razonamiento del agente. Debe proporcionarle evidencia verificable, capacidades deterministas y operaciones seguras sobre proyectos Rust.

El valor principal del MCP será permitir que un agente pase de:

```text
generar código → asumir que funciona
```

a:

```text
generar código
→ inspeccionar el proyecto
→ validar con herramientas reales
→ interpretar diagnósticos estructurados
→ corregir
→ volver a validar
→ verificar seguridad
→ medir rendimiento cuando corresponda
→ entregar evidencia de calidad
```

El servidor se implementará en **Rust** y utilizará el SDK oficial **`rmcp`**.

La primera versión debe ser deliberadamente pequeña. El MVP se concentrará en las operaciones que generan mayor impacto para un agente:

- inspección del proyecto;
- detección del toolchain;
- `cargo check`;
- `cargo clippy`;
- `cargo test`;
- `cargo fmt --check`;
- auditoría de dependencias;
- interpretación estructurada de diagnósticos;
- ejecución de un quality gate configurable;
- catálogo local de crates y versiones con **SQLite**;
- búsqueda semántica de crates por intención con **LanceDB**;
- funcionamiento **offline-first**, incluyendo provenance y freshness de los datos del ecosistema.

Las operaciones mutables, profiling avanzado, mutation testing, Miri, generación de proyectos, búsqueda semántica profunda de documentación y acceso remoto se incorporarán progresivamente.

---

# 2. Decisiones principales

Las siguientes decisiones deben considerarse la línea base de arquitectura.

Los ADRs aceptados en `docs/adr/` resuelven ambigüedades operativas de esta
propuesta y tienen precedencia cuando especifican con mayor precisión lifecycle MCP,
seguridad, estados de resultados, artifacts o autoridad del host.

| Decisión | Selección |
| --- | --- |
| Lenguaje del MCP | Rust |
| SDK MCP | `rmcp` oficial |
| Runtime asíncrono | Tokio |
| Transporte local principal | `stdio` |
| Transporte remoto futuro | Streamable HTTP |
| RPC envelope MCP | JSON-RPC 2.0 gestionado por `rmcp` |
| Comunicación interna | Tipos Rust fuertemente tipados |
| Contratos de tools | `inputSchema` / `outputSchema` mediante JSON Schema |
| Resultado principal | `structuredContent` tipado |
| Ejecución de comandos | `std::process::Command` / `tokio::process::Command`, nunca shell arbitrario |
| Arquitectura | Hexagonal / Ports & Adapters |
| Seguridad | Deny-by-default para filesystem, red, variables de entorno y ejecución |
| Estado del servidor | Stateless por defecto |
| Persistencia principal | **SQLite** embebido como fuente de verdad del catálogo local |
| Índice semántico | **LanceDB** embebido como índice vectorial derivado |
| Estrategia de datos | Offline-first; red no requerida durante la ejecución normal del MCP |
| Búsqueda de crates | Híbrida: SQLite FTS/metadata + LanceDB vector similarity + filtros autoritativos |
| Embeddings | `EmbeddingProvider` abstraído; consulta local y snapshots/imports compatibles con entornos air-gapped |
| Cache | SQLite inicialmente; siempre separado conceptualmente del catálogo autoritativo |
| Configuración | TOML |
| Distribución primaria | GitHub Releases; 0.1.0 publica el artifact core macOS ARM64 definido por ADR-048 |
| Distribución secundaria | `cargo install` / crates.io |
| Versionado | SemVer + matriz de compatibilidad MCP |
| Estrategia de tools | Pocas tools de alto valor, composables y con salida estructurada |
| LLM interno | No en el core |
| Actualización automática | Fuera del protocolo MCP; CLI opcional |
| Mutaciones del proyecto | Deshabilitadas por defecto |

---

# 3. Objetivo

Construir un MCP que proporcione a los agentes capacidades especializadas para:

1. comprender correctamente la estructura de un proyecto Rust;
2. conocer el toolchain y restricciones reales del proyecto;
3. validar código contra `rustc`;
4. detectar errores idiomáticos con Clippy;
5. ejecutar pruebas de forma controlada;
6. verificar formato;
7. analizar dependencias;
8. detectar vulnerabilidades conocidas;
9. identificar uso de `unsafe`;
10. validar compatibilidad y API pública;
11. medir cobertura;
12. ejecutar análisis avanzados cuando sean necesarios;
13. medir rendimiento con evidencia;
14. entregar información estructurada que un agente pueda utilizar para iterar.

---

# 4. No objetivos

El MCP **no** debe convertirse inicialmente en:

- un IDE completo;
- un reemplazo de `rust-analyzer`;
- un reemplazo de Cargo;
- un sistema de CI/CD;
- un agente autónomo adicional;
- un segundo LLM que genere código;
- un shell remoto;
- un wrapper genérico para ejecutar cualquier comando;
- una plataforma de despliegue;
- una base de conocimiento gigantesca embebida;
- una herramienta que modifique código sin límites claros.

Estas restricciones evitan que el proyecto se vuelva demasiado amplio y reducen significativamente la superficie de ataque.

---

# 5. Problema que resuelve

Los agentes actuales pueden producir Rust sintácticamente plausible, pero tienen dificultades recurrentes en áreas donde el lenguaje depende fuertemente de información contextual o verificación real.

Ejemplos:

- reglas de ownership;
- préstamos simultáneos;
- lifetimes;
- trait bounds;
- inferencia de tipos;
- Send/Sync;
- concurrencia;
- async;
- features de crates;
- MSRV;
- editions;
- APIs cambiantes;
- warnings de Clippy;
- vulnerabilidades de dependencias;
- uso innecesario de `unsafe`;
- comportamiento dependiente del target;
- regresiones de rendimiento;
- cambios SemVer en librerías.

La respuesta correcta no es crear una gran colección de reglas heurísticas duplicando el compilador.

La respuesta es **conectar al agente con las herramientas que ya conocen la verdad del proyecto**.

---

# 6. Principio fundamental: evidence-driven coding

Cada tool debe intentar devolver evidencia verificable.

Por ejemplo:

```text
Incorrecto:

"Creo que este código podría tener un problema de lifetime."

Correcto:

rustc E0515
archivo: src/service.rs
línea: 87
span: 87:9-87:21
mensaje: cannot return value referencing local variable ...
```

El MCP debe privilegiar:

```text
compiler evidence
> static analysis
> project metadata
> ecosystem metadata
> heuristics
```

Las heurísticas deben utilizarse únicamente cuando no exista una fuente determinista mejor.

---

# 7. Cómo ayuda a los agentes

## 7.1. Reduce alucinaciones técnicas

El agente deja de depender exclusivamente de su conocimiento entrenado.

Puede consultar:

- versión de Rust;
- edition;
- MSRV;
- features activas;
- dependencias;
- errores reales;
- warnings reales;
- tests reales.

---

## 7.2. Mejora los ciclos de reparación

Un flujo típico puede ser:

```text
agent writes code
      │
      ▼
rust.check
      │
      ├── success ──► rust.clippy
      │
      └── diagnostics
              │
              ▼
       agent fixes code
              │
              └──────► rust.check
```

Esto convierte una generación de código abierta en un ciclo de ingeniería verificable.

---

## 7.3. Permite decisiones basadas en el proyecto

El agente puede saber antes de generar código:

```text
Rust edition: 2024
MSRV: 1.98.1
Workspace: 12 crates
Async runtime: Tokio
Error model: thiserror
Serialization: serde
Forbidden crates: ...
Unsafe policy: deny
```

El resultado debería ser significativamente más consistente con el proyecto existente.

---

## 7.4. Mejora seguridad

El servidor puede incorporar validaciones como:

- RustSec;
- `cargo-deny`;
- inspección de `unsafe`;
- Miri;
- control de fuentes de crates;
- verificación de licencias;
- detección de dependencias duplicadas;
- bloqueo de ejecución fuera de roots preautorizados por el host.

---

## 7.5. Mejora rendimiento

En fases posteriores el agente podrá obtener evidencia mediante:

- Criterion;
- profiling;
- flamegraphs;
- análisis de tamaño;
- asignaciones;
- comparación de benchmarks.

La recomendación de rendimiento debe surgir preferiblemente de datos, no de intuición.

---

# 8. Alineación con MCP actual

El diseño debe seguir la especificación MCP vigente y evitar decisiones heredadas de versiones anteriores.

## 8.1. Transportes

Transportes estándar previstos:

### Local

```text
stdio
```

Debe ser el transporte principal durante las primeras versiones.

Ventajas:

- simple;
- sin puerto;
- sin autenticación HTTP;
- compatible con ejecución local;
- superficie de ataque reducida;
- fácil integración con clientes MCP.

### Remoto

```text
Streamable HTTP
```

Se añadirá únicamente cuando exista una necesidad real de operación remota.

No se utilizará WebSocket como transporte estándar del proyecto salvo que aparezca un requerimiento específico externo.

---

## 8.2. MCP y JSON-RPC: el envelope de comunicación

MCP utiliza **JSON-RPC 2.0** como envoltura de comunicación entre cliente y servidor.
La baseline vigente del proyecto es MCP `2026-07-28`. Su lifecycle moderno es
stateless y usa `server/discover`; `initialize` / `initialized` se conserva únicamente
para compatibilidad negociada con revisiones legacy soportadas por `rmcp`.

Por ejemplo, una invocación conceptual a una tool puede representarse así:

> El ejemplo omite por brevedad la metadata `_meta` obligatoria de la revisión
> `2026-07-28`; `rmcp` debe construir y validar el envelope real.

```json
{
  "jsonrpc": "2.0",
  "id": 42,
  "method": "tools/call",
  "params": {
    "name": "rust.check",
    "arguments": {
      "project_ref": "prj_91af",
      "all_targets": true
    }
  }
}
```

El proyecto **no implementará JSON-RPC manualmente**.

El adapter basado en `rmcp` será responsable de:

```text
MCP lifecycle (`server/discover` y compatibilidad legacy negociada)
JSON-RPC envelope
request/response correlation
capability negotiation
tools/list
tools/call
resources
prompts
cancellation
transport integration
```

El dominio y los casos de uso no conocerán JSON-RPC.

Arquitectónicamente:

```text
Agent
  │
  │ MCP
  ▼
JSON-RPC 2.0 envelope
  │
  ▼
rmcp adapter
  │
  ▼
Application use case
```

---

## 8.3. JSON Schema: contrato de inputs y outputs

**JSON Schema no sustituye JSON-RPC.**

Su función es describir la estructura aceptada y devuelta por cada capability MCP.

Cada tool publicará:

```text
inputSchema
outputSchema
```

siguiendo la versión de JSON Schema admitida por la especificación MCP negociada.

Para MCP `2026-07-28`, si no se declara `$schema`, el dialecto por defecto es JSON
Schema 2020-12. `inputSchema` debe tener raíz objeto. Aunque `outputSchema` puede ser
cualquier tipo JSON, este proyecto usará objetos raíz para conservar compatibilidad
con clientes legacy. Los DTOs públicos rechazarán campos desconocidos y validarán
invariantes de dominio además del schema.

Ejemplo conceptual:

```json
{
  "name": "rust.check",
  "inputSchema": {
    "type": "object",
    "additionalProperties": false,
    "properties": {
      "project_ref": { "type": "string" },
      "all_targets": { "type": "boolean" }
    },
    "required": ["project_ref", "all_targets"]
  },
  "outputSchema": {
    "type": "object",
    "additionalProperties": false,
    "properties": {
      "status": {
        "type": "string",
        "enum": ["passed", "failed", "blocked", "unavailable", "cancelled"]
      },
      "diagnostics": {
        "type": "array",
        "items": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "source": { "type": "string" },
            "severity": { "type": "string", "enum": ["error", "warning", "note", "help"] },
            "code": { "type": ["string", "null"] },
            "message": { "type": "string" },
            "file": { "type": "string" },
            "line": { "type": "integer", "minimum": 1 }
          },
          "required": ["source", "severity", "code", "message"]
        }
      },
      "duration_ms": {
        "type": "integer"
      },
      "error_code": {
        "type": ["string", "null"],
        "enum": [
          "PROJECT_NOT_FOUND",
          "INVALID_PROJECT",
          "TOOL_NOT_INSTALLED",
          "LOCKFILE_UPDATE_REQUIRED",
          "COMMAND_TIMEOUT",
          "SANDBOX_DENIED",
          "NETWORK_DENIED",
          "UNSUPPORTED_PLATFORM",
          "OUTPUT_LIMIT_EXCEEDED",
          null
        ]
      },
      "error_message": {
        "type": ["string", "null"]
      }
    },
    "required": ["status", "diagnostics", "duration_ms", "error_code", "error_message"]
  }
}
```

Los schemas **no deben mantenerse manualmente** cuando puedan derivarse de los tipos Rust.

Patrón recomendado:

```text
Rust type
   │
   ├── serde ───────────► JSON
   │
   └── schemars ────────► JSON Schema
```

Esto convierte los tipos Rust en la principal fuente de verdad del contrato y reduce divergencias entre:

```text
implementación
schema
documentación
tests
```

---

## 8.4. Salida de las tools: `structuredContent`

La salida orientada al agente debe ser estructurada.

Modelo conceptual:

```text
JSON-RPC response
└── result
    └── structuredContent
        └── objeto tipado del dominio
```

Ejemplo:

```json
{
  "jsonrpc": "2.0",
  "id": 42,
  "result": {
    "resultType": "complete",
    "content": [
      {
        "type": "text",
        "text": "{\"status\":\"failed\",\"diagnostics\":[{\"source\":\"rustc\",\"severity\":\"error\",\"code\":\"E0502\",\"file\":\"src/lib.rs\",\"line\":42,\"message\":\"cannot borrow ...\"}],\"duration_ms\":842,\"error_code\":null,\"error_message\":null}"
      }
    ],
    "structuredContent": {
      "status": "failed",
      "diagnostics": [
        {
          "source": "rustc",
          "severity": "error",
          "code": "E0502",
          "file": "src/lib.rs",
          "line": 42,
          "message": "cannot borrow ..."
        }
      ],
      "duration_ms": 842,
      "error_code": null,
      "error_message": null
    },
    "isError": false
  }
}
```

Regla del proyecto:

> La información que el agente necesite procesar programáticamente debe viajar en `structuredContent` y cumplir el `outputSchema`.

Texto humano adicional puede utilizarse como resumen cuando aporte valor, pero no será la representación autoritativa del resultado.
Para compatibilidad con clientes que aún no consuman `structuredContent`, el adapter
también incluirá el JSON serializado en un bloque `TextContent`, generado desde el
mismo resultado tipado.

Por tanto, la pila completa queda definida como:

```text
Protocol:
    MCP

RPC envelope:
    JSON-RPC 2.0

Transport:
    stdio en MVP
    Streamable HTTP posteriormente

Tool contracts:
    JSON Schema

Primary tool result:
    structuredContent

Internal representation:
    structs/enums Rust fuertemente tipados
```

---

## 8.5. Stateless por defecto

El servidor no debe depender de una sesión implícita.

Cuando una operación necesite mantener referencia a un contexto se utilizarán handles explícitos.

Ejemplo:

```json
{
  "project_ref": "project_7fb93"
}
```

El `project_ref` representa un workspace previamente validado por el servidor. No
autoriza por sí mismo un path: solo puede referir a roots configurados previamente
por el host confiable, expira y se revalida en cada uso según ADR-007.

---

## 8.6. Tool annotations

Cada tool debe declarar correctamente sus características.

Ejemplo conceptual:

```text
readOnlyHint
destructiveHint
idempotentHint
openWorldHint
```

Estas anotaciones ayudan al cliente a comprender el riesgo de cada operación.

No sustituyen la política de seguridad del servidor.

---

# 9. No todo debe ser una Tool

Uno de los cambios más importantes respecto a una implementación ingenua consiste en aprovechar las distintas primitivas de MCP.

---

## 9.1. Tools

Se usarán para operaciones que realizan trabajo.

Ejemplos:

```text
rust.check
rust.test
rust.clippy
rust.audit
rust.coverage
```

---

## 9.2. Resources

Se podrán utilizar para exponer información consultable y reutilizable.

Ejemplos conceptuales:

```text
rust-project://workspace/metadata
rust-project://workspace/dependencies
rust-project://workspace/toolchain
rust-project://workspace/diagnostics/latest
rust-catalog://status
rust-catalog://crate/{name}
```

Esto evita ejecutar una tool costosa solo para recuperar información que ya fue calculada.

---

## 9.3. Prompts

Podrán utilizarse posteriormente para ofrecer workflows recomendados.

Ejemplos:

```text
review-rust-change
prepare-rust-release
investigate-rust-performance
harden-rust-project
```

El prompt no sustituye al agente.

Le proporciona una secuencia recomendada de verificaciones.

---

# 10. Arquitectura propuesta

```text
┌───────────────────────────────────────────────────────┐
│                    MCP CLIENT                         │
│ Claude / Codex / Gemini / IDE / Agent Runtime         │
└─────────────────────────┬─────────────────────────────┘
                          │
                    MCP protocol
                          │
┌─────────────────────────▼─────────────────────────────┐
│                 TRANSPORT ADAPTER                     │
│                                                       │
│           stdio          Streamable HTTP              │
└─────────────────────────┬─────────────────────────────┘
                          │
┌─────────────────────────▼─────────────────────────────┐
│                    MCP SERVER                         │
│                                                       │
│  Tool Registry │ Resources │ Prompts │ Capability API │
└─────────────────────────┬─────────────────────────────┘
                          │
┌─────────────────────────▼─────────────────────────────┐
│               APPLICATION / USE CASES                 │
│                                                       │
│ ProjectInspect     CheckProject       RunTests        │
│ AuditProject       QualityGate        Benchmark       │
└─────────────────────────┬─────────────────────────────┘
                          │
┌─────────────────────────▼─────────────────────────────┐
│                     DOMAIN                            │
│                                                       │
│ ProjectRef      Diagnostic      Toolchain             │
│ Finding         QualityGate     ExecutionPolicy       │
│ Artifact        Dependency      Capability            │
└─────────────────────────┬─────────────────────────────┘
                          │
┌─────────────────────────▼─────────────────────────────┐
│                       PORTS                           │
│                                                       │
│ ProcessRunner       FileSystem       CargoMetadata    │
│ SecurityScanner     Registry         Analyzer         │
│ CatalogRepository   SemanticIndex    EmbeddingProvider│
│ Cache               Sandbox          Clock            │
└─────────────────────────┬─────────────────────────────┘
                          │
┌─────────────────────────▼─────────────────────────────┐
│                     ADAPTERS                          │
│                                                       │
│ cargo      rustc      clippy      rustfmt             │
│ rust-analyzer         RustSec     crates.io           │
│ SQLite Catalog        LanceDB Semantic Index          │
│ Cargo Local Cache     Snapshot Import/Sync            │
│ cargo-deny            Miri        Criterion           │
│ cargo-nextest         cargo-llvm-cov                  │
└───────────────────────────────────────────────────────┘
```

---

# 11. Razones para usar Rust

Rust es la elección recomendada para implementar el servidor.

## 11.1. Integración natural

El MCP operará permanentemente sobre:

- Cargo;
- crates;
- metadata;
- manifests;
- toolchains;
- targets;
- estructuras Rust.

Bibliotecas útiles:

- `cargo_metadata`;
- `toml_edit`;
- `serde`;
- `schemars`;
- `tokio`;
- `tracing`.

---

## 11.2. Distribución

El servidor puede distribuirse como binario.

Esto simplifica el onboarding:

```bash
rust-engineering-mcp --stdio
```

---

## 11.3. Seguridad de memoria

El propio MCP manejará:

- procesos;
- archivos;
- streams;
- datos externos;
- parsing;
- concurrencia.

Rust reduce una parte importante de los riesgos de memoria de una herramienta de este tipo.

---

## 11.4. Performance

Aunque el MCP no necesita latencias extremadamente bajas, una implementación ligera permite:

- startup rápido;
- bajo consumo de RAM;
- análisis concurrentes;
- ejecución local constante.

---

# 12. SDK MCP

La implementación debe utilizar:

```toml
rmcp
```

como SDK oficial de Rust para MCP.

No se recomienda mantener una abstracción artificial que permita cambiar de SDK sin costo.

El SDK debe encapsularse únicamente en el adapter MCP para evitar contaminar el dominio.

```text
domain
application
ports
adapters
    └── mcp-rmcp
```

De esta forma un cambio del SDK afecta principalmente el borde del sistema.

---

# 13. Estructura del repositorio

Una estructura inicial adecuada sería:

```text
rust-engineering-mcp/
├── Cargo.toml
├── Cargo.lock
├── rust-toolchain.toml
├── rustfmt.toml
├── clippy.toml
├── deny.toml
├── README.md
├── LICENSE
├── CHANGELOG.md
├── SECURITY.md
├── CONTRIBUTING.md
├── docs/
│   ├── architecture.md
│   ├── tools.md
│   ├── security-model.md
│   ├── compatibility.md
│   └── adr/
│       ├── ADR-001-rust.md
│       ├── ADR-002-rmcp.md
│       ├── ADR-003-stdio-first.md
│       ├── ADR-004-execution-sandbox.md
│       └── ADR-005-structured-diagnostics.md
├── crates/
│   ├── mcp-server/
│   ├── application/
│   ├── domain/
│   ├── cargo-adapter/
│   ├── rustsec-adapter/
│   ├── analyzer-adapter/
│   ├── sqlite-catalog-adapter/
│   ├── lancedb-semantic-adapter/
│   ├── catalog-sync/
│   ├── sandbox/
│   └── test-support/
├── migrations/
│   └── sqlite/
├── fixtures/
│   ├── compile-errors/
│   ├── clippy/
│   ├── security/
│   └── workspaces/
└── .github/
    └── workflows/
```

Para un MVP extremadamente pequeño también podría utilizarse un único crate.

Sin embargo, la separación indicada evita que el código MCP termine mezclado con ejecución de procesos y lógica de análisis.

---

# 14. Modelo de proyecto

No se recomienda permitir que cada tool reciba un `project_path` arbitrario.

En su lugar:

```text
rust.project.open
```

valida el workspace y devuelve:

```json
{
  "project_ref": "prj_c81f",
  "workspace_root": "/workspace/my-project"
}
```

Las operaciones posteriores reciben:

```json
{
  "project_ref": "prj_c81f"
}
```

Beneficios:

- evita path traversal;
- centraliza la validación;
- facilita permisos;
- reduce tokens;
- simplifica caching;
- permite asociar configuración;
- evita inconsistencias entre tools.

---

# 15. Project fingerprint

Cada proyecto puede tener un fingerprint derivado de:

```text
Cargo.toml
Cargo.lock
rust-toolchain.toml
.cargo/config.toml
workspace members
selected features
target triple
```

Ejemplo:

```text
sha256:29f5...
```

Este valor permite determinar si ciertos resultados almacenados siguen siendo válidos.

---

# 16. Context snapshot

Una tool clave será:

```text
rust.project.inspect
```

Debe devolver información útil para el agente antes de modificar código.

Ejemplo:

```json
{
  "workspace": {
    "members": 8,
    "default_members": 6
  },
  "toolchain": {
    "channel": "stable",
    "rustc": "1.xx.x",
    "edition": "2024",
    "msrv": "1.xx"
  },
  "targets": [
    "x86_64-unknown-linux-gnu"
  ],
  "features": {
    "workspace": []
  },
  "dependencies": {
    "direct": 26,
    "total": 143
  },
  "policies": {
    "unsafe": "deny",
    "network_execution": false
  }
}
```

---

# 17. Contrato estándar de las tools

Las tools no deben devolver texto libre como salida principal.

Se recomienda una estructura común.

```json
{
  "status": "passed",
  "summary": "cargo check completed successfully",
  "duration_ms": 1240,
  "error_code": null,
  "error_message": null,
  "project_fingerprint": "sha256:...",
  "diagnostics": [],
  "findings": [],
  "artifacts": [],
  "provenance": null,
  "freshness": null,
  "metadata": {}
}
```

`provenance` y `freshness` serán opcionales para resultados puramente locales del proyecto y obligatorios cuando la respuesta dependa de snapshots del ecosistema, registries, advisories o información remota.

Estados comunes:

```text
passed
failed
blocked
unavailable
cancelled
```

`failed` indica que la tool se ejecutó correctamente y encontró un fallo del proyecto.
Errores operativos recuperables como timeout, tooling ausente o sandbox denegado se
devuelven en un envelope estructurado con `isError=true`. Los errores JSON-RPC quedan
para request MCP malformada, tool desconocida o fallo interno que impida responder.

---

# 18. Diagnóstico estructurado

Uno de los elementos de mayor valor será normalizar diagnósticos de herramientas Rust.

Modelo:

```json
{
  "source": "rustc",
  "severity": "error",
  "code": "E0502",
  "message": "cannot borrow ... as mutable because it is also borrowed as immutable",
  "file": "src/lib.rs",
  "line": 42,
  "column": 9,
  "end_line": 42,
  "end_column": 18,
  "rendered": "...",
  "suggestions": [],
  "documentation": null
}
```

Un agente procesa mucho mejor este formato que una captura de stdout completa.

---

# 19. Captura de salida de Cargo

Siempre que sea posible se utilizarán formatos machine-readable.

Por ejemplo:

```text
cargo check --message-format=json
```

El adapter debe parsear los eventos y producir objetos propios.

No se debe obligar al agente a interpretar cientos de líneas de logs.

---

# 20. Tool design rules

Cada tool debe cumplir las siguientes reglas.

1. Nombre corto y predecible.
2. Una responsabilidad clara.
3. Entrada mínima.
4. Schema estricto.
5. Output estructurado.
6. Timeouts definidos.
7. Límites de tamaño.
8. Sin shell arbitrario.
9. Sin rutas fuera de roots preautorizados por el host.
10. Anotaciones MCP correctas.
11. Idempotencia documentada.
12. Errores categorizados.
13. Output truncado de forma explícita.
14. Posibilidad de obtener el artefacto completo mediante resource cuando sea necesario.

---

# 21. Taxonomía de operaciones

Las tools se clasifican por riesgo.

| Clase | Descripción | Ejemplo |
| --- | --- | --- |
| R0 | Lectura pura | `project.inspect` |
| R1 | Compilación/análisis; puede ejecutar build scripts o proc macros | `rust.check` |
| R2 | Ejecuta código del proyecto | `rust.test` |
| R3 | Modifica archivos | `rust.fmt.apply` |
| R4 | Modifica dependencias/configuración | `dependency.add` |
| R5 | Ejecuta operaciones externas o de red | búsqueda remota explícita futura / sincronización fuera del runtime MCP |

Esto permite configurar políticas.

La clase ordinal no es la frontera de seguridad. Cada operación declara además
efectos concretos (`executes_project_code`, reads/writes, network, external process y
sandbox requerido). `rust.check` y `rust.clippy` se tratan como ejecución potencial
de código no confiable aunque permanezcan en R1 por ergonomía del catálogo.

Ejemplo:

```toml
[permissions]
max_risk = "R2"
```

---

# 22. Catálogo de tools

El catálogo completo se divide por fase.

---

# 23. Tools MVP — 0.1.0

## 23.1. `rust.project.open`

Registra y valida un proyecto.

**Tipo:** R0  
**Read only:** sí

Entrada:

```json
{
  "path": "/workspace/project"
}
```

Salida:

```json
{
  "project_ref": "prj_...",
  "workspace_root": "...",
  "fingerprint": "..."
}
```

---

## 23.2. `rust.project.inspect`

Obtiene contexto estructural.

Incluye:

- workspace members;
- packages;
- targets;
- edition;
- MSRV cuando pueda inferirse explícitamente;
- toolchain;
- features;
- perfiles;
- configuración Cargo relevante;
- dependencias directas.

---

## 23.3. `rust.toolchain.inspect`

Devuelve:

```text
rustc version
cargo version
toolchain channel
host triple
installed targets
installed components
```

No debe consultar Internet por defecto.

---

## 23.4. `rust.check`

Ejecuta validación de compilación.

Equivalente conceptual:

```bash
cargo check
```

Opciones permitidas:

```text
package
workspace
features
all_features
no_default_features
all_targets
target
```

No se permitirán flags arbitrarios.

---

## 23.5. `rust.clippy`

Ejecuta Clippy.

Configuración segura:

```text
workspace
package
features
all_targets
lint_profile
```

Perfiles sugeridos:

```text
default
strict
pedantic
project
```

No debe imponerse `pedantic` globalmente porque algunos lints requieren decisiones de diseño contextual.

---

## 23.6. `rust.fmt.check`

Ejecuta:

```text
cargo fmt --check
```

Devuelve archivos afectados y diff cuando sea razonablemente pequeño.

---

## 23.7. `rust.test`

Ejecuta pruebas.

Inicialmente:

```text
cargo test
```

Debe soportar:

```text
package
test_filter
features
all_features
target
timeout
```

No debe aceptar un comando arbitrario después de `--`.

---

## 23.8. `rust.dependencies.audit`

Integra RustSec mediante `cargo-audit` o librería equivalente.

Devuelve:

- advisory;
- crate;
- versión vulnerable;
- versión corregida;
- dependencia raíz;
- severidad cuando esté disponible;
- árbol de dependencia relevante.

---

## 23.9. Inspección interna de dependencias (sin tool pública M1)

Devuelve:

- dependencias directas;
- transitivas;
- versiones;
- features;
- duplicados;
- sources.

Se basa principalmente en `cargo_metadata` y alimenta `project.inspect`, audit y el
catálogo. `rust.dependencies.inspect` no forma parte de las trece tools públicas del
alcance inmediato M1; exponerla requiere una decisión de scope posterior.

---

## 23.10. `rust.diagnostics.explain`

En lugar de implementar una tool heurística llamada `borrow_check_help`, esta tool utilizará evidencia de `rustc`.

Entrada:

```json
{
  "code": "E0502"
}
```

Puede utilizar:

```text
rustc --explain E0502
```

De esta forma el agente obtiene la explicación correspondiente al compilador instalado.

---

## 23.11. `rust.quality.gate`

Tool compuesta.

Ejemplo:

```json
{
  "project_ref": "prj_...",
  "profile": "fast"
}
```

Perfil `fast`:

```text
fmt-check
check
clippy
```

Perfil `standard`:

```text
fmt-check
check
clippy
test
audit
```

---

# 24. Por qué `quality.gate` es importante

Los agentes no siempre conocerán todas las tools disponibles.

Un workflow compuesto reduce el número de decisiones requeridas.

Ejemplo:

```text
agent
  │
  └── rust.quality.gate(profile="standard")
               │
               ├─ fmt
               ├─ check
               ├─ clippy
               ├─ test
               └─ audit
```

La salida debe conservar detalle suficiente para que el agente repare los problemas encontrados.

---

# 25. Tools 0.2.x — edición segura

Decisión de implementación M2, 2026-09-05: [ADR-050](../adr/ADR-050-local-coordinated-mutation.md)
fija el modo local_coordinated: permiso host explícito, preview/digest, precondiciones,
locks entre instancias MCP, journal y recuperación conservadora. No requiere broker
privilegiado ni cuentas/ownership nuevos. El host mantiene estable el namespace y evita
escrituras simultáneas sobre archivos aplicados durante commit. No se promete exclusión
OS de editores externos, CAS por contenido ni atomicidad visible multiarchivo. Un
conflicto observado antes de publicar no escribe source; uno posterior conserva
backups y puede exigir recuperación sin sobrescribir bytes desconocidos. Esta limitación
no relaja containment de paths ni el sandbox para código de proyecto.


Después de estabilizar el core podrán añadirse operaciones mutables.

---

## 25.1. `rust.fmt.apply`

Aplica `rustfmt`.

Características:

- idempotente;
- restringida al workspace;
- devuelve archivos modificados;
- devuelve diff.

---

## 25.2. `rust.fix.apply`

Ejecuta `cargo fix` con una política controlada.

Debe requerir permisos explícitos.

---

## 25.3. `rust.dependency.add`

Añade una dependencia.

Debe usar Cargo o edición estructurada del manifest.

Nunca concatenar texto manualmente.

---

## 25.4. `rust.dependency.remove`

Elimina una dependencia.

---

## 25.5. `rust.manifest.patch`

Modifica propiedades permitidas de `Cargo.toml`.

Ejemplos:

```text
features
profiles
workspace dependencies
lints
```

No debe convertirse en un editor TOML genérico sin controles.

---

# 26. Tools 0.3.x — calidad avanzada

---

## 26.1. `rust.test.nextest`

Integra `cargo-nextest`.

Beneficios:

- ejecución paralela;
- aislamiento por test;
- filtros;
- timeouts;
- mejor salida machine-readable;
- mejor experiencia en workspaces grandes.

Debe considerarse opcional porque no todos los proyectos lo tendrán instalado.

---

## 26.2. `rust.coverage`

Integra `cargo-llvm-cov`.

Salida:

```json
{
  "line_percent": 84.2,
  "region_percent": 81.9,
  "functions_percent": 88.4,
  "uncovered": []
}
```

Los HTML generados deben exponerse como artifacts/resources, no dentro de la respuesta MCP.

---

## 26.3. `rust.mutation.test`

Integra `cargo-mutants`.

Es una operación costosa.

Debe ejecutarse como tarea larga cuando el cliente MCP lo soporte.

Casos de uso:

- validar tests críticos;
- revisar librerías;
- quality gates de release.

No debe ejecutarse automáticamente en cada cambio.

---

## 26.4. `rust.semver.check`

Integra `cargo-semver-checks`.

Especialmente útil cuando el proyecto es una crate pública o una librería interna versionada.

Puede detectar cambios incompatibles en API.

---

# 27. Tools 0.4.x — seguridad avanzada

---

## 27.1. `rust.deny`

Integra `cargo-deny`.

Checks:

```text
advisories
licenses
bans
sources
```

---

## 27.2. `rust.unsafe.scan`

Analiza uso explícito de:

```rust
unsafe
unsafe fn
unsafe impl
extern
```

Debe diferenciar:

- código del workspace;
- dependencias.

No debe declarar automáticamente que `unsafe` es un problema.

Debe mostrar dónde existe y permitir al agente revisar invariantes.

---

## 27.3. `rust.miri`

Integra Miri para detectar categorías de Undefined Behavior.

Debe considerarse:

- costoso;
- dependiente del toolchain;
- no compatible con todos los proyectos;
- potencialmente lento.

Por ello no pertenece al MVP.

---

## 27.4. `rust.supply_chain.inspect`

Combina información de:

- crates;
- sources;
- advisories;
- duplicados;
- git dependencies;
- yanked versions;
- optional features.

El objetivo es dar al agente una vista resumida del riesgo de la cadena de dependencias.

---

# 28. Tools 0.5.x — performance

---

## 28.1. `rust.benchmark.run`

Ejecuta benchmarks existentes.

No debe generar automáticamente benchmarks en el core.

---

## 28.2. `rust.benchmark.compare`

Compara baseline y candidate.

Ejemplo:

```json
{
  "benchmark": "parse_document",
  "baseline_ns": 1100,
  "candidate_ns": 860,
  "change_percent": -21.8
}
```

---

## 28.3. `rust.profile.flamegraph`

Genera un artifact.

La respuesta MCP devuelve metadata:

```json
{
  "artifact": "artifact://profile/abc123",
  "duration_ms": 10000
}
```

---

## 28.4. `rust.binary.bloat`

Integra `cargo-bloat`.

Permite analizar:

- funciones más pesadas;
- crates con mayor contribución al binario;
- regresiones de tamaño.

---

# 29. Tool que NO se recomienda: `suggest_optimizations`

No debería existir una tool genérica:

```text
suggest_optimizations(code)
```

si simplemente implementa heurísticas.

El agente ya puede razonar sobre el código.

El MCP debe ofrecer evidencia:

```text
benchmark
profile
allocations
binary size
compiler diagnostics
```

A partir de esos datos el agente realizará la optimización.

Si en el futuro se implementa una engine de reglas fiable, podría añadirse como capability separada.

---

# 30. Tool que NO se recomienda: `idiomatic_rust`

Una tool que devuelve recomendaciones genéricas sobre Rust aporta poco valor y puede desactualizarse.

Alternativas más robustas:

```text
rust.clippy
rust-analyzer diagnostics
rustc diagnostics
rustdoc
official documentation resources
```

---

# 31. Tool que NO se recomienda como core: `generate_test`

Generar un test es trabajo natural del agente.

El MCP debe ayudar a:

```text
identificar tests existentes
ejecutarlos
medir cobertura
mutation testing
encontrar gaps
```

El agente puede generar el código de test basándose en esa evidencia.

---

# 32. Integración con rust-analyzer

Debe incorporarse después del MVP.

Capacidades útiles:

- diagnostics;
- hover;
- go-to-definition;
- references;
- symbols;
- code actions;
- rename feasibility.

El MCP no debe duplicar LSP.

Se construirá un adapter.

```text
AnalyzerPort
      │
      └── RustAnalyzerAdapter
```

---

# 33. Consulta de documentación

La propuesta inicial planteaba `rustdoc_query`.

Debe dividirse conceptualmente.

---

## 33.1. Documentación local

Debe priorizarse.

Fuentes:

```text
rustdoc del proyecto
metadata
source code
installed std docs
```

---

## 33.2. Documentación remota

Puede consultar:

- docs.rs;
- crates.io;
- documentación oficial.

Esta operación pertenece a clase `openWorld`.

Debe poder deshabilitarse completamente.

---

# 34. Catálogo local de crates y conocimiento del ecosistema

El catálogo forma parte del **MVP**.

Su objetivo es permitir que el agente consulte crates, versiones y metadata útil incluso cuando el proceso MCP no tenga acceso a Internet.

Tools MVP:

```text
rust.catalog.status
rust.crate.search
rust.crate.inspect
```

La arquitectura no dependerá de una única base de datos para todos los patrones de acceso.

Se utilizarán dos almacenes embebidos con responsabilidades diferentes:

```text
SQLite  → fuente de verdad estructurada
LanceDB → índice semántico derivado
```

---

## 34.1. Principio: offline-first

La ejecución normal del MCP debe poder funcionar con:

```text
network = deny
```

Esto incluye:

- abrir proyectos;
- inspeccionar dependencias;
- buscar crates conocidas por el snapshot;
- realizar búsqueda semántica sobre el índice local;
- consultar versiones conocidas;
- validar freshness;
- consultar advisories previamente sincronizados.

La red será una capacidad opcional, no una dependencia operativa.

---

## 34.2. SQLite como fuente de verdad

SQLite será la base autoritativa para información estructurada.

Datos sugeridos:

```text
crates
crate_versions
crate_features
dependencies
licenses
sources
advisories
crate_advisories
catalog_snapshots
documents
sync_metadata
```

Ejemplo conceptual:

```text
crate
 ├─ versions
 │    ├─ dependencies
 │    ├─ features
 │    ├─ rust_version
 │    ├─ yanked
 │    └─ checksum
 ├─ licenses
 ├─ advisories
 └─ metadata
```

SQLite se utiliza porque aporta:

- almacenamiento embebido;
- un único archivo;
- transacciones ACID;
- índices;
- joins;
- constraints;
- migraciones;
- FTS5;
- operación cross-platform;
- cero proceso servidor.

El adapter recomendado será conceptualmente:

```text
CatalogRepository
        │
        └── SqliteCatalogRepository
```

La librería Rust concreta deberá quedar encapsulada en este adapter.

---

## 34.3. LanceDB como índice semántico del MVP

LanceDB formará parte del MVP y se utilizará para búsquedas por intención.

Ejemplo:

```text
"Necesito una crate para serialización binaria zero-copy"
```

La búsqueda semántica puede recuperar candidatos conceptualmente relacionados aunque la descripción no contenga exactamente las mismas palabras.

Contenido inicial candidato a indexación:

```text
crate name
crate description
keywords/categories
README summary
selected documentation summary
API/topic summary cuando exista
```

LanceDB **no será la fuente de verdad** de versiones, vulnerabilidades o políticas.

Será un índice derivado.

```text
SQLite
   │
   │ catalog build/sync
   ▼
Embedding pipeline
   │
   ▼
LanceDB
```

Si el índice vectorial se elimina o corrompe:

```text
SQLite snapshot
      │
      ▼
rebuild embeddings/index
      │
      ▼
LanceDB
```

No debe perderse información autoritativa.

---

## 34.4. Búsqueda híbrida

`rust.crate.search` no dependerá únicamente de embeddings.

El flujo recomendado será:

```text
                         Query
                           │
              ┌────────────┴────────────┐
              ▼                         ▼
       SQLite FTS5                  LanceDB
     lexical / BM25              vector similarity
              │                         │
              └────────────┬────────────┘
                           ▼
                     candidate merge
                           │
                           ▼
                  metadata filtering
                        SQLite
                           │
           ┌───────────────┼────────────────┐
           ▼               ▼                ▼
         MSRV          license policy     yanked
           │               │                │
           └───────────────┼────────────────┘
                           ▼
                     advisories
                           │
                           ▼
                      reranking
                           │
                           ▼
                    final candidates
```

La semántica ayuda a descubrir.

SQLite decide los facts autoritativos.

---

## 34.5. `CompositeCatalog`

El dominio no dependerá de SQLite, LanceDB ni crates.io directamente.

Se definirá conceptualmente:

```rust
trait CatalogRepository {
    async fn crate_by_name(...);
    async fn versions(...);
    async fn dependencies(...);
    async fn advisories(...);
}

trait SemanticIndex {
    async fn search(...);
    async fn index(...);
    async fn delete(...);
}

trait EmbeddingProvider {
    async fn embed_query(...);
    async fn embed_documents(...);
}

trait CacheStore {
    async fn get(...);
    async fn put(...);
}
```

Y un servicio:

```text
CompositeCatalog
```

que combinará:

```text
1. metadata del proyecto
2. Cargo.lock / cargo metadata
3. cache local de Cargo
4. SQLite Catalog
5. LanceDB Semantic Index
6. fuente remota únicamente cuando la policy lo permita
```

Orden recomendado:

```text
project/local facts
        >
local catalog snapshot
        >
semantic index
        >
Internet
```

---

## 34.6. Datos del proyecto vs catálogo global

No todo debe consultarse en la DB del MCP.

Para el proyecto abierto se priorizará:

| Información | Fuente principal |
| --- | --- |
| Dependencias declaradas | `Cargo.toml` |
| Versiones resueltas | `Cargo.lock` |
| Workspace / targets / features | `cargo metadata` |
| Toolchain | `rustc`, Cargo, rustup/configuración disponible |
| Crates descargadas | Cargo local cache |
| Versiones conocidas globalmente | SQLite Catalog |
| Búsqueda semántica | LanceDB |
| Vulnerabilidades | RustSec snapshot + SQLite correlation |

El catálogo complementa a Cargo; no lo reemplaza.

---

## 34.7. Gestión de actualizaciones del catálogo

La sincronización debe estar separada de la ejecución MCP.

No se recomienda que:

```text
rust.crate.search
```

haga llamadas de red ocultas.

Se utilizará una CLI:

```bash
rust-engineering-mcp catalog status
rust-engineering-mcp catalog sync
rust-engineering-mcp catalog import <snapshot>
rust-engineering-mcp catalog rebuild-index
```

Flujo:

```text
crates.io / registry / RustSec
            │
            ▼
       catalog sync
            │
            ├────────► SQLite
            │
            └────────► embeddings → LanceDB
```

Posteriormente:

```text
rust-engineering-mcp serve --stdio
```

puede funcionar sin red.

---

## 34.8. Entornos con red restringida

Se contemplan tres modos.

### Online-controlled

El actualizador puede acceder únicamente a hosts autorizados.

```text
Internet
   │
allowlist
   │
catalog sync
```

### Corporate mirror

```text
Artifactory / Nexus / registry interno
                │
                ▼
           catalog sync
                │
                ▼
        SQLite + LanceDB
```

### Air-gapped

Se importará un snapshot firmado:

```text
rust-engineering-catalog-2026-09-03.tar.zst
├── catalog.sqlite
├── vectors/
├── embeddings/
├── rustsec/
├── manifest.json
├── SHA256SUMS
└── signature
```

Comando:

```bash
rust-engineering-mcp catalog import rust-engineering-catalog-2026-09-03.tar.zst
```

El proceso MCP no requiere red para consumirlo.

---

## 34.9. Embeddings offline

Incluir LanceDB en el MVP implica resolver la generación del embedding de la query.

Por ello se definirá `EmbeddingProvider`.

El MVP debe soportar al menos un provider **local**.

Modelo conceptual:

```text
Agent query
    │
    ▼
LocalEmbeddingProvider
    │
    ▼
query vector
    │
    ▼
LanceDB
```

El modelo concreto deberá seleccionarse mediante ADR considerando:

- calidad semántica;
- tamaño;
- licencia;
- velocidad CPU;
- soporte x86_64/arm64;
- Windows/macOS/Linux;
- reproducibilidad;
- posibilidad de distribución air-gapped.

El diseño debe permitir otros providers posteriores:

```text
LocalEmbeddingProvider
ExternalEmbeddingProvider
EnterpriseEmbeddingProvider
```

pero **el funcionamiento base no dependerá de APIs externas**.

El bundle air-gapped podrá incluir el modelo necesario o un artifact verificable para su instalación offline.

---

## 34.10. Provenance y freshness

Nunca se debe presentar información offline como si fuera tiempo real.

Ejemplo incorrecto:

```json
{
  "latest": "1.2.3"
}
```

Ejemplo recomendado:

```json
{
  "crate": "example",
  "latest_known": "1.2.3",
  "provenance": {
    "source": "local_registry_snapshot",
    "snapshot_id": "2026-09-03.1",
    "snapshot_at": "2026-09-03T12:00:00Z",
    "network_used": false
  },
  "freshness": {
    "state": "fresh",
    "age_seconds": 7200
  }
}
```

Estados posibles:

```text
fresh
stale
unknown
live
```

El agente debe poder distinguir:

```text
latest_known
```

de:

```text
latest_live
```

---

## 34.11. `rust.catalog.status`

Tool read-only del MVP.

Salida conceptual:

```json
{
  "sqlite": {
    "available": true,
    "snapshot_id": "2026-09-03.1",
    "snapshot_at": "2026-09-03T12:00:00Z"
  },
  "semantic_index": {
    "engine": "lancedb",
    "available": true,
    "documents": 148230,
    "embedding_model": "local-default"
  },
  "rustsec": {
    "available": true,
    "snapshot_at": "2026-09-03T11:30:00Z"
  },
  "network": {
    "allowed": false
  }
}
```

Esto permite al agente conocer la calidad del contexto antes de basar una decisión en él.

---

## 34.12. `rust.crate.search`

Tool del MVP con búsqueda híbrida.

Entrada conceptual:

```json
{
  "query": "zero-copy binary serialization",
  "limit": 10,
  "semantic": true,
  "filters": {
    "msrv_lte": "1.xx",
    "allow_yanked": false
  }
}
```

Salida:

```text
candidatos
semantic score
lexical score
version metadata
MSRV
license
advisory status
provenance
freshness
```

No debe convertir el score vectorial en una afirmación de calidad.

---

## 34.13. `rust.crate.inspect`

Consulta información autoritativa de una crate conocida por el snapshot.

Debe incluir únicamente información útil para decidir:

- versiones conocidas;
- latest known stable;
- yanked;
- rust-version/MSRV declarado cuando exista;
- features;
- dependencias;
- license;
- repository;
- documentation;
- advisories;
- source;
- snapshot/provenance.

El número de descargas, si se incorpora, será únicamente una señal auxiliar y nunca evidencia suficiente de calidad.

---

## 34.14. Layout local

Propuesta:

```text
~/.rust-engineering-mcp/
│
├── catalog/
│   ├── catalog.sqlite
│   ├── manifest.json
│   └── snapshots/
│
├── vectors/
│   └── lancedb/
│
├── embeddings/
│   └── models/
│
├── rustsec/
│   └── advisory-db/
│
├── cache/
└── artifacts/
```

La ubicación será configurable.

---

## 34.15. Integridad del índice vectorial

SQLite almacenará metadata que relacione:

```text
catalog snapshot
embedding model
embedding dimension
semantic index version
LanceDB generation
```

Si existe incompatibilidad:

```text
catalog fingerprint != vector index fingerprint
```

el servidor debe marcar el índice como inválido y:

```text
fallback → lexical/metadata search
```

en lugar de devolver resultados semánticos potencialmente inconsistentes.

---

## 34.16. Versionado del catálogo y migraciones

El catálogo tendrá versiones independientes del SemVer del servidor:

```text
catalog_schema_version
snapshot_format_version
semantic_index_version
embedding_model_id
```

SQLite utilizará migraciones explícitas y monotónicas.

Un snapshot deberá declarar como mínimo:

```json
{
  "snapshot_format_version": 1,
  "catalog_schema_version": 1,
  "semantic_index_version": 1,
  "embedding_model_id": "local-default",
  "created_at": "2026-09-03T12:00:00Z"
}
```

Reglas:

- una versión nueva del servidor puede leer snapshots compatibles anteriores;
- migraciones destructivas requieren backup o reconstrucción;
- LanceDB puede reconstruirse desde SQLite cuando cambie el formato del índice;
- un cambio de modelo/dimensión invalida el índice vectorial anterior;
- `catalog status` debe reportar incompatibilidades antes de una búsqueda.

Esto permite actualizar el servidor sin convertir el catálogo local en un punto frágil de compatibilidad.

---

# 35. Seguridad del servidor

Esta sección es crítica.

El servidor ejecutará herramientas sobre repositorios potencialmente no confiables.

Un proyecto Rust puede contener:

- `build.rs`;
- proc macros;
- tests arbitrarios;
- binaries;
- dependencias con código nativo.

Por tanto:

> `cargo test` y determinadas compilaciones no son operaciones inocuas.

---

# 36. Modelo de amenaza

Amenazas principales:

1. path traversal;
2. ejecución de comandos arbitrarios;
3. lectura de secretos;
4. modificación de archivos externos;
5. acceso de red inesperado;
6. procesos hijos persistentes;
7. build scripts maliciosos;
8. tests maliciosos;
9. proc macros;
10. dependency confusion;
11. output gigantesco;
12. denial of service por compilación;
13. consumo excesivo de CPU;
14. consumo excesivo de RAM;
15. llenado de disco;
16. symlink escape;
17. información sensible en logs.

---

# 37. Execution Gateway

Toda ejecución externa debe atravesar una única capa.

```text
Application
    │
    ▼
ExecutionPort
    │
    ▼
ExecutionGateway
    │
    ├─ policy validation
    ├─ timeout
    ├─ filesystem policy
    ├─ environment policy
    ├─ network policy
    ├─ resource limits
    ├─ output limits
    └─ process lifecycle
          │
          ▼
       Command
```

Ningún use case debe ejecutar procesos directamente.

---

# 38. Política de comandos

No permitido:

```rust
Command::new("sh")
    .arg("-c")
    .arg(user_input)
```

Permitido:

```rust
Command::new("cargo")
    .arg("check")
    .arg("--workspace")
```

Los argumentos se construyen a partir de enums y tipos validados.

---

# 39. Política de filesystem

Por defecto:

```text
read:
    roots de workspace preautorizados por el host
    path dependencies igualmente preautorizadas
    toolchain/sysroot y cache de dependencias sin credenciales

write:
    isolated target directory
    temporary execution directory
    private artifact directory

deny:
    source y Cargo.lock para tools M1
    parent directories
    ~/.ssh
    ~/.aws
    ~/.config
    system directories
```

La raíz solicitada nunca se autoautoriza. Debe resolverse correctamente:

- canonical paths;
- symlinks;
- junctions en Windows.

---

# 40. Variables de entorno

No se debe heredar todo el entorno automáticamente. El proceso comienza con
`env_clear` y agrega valores controlados por tool; no se heredan `PATH`, `HOME`,
`CARGO_HOME` o `RUSTUP_HOME` del host sin reconstruirlos desde configuración confiable.

Debe utilizarse una allowlist.

Ejemplo:

```text
PATH
HOME controlado
CARGO_HOME controlado
RUSTUP_HOME controlado
RUST_BACKTRACE
```

Variables sensibles deben eliminarse.

Ejemplos:

```text
AWS_SECRET_ACCESS_KEY
GITHUB_TOKEN
OPENAI_API_KEY
ANTHROPIC_API_KEY
SSH_AUTH_SOCK
```

La política puede permitir excepciones explícitas.

---

# 41. Red

Por defecto:

```text
network = denied
```

El runtime MCP y sus tools M1 no realizan I/O de red por diseño. Esto no equivale a
`network_isolated`: esa garantía solo se anuncia cuando el sandbox del OS la impide
también para todo proceso externo. Las operaciones explícitas fuera del runtime que
sí pueden necesitar red son:

```text
catalog sync
dependency download
RustSec refresh
```

Estas son operaciones CLI o administrativas fuera del runtime MCP, no tools M1. Una
capability MCP remota futura que use red deberá declararse `openWorld` y tendrá su
propia policy; las tools de catálogo M1 permanecen locales.

La política puede ser:

```toml
[network]
default = "deny"
allowed_hosts = [
  "crates.io",
  "index.crates.io",
  "static.crates.io",
  "rustsec.org"
]
```

La implementación exacta del sandbox variará por plataforma.

---

# 42. Sandboxing

El diseño debe permitir adapters por plataforma.

```text
SandboxPort
   ├── LinuxSandbox
   ├── MacOsSandbox
   ├── WindowsSandbox
   └── NoSandbox
```

Linux puede ofrecer inicialmente el aislamiento más fuerte.

El sandbox expone capacidades concretas de filesystem, red, environment, procesos y
resource limits; no un booleano genérico. Si una tool no puede obtener las garantías
requeridas, devuelve `SANDBOX_DENIED` sin degradación silenciosa. `NoSandbox` bloquea
toda tool que compile o ejecute código potencialmente no confiable, incluidas
`rust.check` y `rust.clippy`.

---

# 43. Ejecución de código

Las siguientes operaciones deben considerarse ejecución de código no confiable:

```text
cargo test
cargo check
cargo clippy
cargo run
benchmarks
Miri sobre tests
mutation testing
build scripts durante compilación
proc macros
```

El usuario debe poder definir:

```toml
[execution]
allow_project_code = false
```

---

# 44. Timeouts

Cada categoría tiene límites.

Ejemplo:

```toml
[timeouts]
check = "120s"
clippy = "180s"
test = "300s"
audit = "60s"
benchmark = "900s"
mutation = "3600s"
```

El proceso y sus hijos deben terminarse cuando expire el timeout.

---

# 45. Output limits

Un compilador puede producir enormes cantidades de logs.

Debe existir:

```text
max_stdout_bytes
max_stderr_bytes
max_diagnostics
```

Cuando se trunque:

```json
{
  "truncated": true,
  "artifact": "artifact://logs/..."
}
```

Los límites se aplican durante streaming. La respuesta conserva diagnósticos ya
normalizados y referencia un artifact privado y acotado. `OUTPUT_LIMIT_EXCEEDED` se
reserva para el caso en que ni siquiera pueda producirse de forma segura ese resultado
parcial/artifact.

---

# 46. Configuración

Archivo:

```text
.rust-engineering-mcp.toml
```

Ejemplo:

```toml
[project]
allow_workspace_write = false

[execution]
allow_project_code = false
network = "deny"

[catalog]
mode = "local"
sqlite_path = "~/.rust-engineering-mcp/catalog/catalog.sqlite"
warn_after = "7d"

[catalog.semantic]
enabled = true
engine = "lancedb"
path = "~/.rust-engineering-mcp/vectors/lancedb"
embedding_provider = "local"

[catalog.remote]
allow_live_fallback = false

[quality]
default_profile = "standard"

[quality.clippy]
profile = "project"

[security]
deny_vulnerabilities = true
unsafe_policy = "report"

[timeouts]
check = "120s"
test = "300s"

[limits]
max_diagnostics = 200
max_output_bytes = 1048576
```

---

# 47. Jerarquía de configuración

Prioridad general para preferencias no relacionadas con seguridad:

```text
CLI confiable del host
  >
user/host config
  >
project config
  >
defaults
```

Para seguridad no basta una prioridad de reemplazo: se aplica una combinación
monotónica. La configuración del proyecto solo puede restringir políticas definidas
por defaults o por el usuario/host y las claves desconocidas son error.

Ejemplo:

```text
user: network=deny
project: network=allow
```

Resultado:

```text
network=deny
```

---

# 48. Quality Gates

Una capacidad central será ejecutar perfiles consistentes.

---

## 48.1. Fast

Pensado para loops frecuentes del agente.

```text
fmt-check
check
clippy
```

---

## 48.2. Standard

Antes de entregar una implementación.

```text
fmt-check
check
clippy
test
audit
```

---

## 48.3. Strict

Para cambios sensibles.

```text
fmt-check
check
clippy
test
audit
deny
coverage
```

---

## 48.4. Release

Para librerías o releases.

```text
fmt-check
check
clippy
test
audit
deny
coverage
semver-check
mutation-test optional
```

---

# 49. Resultados de quality gate

Ejemplo:

```json
{
  "status": "failed",
  "profile": "standard",
  "checks": [
    {
      "name": "fmt",
      "status": "passed",
      "duration_ms": 120
    },
    {
      "name": "check",
      "status": "passed",
      "duration_ms": 914
    },
    {
      "name": "clippy",
      "status": "failed",
      "errors": 0,
      "warnings": 3
    }
  ]
}
```

---

# 50. Workflows para agentes

---

## 50.1. Crear una implementación

```text
project.inspect
      │
      ▼
agent generates change
      │
      ▼
quality.gate(fast)
      │
      ├─ fail → diagnostics → agent fixes
      │
      └─ pass
            │
            ▼
      quality.gate(standard)
```

---

## 50.2. Resolver error de compilación

```text
rust.check
    │
    ▼
structured diagnostic E0xxx
    │
    ├── rust.diagnostics.explain
    │
    └── agent inspects source
            │
            ▼
          fix
            │
            ▼
        rust.check
```

---

## 50.3. Revisar dependencia nueva

```text
crate.inspect
      │
      ▼
agent selects dependency
      │
      ▼
dependency.add
      │
      ▼
dependencies.audit
      │
      ▼
rust.deny
      │
      ▼
rust.check
```

---

## 50.4. Investigar performance

```text
benchmark.run
      │
      ▼
baseline
      │
      ▼
agent modifies code
      │
      ▼
benchmark.compare
      │
      ├─ regression → profile.flamegraph
      │
      └─ improvement → quality.gate
```

---

# 51. Long-running operations

Operaciones como:

```text
mutation testing
coverage grande
benchmarks extensos
profiling
```

pueden tardar mucho.

Cuando el cliente y la versión MCP negociada soporten tareas de larga duración, el servidor podrá utilizar esa capacidad.

El core de aplicación no debe depender de ella.

Se modelará internamente:

```rust
trait JobExecutor {
    async fn execute(...);
}
```

y el adapter MCP decidirá si responde sincrónicamente o mediante task.

---

# 52. Cache

El **catálogo persistente del MVP no debe confundirse con el cache**.

En el MVP existirán:

```text
SQLite Catalog  → persistencia autoritativa del snapshot
LanceDB         → índice semántico derivado
CacheStore      → optimización de resultados; prescindible
```

El cache de resultados puede utilizar SQLite inicialmente para evitar introducir una tercera tecnología de persistencia.

Debe ser **content-aware**.

Ejemplo de key:

```text
tool
project fingerprint
toolchain
target
features
tool version
arguments
```

No usar únicamente:

```text
project_path + tool
```

porque produciría resultados inválidos.

---

# 53. Estrategia de actualizaciones

El servidor debe evolucionar sin romper clientes innecesariamente.

---

## 53.1. SemVer

```text
MAJOR.MINOR.PATCH
```

### MAJOR

- eliminación de tool estable;
- cambio incompatible de schema;
- cambio importante de comportamiento.

### MINOR

- tool nueva;
- campo opcional nuevo;
- capability nueva;
- adapter nuevo.

### PATCH

- bugfix;
- optimización;
- parsing mejorado;
- corrección de seguridad compatible.

---

# 54. Versionado durante 0.x

Antes de 1.0 la API puede evolucionar con mayor libertad.

Aun así:

- evitar renombres innecesarios;
- documentar breaking changes;
- mantener fixtures de compatibilidad;
- publicar migration notes.

---

# 55. No usar `version` como parámetro obligatorio de cada tool

El borrador anterior sugería que cada tool incluyera un campo `version`.

No se recomienda.

Motivos:

- incrementa tokens;
- obliga al agente a conocer una versión interna;
- duplica SemVer del servidor;
- complica schemas;
- no resuelve realmente incompatibilidades.

La versión relevante debe exponerse en metadata/capabilities del servidor.

---

# 56. Capability document

El servidor debe permitir conocer un documento propio de capacidades. No es la
respuesta wire de `server/discover`; el adapter MCP sigue produciendo esa respuesta
con los campos normativos de la revisión negociada.

```json
{
  "document_kind": "rust_engineering_capabilities",
  "server_version": "0.1.0",
  "protocol": {
    "primary_version": "2026-07-28",
    "legacy_negotiation": "rmcp-supported"
  },
  "tools": {
    "rust.check": {
      "stability": "stable",
      "executes_project_code": true,
      "required_sandbox": [
        "filesystem_isolated",
        "network_isolated",
        "env_isolated",
        "children_contained",
        "cpu_limited",
        "memory_limited",
        "pid_limited",
        "disk_limited",
        "wall_time_limited",
        "output_limited"
      ]
    },
    "rust.miri": {
      "stability": "preview"
    }
  },
  "platform": "linux-x86_64",
  "sandbox": {
    "tier": "strict",
    "filesystem_isolated": true,
    "network_isolated": true,
    "env_isolated": true,
    "children_contained": true,
    "cpu_limited": true,
    "memory_limited": true,
    "pid_limited": true,
    "disk_limited": true,
    "wall_time_limited": true,
    "output_limited": true
  }
}
```

---

# 57. Tool stability

Categorías:

```text
stable
preview
experimental
```

Tools experimentales pueden usar namespace:

```text
experimental.rust....
```

o metadata específica.

No deben aparecer en configuración estable salvo opt-in.

---

# 58. Deprecación

Una tool estable se depreca en una versión MINOR.

Proceso:

```text
v1.3 → deprecated
v1.x → sigue funcionando
v2.0 → puede eliminarse
```

No es recomendable una política basada exclusivamente en meses.

La compatibilidad debe asociarse principalmente a releases.

---

# 59. Compatibilidad MCP

Debe existir:

```text
docs/compatibility.md
```

Ejemplo:

| rust-engineering-mcp | MCP | rmcp |
| --- | --- | --- |
| 0.1.x | versión negociada soportada por SDK | versión fijada |
| 0.2.x | ... | ... |

Nunca hardcodear en documentación una única versión del protocolo como si fuera permanente.

---

# 60. Distribución

Orden recomendado:

1. GitHub Releases.
2. crates.io.
3. `cargo-binstall` metadata.
4. Homebrew.
5. Scoop/WinGet.
6. container image para remoto.

---

# 61. GitHub Releases

La matriz aspiracional puede publicar, según viabilidad:

```text
linux x86_64
linux arm64
macOS x86_64
macOS arm64
windows x86_64
```

Para 0.1.0, ADR-048 califica y publica únicamente macOS ARM64. Linux y Windows
conservan CI de portabilidad/fail-closed, pero no se presentan como hosts de
capabilities positivas ni reciben artifacts 0.1.0.

Artefactos:

```text
binary archive
SHA-256 checksums
signature
SBOM
release notes
```

---

# 62. crates.io

Instalación:

```bash
cargo install rust-engineering-mcp --locked
```

Ventaja:

- natural para desarrolladores Rust.

Desventaja:

- requiere toolchain;
- compilación más lenta;
- comportamiento depende parcialmente del entorno.

Por ello los binarios precompilados deben ser el camino recomendado.

---

# 63. `cargo-binstall`

Debe contemplarse desde temprano porque permite instalar binarios publicados sin compilar localmente cuando existe artifact compatible.

Esto puede mejorar significativamente el onboarding de usuarios Rust.

---

# 64. Docker / containers

No debe ser la distribución principal para el modo local.

Problemas:

- mounts;
- toolchain del proyecto;
- filesystem;
- rendimiento;
- permisos;
- complejidad.

Sí será útil para:

- servidor remoto;
- sandbox reproducible;
- CI;
- entornos de evaluación.

---

# 65. UPX

No se recomienda comprimir binarios con UPX por defecto.

El tamaño del binario no justifica normalmente:

- problemas con antivirus;
- debugging más complejo;
- firmas;
- reproducibilidad;
- startup potencialmente diferente.

Puede evaluarse posteriormente si el tamaño se convierte en un problema real.

---

# 66. Firma y supply chain del propio MCP

El proyecto debe publicar:

- checksums;
- releases firmadas;
- SBOM;
- provenance cuando sea posible;
- dependencias auditadas;
- política de seguridad.

CI:

```text
cargo fmt --check
cargo check
cargo clippy
cargo test
cargo audit
cargo deny
```

Para releases:

```text
cargo semver-checks
SBOM
sign
publish
```

---

# 67. Observabilidad

Usar:

```text
tracing
tracing-subscriber
```

Logs siempre por:

```text
stderr
```

cuando se utilice `stdio`.

Nunca escribir logs arbitrarios en stdout porque corromperían el transporte MCP.

---

# 68. Métricas locales

Inicialmente:

```text
tool execution count
duration
timeouts
cache hit
process failures
diagnostic counts
```

No se requiere telemetría externa.

La telemetría remota debe ser opt-in.

---

# 69. Error model

Categorías de fallos operativos:

```text
PROJECT_NOT_FOUND
INVALID_PROJECT
TOOL_NOT_INSTALLED
LOCKFILE_UPDATE_REQUIRED
COMMAND_TIMEOUT
SANDBOX_DENIED
NETWORK_DENIED
UNSUPPORTED_PLATFORM
OUTPUT_LIMIT_EXCEEDED
```

Mapping MCP:

| Caso | Representación |
| --- | --- |
| Tool desconocida o request MCP malformada | Error JSON-RPC / `ErrorData`. |
| `TOOL_NOT_INSTALLED`, timeout, sandbox/red denegados, plataforma no soportada | `OutputEnvelope` tipado, `structuredContent` conforme e `isError: true`. |
| Compilación, lint, test o gate encuentra un problema del proyecto | Resultado tipado `status: failed` e `isError: false`. |
| Fallo interno que impide construir una respuesta de tool | Error JSON-RPC interno. |

`PROJECT_NOT_FOUND` e `INVALID_PROJECT` son errores operativos recuperables cuando la
request MCP es bien formada. `OUTPUT_LIMIT_EXCEEDED` solo se usa como error si no
puede entregarse el resultado parcial seguro descrito en la sección 45.
`INTERNAL_ERROR` es exclusivamente el error JSON-RPC para un fallo interno que
impide construir una respuesta tipada; no pertenece al enum `error_code` del
`OutputEnvelope`.

Errores de código Rust no deben convertirse en error MCP.

Ejemplo:

```text
cargo check encuentra E0502
```

La llamada MCP fue exitosa.

Resultado:

```json
{
  "status": "failed",
  "diagnostics": [
    {
      "source": "rustc",
      "severity": "error",
      "code": "E0502",
      "message": "cannot borrow as mutable because it is also borrowed as immutable"
    }
  ]
}
```

Esto es fundamental para que el agente diferencie:

```text
falló el MCP
```

de:

```text
el proyecto no compila
```

---

# 70. Tool availability

Una tool puede estar definida pero no disponible.

Ejemplo:

```text
rust.miri
```

requiere componente/toolchain adecuado.

El servidor debe reportar:

```json
{
  "available": false,
  "reason": "miri component not installed"
}
```

No instalar herramientas automáticamente durante una llamada.

---

# 71. Dependency strategy del MCP

Dependencias core sugeridas:

```text
rmcp
tokio
serde
serde_json
schemars
thiserror
tracing
tracing-subscriber
cargo_metadata
toml_edit
camino
tempfile
sha2
rusqlite
lancedb
```

Dependencias asociadas al pipeline semántico deberán mantenerse detrás de adapters claros.

Opcionales según fase o provider:

```text
reqwest
semver
url
which
embedding runtime/provider
```

La selección concreta del runtime/modelo local de embeddings requiere un ADR independiente porque afecta distribución, tamaño y soporte cross-platform.

Evitar una colección excesiva de crates en el core.

---

# 72. `anyhow` vs `thiserror`

Recomendación:

```text
thiserror → errores de dominio/adapters
anyhow    → bordes ejecutables si se necesita contexto rápido
```

Las APIs internas deben utilizar errores tipados.

---

# 73. Tipos de dominio

Ejemplos:

```rust
struct ProjectRef(...);
struct ProjectFingerprint(...);
struct ToolchainInfo {...}
struct Diagnostic {...}
struct Finding {...}
struct ArtifactRef(...);
struct ExecutionPolicy {...}
struct ToolResult<T> {...}
```

Evitar utilizar `serde_json::Value` a través de toda la aplicación.

---

# 74. Concurrencia

Tokio es adecuado para:

- MCP I/O;
- procesos;
- HTTP;
- cancelación.

Sin embargo:

> compilaciones concurrentes sobre el mismo `target` pueden competir.

Debe existir un scheduler por proyecto.

Ejemplo:

```text
project A
  check ──────────────┐
  clippy waits        │ lock
                      └─ release

project B
  audit ─────────────── independent
```

---

# 75. Locks

Posibles locks:

```text
workspace read
workspace write
target directory
cargo registry mutation
```

Las tools read-only desde el punto de vista del source pueden seguir escribiendo en `target`.

Por tanto la clasificación de efectos debe diferenciar:

```text
source mutation
build artifact mutation
external mutation
```

---

# 76. Target directory aislado

Opcionalmente:

```text
CARGO_TARGET_DIR=.mcp/target
```

Ventajas:

- aislamiento.

Desventajas:

- pierde cache del proyecto;
- mayor espacio;
- compilaciones duplicadas.

Recomendación:

MVP utiliza target y temporales aislados para cualquier operación que pueda ejecutar
código no confiable. Un modo explícitamente confiable puede reutilizar el target
estándar con locks, pero ese resultado debe reportar la policy efectiva y no puede
calificarse como aislamiento estricto.

---

# 77. Gestión de artifacts

Artifacts posibles:

```text
coverage HTML
flamegraph SVG
benchmark JSON
large logs
SBOM
dependency graph
```

No deben insertarse completos en la respuesta.

Se referencian:

```json
{
  "uri": "artifact://coverage/41ff..."
}
```

---

# 78. Tamaño de contexto y agentes

El diseño debe optimizar tokens.

Evitar:

- logs completos;
- stack traces enormes;
- árboles completos de dependencias;
- documentación completa.

Preferir:

```text
summary
top findings
structured diagnostics
pagination
artifact links/resources
```

Esto es especialmente importante porque el MCP existe para ser consumido por modelos.

---

# 79. Machine readability primero

Siempre que una herramienta externa tenga formato JSON debe priorizarse.

Cuando no exista:

1. parser estable;
2. tests golden;
3. fallback controlado.

Nunca depender exclusivamente de regex sobre output humano si puede evitarse.

---

# 80. Testing del MCP

---

## 80.1. Unit tests

Para:

- parsers;
- policies;
- schemas;
- path validation;
- error mapping;
- fingerprints.

---

## 80.2. Fixtures

Repositorios intencionalmente diseñados:

```text
valid-basic
borrow-error
lifetime-error
clippy-warning
unsafe
vulnerable-dependency
workspace
feature-conflict
build-script
```

---

## 80.3. Integration tests

Ejecutar tools reales contra fixtures.

Ejemplo:

```text
fixture borrow-error
      │
      ▼
rust.check
      │
      ▼
assert diagnostic.code == E....
```

---

## 80.4. Contract tests

Guardar schemas y verificar compatibilidad.

Un cambio accidental de:

```text
field type
required fields
enum values
tool name
```

debe ser detectado en CI.

---

## 80.5. MCP protocol tests

Validar:

- `server/discover` para MCP `2026-07-28`;
- initialization legacy cuando la versión negociada lo requiera;
- tools/list;
- tools/call;
- resources;
- error mapping;
- cancellation;
- transport stdio.

Usar MCP Inspector durante desarrollo.

---

# 81. Security tests

Casos obligatorios:

```text
../../etc/passwd
symlink escape
command injection
huge stdout
timeout
child process leak
secret environment variable
malicious build.rs fixture
invalid project ref
```

---

# 82. Performance del MCP

La prioridad es que el overhead del servidor sea mínimo comparado con Cargo.

Métricas:

```text
server startup
tool dispatch overhead
JSON parsing
diagnostic normalization
memory idle
```

No tiene sentido micro-optimizar el MCP si `cargo check` tarda segundos.

---

# 83. KPIs del producto

El éxito debe medirse desde la perspectiva del agente.

Posibles KPIs:

### Correctness

```text
% de generaciones que pasan cargo check en primer ciclo
número medio de repair loops
% de entregas que pasan quality gate
```

### Calidad

```text
clippy warnings por cambio
test pass rate
vulnerabilidades detectadas antes de entrega
```

### Eficiencia

```text
tools invocadas por tarea
tokens de output por tool
latencia MCP overhead
cache hit rate
```

### Seguridad

```text
sandbox violations blocked
commands denied
path escapes prevented
```

---

# 84. Diseño para múltiples agentes

El MCP no debe contener lógica específica de:

```text
Claude
Gemini
Codex
```

La compatibilidad debe surgir de MCP y schemas bien diseñados.

Evitar descriptions como:

```text
"Claude should call this after..."
```

Preferir:

```text
"Validates the workspace using cargo check and returns normalized compiler diagnostics."
```

---

# 85. Descriptions de tools

Las descriptions deben indicar:

1. cuándo usar la tool;
2. qué hace;
3. efectos secundarios;
4. costo aproximado;
5. qué no hace.

Ejemplo:

```text
Run Cargo check for a registered Rust workspace.
Use after source changes to validate compilation without producing final binaries.
Does not modify source files but may write build artifacts.
Returns normalized rustc diagnostics.
```

Esto mejora significativamente la selección de tools por los agentes.

---

# 86. Naming de tools

Convención propuesta:

```text
rust.project.open
rust.project.inspect
rust.toolchain.inspect
rust.check
rust.clippy
rust.fmt.check
rust.fmt.apply
rust.test
rust.dependencies.inspect
rust.dependencies.audit
rust.catalog.status
rust.crate.search
rust.crate.inspect
rust.quality.gate
```

Ventajas:

- agrupación semántica;
- nombres claros;
- menor ambigüedad.

Si un cliente MCP impone restricciones de naming se puede adaptar a:

```text
rust_project_open
```

sin cambiar los nombres internos del dominio.

---

# 87. Estrategia de prompts MCP

Prompts opcionales posteriores.

---

## 87.1. `review-rust-change`

Secuencia recomendada:

```text
inspect project
review diff
run fast gate
resolve diagnostics
run standard gate
summarize remaining risks
```

---

## 87.2. `prepare-rust-release`

```text
standard gate
deny
coverage
semver check
release build
binary bloat optional
```

---

## 87.3. `investigate-rust-performance`

```text
establish benchmark
capture baseline
profile
modify
compare
validate correctness
```

---

# 88. Recursos MCP sugeridos

```text
rust-project://{project_ref}/metadata
rust-project://{project_ref}/dependencies
rust-project://{project_ref}/toolchain
rust-project://{project_ref}/latest-diagnostics
rust-project://{project_ref}/quality/latest
rust-catalog://status
rust-catalog://crate/{name}
artifact://...
```

---

# 89. Estrategia de conocimientos Rust

No mantener manualmente una enciclopedia completa.

Prioridad:

1. compilador instalado;
2. `Cargo.toml`, `Cargo.lock` y Cargo metadata;
3. rustdoc/source local;
4. SQLite Catalog snapshot;
5. LanceDB Semantic Index;
6. rust-analyzer cuando se incorpore;
7. documentación oficial remota cuando la policy lo permita;
8. crates.io/docs.rs cuando la policy lo permita;
9. reglas específicas propias.

Esto reduce desactualización.

---

# 90. Reglas propias que sí pueden tener valor

Ejemplos:

```text
project policy violations
forbidden dependencies
unsafe policy
required lints
required release profile
MSRV compatibility policy
workspace conventions
```

Estas reglas deben ser configurables.

---

# 91. Integración con políticas de proyecto

Ejemplo:

```toml
[policy]
forbid_unsafe = true
require_docs = true

[policy.dependencies]
deny_git = true
deny_wildcard = true

[policy.release]
lto = "thin"
panic = "abort"
```

El MCP puede detectar desviaciones.

No debe imponer preferencias universales arbitrarias.

---

# 92. Evitar recomendaciones universales de performance

Ejemplos peligrosos:

```text
"Vec siempre es mejor que LinkedList"
"#[inline] mejora performance"
"LTO siempre debe activarse"
"Box es más lento"
```

Todas dependen del contexto.

El MCP debe proporcionar mediciones y facts.

---

# 93. Performance profiles

Cuando se añadan:

```text
dev
ci
release
benchmark
size
```

El agente puede solicitar análisis contextual.

Ejemplo:

```json
{
  "profile": "size"
}
```

que habilite:

```text
cargo-bloat
release binary size
dependency contribution
```

---

# 94. Estrategia MVP

El MVP debe demostrar dos hipótesis complementarias:

> Un agente que utiliza evidencia estructurada de Cargo/rustc produce cambios Rust más correctos con menos iteraciones.

> Un agente que dispone de un catálogo local híbrido —metadata autoritativa en SQLite + búsqueda semántica en LanceDB— selecciona crates y APIs con mejor contexto incluso sin acceso de red.

No intentar demostrar simultáneamente:

- profiling;
- remote MCP;
- scaffolding;
- búsqueda semántica profunda sobre toda la documentación;
- LSP;
- mutation;
- Miri;
- Docker.

---

# 95. MVP 0.1.0

Scope recomendado:

### Core MCP

```text
stdio
server/discover
legacy initialization únicamente por negociación
tools/list
tools/call
JSON-RPC gestionado por rmcp
inputSchema / outputSchema
structuredContent
contract tests
```

### Project

```text
project.open
project.inspect
toolchain.inspect
```

### Validation

```text
check
fmt.check
clippy
test
```

### Security

```text
dependencies.audit
```

### Agent ergonomics

```text
diagnostics.explain
quality.gate
```

### Local ecosystem catalog

```text
SQLite catalog
SQLite FTS5
LanceDB semantic index
LocalEmbeddingProvider
catalog.status
crate.search
crate.inspect
snapshot import
catalog sync CLI
provenance
freshness
offline mode
```

### Platform

```text
Linux
macOS
Windows
```

cuando existan adapters y security tests nativos. Para 0.1.0, ADR-048 conserva CI
portable en los tres OS y califica como host positivo únicamente macOS ARM64/APFS
con el gateway Docker Linux ARM64 aprobado.

---

# 96. Lo que queda fuera del MVP

```text
Streamable HTTP
OAuth
Docker
live crate search remoto durante tools
semantic indexing completo de docs.rs
rust-analyzer
Miri
Criterion
flamegraph
coverage
mutation testing
cargo-deny
dependency mutation
source mutation
scaffolding
self-update
```

---

# 97. Roadmap

---

## M0 — Foundation

**Objetivo:** establecer arquitectura y seguridad.

Entregables:

- repo;
- ADRs;
- domain model;
- process runner;
- project validation;
- roots preautorizados por el host;
- stdio;
- rmcp;
- JSON-RPC/Schema contract boundary;
- security capability tiers y Execution Gateway fail-closed;
- artifact store mínimo para output truncado;
- SQLite migrations y `CatalogRepository`;
- LanceDB adapter y `SemanticIndex`;
- `EmbeddingProvider`;
- catalog snapshot format;
- logging;
- fixtures;
- CI.

---

## M1 — MVP / 0.1.0

Entregables:

```text
project.open
project.inspect
toolchain.inspect
check
fmt.check
clippy
test
dependencies.audit
diagnostics.explain
quality.gate
catalog.status
crate.search
crate.inspect
```

Persistencia y búsqueda:

```text
SQLite catalog
SQLite FTS5
LanceDB semantic index
LocalEmbeddingProvider
snapshot import
catalog sync CLI
provenance/freshness
```

Definition of Done:

- `inputSchema` y `outputSchema` documentados y derivados de tipos Rust cuando sea posible;
- respuestas principales mediante `structuredContent`;
- contract tests que detecten breaking changes de schema;
- Linux/macOS/Windows ejecutan CI portable de core/protocolo/catálogo; cada capability de
  sandbox anunciada tiene security tests reales en esa plataforma y las tools se
  bloquean donde falten garantías;
- output estructurado;
- timeouts;
- filesystem restrictions;
- funcionamiento del MCP con red deshabilitada;
- importación de un snapshot offline;
- búsqueda lexical en SQLite y semántica en LanceDB;
- fallback correcto cuando LanceDB/embeddings no estén disponibles o estén inválidos;
- provenance/freshness en resultados del catálogo;
- integration tests;
- MCP Inspector tests.

ADR-048 separa la calificación completa desde fuente de los artifacts binarios:
el full gate de M1 sigue incluyendo E5/ORT/LanceDB en el host positivo, mientras
el release 0.1.0 distribuye solo un core macOS ARM64 sin modelo, runtime, catálogo
ni fixture. El core por sí solo no califica M1; el cierre usa ambos conjuntos de
evidencia. IUMotion Labs no publica un catálogo oficial en 0.1.0.

---

## M2 — Safe Mutation / 0.2.x

Entregables:

```text
fmt.apply
fix.apply
dependency.add
dependency.remove
manifest.patch
```

Además:

- diffs;
- permission model;
- write locks.

---

## M3 — Quality / 0.3.x

Entregables:

```text
nextest
coverage
semver-check
mutation testing
```

Además:

- task execution abstraction;
- artifacts avanzados;
- resources adicionales.

---

## M4 — Security / 0.4.x

Entregables:

```text
cargo-deny
unsafe scan
Miri
supply-chain inspection
hardening adicional de sandbox
```

---

## M5 — Performance / 0.5.x

Entregables:

```text
benchmark
benchmark compare
flamegraph
cargo-bloat
```

---

## M6 — Analyzer / 0.6.x

Entregables:

```text
rust-analyzer adapter
symbols
references
diagnostics
code actions
```

---

## M7 — Remote / 0.7.x

Entregables:

```text
Streamable HTTP
authorization
rate limits
remote sandbox
multi-project concurrency
```

Solo debe realizarse si existe un caso de uso real.

---

## M8 — Stabilization / 0.8–0.9

Objetivos:

- API cleanup;
- compatibility tests;
- docs;
- performance;
- security review;
- migration tooling.

---

## 1.0.0

Criterios:

- tool contracts estables;
- security model documentado;
- SemVer policy;
- cross-platform;
- stable CLI;
- integration guides;
- protocol compatibility matrix;
- upgrade process;
- release signing.

---

# 98. ADRs iniciales

Deben documentarse como mínimo:

```text
ADR-001 Use Rust
ADR-002 Use official rmcp SDK
ADR-003 stdio-first transport
ADR-004 Hexagonal architecture
ADR-005 No internal LLM in core
ADR-006 Structured diagnostics
ADR-007 Explicit project handles
ADR-008 Execution Gateway
ADR-009 Deny-by-default security
ADR-010 No arbitrary shell execution
ADR-011 MCP resources for reusable context
ADR-012 SemVer compatibility strategy
ADR-013 Safe mutation model
ADR-014 Artifact handling
ADR-015 JSON-RPC envelope vs JSON Schema contracts
ADR-016 SQLite as authoritative local catalog
ADR-017 LanceDB as derived semantic index
ADR-018 Offline-first catalog synchronization
ADR-019 EmbeddingProvider and local embedding strategy
ADR-020 Provenance and freshness model
```

---

# 99. Ejemplo de use case interno

```rust
pub struct CheckProject<R, P>
where
    R: ProcessRunner,
    P: ProjectRepository,
{
    process_runner: R,
    projects: P,
}
```

El use case no conoce:

```text
rmcp
stdio
HTTP
JSON-RPC
```

Solo recibe tipos de dominio.

---

# 100. Adapter Cargo

Responsabilidades:

```text
build command arguments
invoke ExecutionPort
parse cargo JSON messages
normalize diagnostics
map process exit codes
```

No debe contener reglas MCP.

---

# 101. Adapter MCP

Responsabilidades:

```text
input schema
output schema
MCP annotations
mapping request → use case
mapping domain → MCP response
```

No ejecuta Cargo directamente.

---

# 102. Ejemplo conceptual de tool

```rust
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CheckInput {
    project_ref: ProjectRef,
    package: Option<String>,
    all_targets: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
struct CheckOutput {
    status: CheckStatus,
    diagnostics: Vec<Diagnostic>,
    duration_ms: u64,
}
```

La implementación exacta dependerá de la API de `rmcp`.

---

# 103. Cancelación

Si el cliente cancela una petición:

```text
MCP cancellation
      │
      ▼
CancellationToken
      │
      ▼
ExecutionGateway
      │
      ▼
kill process tree
```

No deben quedar procesos Cargo huérfanos.

---

# 104. Reproducibilidad

Los resultados deben incluir metadata suficiente.

```json
{
  "rustc": "...",
  "cargo": "...",
  "target": "...",
  "features": [],
  "profile": "dev"
}
```

Esto permite al agente saber bajo qué condiciones se obtuvo el resultado.

---

# 105. Dependencias y lockfile

Para aplicaciones:

```text
Cargo.lock requerido
```

Para librerías:

se respetará la convención del proyecto.

Las tools M1 no generan ni actualizan `Cargo.lock`. Ejecutan Cargo en modo frozen
cooperativo dentro del sandbox; si el proyecto requiere crear o actualizar el
lockfile, devuelven un fallo operativo tipado. Una estrategia futura sobre copia
efímera requiere ADR porque puede cambiar resolución y fingerprint.

---

# 106. Toolchain

Respetar únicamente cuando provengan de configuración confiable del host o del
entorno efectivo reconstruido por allowlist:

```text
rust-toolchain.toml
rustup override
environment sanitizado
```

Un `rust-toolchain.toml` o override del proyecto solo selecciona un toolchain ya
instalado y permitido por el host. No puede disparar auto-download ni introducir
wrappers/runners/linkers fuera del Execution Gateway.

Nunca actualizar Rust automáticamente durante una validación.

Una tool futura puede sugerir:

```text
toolchain outdated
```

pero la actualización debe ser decisión del usuario/agente host.

---

# 107. Tool installation

No instalar automáticamente:

```text
cargo-audit
cargo-deny
cargo-nextest
cargo-llvm-cov
cargo-mutants
cargo-semver-checks
```

durante una ejecución.

El servidor debe:

```text
detect
report
provide installation guidance
```

La misma regla aplica a actualizaciones del catálogo: las tools de consulta no deben descargar silenciosamente snapshots ni modelos.

Un instalador/CLI separado puede ofrecer bundles opcionales.

---

# 108. Distribution bundles

Futuro:

```text
core
quality
security
full
```

Ejemplo:

```text
core:
  server only

quality:
  + nextest
  + llvm-cov

security:
  + audit
  + deny
```

Sin embargo esta idea solo debe implementarse si el manejo de herramientas externas se vuelve un problema real.

---

# 109. Self-update

Si se implementa:

```bash
rust-engineering-mcp update
```

No debe exponerse inicialmente como:

```text
MCP tool: update_server
```

Razón:

un agente no debería modificar el ejecutable que le proporciona sus capacidades durante una tarea normal.

---

# 110. CLI

Comandos mínimos:

```text
rust-engineering-mcp serve --stdio
rust-engineering-mcp doctor
rust-engineering-mcp version
rust-engineering-mcp capabilities
rust-engineering-mcp catalog status
rust-engineering-mcp catalog sync
rust-engineering-mcp catalog import <snapshot>
rust-engineering-mcp catalog rebuild-index
```

Futuros:

```text
serve --http
update
config validate
catalog export
```

---

# 111. `doctor`

Debe verificar:

```text
rustc
cargo
rustfmt
clippy
cargo-audit
SQLite catalog
LanceDB semantic index
embedding provider/model
catalog freshness
optional tools
sandbox support
filesystem permissions
```

Salida machine-readable opcional:

```bash
rust-engineering-mcp doctor --json
```

---

# 112. Estrategia de publicación

Pipeline:

```text
PR
 │
 ├─ fmt
 ├─ check
 ├─ clippy
 ├─ tests
 ├─ audit
 └─ deny
      │
      ▼
main
      │
      ▼
tag
      │
      ├─ cross-platform build
      ├─ integration tests
      ├─ SBOM
      ├─ checksum
      ├─ sign
      └─ publish
```

---

# 113. Documentación mínima

```text
README
install
client configuration
tool catalog
security model
configuration
troubleshooting
compatibility
release notes
```

---

# 114. Ejemplo de configuración de un cliente MCP

Conceptualmente:

```json
{
  "mcpServers": {
    "rust-engineering": {
      "command": "rust-engineering-mcp",
      "args": ["serve", "--stdio"]
    }
  }
}
```

La sintaxis exacta dependerá del cliente.

---

# 115. Diferenciador frente a simplemente permitir terminal

Un agente con terminal puede ejecutar:

```text
cargo check
cargo test
cargo clippy
```

Entonces, ¿por qué crear el MCP?

El valor diferencial debe ser:

### Seguridad

No existe shell arbitrario.

### Estructura

Los diagnósticos llegan normalizados.

### Descubrimiento

El agente conoce capabilities y tools.

### Portabilidad

Mismo contrato entre agentes.

### Políticas

La organización puede controlar:

```text
network
filesystem
unsafe
dependencies
quality gates
```

### Context efficiency

El agente recibe solo información útil.

### Composición

Un único `quality.gate` puede orquestar varias verificaciones.

### Evidencia

Los resultados tienen metadata reproducible.

Si el MCP se limita a envolver comandos Cargo y devolver stdout, su valor será bajo.

---

# 116. Riesgos del proyecto

---

## 116.1. Tool explosion

Riesgo:

crear 50 tools demasiado específicas.

Mitigación:

tools composables y parámetros tipados.

---

## 116.2. Duplicar rust-analyzer

Mitigación:

adapter, no reimplementación.

---

## 116.3. Duplicar el razonamiento del agente

Mitigación:

priorizar evidence tools.

---

## 116.4. Ejecución insegura

Mitigación:

Execution Gateway + sandbox + deny-by-default.

---

## 116.5. Outputs gigantes

Mitigación:

structured summaries + artifacts.

---

## 116.6. Ecosistema externo cambiante

Mitigación:

adapters + capability detection.

---

## 116.7. Toolchains incompatibles

Mitigación:

toolchain discovery + compatibility metadata.

---

## 116.8. Scope creep

Mitigación:

MVP limitado.

---

## 116.9. Peso de LanceDB y pipeline de embeddings

Riesgo:

incorporar búsqueda semántica aumenta el grafo de dependencias, tamaño de distribución, tiempo de build y complejidad cross-platform.

Mitigación:

- LanceDB detrás de `SemanticIndex`;
- modelo detrás de `EmbeddingProvider`;
- índice completamente reconstruible;
- fallback lexical con SQLite FTS5;
- benchmarks de startup, memoria y tamaño del binario;
- snapshot/model bundle separado si mejora distribución.

---

## 116.10. Staleness del catálogo offline

Riesgo:

un agente puede interpretar `latest_known` como una versión actual en Internet.

Mitigación:

- provenance obligatorio;
- `snapshot_at`;
- freshness state;
- warning configurable;
- nunca etiquetar datos de snapshot como `live`;
- `catalog.status` disponible para el agente.

---

# 117. Criterios de aceptación del MVP

El MVP estará listo cuando un agente pueda:

1. abrir un proyecto;
2. inspeccionar su configuración;
3. modificar código mediante sus propias capacidades;
4. ejecutar `rust.check`;
5. recibir diagnósticos estructurados;
6. corregir el error;
7. ejecutar Clippy;
8. ejecutar tests;
9. ejecutar audit;
10. ejecutar un quality gate;
11. hacerlo sin acceso shell arbitrario;
12. hacerlo sin acceso filesystem fuera del closure de lectura/escritura autorizado
    por el host;
13. cancelar procesos;
14. obtener errores claros si una tool externa falta;
15. buscar crates localmente mediante SQLite FTS5;
16. buscar crates por intención mediante LanceDB;
17. conocer el snapshot/freshness utilizado;
18. operar con la red completamente deshabilitada.

Las afirmaciones 12 y 18 solo se cumplen cuando security tests del adapter de esa
plataforma demuestran aislamiento; una policy cooperativa de Cargo no es suficiente.

---

# 118. Experimento de validación

Para medir utilidad real:

Preparar un benchmark de tareas.

Ejemplos:

```text
ownership error
lifetime error
async Send error
incorrect feature
outdated dependency API
clippy issue
test regression
vulnerable dependency
```

Comparar:

```text
Agent sin MCP
vs
Agent con Rust Engineering MCP
```

Métricas:

```text
success rate
repair loops
tokens
time
security findings
final quality gate result
```

Esto permitirá decidir qué tools realmente aportan valor.

---

# 119. Evolución futura orientada a agentes

Una vez establecida la base, pueden explorarse capacidades más avanzadas:

- incremental context snapshots;
- change impact analysis;
- symbol-aware validation;
- automatic test selection;
- dependency recommendation scoring;
- performance regression gates;
- unsafe invariants registry;
- architecture rules;
- workspace conventions;
- API compatibility checks;
- generated SBOM;
- WASM target validation;
- embedded/no_std validation;
- fuzzing integrations;
- loom for concurrency testing;
- sanitizers;
- cross compilation verification.

Cada feature deberá justificar su costo operacional.

---

# 120. Posibles integraciones futuras

No pertenecen al core inicial:

```text
cargo-fuzz
Loom
Kani
Prusti / verification tools según madurez
cargo-msrv
cargo-hack
cargo-minimal-versions
cargo-bloat
cargo-expand
cargo-outdated
cargo-udeps
```

La inclusión debe decidirse por demanda real.

---

# 121. Recomendación final

La propuesta inicial tenía una buena dirección, pero era demasiado cercana a:

```text
"envolver herramientas Rust como MCP tools"
```

La arquitectura recomendada transforma la idea en:

> **una capa de ingeniería verificable, segura y estructurada entre los agentes y el ecosistema Rust.**

Los elementos que más valor aportarán no serán la cantidad de tools, sino:

1. **project context estructurado**;
2. **diagnósticos normalizados**;
3. **quality gates**;
4. **Execution Gateway seguro**;
5. **contratos MCP estables**;
6. **tool outputs optimizados para modelos**;
7. **catálogo offline-first con SQLite como fuente de verdad**;
8. **búsqueda semántica híbrida mediante LanceDB**;
9. **provenance/freshness explícitos**;
10. **integración directa con herramientas autoritativas del ecosistema Rust**.

El MVP debe demostrar estos principios antes de expandirse.

---

# 122. Decisión recomendada de alcance inmediato

Implementar únicamente:

```text
rust.project.open
rust.project.inspect
rust.toolchain.inspect
rust.check
rust.fmt.check
rust.clippy
rust.test
rust.dependencies.audit
rust.diagnostics.explain
rust.quality.gate
rust.catalog.status
rust.crate.search
rust.crate.inspect
```

con:

```text
stdio
rmcp
Tokio
JSON-RPC gestionado por el adapter MCP
JSON Schema para input/output contracts
structuredContent
structured diagnostics
SQLite como catálogo autoritativo
SQLite FTS5
LanceDB como índice semántico derivado
LocalEmbeddingProvider
snapshot import/sync
provenance/freshness
offline-first
Execution Gateway
timeouts
filesystem allowlist
cross-platform tests
```

Esto constituye una primera versión pequeña pero realmente útil.

---

# 123. Referencias técnicas verificadas

Documentación MCP:

- Model Context Protocol specification: <https://modelcontextprotocol.io/specification/>
- MCP transports: <https://modelcontextprotocol.io/specification/2026-07-28/basic/transports>
- MCP tools: <https://modelcontextprotocol.io/specification/2026-07-28/server/tools>
- MCP schema / ToolAnnotations: <https://modelcontextprotocol.io/specification/2026-07-28/schema>
- MCP Rust SDK: <https://github.com/modelcontextprotocol/rust-sdk>

Persistencia y catálogo local:

- SQLite: <https://www.sqlite.org/>
- SQLite FTS5: <https://www.sqlite.org/fts5.html>
- LanceDB: <https://lancedb.com/>
- Cargo registry index: <https://doc.rust-lang.org/cargo/reference/registry-index.html>
- Cargo offline mode/config: <https://doc.rust-lang.org/cargo/reference/config.html>
- Cargo source replacement: <https://doc.rust-lang.org/cargo/reference/source-replacement.html>
- Cargo vendor: <https://doc.rust-lang.org/cargo/commands/cargo-vendor.html>

Ecosistema Rust:

- RustSec: <https://rustsec.org/>
- cargo-deny: <https://github.com/EmbarkStudios/cargo-deny>
- cargo-nextest: <https://nexte.st/>
- cargo-llvm-cov: <https://github.com/taiki-e/cargo-llvm-cov>
- Miri: <https://github.com/rust-lang/miri>
- cargo-mutants: <https://mutants.rs/>
- cargo-semver-checks: <https://github.com/obi1kenobi/cargo-semver-checks>
- cargo-bloat: <https://github.com/RazrFalcon/cargo-bloat>

---

# 124. Conclusión

Un MCP especializado en Rust puede aportar mucho valor a los agentes si se diseña como una plataforma de **observación, verificación y ejecución controlada**.

La prioridad debe ser:

```text
correctness
→ security
→ agent ergonomics
→ reproducibility
→ performance
→ feature breadth
```

No debe competir con Rust, Cargo, rustc, Clippy o rust-analyzer.

Debe convertir esas capacidades en interfaces estructuradas, seguras y eficientes para agentes.

Ese enfoque permite que el proyecto evolucione desde un MVP pequeño hacia una plataforma de ingeniería Rust para agentes sin perder coherencia arquitectónica.
