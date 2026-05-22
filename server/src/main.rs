use anyhow::{Context, Result};
use clap::Parser;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use tracing::error;
use tracing_forest::ForestLayer;
use tracing_subscriber::{EnvFilter, Registry, layer::SubscriberExt, util::SubscriberInitExt};

use std::{path::PathBuf, sync::Arc};

use tokio::net::TcpListener;
use tokio_rustls::{TlsAcceptor, rustls::ServerConfig};

use crate::client::Client;

mod client;
mod util;

#[derive(Parser)]
struct CliArgs {
    /// .pem encoded public certificate.
    #[arg(short, long)]
    cert: PathBuf,

    #[arg(short, long)]
    key: PathBuf,

    //TODO: make option<> use default if none
    /// Address for server to bind to. Format: ip:port
    #[arg(short, long)]
    server_addr: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    Registry::default()
        .with(EnvFilter::from_default_env())
        .with(ForestLayer::default())
        .init();

    let args = CliArgs::parse();

    let cert: Vec<_> = CertificateDer::pem_file_iter(args.cert)?
        .map(|f| f.expect("invalid certificate in certs file"))
        .collect();

    let key = PrivateKeyDer::from_pem_file(args.key)?;

    start_server(&args.server_addr, cert, key)
        .await
        .context("Failed to start server")?;

    Ok(())
}

async fn start_server(
    server_addr: &str,
    certs: Vec<CertificateDer<'static>>,
    private_cert: PrivateKeyDer<'static>,
) -> Result<()> {
    let cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, private_cert)?;

    let tls_acceptor = TlsAcceptor::from(Arc::new(cfg));

    let tcp_listener = TcpListener::bind(server_addr)
        .await
        .with_context(|| format!("Failed to bind to {}", server_addr))?;

    println!("Started server on {server_addr}");

    loop {
        let (stream, peer_addr) = tcp_listener.accept().await?;

        let client_stream = match tls_acceptor.accept(stream).await {
            Ok(s) => s,
            Err(e) => {
                error!(error = ?e, "failed to accept TLS connection. Conn dropped.");
                continue;
            }
        };

        println!("Got connection from {peer_addr}");

        tokio::spawn(async move {
            let client = Client::try_from_handshake(client_stream)
                .await
                .inspect_err(|e| eprintln!("failed to create client from handshake {e}"))?;

            client
                .handle()
                .await
                .inspect_err(|e| eprintln!("failed to handle client: {e}"))?;

            Ok::<(), anyhow::Error>(())
        });
    }
}
