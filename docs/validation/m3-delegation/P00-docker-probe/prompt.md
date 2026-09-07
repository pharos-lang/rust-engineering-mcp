# Package P00 — sandbox capability probe: can a Codex workspace-write session reach the local Docker daemon? (read-only)

Task: P00. Objective: determine, with exact command outputs, whether this Codex sandbox (workspace-write, with `sandbox_workspace_write.network_access=true` set by the orchestrator for this run) can talk to the Docker daemon through the unix socket. This decides which delegate can run the Docker-based M3 gates. Do not build, pull, run or remove anything.
Model / CLI / effort: GPT-5.6 Luna via codex exec, effort low.

Run exactly these commands and paste their full stdout/stderr and exit codes:
1. `ls -la /Users/cburgosro/.docker/run/docker.sock`
2. `/Applications/Docker.app/Contents/Resources/bin/docker -H unix:///Users/cburgosro/.docker/run/docker.sock version --format '{{.Server.Version}} {{.Server.Os}}/{{.Server.Arch}}'`
3. `/Applications/Docker.app/Contents/Resources/bin/docker -H unix:///Users/cburgosro/.docker/run/docker.sock info --format '{{.ServerVersion}} {{.CgroupVersion}} {{.Runtimes}}'`
4. `python3 -c "import socket;s=socket.socket(socket.AF_UNIX);s.settimeout(3);s.connect('/Users/cburgosro/.docker/run/docker.sock');print('unix connect ok')"`

Prohibited: any other docker subcommand, any file edit, any network fetch, any git command.
Final message sections: Task, Result (one line: REACHABLE / NOT REACHABLE and why), Files changed (none), Tests executed, Evidence (verbatim outputs), Risks, Decisions, Open issues.
