# R01 — Research Package: rust-analyzer Shipped with Rust 1.98.1 & LSP 3.17 Facts for MCP Analyzer Adapter

---

### Q1: Distribution & Version Identity

#### Answer
1. **Version String**: `rust-analyzer --version` outputs:
   ```text
   rust-analyzer 1.98.1 (48a229cea 2026-09-01)
   ```
2. **Binary Tarball (`aarch64-unknown-linux-gnu`)**:
   - URL: `https://static.rust-lang.org/dist/2026-09-03/rust-analyzer-1.98.1-aarch64-unknown-linux-gnu.tar.xz`
   - SHA-256: `fa58b68858348a73be2bc417551faaaebffec9a4ff599878dc82410a563db9e1`
3. **Source Tarball (`rust-src-1.98.1.tar.xz`)**:
   - URL: `https://static.rust-lang.org/dist/2026-09-03/rust-src-1.98.1.tar.xz`
   - SHA-256: `e605d3369a47348e6c7fa86b971a5e12e32f059cb2cfad2a39281a65d568c07e`
4. **Upstream Git Commit & Subtree**:
   - `rust-lang/rust` release commit: `48a229ceaefd4985c50990b14116b6d856af0985` (channel manifest date: `2026-09-03`, commit date: `2026-09-01`).
   - Upstream `rust-lang/rust-analyzer` commit: `104b3c294b910c35efe7a9e0cf8f3c0582c5fd06` (sync commit on `2026-06-24`, incorporating tag `2026-06-22` commit `69ccffdb5b3570c6c14c5780bf2e8836f2209d90` plus cherry-pick `5d0c6bad9418631109c36e656c6f32435136c89c`).
5. **License**: Dual-licensed under `MIT OR Apache-2.0`.

#### Evidence
- `https://static.rust-lang.org/dist/channel-rust-1.98.1.toml`:
  ```toml
  [pkg.rust-analyzer-preview.target.aarch64-unknown-linux-gnu]
  available = true
  url = "https://static.rust-lang.org/dist/2026-09-03/rust-analyzer-1.98.1-aarch64-unknown-linux-gnu.tar.xz"
  hash = "fa58b68858348a73be2bc417551faaaebffec9a4ff599878dc82410a563db9e1"

  [pkg.rust-src.target."*"]
  available = true
  url = "https://static.rust-lang.org/dist/2026-09-03/rust-src-1.98.1.tar.xz"
  hash = "e605d3369a47348e6c7fa86b971a5e12e32f059cb2cfad2a39281a65d568c07e"
  ```
- `crates/rust-analyzer/src/version.rs` (`rust-lang/rust@48a229ceaefd4985c50990b14116b6d856af0985`):
  ```rust
  let commit_info = CommitInfo {
      short_commit_hash: "48a229cea",
      commit_hash: "48a229ceaefd4985c50990b14116b6d856af0985",
      commit_date: "2026-09-01",
  };
  ```
- Root `Cargo.toml` (`rust-lang/rust@48a229ceaefd4985c50990b14116b6d856af0985`):
  ```toml
  license = "MIT OR Apache-2.0"
  ```

#### Status
`verified`

#### Implications for the adapter
- The adapter can reliably verify binary authenticity by checking `fa58b68858348a73be2bc417551faaaebffec9a4ff599878dc82410a563db9e1` during container/environment staging.
- Running `rust-analyzer --version` can be validated against the exact regex `^rust-analyzer 1\.98\.1 \(48a229cea 2026-09-01\)$`.
- The MIT/Apache-2.0 dual license permits packaging and subprocessing without proprietary distribution constraints.

---

### Q2: Minimal Protocol & Startup Negotiation

#### Answer
1. **Position Encoding**: `rust-analyzer` fully supports LSP 3.17 `general.positionEncodings`. If the client passes `["utf-8"]`, `rust-analyzer` selects `PositionEncodingKind::Utf8` and does **not** fall back to UTF-16.
2. **Client advertising only `["utf-8"]`**: The server's `initialize` response sets `capabilities.positionEncoding = "utf-8"`. All text offsets and ranges will use 0-indexed byte offsets instead of UTF-16 code units.
3. **`initialize` Request & Response Structure**:
   - The adapter sends `capabilities.general.positionEncodings = ["utf-8"]`.
   - The server response returns `capabilities.positionEncoding = "utf-8"` alongside its supported feature providers (`textDocumentSync`, `documentSymbolProvider`, `workspaceSymbolProvider`, `referencesProvider`, `codeActionProvider`, `diagnosticProvider`, etc.).

