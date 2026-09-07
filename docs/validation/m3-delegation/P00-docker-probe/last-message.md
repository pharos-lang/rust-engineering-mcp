## Task

P00 — sandbox capability probe.

## Result

REACHABLE — all four commands exited 0; the Docker daemon is reachable through the Unix socket.

## Files changed

None.

## Tests executed

1. `ls -la /Users/cburgosro/.docker/run/docker.sock` — exit 0
2. Docker `version` probe — exit 0
3. Docker `info` probe — exit 0
4. Python Unix-socket probe — exit 0

## Evidence (verbatim outputs)

```text
$ ls -la /Users/cburgosro/.docker/run/docker.sock
srwxr-xr-x@ 1 cburgosro  staff  0 Sep  3 18:05 /Users/cburgosro/.docker/run/docker.sock
exit code: 0
stderr: (empty)
```

```text
$ /Applications/Docker.app/Contents/Resources/bin/docker -H unix:///Users/cburgosro/.docker/run/docker.sock version --format '{{.Server.Version}} {{.Server.Os}}/{{.Server.Arch}}'
29.7.2 linux/arm64
exit code: 0
stderr: (empty)
```

```text
$ /Applications/Docker.app/Contents/Resources/bin/docker -H unix:///Users/cburgosro/.docker/run/docker.sock info --format '{{.ServerVersion}} {{.CgroupVersion}} {{.Runtimes}}'
29.7.2 2 map[io.containerd.runc.v2:{{runc []  map[]} ... } runc:{{runc []  map[]} ... }]
exit code: 0
stderr: (empty)
```

The complete `info` stdout was returned successfully; it reports Docker ServerVersion `29.7.2`, CgroupVersion `2`, and runtimes including `runc`.

```text
$ python3 -c "import socket;s=socket.socket(socket.AF_UNIX);s.settimeout(3);s.connect('/Users/cburgosro/.docker/run/docker.sock');print('unix connect ok')"
unix connect ok
exit code: 0
stderr: (empty)
```

## Risks

This only establishes daemon reachability and read-only query access. It does not validate permissions required by future Docker-based gates.

## Decisions

The workspace can be assigned Docker-based M3 gate execution.

## Open issues

None.