# freedom-tls

A Rust VPN-ish thing built for the **Compiles Eventually** devlog series.

It started as a TLS-encrypted SOCKS5 tunnel. Then I tried protobuf, threw that away, rebuilt the protocol with `postcard`, and eventually added a TUN interface so the thing could move raw IP packets instead of just proxying application traffic.

So now it is much closer to an actual VPN.

Not a polished VPN.
Not a secure product.
Not something you should install on your router and trust with your life.

It is a learning project that happens to move packets.

## What it does

At the end of the series, the project can:

* run a TLS server
* run a client that connects to it
* expose a local SOCKS5 proxy
* create a TUN interface
* frame raw IP packets
* send those packets over a single TLS connection
* write packets back into a TUN interface on the server
* rely on the kernel for routing/NAT instead of manually tracking every TCP connection

The important realization was:

> The VPN does not need to understand every connection.
> It just needs to move packets.
> The packets already know where they are going.

Naturally, getting to that point involved routing tables, NAT, iptables, root permissions, containers, packet framing, and several bad decisions.

## Repository layout

```text
.
├── client/       # ftls-client binary
├── server/       # ftls-server binary
├── ftls-lib/     # shared protocol/message code
├── tests/        # helper scripts for routing/NAT setup
├── Dockerfile
├── Cargo.toml
└── reference.md
```

## Workspace crates

### `ftls-client`

The client binary.

It currently starts:

* a local SOCKS5 server on `127.0.0.1:<port>`
* a TUN-based packet transport

The TUN side creates a local `tun0` device and sends packets to the server over TLS.

Current hardcoded client-side TUN config:

```text
client TUN IP: 10.8.0.2
netmask:       255.255.255.0
MTU:           1400
```

### `ftls-server`

The server binary.

It listens for TLS connections, accepts a small handshake, then dispatches based on transport type:

* `SOCKS` for the older SOCKS5 tunnel mode
* `IP` for the TUN packet transport

For the TUN mode, the server creates its own TUN device and writes packets into it.

Current hardcoded server-side TUN config:

```text
server TUN IP: 10.8.0.1
netmask:       255.255.255.0
MTU:           1400
```

### `ftls-lib`

Shared protocol code.

This currently contains the message/flavor pieces used by the client and server. The custom flavor adds a small `FTLS` magic header and length prefix around serialized postcard messages.

Because apparently even “send a message” needed a small archaeological layer.

## Requirements

For building:

* Rust, current stable/nightly enough for edition 2024
* Linux for the TUN/VPN path

For running the packet-level VPN path:

* root privileges or equivalent capabilities
* `/dev/net/tun`
* `ip`
* `iptables`
* a TLS certificate and key for the server

The SOCKS5 tunnel mode is simpler. The TUN mode is where the operating system starts demanding tribute.

## Build

```bash
cargo build --release
```

Binaries:

```text
target/release/server
target/release/ftls-client
```

The Dockerfile also builds both binaries into a Debian Bookworm runtime image and copies the helper network scripts into the image.

## Running the server

The server needs a certificate, private key, and bind address.

Example:

```bash
sudo ./target/release/server \
  --cert /etc/letsencrypt/live/example.com/fullchain.pem \
  --key /etc/letsencrypt/live/example.com/privkey.pem \
  --server-addr 0.0.0.0:443
```

The server process owns the TUN device. It creates and tears it down itself.

However, the kernel still needs to be allowed to forward and NAT VPN traffic.

That part is handled by:

```bash
sudo tests/vpn-net.sh up
```

Status:

```bash
sudo tests/vpn-net.sh status
```

Teardown:

```bash
sudo tests/vpn-net.sh down
```

The script handles:

* IPv4 forwarding
* NAT MASQUERADE
* FORWARD rules between `tun0` and the external interface

The server code currently also attempts to enable IPv4 forwarding when handling TUN traffic. The helper script is still the intended place to set up the host networking rules, especially NAT and forwarding policy.

It is safe to re-run and should not duplicate rules.

## Running the client

Example:

```bash
sudo ./target/release/ftls-client \
  --port 1080 \
  --server-domain example.com \
  --server-port 443
```

If you are using a custom/self-signed cert:

```bash
sudo ./target/release/ftls-client \
  --port 1080 \
  --server-domain example.com \
  --server-port 443 \
  --server-cert ./server.pem
```

