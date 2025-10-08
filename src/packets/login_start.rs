use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{
    packets::{Packet, ReadPacket, WritePacket},
    types::{MCData, MCString, VarInt},
};

pub struct LoginStart {
    pub username: MCString<16>,
    // It seems the UUID is only sent in later versions?
}

impl Packet for LoginStart {
    const PACKET_ID: Option<u8> = Some(0x00);

    fn byte_size(&self) -> usize {
        self.username.byte_size()
    }
}

impl ReadPacket for LoginStart {
    async fn read<R: AsyncReadExt + Unpin>(
        reader: &mut R,
        _expected_length: VarInt,
    ) -> tokio::io::Result<Self> {
        Ok(LoginStart {
            username: MCString::read(reader).await?,
        })
    }
}

impl WritePacket for LoginStart {
    async fn write<W: AsyncWriteExt + Unpin>(&self, writer: &mut W) -> tokio::io::Result<()> {
        self.username.write(writer).await
    }
}