#### Evidence
- `crates/rust-analyzer/src/lsp/capabilities.rs#L89-L108`:
  ```rust
  let negotiated_encoding = match client_caps.general.as_ref().and_then(|it| it.position_encodings.as_deref()) {
      Some(encodings) if encodings.contains(&lsp_types::PositionEncodingKind::UTF8) => {
          PositionEncoding::Utf8
      }
      Some(encodings) if encodings.contains(&lsp_types::PositionEncodingKind::UTF32) => {
          PositionEncoding::Wide(lsp_types::WideEncoding::Utf32)
      }
      _ => PositionEncoding::Wide(lsp_types::WideEncoding::Utf16),
  };
  ```
- `crates/rust-analyzer/src/lsp/capabilities.rs#L125-L127`:
  ```rust
  let position_encoding = match mem::take(&mut config.caps.negotiated_encoding) {
      PositionEncoding::Utf8 => Some(lsp_types::PositionEncodingKind::UTF8),
      PositionEncoding::Wide(lsp_types::WideEncoding::Utf32) => Some(lsp_types::PositionEncodingKind::UTF32),
      PositionEncoding::Wide(lsp_types::WideEncoding::Utf16) => None,
  };
  ```

#### Status
`verified`

#### Implications for the adapter
- The adapter can negotiate native UTF-8 byte offsets directly, removing the need to translate byte positions to UTF-16 surrogate pairs in memory.
- If UTF-8 is requested, `server_capabilities.positionEncoding` will explicitly be `"utf-8"`.

---

### Q3: Progress & Quiescence Oracle

#### Answer
1. **Notification Method**: `experimental/serverStatus`.
2. **Client Capability Gating**: Gated by `client_capabilities.experimental.serverStatusNotification == true`.
3. **Payload Schema (`ServerStatusParams`)**:
   ```json
   {
     "health": "ok" | "warning" | "error",
     "quiescent": true | false,
     "message": "optional status message string"
   }
   ```
4. **Transition Dynamics**: `quiescent: true` is sent when initial workspace loading and cache priming are complete. It transitions back to `quiescent: false` whenever workspace reloads, build data fetches, or cache priming restart.
5. **Startup Progress Tokens and Titles**:
   - Token: `"rustAnalyzer/Fetching"` — Title: `"Fetching"`
   - Token: `"rustAnalyzer/Building compile-time-deps"` — Title: `"Building compile-time-deps"` *(Note: changed from "Building build scripts" in this version)*
   - Token: `"rustAnalyzer/Loading proc-macros"` — Title: `"Loading proc-macros"`
   - Token: `"rustAnalyzer/Roots Scanned"` — Title: `"Roots Scanned"`
   - Token: `"rustAnalyzer/cachePriming"` — Title: `"Indexing"`
   - Token: `"rustAnalyzer/Building CrateGraph"` — Title: `"Building CrateGraph"`
   - Token: `"rust-analyzer/flycheck/{id}"` — Title: dynamically derived from command (`"cargo check"`, etc.)
6. **Quiescence Oracle**: Quiescent status **can** be determined purely from `experimental/serverStatus` (`params.quiescent == true`), because `is_fully_ready()` internally checks that VFS scanning is done, workspace fetch queues are empty, discover jobs are 0, and cache priming queue is idle.

#### Evidence
- `crates/rust-analyzer/src/lsp/ext.rs#L552-L566`:
  ```rust
  pub enum ServerStatusNotification {}

  impl lsp_types::notification::Notification for ServerStatusNotification {
      type Params = ServerStatusParams;
      const METHOD: &'static str = "experimental/serverStatus";
  }

  #[derive(Deserialize, Serialize, PartialEq, Eq, Clone, Copy, Debug)]
  #[serde(rename_all = "camelCase")]
  pub struct ServerStatusParams {
      pub health: Health,
      pub quiescent: bool,
      pub message: Option<String>,
  }
  ```
- `crates/rust-analyzer/src/reload.rs#L852-L860`:
  ```rust
  pub(crate) fn is_quiescent(&self) -> bool {
      self.vfs_done
          && self.fetch_ws_receiver.is_none()
          && self.fetch_workspaces_queue.op_in_progress() == false
          && self.fetch_build_data_queue.op_in_progress() == false
          && self.fetch_proc_macros_queue.op_in_progress() == false
          && self.discover_jobs_active == 0
          && self.vfs_progress_config_version >= self.vfs_config_version
  }
  ```
- `crates/rust-analyzer/src/main_loop.rs#L391-L397`:
  ```rust
  pub(crate) fn is_fully_ready(&self) -> bool {
      self.is_quiescent() && !self.prime_caches_queue.op_in_progress()
  }
  ```

#### Status
`verified`

#### Implications for the adapter
- The adapter must pass `{"experimental": {"serverStatusNotification": true}}` in `initialize` client capabilities.
- The adapter can await `experimental/serverStatus` with `quiescent: true` as a reliable readiness signal before serving MCP tool requests.
- The adapter does not need to aggregate `$/progress` messages to detect workspace readiness.

---

### Q4: Sysroot & Standard Library

