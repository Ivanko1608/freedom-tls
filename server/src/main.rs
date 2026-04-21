use anyhow::{Context, Result};
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};

use std::sync::Arc;

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_rustls::{TlsAcceptor, rustls::ServerConfig};

#[tokio::main]
async fn main() -> Result<()> {
    // FIXME: No hardcoded paths!
    let cert = CertificateDer::from_pem_file("/home/user/learning/ftls/config/server.pem")?;
    let key = PrivateKeyDer::from_pem_file("/home/user/learning/ftls/config/server-key.pem")?;

    start_server("127.0.0.1:8443", cert, key)
        .await
        .context("Failed to start server")?;

    Ok(())
}

async fn start_server(
    server_addr: &str,
    pub_cert: CertificateDer<'static>,
    private_cert: PrivateKeyDer<'static>,
) -> Result<()> {
    let cfg = ServerConfig::builder()
        .with_no_client_auth()
        // NOTE: Supposed to pass in root or signing CA certs here too?
        .with_single_cert(vec![pub_cert], private_cert)?;

    let tls_acceptor = TlsAcceptor::from(Arc::new(cfg));

    let tcp_listener = TcpListener::bind(server_addr)
        .await
        .with_context(|| format!("Failed to bind to {}", server_addr))?;

    println!("Started server on {server_addr}");

    let (stream, peer_addr) = tcp_listener.accept().await?;

    let mut stream = tls_acceptor.accept(stream).await?;

    let mut buf = String::new();
    stream.read_line(&mut buf).await?;

    println!("Got: {} from {}", buf, peer_addr);

    stream.write_all(&b"Hello Client!"[..]).await?;

    Ok(())
}
