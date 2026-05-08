use anyhow::{Result, anyhow, ensure};
use ftls_lib::message::{DestinationType, Message};
use tokio::{
    io::{AsyncRead, AsyncWrite, AsyncWriteExt, copy_bidirectional},
    net::TcpStream,
};
use uuid::Uuid;

use crate::util::dns::resolve_domain;

pub struct Client<T: AsyncRead + AsyncWrite + Unpin> {
    id: Uuid,
    // NOTE: Version reserver for later use
    _version: String,
    client_stream: T,
}

impl<T: AsyncRead + AsyncWrite + Unpin> Client<T> {
    pub async fn try_from_handshake(mut stream: T) -> Result<Self> {
        let msg = Message::from_async_io(&mut stream).await?;

        let Message::Hello {
            version: [major, minor, patch],
        } = msg
        else {
            return Err(anyhow!(
                "first message in handshake must be a Hello. Got: {:?}",
                msg
            ));
        };

        Ok(Client {
            id: Uuid::new_v4(),
            _version: format!(
                "{}.{}.{}",
                u8::from_le(major),
                u8::from_le(minor),
                u8::from_le(patch)
            ),
            client_stream: stream,
        })
    }

    pub async fn handle(&mut self) -> Result<()> {
        let msg = Message::from_async_io(&mut self.client_stream).await?;

        let Message::Destination(dtype, mut addr, port) = msg else {
            return Err(anyhow!(
                "first message after handle call must be a destination. Got: {msg:?}"
            ));
        };

        println!(
            "Handling conn: \t id: {} \t addr_type: {dtype:?} \t {addr}:{port}",
            self.id
        );
        ensure!(!addr.is_empty());

        if dtype == DestinationType::DOMAIN {
            let Some(domain) = resolve_domain(&addr).await? else {
                let msg = Message::Error(format!("no records found for: {}", addr));

                self.client_stream.write_all(&msg.to_bytes()?).await?;

                self.client_stream.flush().await?;

                self.client_stream.shutdown().await?;

                return Err(anyhow!("no DNS records found for: {addr}"));
            };

            addr = domain.to_string();
        }
        self.client_stream
            .write_all(&Message::Start.to_bytes()?)
            .await?;

        let mut upstream = TcpStream::connect((addr, port)).await?;

        copy_bidirectional(&mut self.client_stream, &mut upstream).await?;

        Ok(())
    }
}
