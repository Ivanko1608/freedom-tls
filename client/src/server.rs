use std::{net::SocketAddr, sync::Arc};

use tokio::{
    io::{AsyncWriteExt, split},
    net::TcpStream,
};
use tokio_rustls::{
    TlsConnector,
    rustls::{ClientConfig, pki_types::ServerName},
};

pub struct Server {
    domain: String,
    port: u16,
    dns_name: ServerName<'static>,
    client_config: Arc<ClientConfig>,
}

impl Server {
    pub fn new(domain: String, port: u16, client_config: ClientConfig) -> anyhow::Result<Self> {
        Ok(Server {
            dns_name: ServerName::try_from(domain.clone())?,
            domain,
            port,
            client_config: Arc::new(client_config),
        })
    }

    pub async fn send(&self, mut client_stream: TcpStream) -> anyhow::Result<()> {
        let connector = TlsConnector::from(self.client_config.clone());

        let server_stream = TcpStream::connect((self.domain.as_ref(), self.port)).await?;

        let stream = connector
            // TODO: No clone pls?
            .connect(self.dns_name.clone(), server_stream)
            .await?;

        let (reader, mut writer) = split(stream);

        let wrote = tokio::io::copy(&mut client_stream, &mut writer).await?;

        dbg!(wrote);

        Ok(())
    }
}
