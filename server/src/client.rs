use std::fmt::Debug;

use anyhow::{Result, anyhow, ensure};
use ftls_lib::message::{AddressType, Message, Transport};
use futures::{SinkExt, StreamExt};
use tokio::{
    fs::File,
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, copy_bidirectional},
    net::TcpStream,
};
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tracing::{Level, debug, error, info, instrument, span, trace};
use uuid::Uuid;

use crate::util::dns::resolve_domain;

#[derive(Debug)]
pub struct Client<T: AsyncRead + AsyncWrite + Unpin + Debug + Send> {
    id: Uuid,
    // NOTE: Version reserved for later use
    _version: String,
    transport: Transport,
    client_stream: T,
}

impl<T: AsyncRead + AsyncWrite + Unpin + Debug + Send + 'static> Client<T> {
    #[instrument]
    pub async fn try_from_handshake(mut stream: T) -> Result<Self> {
        let msg = Message::from_async_io(&mut stream).await?;

        let Message::Hello {
            version: [major, minor, patch],
            transport,
        } = msg
        else {
            return Err(anyhow!(
                "first message in handshake must be a Hello. Got: {:?}",
                msg
            ));
        };

        let client = Client {
            id: Uuid::new_v4(),
            _version: format!(
                "{}.{}.{}",
                u8::from_le(major),
                u8::from_le(minor),
                u8::from_le(patch)
            ),
            transport,
            client_stream: stream,
        };

        debug!(?client, "new client");

        Ok(client)
    }

    #[instrument]
    pub async fn handle(mut self) -> Result<()> {
        info!(?self, "handling client");

        match self.transport {
            Transport::IP => self.handle_ip().await,
            Transport::SOCKS => self.handle_socks5().await,
        }
    }

    #[instrument]
    pub async fn handle_socks5(&mut self) -> Result<()> {
        let msg = Message::from_async_io(&mut self.client_stream).await?;

        let Message::Destination(dtype, mut addr, port) = msg else {
            return Err(anyhow!(
                "first message after handle call must be a destination. Got: {msg:?}"
            ));
        };

        info!(
            "Handling conn: \t id: {} \t addr_type: {dtype:?} \t {addr}:{port}",
            self.id
        );
        ensure!(!addr.is_empty());

        if dtype == AddressType::DOMAIN {
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

    #[instrument]
    pub async fn handle_ip(mut self) -> Result<()> {
        debug!(?self, "Handling IP client");

        let mut cfg = tun::Configuration::default();

        cfg.address((10, 8, 0, 1))
            .netmask((255, 255, 255, 0))
            .mtu(1400)
            .up();

        #[cfg(target_os = "linux")]
        cfg.platform_config(|pc| {
            pc.ensure_root_privileges(true);
        });

        // Set ip forwarding (not persistent across restarts)
        let mut ip_forward = File::open("/proc/sys/net/ipv4/ip_forward").await?;
        ip_forward.write_u8(1).await?;

        let tun = tun::create_as_async(&cfg)?;

        let (mut tun_write, mut tun_read) = tun.split()?;

        self.client_stream
            .write_all(&Message::Start.to_bytes()?)
            .await?;

        let (mut client_writer, mut client_reader) =
            Framed::new(self.client_stream, LengthDelimitedCodec::new()).split();

        tokio::spawn(async move {
            loop {
                span!(
                    Level::DEBUG,
                    "Reading framed packets from client_stream into tun"
                );

                let read_bytes = match client_reader.next().await {
                    Some(Ok(buf)) => buf,
                    Some(Err(e)) => {
                        error!(
                            error = %e,
                            "failed to read next frame from framed client_stream"
                        );
                        return;
                    }
                    None => {
                        info!("Got None from client_stream, exiting read loop");
                        return;
                    }
                };
                trace!(
                    ?read_bytes,
                    len = read_bytes.len(),
                    "read bytes from framed_client_stream",
                );

                tun_write
                    .write_all(&read_bytes)
                    .await
                    .expect("failed to send packet to upstream");
            }
        });

        tokio::spawn(async move {
            loop {
                span!(
                    Level::DEBUG,
                    "Starting to read from tun into the client_stream"
                );

                let mut buf = [0; 4096];
                match tun_read.read(&mut buf).await {
                    Ok(n_bytes) if n_bytes == 0 => {
                        info!(n_bytes, "Got 0 or less bytes from tun")
                    }
                    Ok(n_bytes) => {
                        trace!(n_bytes, "Got bytes from tun {:X?}", &buf[..n_bytes])
                    }
                    Err(e) => {
                        error!(%e, "failed getting bytes from tun");
                        return;
                    }
                };

                if let Err(e) = client_writer.send(buf.to_vec().into()).await {
                    error!(err = %e, "failed to send bytes to client_writer" );
                    return;
                }
            }
        });
        Ok(())
    }
}
