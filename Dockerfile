# syntax=docker/dockerfile:1
# benchlens: a static musl binary on distroless/static, running as non-root.
#
#   docker build -t benchlens .
#   docker run -d -p 8080:8080 -v benchlens-data:/data -e BENCHLENS_TOKEN=<16+ characters> benchlens
#
# It listens on 0.0.0.0 (not loopback): without BENCHLENS_TOKEN the server is read-only and upload, tag
# editing and delete answer 403. `docker stop` sends SIGTERM, which stops the server gracefully.

ARG RUST_VERSION=1
FROM rust:${RUST_VERSION}-alpine AS build
# musl-dev: C runtime for rusqlite's bundled SQLite. On Alpine the host target is
# *-unknown-linux-musl and binaries are statically linked by default.
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY . .
# Multi-arch builds run the platforms in parallel: the registry cache is shared but locked (two cargo
# processes unpacking into it at once fail with "File exists"), and each platform has its own target cache.
ARG TARGETPLATFORM
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/target,id=benchlens-target-${TARGETPLATFORM} \
    cargo build --release --locked -p benchlens \
 && install -D -m 0755 target/release/benchlens /out/benchlens
# The data folder, owned by the non-root user of the final image.
RUN mkdir -p /out/data && chown 65532:65532 /out/data

FROM gcr.io/distroless/static-debian12:nonroot
COPY --from=build /out/benchlens /usr/local/bin/benchlens
COPY --from=build --chown=65532:65532 /out/data /data
USER 65532:65532
WORKDIR /data
ENV BENCHLENS_DB=/data/benchlens.db \
    BENCHLENS_LISTEN=0.0.0.0:8080
VOLUME /data
EXPOSE 8080
ENTRYPOINT ["benchlens"]
CMD ["serve"]
