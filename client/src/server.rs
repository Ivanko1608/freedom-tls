use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use ftls_lib::message::Message;
use tokio::{io::AsyncWriteExt, net::TcpStream};
use tokio_rustls::{
    TlsConnector, client,
    rustls::{ClientConfig, pki_types::ServerName},
};
use tracing::{debug, instrument, trace};

#[derive(Debug)]
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

    pub async fn connect(&self) -> Result<client::TlsStream<TcpStream>> {
        let connector = TlsConnector::from(self.client_config.clone());

        println!(
            "TCP: Attempting to connect to {}:{}",
            self.domain, self.port
        );

        let server_stream = TcpStream::connect((self.domain.as_ref(), self.port)).await?;

        println!("TCP: Connected to {}:{}", self.domain, self.port);

        Ok(connector
            // TODO: No clone pls?
            .connect(self.dns_name.clone(), server_stream)
            .await?)
    }

    #[instrument]
    pub async fn handshake(
        messages: Vec<Message>,
        mut stream: client::TlsStream<TcpStream>,
    ) -> Result<client::TlsStream<TcpStream>> {
        for msg in messages {
            trace!(message = ?msg, "writing message to stream");
            stream.write_all(&msg.to_bytes()?).await.with_context(|| {
                format!("failed to write message header to server - msg: {:?}", msg)
            })?;
        }

        let msg = Message::from_async_io(&mut stream).await?;

        match msg {
            Message::Start => {
                debug!("Got start message from server");
                Ok(stream)
            }
            Message::Error(e) => return Err(anyhow!(e)),
            m => return Err(anyhow!("Unexpected message after handshake: {m:?}")),
        }
    }
}
