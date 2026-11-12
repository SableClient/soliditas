FROM docker.io/library/rust:1-trixie AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

FROM gcr.io/distroless/cc-debian13:nonroot
COPY --from=build /src/target/release/soliditas /usr/local/bin/soliditas
USER 65532:65532
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/soliditas"]
