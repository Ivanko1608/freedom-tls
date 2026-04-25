use anyhow::{Context, Result};
use clap::Parser;
use ftls_lib::proto::dest_header::DestinationHeader;
use protobuf::Message;
use tokio_rustls::{
    rustls::{
        RootCertStore,
        pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
    },
    server::TlsStream,
};

use std::{path::PathBuf, sync::Arc};

use tokio::{
    io::{AsyncReadExt, copy, split},
    net::{TcpListener, TcpStream},
    select,
};
use tokio_rustls::{TlsAcceptor, rustls::ServerConfig};

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

        let client_stream = tls_acceptor
            .accept(stream)
            .await
            .expect("failed to accept tls connection");

        println!("Got connection from {peer_addr}");

        tokio::spawn(async move {
            let _res = handle_connection(client_stream)
                .await
                .inspect_err(|e| eprintln!("failed to handle client connection err: {e}"));
        });
    }
}

async fn handle_connection(mut client_stream: TlsStream<TcpStream>) -> Result<()> {
    let sz_dst_header = client_stream.read_u64().await?;

    let mut buf = vec![0u8; sz_dst_header as usize];
    client_stream.read_exact(&mut buf).await?;

    let dst_header =
        DestinationHeader::parse_from_bytes(&buf).context("parse DestinationHeader from stream")?;

    println!("{dst_header:?}");
    assert!(!dst_header.addr.is_empty());

    let (mut client_rx, mut client_tx) = split(client_stream);

    let upstream = TcpStream::connect(dst_header.addr).await?;

    let (mut upstream_rx, mut upstream_tx) = split(upstream);

    loop {
        select! {
            r = copy(&mut client_rx, &mut upstream_tx) => {
                r?;
            }
            r = copy(&mut upstream_rx, &mut client_tx) => {
                    r?;
            }
        }
    }
}
