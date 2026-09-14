# BuildKit resolves a bare `FROM sha256:…` as a REMOTE reference and fails under
# `--network=none`, and Docker 29's legacy builder no longer runs. The base is
# therefore named by its local tag and the caller must verify, immediately
# before building, that the tag resolves to the approved digest
# sha256:e0a5ca1661b3e49d0a3d68ee3cc0963453078d08eb7fc43c30538c16b7998aac.
# `scripts/build-m6-runtime.py` performs that check and records it in the receipt.
ARG BASE_IMAGE=rust-engineering-runtime:1.98.1-arm64-m5
FROM ${BASE_IMAGE} AS builder
USER 0:0
COPY . /opt/m6-input/
RUN /bin/sh /opt/m6-input/build.sh

FROM scratch
COPY --from=builder / /
ENV PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
USER 65534:65534
WORKDIR /work