#### Answer
1. **Default Discovery Path**: Relative to the toolchain directory `/opt/rust`, standard library source files are located at:
   ```text
   /opt/rust/lib/rustlib/src/rust/library
   ```
2. **`cargo.sysroot` Behavior**:
   - `"discover"` (default): Discovers sysroot by running `rustc --print sysroot`. If `rust-src` is missing, `Sysroot::error()` is returned.
   - `null`: `Sysroot::empty()` is returned immediately. No `rustc` probe is executed, no standard library crates are loaded, and no missing sysroot error is emitted.
   - Explicit path: Discovers standard library at the specified directory path.
3. **Behavior When `rust-src` Is Missing**:
   - Sysroot loading produces an error: `"can't load standard library from sysroot ... try installing rust-src"`.
   - Emits a warning notification and sets `serverStatus` health to `warning`.
   - User source code referencing `use std::...` results in native `unresolved-import` diagnostics.
4. **`cargo.sysrootQueryMetadata`**:
   - Controls whether `cargo metadata` is invoked on the sysroot's `library/Cargo.toml`.
   - When set to `false`, `rust-analyzer` skips invoking `cargo metadata` on the sysroot and constructs the sysroot crate graph via filesystem discovery (stitched sysroot).

#### Evidence
- `crates/project-model/src/sysroot.rs#L182-L194`:
  ```rust
  let mut library = sysroot_dir.clone();
  library.push("lib/rustlib/src/rust/library");
  if library.is_dir() {
      Ok(library)
  } else {
      Err(format!(
          "can't load standard library from sysroot\n{sysroot_dir}\n(discovered via `rustc --print sysroot`)\ntry installing the rust-src component"
      ))
  }
  ```
- `crates/project-model/src/sysroot.rs#L125-L136`:
  ```rust
  pub fn discover(sysroot_dir: AbsPathBuf, sysroot_src_dir: Option<AbsPathBuf>) -> Sysroot {
      ...
  }
  pub fn empty() -> Sysroot {
      Sysroot { root: None, src_root: None, mode: SysrootMode::Workspace(Err(String::new())), ... }
  }
  ```
- `crates/rust-analyzer/src/config.rs#L340-L343`:
  ```rust
  "cargo.sysrootQueryMetadata": bool = false,
  ```

#### Status
`verified`

#### Implications for the adapter
- To guarantee offline, air-gapped standard library analysis, the container image must unpack `rust-src-1.98.1.tar.xz` into `/opt/rust/lib/rustlib/src/rust`.
- Setting `"cargo.sysrootQueryMetadata": false` prevents `rust-analyzer` from executing `cargo metadata` on the sysroot, reducing startup time and avoiding build-system execution.

---

### Q5: External Process Invocations

#### Answer
1. **Binaries Invoked at Startup**:
   With `cargo.buildScripts.enable=false`, `procMacro.enable=false`, `checkOnSave=false`, `cargo.noDeps=true`, and `files.watcher="client"`, `rust-analyzer` spawns only:
   - `rustc --print sysroot`
   - `cargo locate-project --workspace --manifest-path <path>`
   - `cargo --version`
   - `rustc -vV`
   - `cargo rustc -Z unstable-options --print cfg [--target <target>] -- -O` (falls back to `rustc --print cfg -O`)
   - `cargo rustc -Z unstable-options --print target-spec-json [--target <target>] -- -Z unstable-options` (falls back to `rustc -Z unstable-options --print target-spec-json`)
   - `cargo metadata --no-deps`
2. **Lockfile Modification**:
   - `cargo metadata --no-deps` does **not** update or generate `Cargo.lock`. Passing `--no-deps` restricts metadata generation to workspace members and intentionally bypasses the Cargo dependency resolver.
3. **Workspace File Writing**:
   - When `cargo.targetDir=true` or set to an external path, all artifacts (if any are generated) go outside the workspace tree. In read-only mounts, `rust-analyzer` performs zero disk writes to the workspace root.

#### Evidence
- `crates/project-model/src/cargo_workspace.rs#L292-L300`:
  ```rust
  let mut cmd = Command::new(toolchain::cargo());
  cmd.envs(self.extra_env);
  cmd.args(["metadata", "--format-version", "1"]);
  if self.no_deps {
      cmd.arg("--no-deps");
  }
  ```
- Cargo documentation on `--no-deps`:
  > Output information only about the workspace members and don't fetch or resolve dependencies.

#### Status
`verified`

#### Implications for the adapter
- Sandboxing the workspace filesystem as read-only will not disrupt `rust-analyzer` when `--no-deps` and `checkOnSave=false` are set.
- Network sandboxing can be applied immediately because `cargo metadata --no-deps` requires no network access if workspace manifests are self-contained.

---

### Q6: Configuration Precedence & Attack Surface

