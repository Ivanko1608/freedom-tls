use std::sync::Arc;

use anyhow::{Context, anyhow, ensure};
use ftls_lib::message::Message;
use tokio::{
    io::{AsyncWriteExt, copy_bidirectional},
    net::TcpStream,
};
use tokio_rustls::{
    TlsConnector,
    rustls::{ClientConfig, pki_types::ServerName},
};

use crate::types::ProxySender;

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

    // TODO: ClientStream should be generic && close conns properly if error.
    pub async fn send(
        &self,
        message: Message,
        mut client_stream: Box<dyn ProxySender>,
    ) -> anyhow::Result<()> {
        let connector = TlsConnector::from(self.client_config.clone());

        println!(
            "TCP: Attempting to connect to {}:{}",
            self.domain, self.port
        );

        let server_stream = TcpStream::connect((self.domain.as_ref(), self.port)).await?;

        println!("TCP: Connected to {}:{}", self.domain, self.port);

        let mut upstream = connector
            // TODO: No clone pls?
            .connect(self.dns_name.clone(), server_stream)
            .await?;

        ensure!(matches!(message, Message::Destination(..)));

        // TODO: Unhardcode version
        let hello = Message::Hello { version: [0, 0, 1] };

        upstream
            .write_all(&hello.to_bytes()?)
            .await
            .context("failed to write hello message to server")?;

        upstream
            .write_all(&message.to_bytes()?)
            .await
            .with_context(|| {
                format!(
                    "failed to write message header to server - mgs: {:?}",
                    message
                )
            })?;

        let msg = Message::from_async_io(&mut upstream).await?;

        match msg {
            Message::Start => {
                println!("Got start from server")
            }
            Message::Error(e) => return Err(anyhow!(e)),
            m => {
                return Err(anyhow!("Unexpected message after handshake: {m:?}"));
            }
        }

        // Pipe client to upstream then pipe upstream to client reader.
        copy_bidirectional(&mut upstream, &mut client_stream).await?;
        Ok(())
    }
}
