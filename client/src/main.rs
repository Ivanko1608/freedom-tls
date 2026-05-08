use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use clap::Parser;
use tokio::net::TcpListener;
use tokio_rustls::rustls::{
    ClientConfig, RootCertStore,
    pki_types::{CertificateDer, pem::PemObject},
};

use crate::{
    server::Server,
    socks5::{handle_socks5, target_addr_into_dest},
};

mod server;
mod socks5;

#[derive(Parser)]
struct CliArgs {
    /// Socks5 proxy port to send traffic to.
    #[arg(short, long)]
    port: u16,

    /// .pem encoded public certificate.
    #[arg(long)]
    server_cert: Option<PathBuf>,

    #[arg(short, long, name = "d")]
    server_domain: String,

    #[arg(long)]
    server_port: Option<u16>,
    // TODO: Impl server ip override
    // #[arg(long)]
    // server_ip: Option<SocketAddr>,
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

    let server = Arc::new(Server::new(
        args.server_domain,
        args.server_port.unwrap_or(443u16),
        config,
    )?);

    loop {
        match listener.accept().await {
            Ok((sock, client_addr)) => {
                let server = server.clone();

                tokio::spawn(async move {
                    let (dst_header, stream) = match handle_socks5(sock, client_addr).await {
                        Ok((ta, s)) => (target_addr_into_dest(ta), s),
                        Err(e) => {
                            eprintln!("failed to handle socks5 connection: {e}");
                            return Err(e);
                        }
                    };

                    server
                        .send(dst_header, stream)
                        .await
                        .inspect_err(|e| eprintln!("failed to communicate with server: {e}"))?;

                    Ok(())
                });
            }
            Err(e) => {
                eprintln!("failed to accept incoming connection: {e}");
            }
        }
    }
}
