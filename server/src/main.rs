use anyhow::{Context, Result, ensure};
use clap::Parser;
use ftls_lib::{
    flavor::MAGIC_HEADER,
    message::{DestinationType, Message, MessageType},
};
use hickory_resolver::{Resolver, net::NetError};
use tokio_rustls::{
    rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
    server::TlsStream,
};

use core::panic;
use std::{path::PathBuf, sync::Arc, vec};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, copy, split},
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
    let mut header = vec![0u8; MAGIC_HEADER.len()];
    client_stream.read_exact(&mut header).await?;

    ensure!(header == MAGIC_HEADER);

    let len = client_stream.read_u64_le().await?;

    let mut buf = vec![0u8; len as usize];
    client_stream.read_exact(&mut buf).await?;

    let msg: Message = postcard::from_bytes(&buf).expect("parse DestinationHeader from stream");

    let MessageType::Destination(dtype, addr, port) = msg.message_type else {
        panic!("OOOps")
    };

    println!("{dtype:?}:{addr}:{port}");
    ensure!(!addr.is_empty());

    if dtype == DestinationType::DOMAIN {
        // Use the host OS'es `/etc/resolv.conf`
        let resolver = Resolver::builder_tokio()?.build()?;
        let response = match resolver.lookup_ip(&addr).await {
            Ok(r) => Ok(r),
            Err(NetError::Dns(hickory_resolver::net::DnsError::NoRecordsFound(e))) => {
                let msg = Message::new(MessageType::Error(format!(
                    "no records found for: {}",
                    addr
                )));

                client_stream
                    // The size of the vec we create here,  is the size of message (with service_message) + the size
                    // of max dns name (roughly 253, 255 here for safety)
                    .write_all(&postcard::to_stdvec(&msg)?)
                    .await?;

                client_stream.flush().await?;

                client_stream.shutdown().await?;

                return Err(hickory_resolver::net::DnsError::NoRecordsFound(e).into());
            }
            Err(e) => Err(e),
        }?;

        let Some(first_answer) = response.as_lookup().answers().first() else {
            todo!()
        };
    }

    let (mut client_rx, mut client_tx) = split(&mut client_stream);

    let mut upstream = TcpStream::connect((addr, port)).await?;

    let (mut upstream_rx, mut upstream_tx) = split(&mut upstream);

    loop {
        select! {
            r = copy(&mut client_rx, &mut upstream_tx) => {
                if r.is_err() {
                    break;
                }
            }
            r = copy(&mut upstream_rx, &mut client_tx) => {
                if r.is_err() {
                    break;
                }
            }
        }
    }
    // TODO: Ignore prev error shutdown next anyway;
    let _ = upstream
        .shutdown()
        .await
        .inspect_err(|e| eprintln!("failed to shutdown upstream connection: {e}"));
    let _ = client_stream
        .shutdown()
        .await
        .inspect_err(|e| eprintln!("failed to shutdown client_stream connection: {e}"));

    Ok(())
}
