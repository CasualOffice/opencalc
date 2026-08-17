# The WOPI adapter, as a container.
#
# What makes OpenCalc installable into Nextcloud, ownCloud, SharePoint, Moodle
# and Alfresco: it serves the discovery document those hosts read, and speaks
# WOPI back at them. Design: docs/74.
#
# Built from the repository root:
#
#     docker build -f server/casual-calc-wopi/Dockerfile -t opencalc-wopi .

# --- Build -------------------------------------------------------------------
# Pinned by digest, not by tag. A tag is a moving reference: `bookworm-slim`
# is retagged whenever Debian rebuilds it, so the same Dockerfile can produce a
# different image with nothing in this repository having changed. That is the
# supply-chain hole the `repository-policy` job already closes for GitHub
# Actions, and a base image is the larger of the two — it is every byte under
# the binary. The tag stays in the comment so a human can still read it.
FROM rust:1.96-slim-bookworm@sha256:e18a79fc84dfcfc3ab5ba72290398a644c135c97eaa881447fddc354ee4701a3 AS build
WORKDIR /src

# `--locked` for the same reason CI uses it: a container that resolves its own
# dependency versions is not the thing that was tested.
COPY . .
RUN cargo build --release --locked -p casual-calc-wopi

# --- Run ---------------------------------------------------------------------
FROM debian:bookworm-slim@sha256:abd67ffcfa541b485a3dff59865ab629aa048a6c613e639d36e7456b0b229241 AS run

# Required, not optional. This service's whole job is outbound HTTPS to a
# customer's file store, which inside a company is routinely behind that
# company's own CA. The client uses the system trust store for exactly that
# reason, and without this package every request fails at the first one.
RUN apt-get update \
 && apt-get install --no-install-recommends -y ca-certificates \
 && rm -rf /var/lib/apt/lists/*

# Unprivileged. It binds a port above 1024 and reads no files at all.
RUN useradd --system --uid 10001 --no-create-home --shell /usr/sbin/nologin opencalc
USER 10001:10001

COPY --from=build /src/target/release/casual-calc-wopi /usr/local/bin/

EXPOSE 8090

# Readiness rather than liveness, as with the collaboration server: a node at
# its session ceiling is serving fine and should be sent no more documents.
HEALTHCHECK --interval=15s --timeout=3s --start-period=5s --retries=3 \
  CMD ["/usr/local/bin/casual-calc-wopi", "--readycheck"]

# PID 1, so SIGTERM arrives directly. It matters here: shutdown releases every
# WOPI lock this node holds, and a lock left behind makes somebody's file
# read-only in their own file store until WOPI's 30-minute expiry.
ENTRYPOINT ["/usr/local/bin/casual-calc-wopi"]
