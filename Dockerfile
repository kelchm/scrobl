# The same Debian as the image below, so the binary links against the C
# library it will run with.
FROM rust:1.97.1-slim-trixie@sha256:8e8cf8f7fd54a2d23d5a743b3a03f56e26b6c774276c33fa0595111704ebb15c AS build
WORKDIR /src
COPY . .
RUN cargo build --locked --release -p scrobl-cli && mkdir /data

FROM gcr.io/distroless/cc-debian13:nonroot@sha256:cc66d222f0a7ec996b78a99fe76a969bc80b539a5016ad866c14d1949bc6d0ab
LABEL org.opencontainers.image.source="https://github.com/kelchm/scrobl" \
      org.opencontainers.image.description="Keeps a local SQLite copy of a Last.fm listening history. Unofficial." \
      org.opencontainers.image.licenses="MIT"
COPY --from=build /src/target/release/scrobl /usr/local/bin/scrobl
# Nothing else in the image is written to. A volume Docker creates for
# `/data` takes this owner.
COPY --from=build --chown=1000:1000 /data /data
USER 1000:1000
# SQLite puts its temporary files here when no other directory can be written.
WORKDIR /data
ENTRYPOINT ["/usr/local/bin/scrobl"]
