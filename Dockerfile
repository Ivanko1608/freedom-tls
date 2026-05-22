# Multistage build. Builder uses rust:bookworm so the produced binaries link
# against the same glibc as the runtime image (debian:bookworm-slim).
# Avoids GLIBC_2.38+ mismatches when the host (e.g. Arch) ships newer libc.

FROM rust:1-bookworm AS builder
WORKDIR /src

# Workspace manifests first for better layer caching of dependency builds.
COPY Cargo.toml Cargo.lock ./
COPY ftls-lib/Cargo.toml ftls-lib/Cargo.toml
COPY client/Cargo.toml   client/Cargo.toml
COPY server/Cargo.toml   server/Cargo.toml

# Prime dependency cache with empty crate roots.
RUN mkdir -p ftls-lib/src client/src server/src \
 && echo 'fn main() {}' > client/src/main.rs \
 && echo 'fn main() {}' > server/src/main.rs \
 && touch ftls-lib/src/lib.rs \
 && cargo build --release --bin server --bin ftls-client \
 && rm -rf ftls-lib/src client/src server/src

# Now copy real sources and rebuild. Only the bin crates need to recompile.
COPY ftls-lib ftls-lib
COPY client   client
COPY server   server
RUN touch ftls-lib/src/lib.rs client/src/main.rs server/src/main.rs \
 && cargo build --release --bin server --bin ftls-client

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        iproute2 iptables ca-certificates curl \
 && rm -rf /var/lib/apt/lists/*

COPY --from=builder /src/target/release/server      /usr/local/bin/ftls-server
COPY --from=builder /src/target/release/ftls-client /usr/local/bin/ftls-client

# Init scripts are the same ones used on bare-metal hosts; compose invokes
# them via the per-service `command:` so the image carries no role logic.
COPY tests/vpn-net.sh                               /usr/local/sbin/vpn-net.sh
COPY tests/vpn-client-net.sh                        /usr/local/sbin/vpn-client-net.sh
RUN chmod +x /usr/local/bin/ftls-server \
             /usr/local/bin/ftls-client \
             /usr/local/sbin/vpn-net.sh \
             /usr/local/sbin/vpn-client-net.sh