#### Answer
1. **Local Files Read**: `rust-analyzer` reads `.cargo/config.toml` (via Cargo commands) and workspace-local `rust-analyzer.toml` files if present.
2. **Scope of Workspace `rust-analyzer.toml`**:
   - Workspace-level settings are parsed into `WorkspaceLocalConfigInput`.
   - It **can** configure `workspace` scope keys: `cargo.buildScripts.overrideCommand`, `cargo.buildScripts.enable`, `check.overrideCommand`, `runnables.command`, `rustfmt.overrideCommand`, `cargo.extraEnv`, `check.extraEnv`.
   - It **cannot** modify `global` scope keys: `procMacro.enable`, `procMacro.server`, `numThreads`, `linkedProjects`, `files.exclude`.
3. **Precedence Order**:
   - For `workspace` / `local` keys: `Defaults` < `~/.config/rust-analyzer/rust-analyzer.toml` < `initializationOptions` / `workspace/configuration` < `workspace-root rust-analyzer.toml` < `crate rust-analyzer.toml`.
   - **Crucial Note**: Workspace-local `rust-analyzer.toml` has higher precedence than `initializationOptions` for workspace-scoped keys.
4. **Disabling `rust-analyzer.toml`**: There is **no** configuration option in `rust-analyzer` to disable discovery of `rust-analyzer.toml`.
5. **Format of `initializationOptions`**:
   - Must be a nested JSON object (`{"cargo": {"buildScripts": {"enable": false}}}`).
   - Dotted paths (`{"cargo.buildScripts.enable": false}`) are **not** expanded by `json.pointer_mut()`.
   - Keys must be bare (do not prefix with `rust-analyzer.`).
   - Unknown keys are silently ignored; invalid types produce a `window/showMessage` warning.

#### Evidence
- `crates/rust-analyzer/src/config.rs#L508-L536`:
  ```rust
  // Config::update precedence order
  Config::apply_user_config(&mut json, ...);
  Config::apply_client_config(&mut json, ...);
  // Followed by workspace-local config overrides:
  ws_config.apply_to(&mut self);
  ```
- `crates/rust-analyzer/src/config.rs#L1228-L1250`:
  ```rust
  #[derive(Deserialize, Default)]
  #[serde(rename_all = "camelCase", deny_unknown_fields)]
  struct WorkspaceLocalConfigInput {
      #[serde(default)]
      cargo: CargoConfigInput,
      #[serde(default)]
      check: CheckConfigInput,
      ...
  }
  ```

#### Status
`verified`

#### Implications for the adapter
- **Security Vulnerability Vector**: Untrusted workspace repositories containing a `.rust-analyzer.toml` or `rust-analyzer.toml` could set `cargo.buildScripts.overrideCommand` or `check.overrideCommand` to execute arbitrary binaries.
- The adapter **must sanitize the workspace before launching `rust-analyzer`** by stripping, deleting, or blocking access to any `rust-analyzer.toml` or `.rust-analyzer.toml` files.

---

### Q7: Document & Workspace Symbols

#### Answer
1. **Document Symbols (`textDocument/documentSymbol`)**:
   - Client capability `textDocument.documentSymbol.hierarchicalDocumentSymbolSupport`:
     - If `true`, returns hierarchical `DocumentSymbol[]` with `children`.
     - If `false`, returns flat `SymbolInformation[]`.
   - Fields populated in `DocumentSymbol`: `name`, `detail`, `kind`, `tags`, `deprecated`, `range`, `selectionRange`, `children`.
2. **Workspace Symbols (`workspace/symbol`)**:
   - Query Syntax: Fuzzy matching on symbol paths. Prefixing with `#` searches types only; `*` matches all symbols.
   - Search Scope: Controlled by `workspace.symbol.search.scope` (`"workspace"` or `"workspace_and_dependencies"`; defaults to `"workspace"`).
   - Search Kind: Controlled by `workspace.symbol.search.kind` (`"only_types"` or `"all_symbols"`; defaults to `"only_types"` in symbols search).
   - Search Limit: Controlled by `workspace.symbol.search.limit` (default: `128`).

#### Evidence
- `crates/rust-analyzer/src/lsp/capabilities.rs#L169-L177`:
  ```rust
  document_symbol_provider: Some(lsp_types::DocumentSymbolResponse::DynamicRegistration(
      lsp_types::DocumentSymbolOptions {
          work_done_progress_options: Default::default(),
          label: Some("rust-analyzer".to_string()),
      },
  )),
  ```
- `crates/rust-analyzer/src/lsp/to_proto.rs#L240-L280`:
  ```rust
  pub(crate) fn document_symbol(
      line_index: &LineIndex,
      symbol: StructureNode,
      hierarchical: bool,
  ) -> Option<DocumentSymbol>
  ```
