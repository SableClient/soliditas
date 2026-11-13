# syntax=docker/dockerfile:1

FROM --platform=$BUILDPLATFORM docker.io/library/rust:1-bookworm AS build
ARG BUILDARCH
# renovate: datasource=github-releases depName=ziglang/zig
ARG ZIG_VERSION=0.16.0
# renovate: datasource=github-releases depName=rust-cross/cargo-zigbuild
ARG CARGO_ZIGBUILD_VERSION=0.23.0
# renovate: datasource=github-releases depName=mozilla/sccache
ARG SCCACHE_VERSION=0.17.0
# sccache cannot cache incremental artifacts, and release builds do not use them.
ENV CARGO_INCREMENTAL=0

RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake clang libclang-dev \
    && rm -rf /var/lib/apt/lists/*

RUN case "${BUILDARCH}" in \
        amd64) HOST_TARGET=x86_64-unknown-linux-musl; ZIG_ARCH=x86_64 ;; \
        arm64) HOST_TARGET=aarch64-unknown-linux-musl; ZIG_ARCH=aarch64 ;; \
        *) echo "Unsupported BUILDARCH: ${BUILDARCH}" >&2; exit 1 ;; \
    esac \
    && curl -fsSL "https://ziglang.org/download/${ZIG_VERSION}/zig-${ZIG_ARCH}-linux-${ZIG_VERSION}.tar.xz" \
       | tar -xJ -C /opt \
    && ln -s "/opt/zig-${ZIG_ARCH}-linux-${ZIG_VERSION}/zig" /usr/local/bin/zig \
    && curl -fsSL "https://github.com/rust-cross/cargo-zigbuild/releases/download/v${CARGO_ZIGBUILD_VERSION}/cargo-zigbuild-${HOST_TARGET}.tar.xz" \
       | tar -xJO "cargo-zigbuild-${HOST_TARGET}/cargo-zigbuild" > /usr/local/bin/cargo-zigbuild \
    && chmod +x /usr/local/bin/cargo-zigbuild \
    && curl -fsSL "https://github.com/mozilla/sccache/releases/download/v${SCCACHE_VERSION}/sccache-v${SCCACHE_VERSION}-${ZIG_ARCH}-unknown-linux-musl.tar.gz" \
       | tar -xzO "sccache-v${SCCACHE_VERSION}-${ZIG_ARCH}-unknown-linux-musl/sccache" > /usr/local/bin/sccache \
    && chmod +x /usr/local/bin/sccache

WORKDIR /src
ARG TARGETARCH
ARG ENABLE_SCCACHE=0
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry,id=cargo-registry-${TARGETARCH},sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,id=cargo-git-${TARGETARCH},sharing=locked \
    --mount=type=secret,id=SCCACHE_AWS_ACCESS_KEY_ID,required=false \
    --mount=type=secret,id=SCCACHE_AWS_SECRET_ACCESS_KEY,required=false \
    set -eu \
    && case "${TARGETARCH}" in \
        amd64) RUST_TARGET=x86_64-unknown-linux-gnu; SCCACHE_SERVER_PORT=4226 ;; \
        arm64) RUST_TARGET=aarch64-unknown-linux-gnu; SCCACHE_SERVER_PORT=4227 ;; \
        *) echo "Unsupported TARGETARCH: ${TARGETARCH}" >&2; exit 1 ;; \
    esac \
    && if [ "$ENABLE_SCCACHE" = "1" ]; then \
        export RUSTC_WRAPPER=sccache \
               SCCACHE_IGNORE_SERVER_IO_ERROR=1 \
               SCCACHE_SERVER_PORT \
               SCCACHE_IDLE_TIMEOUT=0 \
               SCCACHE_BUCKET=sccache \
               SCCACHE_S3_KEY_PREFIX=soliditas \
               SCCACHE_ENDPOINT=https://s3.erwanleboucher.dev \
               SCCACHE_REGION=us-east-1 \
               SCCACHE_S3_USE_SSL=true \
               AWS_ACCESS_KEY_ID="$(cat /run/secrets/SCCACHE_AWS_ACCESS_KEY_ID)" \
               AWS_SECRET_ACCESS_KEY="$(cat /run/secrets/SCCACHE_AWS_SECRET_ACCESS_KEY)"; \
        sccache --start-server || unset RUSTC_WRAPPER; \
    fi \
    && rustup target add "$RUST_TARGET" \
    && cargo zigbuild --release --locked --target "$RUST_TARGET" \
    && if [ "$ENABLE_SCCACHE" = "1" ]; then sccache --show-stats || true; fi \
    && cp "target/${RUST_TARGET}/release/soliditas" /soliditas

FROM gcr.io/distroless/cc-debian13:nonroot
COPY --from=build /soliditas /usr/local/bin/soliditas
USER 65532:65532
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/soliditas"]
