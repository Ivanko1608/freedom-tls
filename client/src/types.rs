use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
};
use tun::AsyncDevice;

pub(crate) trait ProxySender: AsyncRead + AsyncWrite + Unpin + Send {}

impl ProxySender for TcpStream {}

impl ProxySender for AsyncDevice {}
