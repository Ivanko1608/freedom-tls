#!/usr/bin/env bash
#
# vpn-client-net.sh — set up or tear down client-side routing so that
#                     all traffic goes through the VPN tunnel.
#
# The VPN client's Rust process owns the TUN device — it creates,
# configures, and tears it down. This script handles the routing changes
# needed to send all the OS's outbound traffic into that TUN, except for
# the encrypted tunnel traffic itself.
#
# Usage:
#   sudo ./vpn-client-net.sh up
#   sudo ./vpn-client-net.sh down
#   sudo ./vpn-client-net.sh status
#
# Before "up", the VPN client process must already be running so that
# the TUN device exists. (Routes can't reference a device that doesn't
# exist.)
#
# Configuration: set SERVER_IP to your VPN server's *public* IP, either
# in the script or as an environment variable.
#
# Safe to re-run.

set -euo pipefail

# ----------------------------------------------------------------------
# CONFIG
# ----------------------------------------------------------------------

# Public IP of the VPN server. Required. Must be the IP your Rust client
# actually connects to over TLS.
SERVER_IP="${SERVER_IP:-}"

# Name of the TUN device the VPN client creates. Must match the Rust
# client's configuration.
TUN_IFACE="${TUN_IFACE:-tun0}"

# Seconds to wait for TUN_IFACE to appear before failing. 0 means fail
# immediately if absent (legacy behavior — host workflow expects the
# user to start the Rust client before invoking this script). Set to a
# positive value when the script is launched alongside the Rust client
# (e.g. inside a container) so the route install can race the TUN setup.
TUN_WAIT_SECS="${TUN_WAIT_SECS:-0}"

# If true, also reconfigure /etc/resolv.conf to use a public DNS server
# while the VPN is up. Backs up the original and restores it on "down".
# Default off — leave DNS as-is and accept that DNS may leak.
FORCE_DNS="${FORCE_DNS:-0}"

# DNS server to use when FORCE_DNS=1.
DNS_SERVER="${DNS_SERVER:-1.1.1.1}"

# Path where we stash the saved gateway so down knows what to undo.
STATE_DIR="${STATE_DIR:-/var/run/vpn-client-net}"
GATEWAY_FILE="$STATE_DIR/gateway"
GATEWAY_DEV_FILE="$STATE_DIR/gateway_dev"
RESOLV_BACKUP="$STATE_DIR/resolv.conf.backup"

# ----------------------------------------------------------------------
# Helpers
# ----------------------------------------------------------------------

log()  { printf '[vpn-client-net] %s\n' "$*" >&2; }
fail() { printf '[vpn-client-net] error: %s\n' "$*" >&2; exit 1; }

require_root() {
    if [[ $EUID -ne 0 ]]; then
        fail "must be run as root (try: sudo $0 $*)"
    fi
}

require_cmd() {
    command -v "$1" >/dev/null 2>&1 || fail "required command not found: $1"
}

ensure_state_dir() {
    mkdir -p "$STATE_DIR"
}

detect_default_gateway() {
    # Returns: gateway_ip|interface_name
    ip route show default 2>/dev/null \
        | awk '/^default/ {printf "%s|%s\n", $3, $5; exit}'
}

# ----------------------------------------------------------------------
# up
# ----------------------------------------------------------------------

cmd_up() {
    require_root "$@"
    require_cmd ip

    [[ -n "$SERVER_IP" ]] || fail "SERVER_IP is not set. Edit the script or run: sudo SERVER_IP=1.2.3.4 $0 up"

    if ! ip link show "$TUN_IFACE" >/dev/null 2>&1; then
        if (( TUN_WAIT_SECS > 0 )); then
            log "waiting up to ${TUN_WAIT_SECS}s for $TUN_IFACE to appear"
            local deadline=$(( $(date +%s) + TUN_WAIT_SECS ))
            until ip link show "$TUN_IFACE" >/dev/null 2>&1; do
                if (( $(date +%s) > deadline )); then
                    fail "$TUN_IFACE never appeared within ${TUN_WAIT_SECS}s"
                fi
                sleep 0.1
            done
        else
            fail "$TUN_IFACE does not exist. Start the VPN client process first."
        fi
    fi

    local default_info
    default_info=$(detect_default_gateway)
    [[ -n "$default_info" ]] || fail "no default route found — are you connected to a network?"

    local gateway_ip="${default_info%|*}"
    local gateway_dev="${default_info#*|}"

    log "current default: via $gateway_ip dev $gateway_dev"
    log "VPN server: $SERVER_IP"
    log "tunnel device: $TUN_IFACE"

    ensure_state_dir
    echo "$gateway_ip" > "$GATEWAY_FILE"
    echo "$gateway_dev" > "$GATEWAY_DEV_FILE"

    log "pinning route to VPN server via real gateway"
    # Remove any prior pin first, then add fresh.
    ip route del "$SERVER_IP" 2>/dev/null || true
    ip route add "$SERVER_IP" via "$gateway_ip" dev "$gateway_dev"

    log "installing /1 routes covering all of IPv4 via $TUN_IFACE"
    ip route del 0.0.0.0/1 2>/dev/null || true
    ip route del 128.0.0.0/1 2>/dev/null || true
    ip route add 0.0.0.0/1 dev "$TUN_IFACE"
    ip route add 128.0.0.0/1 dev "$TUN_IFACE"

    if [[ "$FORCE_DNS" == "1" ]]; then
        log "reconfiguring /etc/resolv.conf to use $DNS_SERVER"
        if [[ -f /etc/resolv.conf && ! -f "$RESOLV_BACKUP" ]]; then
            cp /etc/resolv.conf "$RESOLV_BACKUP"
        fi
        # If resolv.conf is a symlink (systemd-resolved managed), replace
        # it with a regular file. We'll restore the original on down.
        if [[ -L /etc/resolv.conf ]]; then
            rm /etc/resolv.conf
        fi
        printf 'nameserver %s\n' "$DNS_SERVER" > /etc/resolv.conf
    else
        log "leaving DNS configuration alone (set FORCE_DNS=1 to override)"
    fi

    log "done. all IPv4 traffic should now route through $TUN_IFACE,"
    log "      except connections to $SERVER_IP."
}

