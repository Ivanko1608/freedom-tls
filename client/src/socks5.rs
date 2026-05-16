use std::net::SocketAddr;

use anyhow::Result;
use fast_socks5::{Socks5Command, server::Socks5ServerProtocol, util::target_addr::TargetAddr};
use ftls_lib::message::Message;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::mpsc::UnboundedSender,
};

use crate::types::ProxySender;

pub async fn server_start(
    ch: UnboundedSender<(Message, Box<dyn ProxySender>)>,
    addr: String,
    port: u16,
) -> Result<()> {
    let listener = TcpListener::bind(format!("{}:{}", addr, port)).await?;

    println!("Started socks5 server at: {}", listener.local_addr()?);

    loop {
        let (stream, client_addr) = listener.accept().await?;

        let socks_conn = Socks5ServerProtocol::accept_no_auth(stream).await?;

        let ch = ch.clone();
        tokio::spawn(async move {
            let (ta, stream) = handle_socks5(socks_conn, client_addr)
                .await
                .expect("failed to handle socks5 connection: {e}");
            let msg = util::target_addr_into_dest(ta);
            ch.send((msg, Box::new(stream)))
        });
    }
}

async fn handle_socks5(
    conn: Socks5ServerProtocol<TcpStream, fast_socks5::server::states::Authenticated>,
    client_addr: SocketAddr,
) -> Result<(TargetAddr, TcpStream)> {
    let (stream, cmd, target_addr) = conn.read_command().await?;

    match cmd {
        Socks5Command::TCPConnect => {
            println!("Socks5: TCPConnect");
            // target_addr is an enum of either Ip(SocketAddr) or Domain (String, u16)
            println!("Got new socks5 tcp conn to {target_addr}");
            //If the reply code indicates a success, and the
            //request was either a BIND or a CONNECT, the client may now start
            //passing data.
            let stream = stream.reply_success(client_addr).await?;

            Ok((target_addr, stream))
        }
        // BIND
        // The BIND request is used in protocols which require the client to
        // accept connections from the server.  FTP is a well-known example,
        // which uses the primary client-to-server connection for commands and
        // status reports, but may use a server-to-client connection for
        // transferring data on demand (e.g. LS, GET, PUT).
        //
        // It is expected that the client side of an application protocol will
        // use the BIND request only to establish secondary connections after a
        // primary connection is established using CONNECT.  In is expected that
        // a SOCKS server will use DST.ADDR and DST.PORT in evaluating the BIND
        // request.
        //
        // Two replies are sent from the SOCKS server to the client during a
        // BIND operation.  The first is sent after the server creates and binds
        // a new socket.  The BND.PORT field contains the port number that the
        // SOCKS server assigned to listen for an incoming connection.  The
        // BND.ADDR field contains the associated IP address.  The client will
        // typically use these pieces of information to notify (via the primary
        // or control connection) the application server of the rendezvous
        // address.  The second reply occurs only after the anticipated incoming
        // connection succeeds or fails.
        //
        Socks5Command::TCPBind => {
            eprintln!("TCPBind");
            todo!()
        }
        Socks5Command::UDPAssociate => unimplemented!(),
    }
}

mod util {
    use fast_socks5::util::target_addr::TargetAddr;
    use ftls_lib::message::{DestinationType, Message};

    pub fn target_addr_into_dest(ta: TargetAddr) -> Message {
        match ta {
            TargetAddr::Domain(domain, port) => {
                ftls_lib::message::Message::Destination(DestinationType::DOMAIN, domain, port)
            }

            TargetAddr::Ip(sa) => ftls_lib::message::Message::Destination(
                DestinationType::IP,
                sa.ip().to_string(),
                sa.port(),
            ),
        }
    }
}
