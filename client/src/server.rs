use std::sync::Arc;

use tokio::net::TcpStream;
use tokio_rustls::{TlsConnector, rustls::ClientConfig};

pub struct Server {
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

    pub async fn send(&self, stream: TcpStream) -> anyhow::Result<()> {
        let connector = TlsConnector::from(self.client_config.clone());

        // let stream = TcpStream::connect(&addr).await?;

        todo!()
    }
}