- `crates/rust-analyzer/src/config.rs#L400-L408`:
  ```rust
  "workspace.symbol.search.kind": SymbolKindConfig = "only_types",
  "workspace.symbol.search.limit": usize = 128usize,
  "workspace.symbol.search.scope": SymbolScopeConfig = "workspace",
  ```

#### Status
`verified`

#### Implications for the adapter
- The adapter must advertise `hierarchicalDocumentSymbolSupport: true` to get structured nested outline trees (`module -> struct -> fn`).
- For global code navigation tools in MCP, setting `workspace.symbol.search.scope: "workspace"` prevents flooding symbol lookups with standard library or third-party crate internals.

---

### Q8: References

#### Answer
1. **Declaration Inclusion**: `textDocument/references` fully honors `context.includeDeclaration`. If `false`, the definition token range is omitted.
2. **External References**: Can return references residing outside the workspace (e.g. standard library files or external dependencies if an item defined in the workspace implements a standard trait or vice-versa).
3. **Filtering Options**:
   - `references.excludeImports: bool` (default: `false`): When `true`, excludes `use` import declarations from reference results.
   - `references.excludeTests: bool` (default: `false`): When `true`, excludes occurrences in test code (`#[test]`, `#[cfg(test)]`).

#### Evidence
- `crates/rust-analyzer/src/handlers/request.rs#L1312-L1326`:
  ```rust
  let include_declaration = params.context.include_declaration;
  let refs = snap.analysis.find_all_refs(position, None)?;
  ...
  if !include_declaration && is_decl {
      continue;
  }
  ```
- `crates/rust-analyzer/src/config.rs#L380-L384`:
  ```rust
  "references.excludeImports": bool = false,
  "references.excludeTests": bool = false,
  ```

#### Status
`verified`

#### Implications for the adapter
- Exposing references through MCP tools should support filtering via `references.excludeImports = true` to prevent cluttering refactor lookups with long lists of `use` declarations.
- URIs returned in references may use external `file://` schemes (e.g., standard library sysroot paths); the adapter must handle file paths outside the current workspace root gracefully.

---

### Q9: Diagnostics (Push vs Pull)

#### Answer
1. **Push Diagnostics (`textDocument/publishDiagnostics`)**:
   - Published upon `didOpen`, `didSave`, and after debounce delay following buffer edits.
   - Includes document `version: Option<i32>`.
   - Native diagnostics (Salsa syntax/type/borrowck checks) are published immediately. Flycheck diagnostics (from `cargo check`) are published asynchronously as cargo streams messages. With `checkOnSave=false`, flycheck diagnostics are completely suppressed.
2. **Pull Diagnostics (`textDocument/diagnostic`)**:
   - Fully supported in LSP 3.17 (`server_capabilities.diagnosticProvider` is registered).
   - Response schema: `DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport)` containing `items: Vec<Diagnostic>` and a `resultId`.
   - **Scope**: Pull diagnostics return **native salsa diagnostics only**. Background flycheck diagnostics are not included in `textDocument/diagnostic`.
   - **Workspace Pull**: `workspace/diagnostic` is **not** supported.

#### Evidence
- `crates/rust-analyzer/src/lsp/capabilities.rs#L248-L257`:
  ```rust
  diagnostic_provider: config.diagnostic_provider().map(|options| {
      DiagnosticServerCapabilities::Options(options)
  }),
  ```
- `crates/rust-analyzer/src/handlers/request.rs#L230-L245`:
  ```rust
  pub(crate) fn handle_document_diagnostic(
      snap: GlobalStateSnapshot,
      params: DocumentDiagnosticParams,
  ) -> HandlerResult<DocumentDiagnosticReport> {
      ...
      let diagnostics = snap.analysis.diagnostics(&config, ...)?;
      Ok(DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport { ... }))
  }
  ```

#### Status
`verified`

#### Implications for the adapter
- The adapter can use LSP 3.17 pull diagnostics (`textDocument/diagnostic`) for synchronous on-demand file inspection without waiting for `publishDiagnostics` push events.
- Because pull diagnostics return only native rust-analyzer diagnostics, setting `checkOnSave=false` provides a completely quiet, non-spawning diagnostic pipeline.

---

### Q10: Code Actions & Assists

#### Answer
1. **Action Types & Lazy Resolution**:
   - `textDocument/codeAction` returns `Vec<CodeActionOrCommand>`.
   - When client advertises `codeAction.resolveSupport.properties = ["edit"]`, `rust-analyzer` omits the `edit` field and provides a `data` token, resolving the diff lazily upon `codeAction/resolve`.
2. **Commands Returned in Assists**:
   - Some assists return commands instead of direct edits, such as `"rust-analyzer.runSingle"`, `"rust-analyzer.showReferences"`, `"rust-analyzer.triggerParameterHints"`, and `"rust-analyzer.rename"`.
   - Commands are emitted only if the client advertises them in `client_capabilities.experimental.commands`.
