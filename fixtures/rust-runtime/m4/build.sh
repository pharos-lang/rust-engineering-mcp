#!/bin/sh
set -eu
cd /opt/m4-input
sha256sum --check SHA256SUMS
mkdir -p /opt/m4-build/vendor /opt/m4-build/deny /opt/m4-build/nightly /opt/security/bin
tar -xzf cargo-deny-0.19.7.crate -C /opt/m4-build/deny --no-same-owner
cmp /opt/m4-build/deny/cargo-deny-0.19.7/Cargo.lock cargo-deny-tag.Cargo.lock
while IFS="$(printf '\t')" read -r archive directory package_checksum
do
  test -n "$archive" && test -n "$directory" && test -n "$package_checksum"
  tar -xzf "$archive" -C /opt/m4-build/vendor --no-same-owner
  root="/opt/m4-build/vendor/$directory"
  test -d "$root"
  {
    printf '{"files":{'
    first=1
    find "$root" -type f ! -name .cargo-checksum.json -print | LC_ALL=C sort | while IFS= read -r file
    do
      relative="${file#$root/}"
      digest="$(sha256sum "$file")"; digest="${digest%% *}"
      if [ "$first" -eq 0 ]; then printf ','; fi
      first=0
      printf '"%s":"%s"' "$relative" "$digest"
    done
    printf '},"package":"%s"}\n' "$package_checksum"
  } > "$root/.cargo-checksum.json"
done < dependency-map.tsv
mkdir -p /opt/m4-build/cargo-home
printf '%s\n' '[source.crates-io]' 'replace-with = "vendored-sources"' '[source.vendored-sources]' 'directory = "/opt/m4-build/vendor"' '[net]' 'offline = true' > /opt/m4-build/cargo-home/config.toml
PATH=/opt/rust/bin:/usr/bin:/bin RUSTC=/opt/rust/bin/rustc CARGO_HOME=/opt/m4-build/cargo-home CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=/opt/m4-build/deny-target /opt/rust/bin/cargo build --release --locked --offline --manifest-path /opt/m4-build/deny/cargo-deny-0.19.7/Cargo.toml
install -m 0755 /opt/m4-build/deny-target/release/cargo-deny /opt/security/bin/cargo-deny
for archive in rustc-nightly-aarch64-unknown-linux-gnu.tar.xz cargo-nightly-aarch64-unknown-linux-gnu.tar.xz rust-std-nightly-aarch64-unknown-linux-gnu.tar.xz rust-src-nightly.tar.xz miri-nightly-aarch64-unknown-linux-gnu.tar.xz
do
  tar -xJf "$archive" -C /opt/m4-build/nightly --no-same-owner
done
for installer in /opt/m4-build/nightly/*/install.sh
do
  "$installer" --prefix=/opt/rust-nightly-2026-09-07 --disable-ldconfig
done
cmp /opt/rust-nightly-2026-09-07/lib/rustlib/src/rust/library/Cargo.lock rust-library.Cargo.lock
mkdir -p /opt/miri-sysroot/2026-09-07/aarch64-unknown-linux-gnu
PATH=/opt/rust-nightly-2026-09-07/bin:/usr/bin:/bin CARGO=/opt/rust-nightly-2026-09-07/bin/cargo RUSTC=/opt/rust-nightly-2026-09-07/bin/rustc CARGO_HOME=/opt/m4-build/cargo-home CARGO_NET_OFFLINE=true MIRI_LIB_SRC=/opt/rust-nightly-2026-09-07/lib/rustlib/src/rust/library MIRI_SYSROOT=/opt/miri-sysroot/2026-09-07/aarch64-unknown-linux-gnu /opt/rust-nightly-2026-09-07/bin/cargo miri setup --target aarch64-unknown-linux-gnu
mkdir -p /usr/share/doc/rust-runtime/m4
cp build-inputs.json /usr/share/doc/rust-runtime/m4/build-inputs.json
cp m4-sbom.json /usr/share/doc/rust-runtime/m4/sbom.json
cp cargo-deny-tag.Cargo.lock /usr/share/doc/rust-runtime/m4/cargo-deny.Cargo.lock
cp rust-library.Cargo.lock /usr/share/doc/rust-runtime/m4/rust-library.Cargo.lock
cp channel-rust-nightly.toml /usr/share/doc/rust-runtime/m4/channel-rust-nightly.toml
cd /opt/m4-build
find deny/cargo-deny-0.19.7 nightly vendor -type f \( -iname 'LICENSE*' -o -iname 'NOTICE*' -o -iname 'COPYRIGHT*' -o -iname 'COPYING*' \) -print | LC_ALL=C sort > /usr/share/doc/rust-runtime/m4/license-files.list
tar -cf /usr/share/doc/rust-runtime/m4/license-texts.tar -T /usr/share/doc/rust-runtime/m4/license-files.list
sha256sum /usr/share/doc/rust-runtime/m4/license-texts.tar > /usr/share/doc/rust-runtime/m4/license-files.sha256
cd /opt/m4-input
sha256sum /opt/security/bin/cargo-deny
rm -f /opt/rust-nightly-2026-09-07/lib/rustlib/install.log /opt/rust-nightly-2026-09-07/lib/rustlib/uninstall.sh
rm -rf /opt/m4-input /opt/m4-build
