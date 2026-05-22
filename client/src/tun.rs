use std::sync::Arc;

use ftls_lib::message::{Message, Transport};
use futures::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tracing::{Level, error, info, instrument, span, trace};

use crate::{server::Server, types::ProxySender};

#[derive(Debug)]
pub(super) struct Tun {
    pub server: Arc<Server>,
    pub addr: (u8, u8, u8, u8),
    pub netmask: (u8, u8, u8, u8),
    pub mtu: u16,
}

impl ProxySender for Tun {
    #[instrument(level = "trace")]
    async fn server_start(self) -> anyhow::Result<()> {
        let mut cfg = tun::Configuration::default();

        #[cfg(target_os = "linux")]
        cfg.platform_config(|pc| {
            pc.ensure_root_privileges(true);
        });

        cfg.address(self.addr)
            .netmask(self.netmask)
            .mtu(self.mtu)
            .up();

        let (mut tun_writer, mut tun_reader) = tun::create_as_async(&cfg)?.split()?;

        // TODO: No version hardcode
        let msg = Message::Hello {
            version: [0, 0, 1],
            transport: Transport::IP,
        };

        let upstream = self.server.connect().await?;

        let upstream = Server::handshake(vec![msg], upstream).await?;

        let (mut transport_write, mut transport_read) =
            Framed::new(upstream, LengthDelimitedCodec::new()).split();

        tokio::spawn(async move {
            let mut buf = [0; 4096];
            loop {
                let read_bytes = tun_reader
                    .read(&mut buf)
                    .await
                    .expect("failed to read from tun");

                transport_write
                    .send(buf[..read_bytes].to_vec().into())
                    .await
                    .expect("failed to send packet to upstream");
            }
        });

        tokio::spawn(async move {
            span!(
                Level::DEBUG,
                "Starting to read upstream packets into the tun"
            );

            while let Some(packet) = transport_read.next().await {
                let packet = packet.expect("failed to read next packed");

                trace!("received packet bytes: {:x?}", packet);

                if let Err(e) = tun_writer.write_all(&packet).await {
                    error!(err = %e, "failed to write packet to tun");
                }
            }
            info!("Got None from transport, exiting read loop");
        });

        Ok(())
    }
}
