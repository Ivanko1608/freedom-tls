#!/usr/bin/env bash
# Generate a self-signed cert for the in-container server.
# SAN must cover the hostname the client uses (`vpn-server`) plus its
# vpn-net IP, so rustls accepts the handshake regardless of how the
# client is configured.
set -euo pipefail

CERTS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/certs"
mkdir -p "$CERTS_DIR"

if [[ -f "$CERTS_DIR/server.pem" && -f "$CERTS_DIR/server-key.pem" ]]; then
  exit 0
fi

openssl req -x509 -newkey ed25519 \
  -keyout "$CERTS_DIR/server-key.pem" \
  -out    "$CERTS_DIR/server.pem" \
  -days 30 -nodes \
  -subj "/CN=vpn-server" \
  -addext "subjectAltName=DNS:vpn-server,DNS:localhost,IP:10.10.0.10" \
  -addext "basicConstraints=critical,CA:FALSE" \
  -addext "keyUsage=critical,digitalSignature" \
  -addext "extendedKeyUsage=serverAuth"

chmod 644 "$CERTS_DIR/server-key.pem"
