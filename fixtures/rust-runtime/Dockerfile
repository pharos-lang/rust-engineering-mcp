# Explicit host provisioning only. No project source enters this build context.
FROM docker.io/library/debian@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171 AS prepared
RUN apt-get update && apt-get install --yes --no-install-recommends ca-certificates gcc libc6-dev libssl3 xz-utils && rm -rf /var/lib/apt/lists/*
COPY *.tar.xz SHA256SUMS /opt/install/
RUN cd /opt/install && sha256sum --check SHA256SUMS && for archive in *.tar.xz; do tar -xJf "$archive"; done && for installer in */install.sh; do "$installer" --prefix=/opt/rust --disable-ldconfig; done && rm -rf /opt/install && mkdir -p /source /work
# Copy only filesystem state to discard inherited execution configuration.
FROM scratch
COPY --from=prepared / /
ENV PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
USER 65534:65534
WORKDIR /work
