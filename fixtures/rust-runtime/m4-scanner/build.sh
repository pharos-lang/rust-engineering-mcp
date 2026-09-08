#!/bin/sh
set -eu

INPUT=/opt/m4-scanner-input
BUILD=/opt/m4-scanner-build
VENDOR="$BUILD/vendor"
CARGO_HOME_DIR="$BUILD/cargo-home"
TARGET_DIR="$BUILD/target"
DOCS=/usr/share/doc/rust-runtime/m4-scanner
TARGET=aarch64-unknown-linux-gnu

cd "$INPUT"
sha256sum --check SHA256SUMS
/opt/rust/bin/rustc --version --verbose | grep '^release: 1\.98\.1$'
/opt/rust/bin/rustc --version --verbose | grep '^host: aarch64-unknown-linux-gnu$'

mkdir -p "$VENDOR" "$CARGO_HOME_DIR" "$TARGET_DIR" "$DOCS/source" /opt/security/bin
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
  'directory = "/opt/m4-scanner-build/vendor"' \
  '[net]' \
  'offline = true' > "$CARGO_HOME_DIR/config.toml"

cp -R "$INPUT/helper" "$BUILD/helper"
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
  --manifest-path "$BUILD/helper/Cargo.toml" \
  --bin rust-mcp-unsafe-helper

install -m 0755 \
  "$TARGET_DIR/$TARGET/release/rust-mcp-unsafe-helper" \
  /opt/security/bin/rust-mcp-unsafe-helper
readelf -h /opt/security/bin/rust-mcp-unsafe-helper | grep 'Machine:.*AArch64'
if ldd /opt/security/bin/rust-mcp-unsafe-helper 2>&1 | grep 'not found'; then
  exit 1
fi

cp "$INPUT/build-inputs.json" "$DOCS/sbom.json"
cp "$INPUT/SHA256SUMS" "$DOCS/build-inputs.sha256"
cp "$INPUT/dependency-map.tsv" "$DOCS/dependency-map.tsv"
cp -R "$INPUT/helper/." "$DOCS/source/"
cp -R "$INPUT/notices" "$BUILD/notices"

cd "$BUILD"
find vendor notices -type f \( \
  -iname 'LICENSE*' -o \
  -iname 'NOTICE*' -o \
  -iname 'COPYRIGHT*' -o \
  -iname 'COPYING*' \
\) -print | LC_ALL=C sort > "$DOCS/license-files.list"
tar -cf "$DOCS/license-texts.tar" -T "$DOCS/license-files.list"
sha256sum "$DOCS/license-texts.tar" > "$DOCS/license-texts.sha256"

cd "$DOCS"
find source -type f -print | LC_ALL=C sort | xargs sha256sum > source-files.sha256
binary_hash="$(sha256sum /opt/security/bin/rust-mcp-unsafe-helper)"
binary_hash="${binary_hash%% *}"
binary_size="$(stat -c '%s' /opt/security/bin/rust-mcp-unsafe-helper)"
printf '%s  %s\n' "$binary_hash" /opt/security/bin/rust-mcp-unsafe-helper > binary.sha256
printf '%s\n' \
  '{' \
  '  "schema": "rust-engineering-mcp.m4-scanner-installed.v1",' \
  '  "base_image_id": "sha256:95dddeb5305f10b09b441e3cc4018ebb1a8a296d365c65106327d59f933c64e7",' \
  '  "target": "aarch64-unknown-linux-gnu",' \
  '  "helper": {' \
  '    "name": "rust-mcp-unsafe-helper",' \
  '    "version": "0.1.0",' \
  "    \"size\": $binary_size," \
  "    \"sha256\": \"$binary_hash\"" \
  '  }' \
  '}' > installed.json
find . -type f ! -name runtime-inventory.sha256 -print | LC_ALL=C sort | xargs sha256sum > runtime-inventory.sha256

rm -rf "$INPUT" "$BUILD"
