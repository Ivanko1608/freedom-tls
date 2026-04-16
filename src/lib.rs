use std::net::TcpListener;

use anyhow::Result;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};

pub async fn start_server(
    server_addr: &str,
    pub_cert: CertificateDer<'static>,
    private_cert: PrivatePkcs8KeyDer<'static>,
) -> Result<()> {
    Ok(())
}
