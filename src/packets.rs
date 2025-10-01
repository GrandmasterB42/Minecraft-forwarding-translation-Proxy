use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::warn;

use crate::types::{MCData, MCString, VarInt};

pub struct Handshake {
    pub protocol_version: VarInt,
    pub server_address: MCString,
    pub server_port: u16,
    pub next_state: VarInt,
}

impl MCData for Handshake {
    async fn read<R: AsyncReadExt + Unpin>(reader: &mut R) -> tokio::io::Result<Self> {
        let packet_id = reader.read_u8().await?;
        if packet_id != 0 {
            return Err(tokio::io::Error::new(
                tokio::io::ErrorKind::InvalidData,
                format!("Invalid packet ID for handshake: {packet_id}"),
            ));
        }

        Ok(Handshake {
            protocol_version: VarInt::read(reader).await?,
            server_address: MCString::read(reader).await?,
            server_port: reader.read_u16().await?,
            next_state: VarInt::read(reader).await?,
        })
    }

    async fn write<W: AsyncWriteExt + Unpin>(&self, writer: &mut W) -> tokio::io::Result<()> {
        writer.write_u8(0x00).await?; // Packet ID
        self.protocol_version.write(writer).await?;
        self.server_address.write(writer).await?;
        writer.write_u16(self.server_port).await?;
        self.next_state.write(writer).await
    }

    fn byte_size(&self) -> usize {
        1 // Packet ID
            + self.protocol_version.byte_size()
            + self.server_address.byte_size()
            + 2 // u16
            + self.next_state.byte_size()
    }
}

pub trait ReadPacket
where
    Self: AsyncReadExt + Unpin + Sized,
{
    async fn read_packet<T: MCData>(&mut self) -> tokio::io::Result<T>;
}

impl<R: AsyncReadExt + Unpin + Sized> ReadPacket for R {
    async fn read_packet<T: MCData>(&mut self) -> tokio::io::Result<T> {
        let packet_length = VarInt::read(self).await?;
        let packet = T::read(self).await?;

        let packet_size = packet.byte_size();
        if *packet_length as usize != packet.byte_size() {
            warn!(
                "Packet length mismatch: expected {}, got {}",
                *packet_length, packet_size
            );
            let remaining = (*packet_length as usize).saturating_sub(packet_size);
            self.read_exact(&mut vec![0u8; remaining]).await?;
        }

        Ok(packet)
    }
}

pub trait WritePacket
where
    Self: AsyncWriteExt + Unpin,
{
    async fn write_packet(&mut self, packet: impl MCData) -> tokio::io::Result<()>;
}

impl<W: AsyncWriteExt + Unpin> WritePacket for W {
    async fn write_packet(&mut self, packet: impl MCData) -> tokio::io::Result<()> {
        let packet_size = VarInt::new(packet.byte_size() as i32)
            .map_err(|e| tokio::io::Error::new(tokio::io::ErrorKind::InvalidData, e))?;

        let mut buffer = Vec::with_capacity(packet.byte_size() + packet_size.byte_size());

        packet_size.write(&mut buffer).await?;
        packet.write(&mut buffer).await?;

        self.write_all(&buffer).await
    }
}