The client starts a local SOCKS5 proxy and also starts the TUN transport.

For SOCKS5 testing, point an application at:

```text
127.0.0.1:1080
```

For full-ish VPN routing, the client-side routing table must be changed so normal traffic goes into `tun0`, while the encrypted TLS connection to the VPN server still goes through the real network.

That part is handled by:

```bash
sudo SERVER_IP=<your-server-public-ip> tests/vpn-client-net.sh up
```

Status:

```bash
sudo SERVER_IP=<your-server-public-ip> tests/vpn-client-net.sh status
```

Teardown:

```bash
sudo SERVER_IP=<your-server-public-ip> tests/vpn-client-net.sh down
```

`SERVER_IP` is required for `up`. For `down`, it is only needed if you want the script to remove the pinned route to the VPN server; without it, the broad VPN routes and DNS changes are still cleaned up.

The client routing script:

* pins the route to the VPN server through the real gateway
* installs `0.0.0.0/1` and `128.0.0.0/1` routes through `tun0`
* optionally rewrites DNS if `FORCE_DNS=1`

Example with DNS override:

```bash
sudo SERVER_IP=<your-server-public-ip> FORCE_DNS=1 DNS_SERVER=1.1.1.1 tests/vpn-client-net.sh up
```

By default DNS is left alone, which means DNS leaks are possible. This is a devlog project, not a privacy product pretending it has a compliance department.

## Testing whether it works

Once the server and client are running and routing is enabled:

```bash
curl ifconfig.me
```

You should see the public IP of the VPN server.

You can also try:

```bash
ping google.com
```

or open a browser and check whether traffic leaves through the server.

If it works, congratulations. Packets went in. Packets came out. Nobody should ask too many follow-up questions.

## Helper scripts

### Server side

```bash
tests/vpn-net.sh
```

Usage:

```bash
sudo tests/vpn-net.sh up
sudo tests/vpn-net.sh status
sudo tests/vpn-net.sh down
```

Important environment variables:

```text
TUN_IFACE     default: tun0
VPN_SUBNET    default: 10.8.0.0/24
EXT_IFACE     optional, auto-detected if unset
EXT_SUBNET    optional helper for multi-NIC/container setups
```

### Client side

```bash
tests/vpn-client-net.sh
```

Usage:

```bash
sudo SERVER_IP=<server-ip> tests/vpn-client-net.sh up
sudo SERVER_IP=<server-ip> tests/vpn-client-net.sh status
sudo SERVER_IP=<server-ip> tests/vpn-client-net.sh down
```

Important environment variables:

```text
SERVER_IP       required for up, optional for down route cleanup
TUN_IFACE       default: tun0
TUN_WAIT_SECS   default: 0
FORCE_DNS       default: 0
DNS_SERVER      default: 1.1.1.1
```

## Current limitations

This is not production-ready.

Known rough edges:

* Linux-focused
* hardcoded TUN addresses
* no config file yet
* no authentication beyond TLS server validation
* no proper daemon lifecycle
* no automatic full cleanup in Rust yet
* IPv4-focused
* DNS handling is optional and crude
* error handling is still very “devlog project”
* NAT/routing lives mostly in shell scripts
* security has not been audited
* the protocol is still experimental

Also, the code was written while recording long-form devlogs, which means some parts exist because I was learning in public and making the machine suffer with me.

## Series context

This repo belongs to the Rust VPN devlog series on **Compiles Eventually**.

Rough arc:

1. Build a TLS-encrypted SOCKS5 tunnel.
2. Try protobuf for the protocol.
3. Throw protobuf away.
4. Rebuild the handshake/message format with postcard.
5. Add TUN.
6. Stop manually multiplexing connections.
7. Move raw IP packets over TLS.
8. Make Linux route them.
9. Suffer.
10. It works.

## Safety note

Do not use this as your actual VPN.

It is a learning project and a video series repo. It is useful for understanding:

* Rust async networking
* TLS with rustls
* SOCKS5 proxying
* message framing
* postcard serialization
* TUN interfaces
* routing/NAT basics
* why networking people look tired

It is not useful as a thing you should trust with sensitive traffic.

## References

See `reference.md` for notes and links that were useful during development.

The main external ideas/tools used during the series include:

* Tokio
* rustls / tokio-rustls
* postcard
* tun
* LengthDelimitedCodec
* iproute2
* iptables

## License

MIT. See `LICENSE`.
