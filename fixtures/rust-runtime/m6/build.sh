#!/bin/sh
# Offline installation of rust-analyzer 1.98.1 and rust-src 1.98.1 into the M5
# guest filesystem. No network is used or available: both tarballs already sit
# in the build context and are verified against SHA256SUMS before anything is
# extracted. Each install.sh comes from the tarball itself (rust-installer),
# the same mechanism fixtures/rust-runtime/Dockerfile already uses to install
# rustc/cargo/rust-std/rustfmt and the llvm-tools plugin into arbitrary
# prefixes; it targets /opt/analyzer and /opt/rust cleanly with no fallback.
set -eu

INPUT=/opt/m6-input
BUILD=/opt/m6-build
DOCS=/usr/share/doc/rust-runtime/m6
TARGET=aarch64-unknown-linux-gnu
ANALYZER_ARCHIVE=rust-analyzer-1.98.1-aarch64-unknown-linux-gnu.tar.xz
ANALYZER_ROOT=rust-analyzer-1.98.1-aarch64-unknown-linux-gnu
SRC_ARCHIVE=rust-src-1.98.1.tar.xz
SRC_ROOT=rust-src-1.98.1

cd "$INPUT"
sha256sum --check SHA256SUMS
/opt/rust/bin/rustc --version --verbose | grep '^release: 1\.98\.1$'
/opt/rust/bin/rustc --version --verbose | grep '^host: aarch64-unknown-linux-gnu$'

mkdir -p "$BUILD" "$DOCS"

tar -xJf "$ANALYZER_ARCHIVE" -C "$BUILD" --no-same-owner
test -d "$BUILD/$ANALYZER_ROOT"
"$BUILD/$ANALYZER_ROOT/install.sh" --prefix=/opt/analyzer --disable-ldconfig
rm -f /opt/analyzer/lib/rustlib/uninstall.sh /opt/analyzer/lib/rustlib/install.log

tar -xJf "$SRC_ARCHIVE" -C "$BUILD" --no-same-owner
test -d "$BUILD/$SRC_ROOT"
"$BUILD/$SRC_ROOT/install.sh" --prefix=/opt/rust --disable-ldconfig
rm -f /opt/rust/lib/rustlib/uninstall.sh /opt/rust/lib/rustlib/install.log

# The rust-analyzer-preview tarball ships only the binary: `readelf -d` shows
# its RUNPATH is `$ORIGIN/../lib` (i.e. /opt/analyzer/lib) and it is dynamically
# linked against librustc_driver, which links against libLLVM in turn -- both
# already installed under /opt/rust/lib by the rustc component this image's
# base carries, both absent from /opt/analyzer/lib. `--disable-ldconfig` is
# used everywhere in this runtime precisely to avoid a global, mutable
# ld.so.cache, so the fix stays local and RUNPATH-relative: symlink the two
# real shared objects into rust-analyzer's own lib directory instead. No
# LD_LIBRARY_PATH, no ldconfig, no PATH change.
mkdir -p /opt/analyzer/lib
for shared_object in /opt/rust/lib/librustc_driver-*.so /opt/rust/lib/libLLVM*
do
  test -e "$shared_object"
  ln -s "$shared_object" "/opt/analyzer/lib/$(basename "$shared_object")"
done
set -- /opt/analyzer/lib/librustc_driver-*.so
test -e "$1"

readelf -h /opt/analyzer/bin/rust-analyzer | grep 'Machine:.*AArch64'
if ldd /opt/analyzer/bin/rust-analyzer 2>&1 | grep 'not found'; then
  exit 1
fi
/opt/analyzer/bin/rust-analyzer --version > "$DOCS/rust-analyzer-version.txt"

test -d /opt/rust/lib/rustlib/src/rust/library
test -f /opt/rust/lib/rustlib/src/rust/library/Cargo.lock

# rust-analyzer is a guest-only tool invoked by absolute path from the
# gateway. Neither install left it reachable through a PATH lookup: check
# every directory of the final image's PATH plus the rustc install prefix.
for dir in /usr/local/sbin /usr/local/bin /usr/sbin /usr/bin /sbin /bin /opt/rust/bin
do
  test ! -e "$dir/rust-analyzer"
done

cp "$INPUT/build-inputs.json" "$DOCS/sbom.json"
cp "$INPUT/SHA256SUMS" "$DOCS/build-inputs.sha256"
cp -R "$INPUT/notices" "$BUILD/notices"

cd "$BUILD"
find notices -type f \( \
  -iname 'LICENSE*' -o \
  -iname 'NOTICE*' -o \
  -iname 'COPYRIGHT*' -o \
  -iname 'COPYING*' \
\) -print | LC_ALL=C sort > "$DOCS/license-files.list"
tar -cf "$DOCS/license-texts.tar" -T "$DOCS/license-files.list"
sha256sum "$DOCS/license-texts.tar" > "$DOCS/license-texts.sha256"

cd "$DOCS"
analyzer_hash="$(sha256sum /opt/analyzer/bin/rust-analyzer)"
analyzer_hash="${analyzer_hash%% *}"
analyzer_size="$(stat -c '%s' /opt/analyzer/bin/rust-analyzer)"
src_lock_hash="$(sha256sum /opt/rust/lib/rustlib/src/rust/library/Cargo.lock)"
src_lock_hash="${src_lock_hash%% *}"
src_file_count="$(find /opt/rust/lib/rustlib/src/rust/library -type f | wc -l | tr -d ' ')"
printf '%s\n' \
  '{' \
  '  "schema": "rust-engineering-mcp.m6-installed.v1",' \
  '  "base_image_id": "sha256:e0a5ca1661b3e49d0a3d68ee3cc0963453078d08eb7fc43c30538c16b7998aac",' \
  '  "target": "aarch64-unknown-linux-gnu",' \
  '  "components": [' \
  '    {' \
  '      "name": "rust-analyzer",' \
  '      "path": "/opt/analyzer/bin/rust-analyzer",' \
  "      \"size\": $analyzer_size," \
  "      \"sha256\": \"$analyzer_hash\"" \
  '    },' \
  '    {' \
  '      "name": "rust-src",' \
  '      "path": "/opt/rust/lib/rustlib/src/rust/library",' \
  "      \"file_count\": $src_file_count," \
  "      \"cargo_lock_sha256\": \"$src_lock_hash\"" \
  '    }' \
  '  ]' \
  '}' > installed.json
find . -type f ! -name runtime-inventory.sha256 -print | LC_ALL=C sort | xargs sha256sum > runtime-inventory.sha256

rm -rf "$INPUT" "$BUILD"
