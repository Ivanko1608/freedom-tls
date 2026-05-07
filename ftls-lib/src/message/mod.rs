use anyhow::Result;
use postcard::{Deserializer, serialize_with_flavor};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::flavor::{deserialize, serialize::FtlsSeFlavor};

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug)]
pub enum DestinationType {
    DOMAIN,
    IP,
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug)]
pub enum MessageType {
    Hello,
    Destination(DestinationType, String, u16),
    DataFollows(u64),
    Error(String),
    Service(()),
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug)]
pub struct Message {
    pub message_type: MessageType,
}

impl Message {
    pub fn new(msg_type: MessageType) -> Self {
        Message {
            message_type: msg_type,
        }
    }

    pub fn from_bytes(buf: &[u8]) -> Result<Self> {
        let mut de = Deserializer::from_flavor(deserialize::FtlsDeFlavor::new(buf)?);
        Ok(Message::deserialize(&mut de)?)
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
