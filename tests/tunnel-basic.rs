use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result};
use ftls::start_server;
use rcgen::{CertifiedKey, generate_simple_self_signed};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};
use tokio_rustls::{
    TlsConnector,
    rustls::{
        ClientConfig, RootCertStore,
        pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer},
    },
};

#[tokio::test]
async fn test_client_successfully_connects_sends_and_receives() -> Result<()> {
    let subject_alternative_names = vec!["test.ftls.local".to_string(), "localhost".to_string()];

    let CertifiedKey { cert, signing_key } =
        generate_simple_self_signed(subject_alternative_names)?;

    let priv_key = PrivatePkcs8KeyDer::from(signing_key.serialize_der());

    start_server("127.0.0.1:443", cert.der().to_owned(), priv_key)
        .await
        .context("Start server")?;

    let mut root_store = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    root_store.add(cert.der().to_owned())?;

    let config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    let conn = TlsConnector::from(Arc::new(config));

    let stream = TcpStream::connect("127.0.0.1:443").await?;

    let mut stream = conn
        .connect(
            "localhost"
                .try_into()
                .context("Convert domain into rustls domain")?,
            stream,
        )
        .await?;

    stream
        .write_all("Hello TLS!".to_string().as_bytes())
        .await?;

    let mut res = String::new();
    stream.read_to_string(&mut res).await?;

    assert_eq!(res, "Hello Client!");

    Ok(())
}
