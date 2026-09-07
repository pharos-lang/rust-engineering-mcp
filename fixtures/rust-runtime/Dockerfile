# Explicit host provisioning only. No project source enters this build context.
ARG ENABLE_PLUGINS=0

FROM docker.io/library/debian@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171 AS prepared
COPY . /opt/install/
RUN apt-get update && apt-get install --yes --no-install-recommends ca-certificates gcc libc6-dev libssl3 xz-utils && rm -rf /var/lib/apt/lists/*
RUN cd /opt/install && sha256sum --check SHA256SUMS && for archive in *.tar.xz; do case "$archive" in llvm-tools-*) continue;; esac; tar -xJf "$archive"; done && for installer in */install.sh; do "$installer" --prefix=/opt/rust --disable-ldconfig; done && rm -rf /opt/install && mkdir -p /source /work

# This stage is also used by the M1 rebuild. M3 verifies every archive again at
# the point where plugin bytes enter the prepared Rust filesystem.
FROM prepared AS plugins
ARG ENABLE_PLUGINS=0
COPY . /opt/plugin-install/
RUN if [ "$ENABLE_PLUGINS" = "1" ]; then \
      cd /opt/plugin-install && sha256sum --check SHA256SUMS && \
      mkdir -p /opt/plugin-install/unpacked /opt/rust/bin && \
      for spec in \
        "cargo-nextest-0.9.143-aarch64-unknown-linux-gnu.tar.gz cargo-nextest" \
        "cargo-llvm-cov-aarch64-unknown-linux-gnu.tar.gz cargo-llvm-cov" \
        "cargo-semver-checks-aarch64-unknown-linux-gnu.tar.gz cargo-semver-checks"; do \
        archive="${spec%% *}"; binary="${spec#* }"; \
        tar -tzf "$archive" | grep -Fx "$binary"; \
        tar -xzf "$archive" -C /opt/plugin-install/unpacked --no-same-owner; \
        test -x "/opt/plugin-install/unpacked/$binary"; \
        install -m 0755 "/opt/plugin-install/unpacked/$binary" "/opt/rust/bin/$binary"; \
      done && \
      llvm_archive=llvm-tools-1.98.1-aarch64-unknown-linux-gnu.tar.xz && \
      llvm_root=llvm-tools-1.98.1-aarch64-unknown-linux-gnu && \
      tar -tJf "$llvm_archive" | grep -Fx "$llvm_root/install.sh" && \
      tar -tJf "$llvm_archive" | grep -Fx "$llvm_root/llvm-tools-preview/lib/rustlib/aarch64-unknown-linux-gnu/bin/llvm-cov" && \
      tar -tJf "$llvm_archive" | grep -Fx "$llvm_root/llvm-tools-preview/lib/rustlib/aarch64-unknown-linux-gnu/bin/llvm-profdata" && \
      tar -xJf "$llvm_archive" -C /opt/plugin-install/unpacked --no-same-owner && \
      "/opt/plugin-install/unpacked/$llvm_root/install.sh" --prefix=/opt/rust --disable-ldconfig && \
      grep -Fx llvm-tools-preview /opt/rust/lib/rustlib/components && \
      test -x /opt/rust/lib/rustlib/aarch64-unknown-linux-gnu/bin/llvm-cov && \
      test -x /opt/rust/lib/rustlib/aarch64-unknown-linux-gnu/bin/llvm-profdata && \
      mkdir -p /usr/share/doc/rust-runtime/notices && \
      find /opt/plugin-install/unpacked -type f \( -iname 'LICENSE*' -o -iname 'NOTICE*' -o -iname 'COPYING*' \) -exec cp -p {} /usr/share/doc/rust-runtime/notices/ \; && \
      printf '%s\n' \
        'cargo-nextest 0.9.143 | Apache-2.0 OR MIT | https://raw.githubusercontent.com/nextest-rs/nextest/cargo-nextest-0.9.143/cargo-nextest/Cargo.toml' \
        'cargo-llvm-cov 0.9.0 | Apache-2.0 OR MIT | https://raw.githubusercontent.com/taiki-e/cargo-llvm-cov/v0.9.0/Cargo.toml' \
        'cargo-semver-checks 0.50.0 | Apache-2.0 OR MIT | https://raw.githubusercontent.com/obi1kenobi/cargo-semver-checks/v0.50.0/Cargo.toml' \
        'cargo-mutants 27.1.0 | MIT | https://raw.githubusercontent.com/sourcefrog/cargo-mutants/v27.1.0/Cargo.toml' \
        'llvm-tools 1.98.1 | Apache-2.0 WITH LLVM-exception | official archive LICENSE/NOTICE files' \
        > /usr/share/doc/rust-runtime/plugin-license-sources.txt && \
      dpkg-query -W > /usr/share/doc/rust-runtime/dpkg-query.txt && \
      rm -f /opt/rust/lib/rustlib/uninstall.sh && \
      rm -f /usr/bin/apt /usr/bin/apt-cache /usr/bin/apt-get /usr/bin/apt-mark \
        /usr/bin/dpkg /usr/bin/dpkg-deb /usr/bin/dpkg-query /usr/bin/dpkg-split \
        /usr/bin/dpkg-statoverride /usr/bin/dpkg-trigger && \
      rm -rf /usr/lib/apt /usr/lib/dpkg /usr/share/dpkg /var/lib/apt /var/lib/dpkg /var/cache/apt; \
    fi && rm -rf /opt/plugin-install

