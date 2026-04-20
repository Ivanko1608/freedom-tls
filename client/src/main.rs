use std::{
    net::{SocketAddr, ToSocketAddrs},
    path::PathBuf,
    sync::Arc,
};

use anyhow::{Context, Result, anyhow};
use clap::{Args, Parser};
use fast_socks5::{Socks5Command, server::Socks5ServerProtocol};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::rustls::{
    ClientConfig, RootCertStore,
    pki_types::{CertificateDer, pem::PemObject},
};

use crate::server::Server;

mod server;

#[derive(Parser)]
struct CliArgs {
    /// Socks5 proxy port to send traffic to.
    #[arg(short, long)]
    port: u16,

    #[command(flatten)]
    server_addr: ServerAddressArg,

    #[arg(long)]
    server_cert: Option<PathBuf>,

    #[arg(long, conflicts_with = "ip_port")]
    server_port: Option<u16>,
}

#[derive(Args)]
#[group(required = true, multiple = false)]
struct ServerAddressArg {
    #[arg(short, long)]
    ip_port: Option<SocketAddr>,

    #[arg(short, long)]
    domain: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = CliArgs::parse();

    let listener = TcpListener::bind(format!("127.0.0.1:{}", args.port)).await?;

    println!("Started socks5 server at: {}", listener.local_addr()?);

    let mut root_cert_store = RootCertStore::empty();
    root_cert_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    if let Some(server_cert) = args.server_cert {
        root_cert_store.add(CertificateDer::from_pem_file(server_cert)?)?
    };

    let config = ClientConfig::builder()
        .with_root_certificates(root_cert_store)
        .with_no_client_auth();

    let server_addr = match (args.server_addr.ip_port, args.server_addr.domain) {
        (Some(ip), _) => ip,
        (None, Some(mut domain)) => {
            if !domain.contains(':') {
                domain.insert(domain.len(), ':');
                domain.insert_str(
                    domain.len(),
                    &(args.server_port.unwrap_or(443u16)).to_string(),
                );
            }
            domain
                .to_socket_addrs()
                .with_context(|| format!("failed to resolve server domain: {domain}"))?
                .next()
                .ok_or_else(|| anyhow!("sever domain: {domain} resolved to no addresses"))?
        }
        // NOTE: We should do some more data collection and logging in case this case ever happens.
        _ => panic!(
            "neither domain or ip from server_addr were Some which is dissallowed by validation and should never happen"
        ),
    };

    dbg!(server_addr);

    let server = Arc::new(Server::new(server_addr, config));

    loop {
        match listener.accept().await {
            Ok((sock, client_addr)) => {
                let server = server.clone();
                tokio::spawn(async move {
                    let stream = match handle_socks5(sock, client_addr).await {
                        Ok(s) => s,
                        Err(e) => {
                            eprintln!("failed to handle socks5 connection: {e}");
                            return;
                        }
                    };

                    server.send(stream).await;
                });
            }
            Err(e) => {
                todo!();
            }
        }
    }
}

async fn handle_socks5(
    socket: tokio::net::TcpStream,
    client_addr: SocketAddr,
) -> Result<TcpStream> {
    let socks_conn = Socks5ServerProtocol::accept_no_auth(socket).await?;

    let (stream, cmd, target_addr) = socks_conn.read_command().await?;

    match cmd {
        Socks5Command::TCPConnect => {
            eprintln!("TCPConnect");
            // target_addr is an enum of either Ip(SocketAddr) or Domain (String, u16)
            dbg!(target_addr);

            //If the reply code indicates a success, and the
            //request was either a BIND or a CONNECT, the client may now start
            //passing data.
            let stream = stream.reply_success(client_addr).await?;

            Ok(stream)
        }
        // BIND
        // The BIND request is used in protocols which require the client to
        // accept connections from the server.  FTP is a well-known example,
        // which uses the primary client-to-server connection for commands and
        // status reports, but may use a server-to-client connection for
        // transferring data on demand (e.g. LS, GET, PUT).
        //
        // It is expected that the client side of an application protocol will
        // use the BIND request only to establish secondary connections after a
        // primary connection is established using CONNECT.  In is expected that
        // a SOCKS server will use DST.ADDR and DST.PORT in evaluating the BIND
        // request.
        //
        // Two replies are sent from the SOCKS server to the client during a
        // BIND operation.  The first is sent after the server creates and binds
        // a new socket.  The BND.PORT field contains the port number that the
        // SOCKS server assigned to listen for an incoming connection.  The
        // BND.ADDR field contains the associated IP address.  The client will
        // typically use these pieces of information to notify (via the primary
        // or control connection) the application server of the rendezvous
        // address.  The second reply occurs only after the anticipated incoming
        // connection succeeds or fails.
        //
        Socks5Command::TCPBind => {
            eprintln!("TCPBind");
            todo!()
        }
        Socks5Command::UDPAssociate => unimplemented!(),
    }
}
