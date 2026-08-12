# The collaboration server, as a container.
#
# `main.rs` reads its entire configuration from the environment because "the
# deployments this is for hand a container an environment and nothing else" —
# and there was no container. This is it.
#
# Built from the repository root:
#
#     docker build -f server/casual-calc-collab-server/Dockerfile -t opencalc-collab .

# --- Build -------------------------------------------------------------------
#
# The whole workspace is copied rather than the server alone: the server depends
# on nine path crates, and a build context narrowed to its own directory cannot
# see any of them.
FROM rust:1.96-slim-bookworm AS build
WORKDIR /src

# `--locked` for the same reason CI uses it: a container that resolves its own
# dependency versions is not the thing that was tested.
COPY . .
RUN cargo build --release --locked -p casual-calc-collab-server

# --- Run ---------------------------------------------------------------------
FROM debian:bookworm-slim AS run

# Required, not optional. The HTTP client uses the **system trust store** rather
# than a bundled copy of Mozilla's roots, deliberately: an integrator inside a
# company is routinely behind a certificate issued by that company's own CA, and
# a bundled list rejects it correctly and uselessly. Without this package every
# outbound fetch fails at the first request — loudly, which is the right way
# round, but only if somebody remembered to install it.
RUN apt-get update \
 && apt-get install --no-install-recommends -y ca-certificates \
 && rm -rf /var/lib/apt/lists/*

# Unprivileged. The process binds a port above 1024 and reads no files it did
# not fetch, so root buys nothing and costs the usual.
RUN useradd --system --uid 10001 --no-create-home --shell /usr/sbin/nologin opencalc
USER 10001:10001

COPY --from=build /src/target/release/casual-calc-collab-server /usr/local/bin/

# The default `OPENCALC_BIND`. An operator overriding it must publish whatever
# they chose instead; this is documentation of the default, not a constraint.
EXPOSE 8443

# `/healthz` says only that the process is serving — deliberately nothing about
# documents, because a node holding none is healthy and one that cannot reach
# Redis is a cluster problem rather than a reason to take this node out of
# rotation and make it worse.
HEALTHCHECK --interval=15s --timeout=3s --start-period=5s --retries=3 \
  CMD ["/usr/local/bin/casual-calc-collab-server", "--healthcheck"]

# No shell in front of it, so the process is PID 1 and receives SIGTERM
# directly. That matters here: the drain on shutdown is what saves documents
# whose participants have all left, and a shell swallowing the signal would turn
# every deploy into silent data loss.
ENTRYPOINT ["/usr/local/bin/casual-calc-collab-server"]