# ----------------------------------------------------------------------
# down
# ----------------------------------------------------------------------

cmd_down() {
    require_root "$@"
    require_cmd ip

    log "removing /1 routes"
    ip route del 0.0.0.0/1 2>/dev/null || true
    ip route del 128.0.0.0/1 2>/dev/null || true

    if [[ -n "$SERVER_IP" ]]; then
        log "removing pinned route to $SERVER_IP"
        ip route del "$SERVER_IP" 2>/dev/null || true
    fi

    if [[ -f "$RESOLV_BACKUP" ]]; then
        log "restoring /etc/resolv.conf"
        # If the original was a symlink, the backup is the resolved file;
        # the safest restore is just to copy it back as a regular file.
        # systemd-resolved will re-create its symlink on its own next time
        # it's restarted, if needed.
        if [[ -L /etc/resolv.conf ]]; then
            rm /etc/resolv.conf
        fi
        cp "$RESOLV_BACKUP" /etc/resolv.conf
        rm "$RESOLV_BACKUP"
    fi

    # Clean up state files.
    rm -f "$GATEWAY_FILE" "$GATEWAY_DEV_FILE"
    rmdir "$STATE_DIR" 2>/dev/null || true

    log "done. normal routing restored."
}

# ----------------------------------------------------------------------
# status
# ----------------------------------------------------------------------

cmd_status() {
    require_cmd ip

    echo "VPN client routing status"
    echo "-------------------------"

    printf 'TUN device         : '
    if ip link show "$TUN_IFACE" >/dev/null 2>&1; then
        echo "$TUN_IFACE present"
    else
        echo "$TUN_IFACE absent (VPN client process not running)"
    fi

    echo
    echo "Default route:"
    ip route show default || echo "  (none)"

    echo
    echo "Routes through $TUN_IFACE:"
    ip route show dev "$TUN_IFACE" 2>/dev/null || echo "  (none)"

    if [[ -n "$SERVER_IP" ]]; then
        echo
        echo "Route to VPN server ($SERVER_IP):"
        ip route get "$SERVER_IP" 2>/dev/null || echo "  (cannot resolve)"
    fi

    echo
    echo "DNS configuration (/etc/resolv.conf):"
    if [[ -L /etc/resolv.conf ]]; then
        printf '  (symlink to %s)\n' "$(readlink /etc/resolv.conf)"
    fi
    grep -E '^nameserver' /etc/resolv.conf 2>/dev/null | sed 's/^/  /' \
        || echo "  (no nameserver lines found)"

    if [[ -f "$RESOLV_BACKUP" ]]; then
        echo
        echo "  (a backup exists at $RESOLV_BACKUP — VPN appears to have"
        echo "   modified DNS; run 'down' to restore)"
    fi
}

# ----------------------------------------------------------------------
# usage / dispatch
# ----------------------------------------------------------------------

usage() {
    cat <<EOF
Usage: $0 {up|down|status}

This script configures client-side routing so all traffic goes through
the VPN. The TUN device itself is owned by the VPN client process.

Commands:
  up      Route all traffic into the TUN, except the VPN server's IP.
  down    Restore normal routing.
  status  Show current routing state.

Configuration:
  SERVER_IP       Public IP of the VPN server     (required)
  TUN_IFACE       TUN device name                 (default: tun0)
  TUN_WAIT_SECS   Wait this many seconds for the TUN device to
                  appear (default: 0 = fail fast). Use a positive
                  value when this script runs alongside the Rust
                  client (e.g. in a container).
  FORCE_DNS       Set to 1 to override DNS too    (default: 0)
  DNS_SERVER      DNS server used if FORCE_DNS=1  (default: 1.1.1.1)

Typical workflow:
  ./vpn-client                                # start the Rust process first
  sudo SERVER_IP=203.0.113.5 $0 up            # then route everything in
  ...
  sudo $0 down                                # restore normal routing
  Ctrl-C the VPN client                       # TUN disappears with it

Notes:
  - Run this AFTER starting the VPN client, since routes need the TUN
    device to exist.
  - "down" only needs SERVER_IP if you want it to clean up the pinned
    route to the server. Without it, that pinned route is left in place;
    harmless but slightly untidy.
EOF
}

case "${1:-}" in
    up)     shift; cmd_up     "$@" ;;
    down)   shift; cmd_down   "$@" ;;
    status) shift; cmd_status "$@" ;;
    -h|--help|help|"") usage ;;
    *)      usage; exit 2 ;;
esac
