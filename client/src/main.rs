use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use clap::Parser;
use tokio::try_join;
use tokio_rustls::rustls::{
    ClientConfig, RootCertStore,
    pki_types::{CertificateDer, pem::PemObject},
};
use tracing::error;
use tracing_forest::ForestLayer;
use tracing_subscriber::{EnvFilter, Registry, layer::SubscriberExt, util::SubscriberInitExt};

mod server;
mod socks5;
mod tun;
mod types;

use crate::{server::Server, socks5::Socks5Server, tun::Tun, types::ProxySender};

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
    Registry::default()
        .with(EnvFilter::from_default_env())
        .with(ForestLayer::default())
        .init();

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

    let s = server.clone();
    let h_socks5 = async move {
        let socks5 = Socks5Server::new(s, "127.0.0.1".to_string(), args.port);
        socks5.server_start().await?;

        Ok::<(), anyhow::Error>(())
    };

    let tun = Tun {
        server: server.clone(),
        addr: (10, 8, 0, 2),
        netmask: (255, 255, 255, 0),
        mtu: 1400,
    };

    let h_tun = tun.server_start();

    if let Err(e) = try_join!(h_socks5, h_tun) {
        error!(?e, "Fatal server error:");
        return Err(e);
    }

    Ok(())
}