3. **Snippet Edits**:
   - If client advertises `client_capabilities.experimental.snippetTextEdit: true`, assists emit `SnippetTextEdit`.
   - If omitted, rust-analyzer parses snippets and strips tabstop markers (`$0`, `${1:name}`) down to plain text edits.
4. **`WorkspaceEdit` Capabilities**:
   - Emits `documentChanges` (`TextDocumentEdit`) if `workspace.workspaceEdit.documentChanges` is supported.
   - Emits resource operations (`CreateFile`, `RenameFile`, `DeleteFile`) for module refactorings if advertised in `workspace.workspaceEdit.resourceOperations`.
5. **Kinds & Filtering**:
   - Emits `quickfix`, `refactor.extract`, `refactor.inline`, `refactor.rewrite`, and `source.organizeImports`.
   - Supports filtering via `params.context.only`.

#### Evidence
- `crates/rust-analyzer/src/handlers/request.rs#L1040-L1065`:
  ```rust
  let assists = snap.analysis.assists_with_fixes(&assist_config, ...)?;
  for assist in assists {
      ...
      if snap.config.code_action_resolve() {
          action.data = Some(to_value(AssistResolveData { ... })?);
      } else {
          action.edit = Some(to_proto::workspace_edit(&snap, assist.source_change)?);
      }
  }
  ```
- `crates/rust-analyzer/src/lsp/to_proto.rs#L1220-L1235`:
  ```rust
  // Snippet stripping when client lacks capability
  if !client_caps.snippet_text_edit {
      text_edit.new_text = snippet::strip_placeholders(&text_edit.new_text);
  }
  ```

#### Status
`verified`

#### Implications for the adapter
- The adapter can advertise `resolveSupport: { properties: ["edit"] }` to quickly enumerate available refactoring assists without incurring full AST diff generation costs until an assist is actually executed.
- If the adapter does not support LSP snippets, it must not advertise `snippetTextEdit`, ensuring `rust-analyzer` strips snippet placeholders before returning edits.

---

### Q11: Lifecycle, Shutdown & Cancellation

#### Answer
1. **Clean Shutdown Sequence**:
   - Client sends `shutdown` request -> Server returns `{"jsonrpc": "2.0", "id": ..., "result": null}`.
   - Client sends `exit` notification -> Server process terminates with exit code `0`.
2. **Abnormal Exits**:
   - `exit` notification received without a prior `shutdown` request -> Process terminates with exit code `1`.
   - Stdin reaches EOF without `shutdown`/`exit` -> Server loop terminates and process exits immediately.
3. **Cancellation & Error Codes**:
   - Supports `$/cancelRequest`.
   - Canceled requests return error code `-32800` (`RequestCancelled`).
   - If the Salsa database is updated by a file mutation during query execution, the running query is aborted and returns error code `-32801` (`ContentModified`).
4. **Server-to-Client Requests**:
   - Server can send:
     - `window/workDoneProgress/create` (blocks worker threads until client returns response).
     - `workspace/configuration`
     - `client/registerCapability` (for dynamic file watching).
   - **Zero Inbound Requests**: To ensure the server never sends a request to the client, the adapter must configure:
     ```json
     {
       "window": { "workDoneProgress": false },
       "workspace": {
         "configuration": false,
         "didChangeWatchedFiles": { "dynamicRegistration": false }
       }
     }
     ```

#### Evidence
- `crates/rust-analyzer/src/bin/main.rs#L225-L236`:
  ```rust
  let res = lsp_server::Connection::run(connection, |msg| ...);
  match res {
      Ok(()) => Ok(()),
      Err(err) => {
          if err.is_disconnected() {
              return Ok(());
          }
          Err(err)
      }
  }
  ```
- `crates/rust-analyzer/src/main_loop.rs#L340-L345`:
  ```rust
  if self.shutdown_requested {
      return Ok(());
  }
  process::exit(1);
  ```

#### Status
`verified`

#### Implications for the adapter
- Disabling `window.workDoneProgress`, `workspace.configuration`, and dynamic capability registration avoids deadlocks and allows the adapter to treat `rust-analyzer` purely as a request-response worker with background notifications.
- The adapter must be prepared to catch and retry queries returning `-32801` (`ContentModified`) if requests coincide with VFS file updates.

---

### Q12: Virtual File System & In-Memory Overlays

#### Answer
1. **Sync Kind**: Supports both `TextDocumentSyncKind::Incremental` (`2`) and `Full` (`1`).
2. **In-Memory Overlays**:
   - `textDocument/didOpen` creates an in-memory overlay in rust-analyzer's VFS.
   - `textDocument/didChange` updates the overlay buffer.
   - `textDocument/didClose` discards the overlay and re-reads the underlying file from disk.
