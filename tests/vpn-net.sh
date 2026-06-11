#!/usr/bin/env bash
#
# vpn-net.sh — set up or tear down the server-side networking
#              required by the VPN.
#
# The VPN server's Rust process owns the TUN device — it creates, configures,
# and tears it down on its own lifecycle. This script handles the *other*
# kernel configuration the VPN needs, which the Rust process does not touch:
#
#   - IPv4 forwarding (sysctl)
#   - NAT MASQUERADE rule (iptables nat table)
#   - FORWARD chain rules permitting traffic between the TUN and the
#     external interface (iptables filter table)
#
# Usage:
#   sudo ./vpn-net.sh up
#   sudo ./vpn-net.sh down
#   sudo ./vpn-net.sh status
#
# Run "up" once before (or after) starting the VPN server. The rules
# reference the TUN device by name; they don't require the device to exist
# at the moment they're added. iptables will match packets if/when the
# device shows up.
#
# Safe to re-run. "up" will not duplicate rules; "down" will not error if
# nothing is set up.

set -euo pipefail

# ----------------------------------------------------------------------
# CONFIG
# ----------------------------------------------------------------------

TUN_IFACE="${TUN_IFACE:-tun0}"
# Must match the /24 the Rust server/client assign to their TUN devices
# (currently hard-coded to 10.8.0.1 / 10.8.0.2 — see server/src/client.rs
# and client/src/tun.rs). MASQUERADE matches by source, so a mismatch
# silently drops every VPN packet at the cloud provider's egress filter.
VPN_SUBNET="${VPN_SUBNET:-10.8.0.0/24}"
EXT_IFACE="${EXT_IFACE:-}"
# If EXT_IFACE is unset, pick the iface whose IPv4 address starts with this
# prefix (e.g. "10.20."). Used in environments where the default route does
# not point at the desired uplink — typically multi-NIC containers.
EXT_SUBNET="${EXT_SUBNET:-}"
RULE_TAG="vpn-net.sh"

# ----------------------------------------------------------------------
# Helpers
# ----------------------------------------------------------------------

log() { printf '[vpn-net] %s\n' "$*" >&2; }
fail() {
  printf '[vpn-net] error: %s\n' "$*" >&2
  exit 1
}

require_root() {
  if [[ $EUID -ne 0 ]]; then
    fail "must be run as root (try: sudo $0 $*)"
  fi
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || fail "required command not found: $1"
}

detect_ext_iface() {
  local iface
  if [[ -n "$EXT_SUBNET" ]]; then
    iface=$(ip -o -4 addr show | awk -v s="$EXT_SUBNET" '$4 ~ s {print $2; exit}')
    [[ -n "$iface" ]] ||
      fail "no iface found in EXT_SUBNET=$EXT_SUBNET"
    printf '%s' "$iface"
    return
  fi
  iface=$(ip route show default 2>/dev/null | awk '/^default/ {print $5; exit}')
  [[ -n "$iface" ]] || fail "could not auto-detect external interface — set EXT_IFACE or EXT_SUBNET"
  printf '%s' "$iface"
}

# ----------------------------------------------------------------------
# up
# ----------------------------------------------------------------------

cmd_up() {
  require_root "$@"
  require_cmd iptables

  if [[ -z "$EXT_IFACE" ]]; then
    EXT_IFACE=$(detect_ext_iface)
    log "auto-detected external interface: $EXT_IFACE"
  fi

  # Skip the write if the caller (e.g. the container runtime via
  # compose `sysctls:`) already enabled it. In a netns with the key
  # pre-set, /proc/sys/net/ipv4/ip_forward is often read-only and
  # `sysctl -w` would EPERM.
  if [[ "$(cat /proc/sys/net/ipv4/ip_forward 2>/dev/null || echo 0)" == "1" ]]; then
    log "IPv4 forwarding already enabled, skipping"
  else
    require_cmd sysctl
    log "enabling IPv4 forwarding"
    sysctl -w net.ipv4.ip_forward=1 >/dev/null
  fi

  if ! ip link show "$TUN_IFACE" >/dev/null 2>&1; then
    log "note: $TUN_IFACE does not exist yet — rules will activate"
    log "      when the VPN server starts and creates it."
  fi

  # Source-only MASQUERADE (no `-o` scoping). Lets a multi-NIC server
  # NAT VPN traffic out whichever iface its routing table picks for the
  # destination — e.g. an isolated "remote LAN" bridge for one route,
  # a real uplink bridge for the rest. VPN traffic never re-enters the
  # client-facing iface (no route back), so the broad rule is safe.
  log "installing NAT rule (MASQUERADE for $VPN_SUBNET, any out-iface)"
  if ! iptables -t nat -C POSTROUTING \
    -s "$VPN_SUBNET" -j MASQUERADE \
    -m comment --comment "$RULE_TAG" 2>/dev/null; then
    iptables -t nat -A POSTROUTING \
      -s "$VPN_SUBNET" -j MASQUERADE \
      -m comment --comment "$RULE_TAG"
  fi

  log "installing FORWARD rule (outbound: $TUN_IFACE -> $EXT_IFACE)"
  if ! iptables -C FORWARD \
    -i "$TUN_IFACE" -o "$EXT_IFACE" -j ACCEPT \
    -m comment --comment "$RULE_TAG" 2>/dev/null; then
    iptables -A FORWARD \
      -i "$TUN_IFACE" -o "$EXT_IFACE" -j ACCEPT \
      -m comment --comment "$RULE_TAG"
  fi

  log "installing FORWARD rule (inbound replies: $EXT_IFACE -> $TUN_IFACE)"
  if ! iptables -C FORWARD \
    -i "$EXT_IFACE" -o "$TUN_IFACE" \
    -m state --state RELATED,ESTABLISHED -j ACCEPT \
    -m comment --comment "$RULE_TAG" 2>/dev/null; then
    iptables -A FORWARD \
      -i "$EXT_IFACE" -o "$TUN_IFACE" \
      -m state --state RELATED,ESTABLISHED -j ACCEPT \
      -m comment --comment "$RULE_TAG"
  fi

  log "done. kernel is ready to forward VPN traffic."
  log "      start the VPN server next (it manages $TUN_IFACE itself)."
}

