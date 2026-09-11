# Derived M4 scanner runtime fixture

This fixture prepares a closed build context for an image derived from the exact
admitted M4 image:

```text
sha256:95dddeb5305f10b09b441e3cc4018ebb1a8a296d365c65106327d59f933c64e7
```

`provision.py` performs no network request and does not invoke Docker. It verifies
the helper's private manifest and lockfile, requires the exact eleven `.crate`
archives already present in a caller-selected Cargo cache, validates their Cargo
checksums and archive members, and copies only the closed helper source, repository
notices, build scripts, and generated inventory into the build context. Missing,
linked, malformed, extra, or checksum-divergent inputs fail closed.

Prepare from the repository root:

```text
PYTHONDONTWRITEBYTECODE=1 python3 fixtures/rust-runtime/m4-scanner/provision.py \
  --cargo-cache ~/.cargo/registry/cache \
  --output target/m4-scanner-provision
```

The generated `prepare-receipt.json` has status `prepared_not_built`. Preparation
does not claim that an image exists or that the runtime passed admission or hostile
qualification.

Build the authorized prepared context using a verified local base tag and a tar
stream. Docker treats a bare `sha256:…` in `FROM` as a registry reference; its
`--pull=false` flag alone does not prevent a metadata lookup. Verify the local tag
before and after the build, require its immutable ID to match the value above,
and preserve both inspections. Use the same Docker executable/socket throughout:

```sh
DOCKER=/Applications/Docker.app/Contents/Resources/bin/docker
SOCKET=unix:///Users/cburgosro/.docker/run/docker.sock
BASE=rust-engineering-runtime:1.98.1-arm64-m4
EXPECTED=sha256:95dddeb5305f10b09b441e3cc4018ebb1a8a296d365c65106327d59f933c64e7
CONTEXT=target/m4-scanner-provision/build-context
test "$("$DOCKER" --host "$SOCKET" image inspect --format '{{.Id}}' "$BASE")" = "$EXPECTED"
(cd "$CONTEXT" && shasum -a 256 --check SHA256SUMS)
COPYFILE_DISABLE=1 tar -cf target/m4-scanner-provision/build-context.tar -C "$CONTEXT" .
shasum -a 256 target/m4-scanner-provision/build-context.tar
"$DOCKER" --host "$SOCKET" build --platform linux/arm64 --network=none \
  --pull=false --no-cache --build-arg "BASE_IMAGE=$BASE" \
  --tag rust-engineering-runtime:1.98.1-arm64-m4-scanner \
  - < target/m4-scanner-provision/build-context.tar
test "$("$DOCKER" --host "$SOCKET" image inspect --format '{{.Id}}' "$BASE")" = "$EXPECTED"
```

Do not run another Docker writer concurrently. The tar stream binds the exact
input bytes independently of BuildKit directory-cache metadata. The successful
v3 build and the two failed approaches remain in
`docs/validation/M4/scanner-provisioning/`; the qualified image is selected by its
resulting ID, never by the build tag. A rebuild creates a new admission candidate
and requires its own installed-source, binary, configuration and native evidence.

The Dockerfile preserves the admitted base rootfs, builds with the stable GNU ARM64
toolchain at `/opt/rust/bin`, uses Cargo's vendored source replacement with
`--locked --offline`, and installs only
`/opt/security/bin/rust-mcp-unsafe-helper`. The final config restores the base
runtime values `USER 65534:65534`, the admitted `PATH`, and `WORKDIR /work`.

The image records under `/usr/share/doc/rust-runtime/m4-scanner`:

- input and installed SBOMs;
- exact helper sources and private `Cargo.lock`;
- source and binary hashes;
- archive dependency map and checksums;
- repository and dependency license/notice texts;
- a hash inventory of every emitted evidence file.

Benign host checks are:

```text
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest fixtures/rust-runtime/m4-scanner/test_provision.py
```

The ADR-069 hostile generators are excluded from this build context. They must not
run on the host; later qualification executes them only in the contained guest.
