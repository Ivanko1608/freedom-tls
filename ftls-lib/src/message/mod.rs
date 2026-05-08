use anyhow::{Result, ensure};
use postcard::{Deserializer, serialize_with_flavor};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt};

use crate::flavor::{deserialize, serialize::FtlsSeFlavor};

pub const MAGIC_HEADER: [u8; 4] = *b"FTLS";

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug)]
pub enum DestinationType {
    DOMAIN,
    IP,
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug)]
pub enum Message {
    Hello { version: [u8; 3] },
    Destination(DestinationType, String, u16),
    Start,
    Error(String),
}

impl Message {
    pub fn from_bytes(buf: &[u8]) -> Result<Self> {
        let mut de = Deserializer::from_flavor(deserialize::FtlsDeFlavor::new(buf)?);
        Ok(Message::deserialize(&mut de)?)
    }

    pub async fn from_async_io<T: AsyncRead + Unpin>(reader: &mut T) -> Result<Self> {
        let mut header = vec![0u8; MAGIC_HEADER.len()];
        reader.read_exact(&mut header).await?;

        ensure!(
            header == MAGIC_HEADER,
            MessageParsingError::InvalidHeader(header)
        );

        let len = reader.read_u64_le().await?;

        let mut buf = vec![0u8; len as usize];
        reader.read_exact(&mut buf).await?;

        Ok(postcard::from_bytes::<Message>(&buf)?)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let res =
            serialize_with_flavor::<Message, FtlsSeFlavor, Vec<u8>>(self, FtlsSeFlavor::new())?;

        Ok(res)
    }
}

#[derive(Error, Debug)]
pub enum MessageParsingError {
    // TODO: Better error
    #[error("failed to convert primitive to relevant type")]
    PrimitiveConversion(#[from] std::array::TryFromSliceError),

    #[error("provided u16: {0} does not correspond to a valid message type")]
    InvalidMessageType(u16),

    #[error("provided u8: {0} does not correspond to a valid destination type")]
    InvalidDestinationType(u8),

    #[error("invalide message header bytes: {0:?}")]
    InvalidHeader(Vec<u8>),

    #[error("couldn't get length from message header")]
    InvalidHeaderLength(#[from] bytes::TryGetError),
}
