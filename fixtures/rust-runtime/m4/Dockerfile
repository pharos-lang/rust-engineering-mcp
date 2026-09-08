ARG BASE_IMAGE
FROM ${BASE_IMAGE} AS builder
USER 0:0
COPY . /opt/m4-input/
RUN /bin/sh /opt/m4-input/build.sh
FROM scratch
COPY --from=builder / /
ENV PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
USER 65534:65534
WORKDIR /work
