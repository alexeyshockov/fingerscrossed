# syntax=docker/dockerfile:1.11
ARG RUST_VERSION=1.85
# "bookworm" (or other Debian version) or "alpine"
ARG RUST_OS=bookworm
# "debian:bookworm-slim" or "alpine:3.21"
ARG APP_OS=debian:bookworm-slim


FROM rust:${RUST_VERSION}-${RUST_OS} AS builder
ARG RUST_OS
WORKDIR /usr/src/fingerscrossed
COPY . .
RUN set -o errexit -o nounset -o allexport && \
    if [ "${RUST_OS}" = "alpine" ]; then \
        apk add --no-cache musl-dev; \
    fi && \
    cargo build --locked --release


FROM ${APP_OS}
LABEL maintainer="Alexey Shokov <alexey@shokov.dev>"
LABEL org.opencontainers.image.authors="Alexey Shokov <alexey@shokov.dev>"
LABEL org.opencontainers.image.title="Logging, fingers crossed"
LABEL org.opencontainers.image.licenses="MIT"
COPY --from=builder /usr/src/fingerscrossed/target/release/fingerscrossed /usr/local/bin/fingerscrossed
ENTRYPOINT ["fingerscrossed"]
CMD ["--help"]
