#!/bin/sh
# Offline construction of the two M5 performance binaries. No network is used or
# available: every registry archive arrives in the build context and is verified
# against the checksum Cargo itself recorded in the pinned lockfiles.
set -eu

INPUT=/opt/m5-input
BUILD=/opt/m5-build
VENDOR="$BUILD/vendor"
CARGO_HOME_DIR="$BUILD/cargo-home"
TARGET_DIR="$BUILD/target"
DOCS=/usr/share/doc/rust-runtime/m5
TARGET=aarch64-unknown-linux-gnu

cd "$INPUT"
sha256sum --check SHA256SUMS
/opt/rust/bin/rustc --version --verbose | grep '^release: 1\.98\.1$'
/opt/rust/bin/rustc --version --verbose | grep '^host: aarch64-unknown-linux-gnu$'

mkdir -p "$VENDOR" "$CARGO_HOME_DIR" "$TARGET_DIR" "$DOCS/source" /opt/perf/bin
while IFS="$(printf '\t')" read -r archive directory package_checksum
do
  test -n "$archive" && test -n "$directory" && test -n "$package_checksum"
  tar -xzf "$archive" -C "$VENDOR" --no-same-owner
  root="$VENDOR/$directory"
  test -d "$root"
  {
    printf '{"files":{'
    first=1
    find "$root" -type f ! -name .cargo-checksum.json -print | LC_ALL=C sort | while IFS= read -r file
    do
      relative="${file#$root/}"
      digest="$(sha256sum "$file")"
      digest="${digest%% *}"
      if [ "$first" -eq 0 ]; then printf ','; fi
      first=0
      printf '"%s":"%s"' "$relative" "$digest"
    done
    printf '},"package":"%s"}\n' "$package_checksum"
  } > "$root/.cargo-checksum.json"
done < dependency-map.tsv

printf '%s\n' \
  '[source.crates-io]' \
  'replace-with = "vendored-sources"' \
  '[source.vendored-sources]' \
  'directory = "/opt/m5-build/vendor"' \
  '[net]' \
  'offline = true' > "$CARGO_HOME_DIR/config.toml"

build_one() {
  manifest="$1"
  binary="$2"
  PATH=/opt/rust/bin:/usr/bin:/bin \
  RUSTC=/opt/rust/bin/rustc \
  CARGO_HOME="$CARGO_HOME_DIR" \
  CARGO_NET_OFFLINE=true \
  CARGO_TARGET_DIR="$TARGET_DIR" \
  /opt/rust/bin/cargo build \
    --release \
    --locked \
    --offline \
    --target "$TARGET" \
    --manifest-path "$manifest" \
    --bin "$binary"
  install -m 0755 "$TARGET_DIR/$TARGET/release/$binary" "/opt/perf/bin/$binary"
  readelf -h "/opt/perf/bin/$binary" | grep 'Machine:.*AArch64'
  if ldd "/opt/perf/bin/$binary" 2>&1 | grep 'not found'; then
    exit 1
  fi
}

# cargo-bloat ships its own pinned Cargo.lock inside the published archive.
cp -R "$INPUT/cargo-bloat-src" "$BUILD/cargo-bloat"
build_one "$BUILD/cargo-bloat/Cargo.toml" cargo-bloat

cp -R "$INPUT/helper" "$BUILD/helper"
build_one "$BUILD/helper/Cargo.toml" rust-mcp-profile-helper

# The profiler must never be reachable through a cargo subcommand lookup, and
# cargo-bloat must never be invoked as anything but the exact absolute path the
# gateway builds. Neither directory is on PATH for a job container.
test ! -e /usr/local/bin/cargo-bloat
test ! -e /usr/local/bin/rust-mcp-profile-helper

cp "$INPUT/build-inputs.json" "$DOCS/sbom.json"
cp "$INPUT/SHA256SUMS" "$DOCS/build-inputs.sha256"
cp "$INPUT/dependency-map.tsv" "$DOCS/dependency-map.tsv"
cp -R "$INPUT/helper/." "$DOCS/source/"
cp -R "$INPUT/notices" "$BUILD/notices"

cd "$BUILD"
find vendor notices cargo-bloat -type f \( \
  -iname 'LICENSE*' -o \
  -iname 'NOTICE*' -o \
  -iname 'COPYRIGHT*' -o \
  -iname 'COPYING*' \
\) -print | LC_ALL=C sort > "$DOCS/license-files.list"
tar -cf "$DOCS/license-texts.tar" -T "$DOCS/license-files.list"
sha256sum "$DOCS/license-texts.tar" > "$DOCS/license-texts.sha256"

cd "$DOCS"
find source -type f -print | LC_ALL=C sort | xargs sha256sum > source-files.sha256
{
  printf '%s\n' \
    '{' \
    '  "schema": "rust-engineering-mcp.m5-installed.v1",' \
    '  "base_image_id": "sha256:25ed3626e710081a571a86a29521eaf2e890e796afd422ba5e409e0ce1891635",' \
    '  "target": "aarch64-unknown-linux-gnu",' \
    '  "binaries": ['
  first=1
  for binary in cargo-bloat rust-mcp-profile-helper
  do
    hash="$(sha256sum "/opt/perf/bin/$binary")"
    hash="${hash%% *}"
    size="$(stat -c '%s' "/opt/perf/bin/$binary")"
    if [ "$first" -eq 0 ]; then printf ',\n'; fi
    first=0
    printf '    {"name": "%s", "path": "/opt/perf/bin/%s", "size": %s, "sha256": "%s"}' \
      "$binary" "$binary" "$size" "$hash"
  done
  printf '\n  ]\n}\n'
} > installed.json
sha256sum /opt/perf/bin/cargo-bloat /opt/perf/bin/rust-mcp-profile-helper > binaries.sha256
find . -type f ! -name runtime-inventory.sha256 -print | LC_ALL=C sort | xargs sha256sum > runtime-inventory.sha256

rm -rf "$INPUT" "$BUILD"
