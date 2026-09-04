#!/bin/sh
# Explicit local security gate. Builds only our reviewed offline fixture.
set -eu
: "${RUST_MCP_TEST_SOCKET:?Set the approved local Docker Unix socket}"
DOCKER=/Applications/Docker.app/Contents/Resources/bin/docker
TASK_CONTEXT=$(mktemp -d /private/tmp/rust-mcp-probe-build.XXXXXXXX)
trap 'rm -rf "$TASK_CONTEXT"' EXIT HUP INT TERM
mkdir "$TASK_CONTEXT/config"
printf '{"cliPluginsExtraDirs":["/Applications/Docker.app/Contents/Resources/cli-plugins"]}' > "$TASK_CONTEXT/config/config.json"
GOOS=linux GOARCH=arm64 CGO_ENABLED=0 go build -trimpath -ldflags='-s -w -buildid=' -o "$TASK_CONTEXT/probe" fixtures/execution-probe/main.go
cp fixtures/execution-probe/Dockerfile fixtures/execution-probe/canary "$TASK_CONTEXT/"
"$DOCKER" --config "$TASK_CONTEXT/config" --host "unix://$RUST_MCP_TEST_SOCKET" build --pull=false --network=none --tag rust-mcp-probe:m0 "$TASK_CONTEXT"
RUST_MCP_TEST_IMAGE=$("$DOCKER" --config "$TASK_CONTEXT/config" --host "unix://$RUST_MCP_TEST_SOCKET" image inspect rust-mcp-probe:m0 --format '{{.Id}}')
export RUST_MCP_TEST_IMAGE
RUST_MCP_HOST_CANARY=synthetic-not-a-secret cargo test -p rust-engineering-execution --test gateway --locked --offline -- --ignored --test-threads=1

cargo run --locked --offline -- capabilities --docker "$DOCKER" --docker-socket "$RUST_MCP_TEST_SOCKET" --state-root "$TASK_CONTEXT" --probe-image "$RUST_MCP_TEST_IMAGE"