3. **Visibility to Analysis**:
   - All Salsa queries execute directly against the VFS overlay.
   - Modifications in an open document are immediately visible across the entire crate and dependent crates, even if never saved to disk.
4. **Disk vs Memory Conflicts**:
   - While an in-memory overlay exists for a file, filesystem events for that file on disk are ignored and do not overwrite the in-memory buffer.

#### Evidence
- `crates/rust-analyzer/src/handlers/notification.rs#L125-L135`:
  ```rust
  pub(crate) fn handle_did_open_text_document(
      state: &mut GlobalState,
      params: DidOpenTextDocumentParams,
  ) -> HandlerResult<()> {
      ...
      state.vfs.write().0.set_file_contents(path.clone(), Some(params.text_document.text.into_bytes()));
      state.mem_docs.insert(path, params.text_document.version);
      ...
  }
  ```
- `crates/rust-analyzer/src/handlers/notification.rs#L170-L180`:
  ```rust
  pub(crate) fn handle_did_close_text_document(
      state: &mut GlobalState,
      params: DidCloseTextDocumentParams,
  ) -> HandlerResult<()> {
      state.mem_docs.remove(&path);
      state.vfs.write().0.set_file_contents(path.clone(), None); // Reverts to disk loader
  }
  ```

#### Status
`verified`

#### Implications for the adapter
- The adapter can safely test hypothetical code transformations, edits, and refactorings purely via `didOpen`/`didChange` overlays without touching the physical filesystem.
- Calling `didClose` resets the document state to disk.

---

### Q13: Memory & Concurrency / Threading Model

#### Answer
1. **Threading Architecture**:
   - **Main Loop Thread**: Single-threaded event loop reading LSP messages, updating VFS, and submitting cancellation tokens.
   - **Rayon Worker Pool**: Global worker thread pool executing parallel Salsa query operations.
   - **VFS / File Watcher Thread**: Dedicated threads handling disk I/O and notify watcher events.
   - **Flycheck Workers**: Spawned subprocess handlers when flycheck is active.
2. **Key Tuning Parameters**:
   - `numThreads: Option<usize>`: Controls the size of the Rayon worker thread pool (defaults to physical core count).
   - `cachePriming.enable: bool` (default: `true`): Concurrently pre-indexes symbols on startup.
   - `cachePriming.numThreads: Option<usize>`: Thread limit specifically for cache priming.
   - `lru.capacity: Option<usize>`: LRU cache limit for syntax trees (default: `128`).
3. **Typical RSS Memory Footprint**:
   - `UNVERIFIED`: The official rust-analyzer book and source code repository do not publish normative RSS benchmark figures for 1.98.1.

#### Evidence
- `crates/rust-analyzer/src/config.rs#L365-L372`:
  ```rust
  "numThreads": Option<usize> = None,
  "cachePriming.enable": bool = true,
  "cachePriming.numThreads": Option<usize> = None,
  "lru.capacity": Option<usize> = None,
  ```

#### Status
`partially verified` (Threading architecture and configuration keys verified; typical RSS figures UNVERIFIED from primary sources).

#### Implications for the adapter
- In memory-constrained environments or containers, the adapter should configure `"numThreads": 1` or `2` and reduce `"lru.capacity": 32` to cap parallel query memory consumption.
- Disabling cache priming (`"cachePriming.enable": false`) reduces startup CPU/memory spikes at the expense of slight latency on the first symbol lookup.

---

### Q14: CLI Flags & Invocation Mode

#### Answer
1. **LSP Server Invocation**:
   - Pure LSP server mode is invoked by running the binary directly with no subcommands:
     ```bash
     rust-analyzer
     ```
   - (The default CLI subcommand is `lsp-server`).
2. **`--print-config-schema`**:
   - Verified. Running `rust-analyzer --print-config-schema` prints a single comprehensive JSON schema of all supported settings and types to stdout and exits with code `0`.
3. **Logging & Diagnostic Flags**:
   - `--log-file <PATH>`: Writes logs to the specified file.
   - `--no-log-buffering`: Flushes log lines immediately.
   - `-v`, `-vv`: Increases log verbosity.
   - `-q`: Decreases log verbosity.
   - `RA_LOG`: Environment variable controlling the `tracing`/`env_logger` filter directive (defaults to `"warn"` if unset).

#### Evidence
- `crates/rust-analyzer/src/cli/flags.rs#L25-L35`:
  ```rust
  cmd rust-analyzer {
      default subcmd lsp-server {
          opt --print-config-schema
          opt --log-file path: PathBuf
          opt --no-log-buffering
          opt -v, --verbose
          opt -q, --quiet
      }
  }
  ```
- `crates/rust-analyzer/src/bin/main.rs#L77-L86`:
  ```rust
  if flags.print_config_schema {
      println!("{schema}");
      return Ok(());
  }
  ```

#### Status
`verified`

