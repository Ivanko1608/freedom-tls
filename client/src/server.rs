use std::{net::SocketAddr, sync::Arc};

use tokio::net::TcpStream;
use tokio_rustls::{TlsConnector, rustls::ClientConfig};

pub struct Server {
    addr: SocketAddr,
    client_config: Arc<ClientConfig>,
}

impl Server {
    pub fn new(addr: SocketAddr, client_config: ClientConfig) -> Self {
        Server {
            addr,
            client_config: Arc::new(client_config),
        }
    }

    pub async fn send(&self, client_stream: TcpStream) -> anyhow::Result<()> {
        let connector = TlsConnector::from(self.client_config.clone());

        let server_stream = TcpStream::connect(&self.addr).await?;

        // let addr = match self.addr {};
        //
        // let mut stream = connector.connect(&self.addr, server_stream).await;

        todo!()
    }
}