# ----------------------------------------------------------------------
# down
# ----------------------------------------------------------------------

cmd_down() {
  require_root "$@"
  require_cmd iptables

  if [[ -z "$EXT_IFACE" ]]; then
    EXT_IFACE=$(ip route show default 2>/dev/null |
      awk '/^default/ {print $5; exit}' || true)
    [[ -n "$EXT_IFACE" ]] && log "auto-detected external interface: $EXT_IFACE"
  fi

  log "removing NAT rule"
  iptables -t nat -D POSTROUTING \
    -s "$VPN_SUBNET" -j MASQUERADE \
    -m comment --comment "$RULE_TAG" 2>/dev/null || true

  log "removing FORWARD rules"
  iptables -D FORWARD \
    -i "$TUN_IFACE" -o "${EXT_IFACE:-eth0}" -j ACCEPT \
    -m comment --comment "$RULE_TAG" 2>/dev/null || true

  iptables -D FORWARD \
    -i "${EXT_IFACE:-eth0}" -o "$TUN_IFACE" \
    -m state --state RELATED,ESTABLISHED -j ACCEPT \
    -m comment --comment "$RULE_TAG" 2>/dev/null || true

  log "done. the TUN device (if any) is the VPN server's responsibility"
  log "      — it will disappear when the server process exits."
  log "      ip_forward left enabled (intentional — harmless and"
  log "      possibly needed by other services). disable manually with:"
  log "        sudo sysctl -w net.ipv4.ip_forward=0"
}

# ----------------------------------------------------------------------
# status
# ----------------------------------------------------------------------

cmd_status() {
  require_cmd iptables

  if [[ -z "$EXT_IFACE" ]]; then
    EXT_IFACE=$(ip route show default 2>/dev/null |
      awk '/^default/ {print $5; exit}' || echo "?")
  fi

  echo "VPN server networking status"
  echo "----------------------------"
  echo "External interface : $EXT_IFACE"
  echo "TUN interface      : $TUN_IFACE (owned by VPN server process)"
  echo "VPN subnet         : $VPN_SUBNET"
  echo

  printf 'ip_forward         : '
  cat /proc/sys/net/ipv4/ip_forward 2>/dev/null |
    awk '{print ($1 == "1" ? "enabled" : "disabled")}'

  printf 'TUN device         : '
  if ip link show "$TUN_IFACE" >/dev/null 2>&1; then
    echo "present (VPN server is running)"
  else
    echo "not present (VPN server is not running, or hasn't created it yet)"
  fi

  echo
  echo "iptables rules tagged '$RULE_TAG':"
  iptables-save 2>/dev/null | grep -- "$RULE_TAG" || echo "  (none)"
}

# ----------------------------------------------------------------------
# usage / dispatch
# ----------------------------------------------------------------------

usage() {
  cat <<EOF
Usage: $0 {up|down|status}

This script configures kernel-level networking (forwarding, NAT, FORWARD
rules) required by the VPN server. The TUN device itself is created and
managed by the VPN server process — not by this script.

Commands:
  up      Enable ip_forward and install iptables rules.
  down    Remove the iptables rules. Leaves ip_forward alone.
  status  Print current configuration.

Configuration (set via environment or edit the script):
  TUN_IFACE    TUN device name           (default: tun0)
  VPN_SUBNET   VPN subnet in CIDR        (default: 10.8.0.0/24)
  EXT_IFACE    External (uplink) iface   (default: auto-detected)
  EXT_SUBNET   IPv4 prefix to pick EXT_IFACE by (e.g. "10.20.").
               Used when the default route does not point at the
               desired uplink — e.g. multi-NIC containers.

Typical workflow:
  sudo $0 up              # one-time, before starting the VPN server
  ./vpn-server            # your Rust process, which owns the TUN device
  sudo $0 down            # when you're done, to clean up the iptables rules
EOF
}

case "${1:-}" in
up)
  shift
  cmd_up "$@"
  ;;
down)
  shift
  cmd_down "$@"
  ;;
status)
  shift
  cmd_status "$@"
  ;;
-h | --help | help | "") usage ;;
*)
  usage
  exit 2
  ;;
esac
