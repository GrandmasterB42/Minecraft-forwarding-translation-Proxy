use std::ops::Deref;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub trait MCData {
    async fn read<R>(reader: &mut R) -> tokio::io::Result<Self>
    where
        R: AsyncReadExt + Unpin,
        Self: std::marker::Sized;

    async fn write<W>(&self, writer: &mut W) -> tokio::io::Result<()>
    where
        W: AsyncWriteExt + Unpin;

    fn byte_size(&self) -> usize;
}

pub struct VarInt {
    value: i32,
    bytes_needed: u8,
}

impl VarInt {
    pub fn new(value: i32) -> Result<Self, &'static str> {
        Self::try_from(value)
    }
}

impl Deref for VarInt {
    type Target = i32;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl TryFrom<i32> for VarInt {
    type Error = &'static str;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        let bytes_needed = if value == 0 {
            1
        } else {
            let mut temp = value;
            let mut count = 0;
            while temp != 0 {
                temp >>= 7;
                count += 1;
            }
            count
        };

        if bytes_needed > 5 {
            return Err("VarInt is too big");
        }
        Ok(VarInt {
            value,
            bytes_needed,
        })
    }
}

impl MCData for VarInt {
    async fn read<R>(reader: &mut R) -> tokio::io::Result<Self>
    where
        R: AsyncReadExt + Unpin,
    {
        let mut num_read = 0;
        let mut result = 0;
        loop {
            let byte = reader.read_u8().await?;
            let value = (byte & 0b01111111) as i32;
            result |= value << (7 * num_read);

            num_read += 1;
            if num_read > 5 {
                return Err(tokio::io::Error::new(
                    tokio::io::ErrorKind::InvalidData,
                    "VarInt is too big",
                ));
            }

            if (byte & 0b10000000) == 0 {
                break;
            }
        }
        Ok(Self {
            value: result,
            bytes_needed: num_read,
        })
    }

    async fn write<W>(&self, writer: &mut W) -> tokio::io::Result<()>
    where
        W: AsyncWriteExt + Unpin,
    {
        let mut buffer = Vec::with_capacity(self.bytes_needed as usize);
        let mut value = self.value;
        loop {
            let mut temp = (value & 0b01111111) as u8;
            value >>= 7;
            if value != 0 {
                temp |= 0b10000000;
            }
            buffer.push(temp);
            if value == 0 {
                break;
            }
        }
        writer.write_all(&buffer).await
    }

    fn byte_size(&self) -> usize {
        self.bytes_needed as usize
    }
}

pub struct MCString {
    length: VarInt,
    value: String,
}

impl MCData for MCString {
    async fn read<R: AsyncReadExt + Unpin>(reader: &mut R) -> tokio::io::Result<Self> {
        let length = VarInt::read(reader).await?;
        let mut buffer = vec![0u8; *length as usize];
        reader.read_exact(&mut buffer).await?;
        Ok(MCString {
            length,
            value: String::from_utf8_lossy(&buffer).to_string(),
        })
    }

    async fn write<W>(&self, writer: &mut W) -> tokio::io::Result<()>
    where
        W: AsyncWriteExt + Unpin,
    {
        self.length.write(writer).await?;
        writer.write_all(self.value.as_bytes()).await
    }

    fn byte_size(&self) -> usize {
        self.length.byte_size() + self.value.len()
    }
}