FROM plugins AS builder
ARG ENABLE_PLUGINS=0
ENV PATH=/opt/rust/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
ENV CARGO_HOME=/opt/cargo
ENV CARGO_TARGET_DIR=/opt/cargo-target
RUN if [ "$ENABLE_PLUGINS" = "1" ]; then \
      cargo install cargo-mutants --version 27.1.0 --locked --root /opt/plugins && \
      index_file="$(find /opt/cargo/registry/index -type f -path '*/.cache/ca/rg/cargo-mutants' -print -quit 2>/dev/null || true)" && \
      registry_checksum=unavailable && \
      if [ -n "$index_file" ]; then \
        registry_checksum="$(tr '\000' '\n' < "$index_file" | sed -n 's/.*"vers":"27.1.0".*"cksum":"\([0-9a-f]\{64\}\)".*/\1/p' | tail -n 1)"; \
        test -n "$registry_checksum" || registry_checksum=unavailable; \
      fi && \
      crate_file="$(find /opt/cargo/registry/cache -type f -name 'cargo-mutants-27.1.0.crate' -print -quit 2>/dev/null || true)" && \
      crate_sha256=unavailable && \
      if [ -n "$crate_file" ]; then crate_sha256="$(sha256sum "$crate_file" | awk '{print $1}')"; fi && \
      binary_sha256="$(sha256sum /opt/plugins/bin/cargo-mutants | awk '{print $1}')" && \
      mkdir -p /usr/share/doc/rust-runtime && \
      printf '{"crate":"cargo-mutants","version":"27.1.0","command":"cargo install cargo-mutants --version 27.1.0 --locked --root /opt/plugins","registry":"crates.io","registry_index_checksum":"%s","crate_sha256":"%s","binary_sha256":"%s"}\n' "$registry_checksum" "$crate_sha256" "$binary_sha256" > /usr/share/doc/rust-runtime/cargo-mutants-build-inputs.json && \
      printf 'cargo-mutants-registry-index-checksum=%s\n' "$registry_checksum" && \
      printf 'cargo-mutants-crate-sha256=%s\n' "$crate_sha256" && \
      printf 'cargo-mutants-binary-sha256=%s\n' "$binary_sha256" && \
      install -m 0755 /opt/plugins/bin/cargo-mutants /opt/rust/bin/cargo-mutants && \
      rm -rf /opt/plugins /opt/cargo /opt/cargo-target; \
    fi

# Copy only the cleaned runtime filesystem. The builder cache and build dirs are
# removed above; no project source, package manager, installer or network tool
# from the provisioning stages is copied separately.
FROM scratch
COPY --from=builder / /
ENV PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
USER 65534:65534
WORKDIR /work
