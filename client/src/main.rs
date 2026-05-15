use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use clap::Parser;
use ftls_lib::message::Message;
use tokio::{net::TcpStream, sync::mpsc};
use tokio_rustls::rustls::{
    ClientConfig, RootCertStore,
    pki_types::{CertificateDer, pem::PemObject},
};

use crate::server::Server;

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

    let (ch_send, mut ch_recv) = mpsc::unbounded_channel::<(Message, TcpStream)>();

    tokio::spawn(socks5::server_start(
        ch_send,
        "127.0.0.1".to_string(),
        args.port,
    ));

    loop {
        match ch_recv.recv().await {
            Some((message, stream)) => {
                let server = server.clone();

                tokio::spawn(async move {
                    server
                        .send(message, stream)
                        .await
                        .inspect_err(|e| eprintln!("failed to send to server: {e}"))
                        .unwrap();
                });
            }
            None => {
                todo!()
            }
        }
    }
}
