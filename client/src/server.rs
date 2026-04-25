use std::sync::Arc;

use anyhow::Context;
use ftls_lib::proto::dest_header::DestinationHeader;
use protobuf::Message;
use tokio::{
    io::{AsyncWriteExt, copy, split},
    net::TcpStream,
    select,
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

    // TODO: ClientStream should be generic
    pub async fn send(
        &self,
        dst_header: DestinationHeader,
        client_stream: TcpStream,
    ) -> anyhow::Result<()> {
        let connector = TlsConnector::from(self.client_config.clone());

        println!(
            "TCP: Attempting to connect to {}:{}",
            self.domain, self.port
        );

        let server_stream = TcpStream::connect((self.domain.as_ref(), self.port)).await?;

        println!("TCP: Connected to {}:{}", self.domain, self.port);

        let upstream = connector
            // TODO: No clone pls?
            .connect(self.dns_name.clone(), server_stream)
            .await?;

        let (mut server_rx, mut server_tx) = split(upstream);
        let (mut client_rx, mut client_tx) = split(client_stream);

        server_tx.write_u64(dst_header.compute_size()).await?;

        server_tx
            .write_all(&dst_header.write_to_bytes()?)
            .await
            .with_context(|| {
                format!(
                    "failed to write message header to server - mgs: {:?}",
                    dst_header
                )
            })?;

        // Pipe client to upstream then pipe upstream to client reader.

        loop {
            select! {
                r = copy(&mut client_rx, &mut server_tx) => {
                    r.unwrap();
                }
                r = copy(&mut server_rx, &mut client_tx) => {
                        r.unwrap();
                    }
            }
        }
    }
}
