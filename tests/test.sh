#!/usr/bin/env bash
# End-to-end VPN test:
#   1. build the two binaries on the host (fast incremental)
#   2. ensure a cert exists with SAN=vpn-server
#   3. build the runtime image
#   4. bring up server / client / target
#   5. wait for client tun0
#   6. curl target through the tunnel
#   7. assert payload, then teardown
#
# Override the compose tool with COMPOSE=... (default: `podman compose`).
# Override the container runtime with RUNTIME=... (default: `podman`).
# `exec` calls go through RUNTIME directly because the native podman-compose
# implementation does not accept docker-compose's `-T` (no-TTY) flag, while
# `podman exec` works the same on both runtimes.
# On Arch + rootless Podman, NET_ADMIN + /dev/net/tun usually require
# rootful execution: `sudo COMPOSE='podman compose' tests/test.sh`.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
COMPOSE="${COMPOSE:-podman compose}"
RUNTIME="${RUNTIME:-podman}"
COMPOSE_FILE="$SCRIPT_DIR/compose.yml"
# Container names from compose.yml (container_name: fields). Service names
# (vpn-server / vpn-client) are still used for compose-level operations
# like logs/build/up/down; container names are used for `exec`.
SERVER_CT="ftls-vpn-server"
CLIENT_CT="ftls-vpn-client"
KEEP="${KEEP:-0}"   # KEEP=1 leaves containers up for manual inspection
LOG_DIR="$SCRIPT_DIR/logs"

dump_logs() {
  mkdir -p "$LOG_DIR"
  $COMPOSE -f "$COMPOSE_FILE" logs --no-color vpn-server >"$LOG_DIR/server.log" 2>&1 || true
  $COMPOSE -f "$COMPOSE_FILE" logs --no-color vpn-client >"$LOG_DIR/client.log" 2>&1 || true
  echo "logs: $LOG_DIR/{server,client}.log" >&2
}

cleanup() {
  dump_logs
  if [[ "$KEEP" != "1" ]]; then
    $COMPOSE -f "$COMPOSE_FILE" down -v --remove-orphans >/dev/null 2>&1 || true
  else
    echo "KEEP=1, containers left running. Tear down with:" >&2
    echo "  $COMPOSE -f $COMPOSE_FILE down -v --remove-orphans" >&2
  fi
}
trap cleanup EXIT

echo "==> [1/5] cert"
"$SCRIPT_DIR/gen-cert.sh"

mkdir -p "$SCRIPT_DIR/target-www"
printf 'ftls-test-payload\n' > "$SCRIPT_DIR/target-www/probe.txt"

echo "==> [2/5] image build (builds binaries inside container)"
$COMPOSE -f "$COMPOSE_FILE" build

echo "==> [3/5] compose up"
$COMPOSE -f "$COMPOSE_FILE" up -d

echo "==> [4/5] wait for tun0 on client (30s)"
deadline=$(( $(date +%s) + 30 ))
until $RUNTIME exec "$CLIENT_CT" ip link show tun0 >/dev/null 2>&1; do
  # Detect "container exited before tun0 appeared" so we don't burn the
  # full 30s waiting on a dead container.
  if ! $RUNTIME container exists "$CLIENT_CT" \
       || [[ "$($RUNTIME inspect -f '{{.State.Status}}' "$CLIENT_CT" 2>/dev/null)" != "running" ]]; then
    echo "FAIL: $CLIENT_CT is not running (see $LOG_DIR/client.log)" >&2
    exit 1
  fi
  if (( $(date +%s) > deadline )); then
    echo "FAIL: client never created tun0" >&2
    exit 1
  fi
  sleep 0.3
done

echo "==> [5/5] probe target via vpn"
if ! got=$($RUNTIME exec "$CLIENT_CT" \
             curl -fsS --max-time 5 http://10.20.0.50/probe.txt); then
  echo "FAIL: curl through tunnel failed" >&2
  exit 1
fi

expected="ftls-test-payload"
if [[ "$got" != "$expected" ]]; then
  echo "FAIL: payload mismatch" >&2
  echo "  expected: $expected" >&2
  echo "  got:      $got" >&2
  exit 1
fi

echo "OK"
