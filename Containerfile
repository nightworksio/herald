FROM gcr.io/distroless/static-debian12:nonroot@sha256:afa5c872c891853ca7fcf1f12c3edb23f7eeef36189728842dd51042ff57f7ab
ARG TARGETARCH
LABEL org.opencontainers.image.title="herald" \
      org.opencontainers.image.description="Announce what happened to the places people gather." \
      org.opencontainers.image.source="https://github.com/nightworksio/herald" \
      org.opencontainers.image.licenses="MIT OR Apache-2.0"
COPY --chmod=0555 dist/${TARGETARCH}/herald /usr/local/bin/herald
COPY --chown=65532:65532 --chmod=0700 deploy/image/state/ /var/lib/herald/
ENV HERALD_CONFIG=/etc/herald/herald.toml \
    HERALD_LISTEN=0.0.0.0:8080 \
    HERALD_SECRETS_DIR=/run/secrets
WORKDIR /var/lib/herald
VOLUME ["/var/lib/herald"]
EXPOSE 8080
USER 65532:65532
HEALTHCHECK --interval=30s --timeout=10s --start-period=15s --retries=3 CMD ["herald", "health"]
ENTRYPOINT ["herald"]
CMD ["server"]
