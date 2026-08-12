# The demo host: documents, tokens, share links — and the editor it serves.
#
# Built from the repository root:
#   docker build -f server/casual-calc-host/Dockerfile -t opencalc-host .

# --- The editor, compiled to WebAssembly ------------------------------------
FROM rust:1.96-slim-bookworm AS editor
WORKDIR /src
# Built from source rather than fetched as a release tarball. The tarballs are
# per-architecture, and a hardcoded `x86_64` URL fails on an arm64 machine with
# a tar error that says nothing about architecture — which is exactly how this
# was got wrong the first time. `cargo install` is slower and works everywhere
# the builder image runs.
RUN cargo install wasm-pack --version 0.13.1 --locked
RUN rustup target add wasm32-unknown-unknown
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
