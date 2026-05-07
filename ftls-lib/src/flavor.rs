pub const MAGIC_HEADER: [u8; 4] = *b"FTLS";

pub(super) mod serialize {

    #[derive(Default)]
    pub struct FtlsSeFlavor {
        inner: Vec<u8>,
    }

    impl FtlsSeFlavor {
        pub fn new() -> Self {
            Self { inner: vec![] }
        }
    }

    impl postcard::ser_flavors::Flavor for FtlsSeFlavor {
        type Output = Vec<u8>;

        fn try_push(&mut self, data: u8) -> postcard::Result<()> {
            self.inner.push(data);
            Ok(())
        }

        fn finalize(self) -> postcard::Result<Self::Output> {
            let mut out = Vec::new();

            out.extend_from_slice(&super::MAGIC_HEADER);
            out.extend_from_slice(&(self.inner.len() as u64).to_le_bytes());
            out.extend_from_slice(&self.inner);

            Ok(out)
        }
    }
}

pub(super) mod deserialize {
    use bytes::Buf;

    use crate::{flavor::MAGIC_HEADER, message::MessageParsingError};

    pub struct FtlsDeFlavor<'de> {
        inner: &'de [u8],
        len: u64,
        cursor: usize,
    }

    impl<'de> FtlsDeFlavor<'de> {
        pub fn new(buf: &'de [u8]) -> Result<Self, MessageParsingError> {
            let header = buf.take(MAGIC_HEADER.len()).into_inner();

            if header != MAGIC_HEADER {
                return Err(MessageParsingError::InvalidHeader(header.to_vec()));
            }

            let len = buf.take(size_of::<u64>()).into_inner();

            let len = u64::from_le_bytes(len.try_into()?);

            Ok(FtlsDeFlavor {
                cursor: 0,
                inner: &buf[MAGIC_HEADER.len() + size_of::<u64>()..],
                len,
            })
        }
    }

    impl<'de> postcard::de_flavors::Flavor<'de> for FtlsDeFlavor<'de> {
        type Remainder = &'de [u8];

        type Source = &'de [u8];

        fn pop(&mut self) -> postcard::Result<u8> {
            if self.cursor as u64 > self.len {
                return Err(postcard::Error::DeserializeBadEncoding);
            }

            let next = self.inner[self.cursor];
            self.cursor += 1;
            Ok(next)
        }

        fn try_take_n(&mut self, ct: usize) -> postcard::Result<&'de [u8]> {
            if (self.cursor + ct) as u64 >= self.len {
                return Err(postcard::Error::DeserializeBadEncoding);
            }

            // ct not included
            let next = &self.inner[self.cursor..self.cursor + ct];
            self.cursor += ct;
            Ok(next)
        }

        fn finalize(self) -> postcard::Result<Self::Remainder> {
            Ok(&self.inner[self.cursor..])
        }
    }
}
