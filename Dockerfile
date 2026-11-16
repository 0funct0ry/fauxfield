# Headless Fauxfield (no GUI). Mount a workspace at /workspace:
#   docker run --rm -v "$PWD":/workspace -p 8080:8080 ghcr.io/0funct0ry/fauxfield:1.0.0
# Servers must listen on 0.0.0.0 inside the container (set `host = "0.0.0.0"` in the Server file).
FROM rust:1.89-alpine AS build
RUN apk add --no-cache musl-dev build-base perl
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p fauxfield --no-default-features --features headless \
    && cp target/release/fauxfield /fauxfield

FROM scratch
COPY --from=build /fauxfield /fauxfield
WORKDIR /workspace
VOLUME /workspace
ENTRYPOINT ["/fauxfield"]
CMD ["run", "/workspace"]
