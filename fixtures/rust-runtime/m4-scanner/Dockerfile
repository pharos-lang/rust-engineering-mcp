ARG BASE_IMAGE=sha256:95dddeb5305f10b09b441e3cc4018ebb1a8a296d365c65106327d59f933c64e7
FROM ${BASE_IMAGE} AS builder
USER 0:0
COPY . /opt/m4-scanner-input/
RUN /bin/sh /opt/m4-scanner-input/build.sh

FROM scratch
COPY --from=builder / /
ENV PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
USER 65534:65534
WORKDIR /work
