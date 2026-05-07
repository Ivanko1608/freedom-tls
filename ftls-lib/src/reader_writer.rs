use anyhow::Context;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};

use crate::message::{Message, MessageType};

const MAX_DATA_SIZE: usize = 16_384;

pub async fn read<T>(mut stream: T) -> anyhow::Result<Vec<u8>>
where
    T: AsyncRead + Unpin,
{
    let mut buf = vec![0u8; size_of::<Message>()];
    stream
        .read_exact(&mut buf)
        .await
        .context("read bytes from stream of size Message")?;

    let msg: Message = postcard::from_bytes(&buf)?;

    match msg.message_type {
        MessageType::DataFollows(len) => {
            let mut buf = Vec::with_capacity(MAX_DATA_SIZE);
            let mut bytes_read = 0;

            while bytes_read < len {
                let read = stream.read(&mut buf).await?;

                bytes_read += read as u64;
            }
            Ok(buf)
        }
        MessageType::Destination(dtype, addr, port) => {
            todo!();
        }
        _ => unimplemented!(),
    }
}

pub async fn write<T>(stream: T)
where
    T: AsyncWrite,
{
}