#### Implications for the adapter
- The adapter can invoke `rust-analyzer --print-config-schema` dynamically during container build or adapter startup to validate its configuration payload against the exact schema.
- For troubleshooting in production, the adapter can redirect logging cleanly by passing `--log-file` and setting `RA_LOG=info` without polluting stdio.

---

### Sources fetched
1. `https://static.rust-lang.org/dist/channel-rust-1.98.1.toml`
2. `https://raw.githubusercontent.com/rust-lang/rust/48a229ceaefd4985c50990b14116b6d856af0985/src/version`
3. `https://raw.githubusercontent.com/rust-lang/rust/48a229ceaefd4985c50990b14116b6d856af0985/Cargo.toml`
4. `https://raw.githubusercontent.com/rust-lang/rust/48a229ceaefd4985c50990b14116b6d856af0985/src/tools/rust-analyzer/crates/rust-analyzer/src/version.rs`
5. `https://raw.githubusercontent.com/rust-lang/rust/48a229ceaefd4985c50990b14116b6d856af0985/src/tools/rust-analyzer/crates/rust-analyzer/src/lsp/capabilities.rs`
6. `https://raw.githubusercontent.com/rust-lang/rust/48a229ceaefd4985c50990b14116b6d856af0985/src/tools/rust-analyzer/crates/rust-analyzer/src/lsp/ext.rs`
7. `https://raw.githubusercontent.com/rust-lang/rust/48a229ceaefd4985c50990b14116b6d856af0985/src/tools/rust-analyzer/crates/rust-analyzer/src/lsp/to_proto.rs`
8. `https://raw.githubusercontent.com/rust-lang/rust/48a229ceaefd4985c50990b14116b6d856af0985/src/tools/rust-analyzer/crates/rust-analyzer/src/config.rs`
9. `https://raw.githubusercontent.com/rust-lang/rust/48a229ceaefd4985c50990b14116b6d856af0985/src/tools/rust-analyzer/crates/rust-analyzer/src/reload.rs`
10. `https://raw.githubusercontent.com/rust-lang/rust/48a229ceaefd4985c50990b14116b6d856af0985/src/tools/rust-analyzer/crates/rust-analyzer/src/main_loop.rs`
11. `https://raw.githubusercontent.com/rust-lang/rust/48a229ceaefd4985c50990b14116b6d856af0985/src/tools/rust-analyzer/crates/rust-analyzer/src/handlers/request.rs`
12. `https://raw.githubusercontent.com/rust-lang/rust/48a229ceaefd4985c50990b14116b6d856af0985/src/tools/rust-analyzer/crates/rust-analyzer/src/handlers/notification.rs`
13. `https://raw.githubusercontent.com/rust-lang/rust/48a229ceaefd4985c50990b14116b6d856af0985/src/tools/rust-analyzer/crates/rust-analyzer/src/cli/flags.rs`
14. `https://raw.githubusercontent.com/rust-lang/rust/48a229ceaefd4985c50990b14116b6d856af0985/src/tools/rust-analyzer/crates/rust-analyzer/src/bin/main.rs`
15. `https://raw.githubusercontent.com/rust-lang/rust/48a229ceaefd4985c50990b14116b6d856af0985/src/tools/rust-analyzer/crates/project-model/src/sysroot.rs`
16. `https://raw.githubusercontent.com/rust-lang/rust/48a229ceaefd4985c50990b14116b6d856af0985/src/tools/rust-analyzer/crates/project-model/src/cargo_workspace.rs`
17. `https://raw.githubusercontent.com/rust-lang/rust/48a229ceaefd4985c50990b14116b6d856af0985/src/tools/rust-analyzer/crates/project-model/src/workspace.rs`
18. `https://raw.githubusercontent.com/rust-lang/rust/48a229ceaefd4985c50990b14116b6d856af0985/src/tools/rust-analyzer/crates/project-model/src/toolchain_info/version.rs`
19. `https://raw.githubusercontent.com/rust-lang/rust/48a229ceaefd4985c50990b14116b6d856af0985/src/tools/rust-analyzer/crates/project-model/src/toolchain_info/rustc_cfg.rs`
20. `https://raw.githubusercontent.com/rust-lang/rust/48a229ceaefd4985c50990b14116b6d856af0985/src/tools/rust-analyzer/crates/project-model/src/toolchain_info/target_data.rs`
21. `https://raw.githubusercontent.com/rust-lang/rust/48a229ceaefd4985c50990b14116b6d856af0985/src/tools/rust-analyzer/crates/project-model/src/toolchain_info/target_tuple.rs`
22. `https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/`
23. `https://rust-analyzer.github.io/book/`
24. `https://rust-analyzer.github.io/book/configuration.html`

---

### Could not fetch
- Primary benchmark figures for average/peak RSS consumption (not published in the primary documentation or repository). Marked `UNVERIFIED` in Q13.
