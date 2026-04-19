use std::sync::Arc;

use tokio::net::TcpStream;
use tokio_rustls::{TlsConnector, rustls::ClientConfig};

pub struct Server {
    /// Either a domain or an IP address of the FTLS server we are connecting to.
    addr: String,
    client_config: Arc<ClientConfig>,
}

impl Server {
    pub fn new(addr: String, client_config: ClientConfig) -> Self {
        Server {
            addr,
            client_config: Arc::new(client_config),
        }
    }

    pub async fn send(&self, client_stream: TcpStream) -> anyhow::Result<()> {
        let connector = TlsConnector::from(self.client_config.clone());

        let server_stream = TcpStream::connect(&self.addr).await?;

        let mut stream = connector.connect(&self.addr, server_stream).await;

        todo!()
    }
}
