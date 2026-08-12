# The demo host: documents, tokens, share links — and the editor it serves.
#
# Built from the repository root:
#   docker build -f server/casual-calc-host/Dockerfile -t opencalc-host .

# --- The editor, compiled to WebAssembly ------------------------------------
FROM rust:1.96-slim-bookworm AS editor
WORKDIR /src
RUN apt-get update && apt-get install --no-install-recommends -y curl \
 && rm -rf /var/lib/apt/lists/* \
 && curl -sSfL https://github.com/drager/wasm-pack/releases/download/v0.13.1/wasm-pack-v0.13.1-x86_64-unknown-linux-musl.tar.gz \
    | tar -xz --strip-components=1 -C /usr/local/bin --wildcards '*/wasm-pack'
COPY . .
RUN wasm-pack build crates/casual-calc-wasm --release --target web --out-dir /src/webapp/pkg

# --- The host binary ---------------------------------------------------------
FROM rust:1.96-slim-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p casual-calc-host

# --- Run ---------------------------------------------------------------------
FROM debian:bookworm-slim AS run
RUN apt-get update && apt-get install --no-install-recommends -y ca-certificates \
 && rm -rf /var/lib/apt/lists/*
RUN useradd --system --uid 10002 --no-create-home --shell /usr/sbin/nologin opencalc

COPY --from=build /src/target/release/casual-calc-host /usr/local/bin/
# The editor is served from this same origin, so a share link is one host and
# there is no CORS story to explain to anybody evaluating it.
COPY --from=editor /src/webapp /editor

# Documents live here. A named volume in the compose file, so `docker compose
# down` does not throw away what somebody just typed.
RUN mkdir -p /data && chown 10002:10002 /data
VOLUME ["/data"]
USER 10002:10002
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/casual-calc-host"]
