FROM scratch
COPY --chmod=0555 probe /mcp-probe
COPY --chmod=0666 canary /rootfs-canary
USER 65532:65532
ENTRYPOINT ["/mcp-probe"]
