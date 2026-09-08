ARG BASE_IMAGE=sha256:25ed3626e710081a571a86a29521eaf2e890e796afd422ba5e409e0ce1891635
FROM ${BASE_IMAGE} AS builder
USER 0:0
COPY . /opt/m5-input/
RUN /bin/sh /opt/m5-input/build.sh

FROM scratch
COPY --from=builder / /
ENV PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
USER 65534:65534
WORKDIR /work
