use std::io::Write;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::warn;

use crate::types::{MCData, VarInt};

mod handshake;
pub use handshake::Handshake;

mod login_start;
pub use login_start::LoginStart;

mod velocity_plugin_request;
pub use velocity_plugin_request::VelocityLoginPluginRequest;

mod velocity_plugin_response;
pub use velocity_plugin_response::VelocityLoginPluginResponse;

mod generic;
pub use generic::{GenericPacket, InterpretError};

// TODO: Think of a way to make PACKET_ID not an option while also not having a magic value or duplicating a bunch of code
// You could add more traits, but it's already three...
pub trait Packet {
    // None for packets without a packet ID, meaning all data gets forwarded to the read call in ReadPacket
    const PACKET_ID: Option<u8>;
    // The size in bytes of the packet when serialized, meaning the data read and written by ReadPacket and WritePacket respectively
    fn byte_size(&self) -> usize;
}

pub trait ReadPacket: Packet {
    async fn read<R: AsyncReadExt + Unpin + Sized>(
        reader: &mut R,
        expected_length: VarInt,
    ) -> tokio::io::Result<Self>
    where
        Self: Sized;
}

pub trait WritePacket: Packet {
    async fn write<W: AsyncWriteExt + Unpin + Sized>(
        &self,
        writer: &mut W,
    ) -> tokio::io::Result<()>;
}

pub trait ReadPacketExt
where
    Self: AsyncReadExt + Unpin + Sized,
{
    async fn read_packet<P: ReadPacket>(&mut self) -> tokio::io::Result<P>;
}

impl<R: AsyncReadExt + Unpin + Sized> ReadPacketExt for R {
    async fn read_packet<P: ReadPacket>(&mut self) -> tokio::io::Result<P> {
        let packet_length = VarInt::read(self).await?;

        // Account for the packet ID if specified
        if let Some(packet_id) = P::PACKET_ID {
            let read_packet_id = self.read_u8().await?;
            if read_packet_id != packet_id {
                return Err(tokio::io::Error::new(
                    tokio::io::ErrorKind::InvalidData,
                    format!(
                        "Invalid packet ID: expected {}, got {}",
                        packet_id, read_packet_id
                    ),
                ));
            }
        }

        // Read the packet data
        let packet = P::read(self, packet_length).await?;

        // Adjust the expected packet size based on whether a packet ID was read
        let packet_size = if P::PACKET_ID.is_some() {
            packet.byte_size() + 1
        } else {
            packet.byte_size()
        };

        if *packet_length as usize != packet_size {
            warn!(
                "Packet length mismatch: expected {}, got {}",
                *packet_length, packet_size
            );

            // This is bad, as we are now at an invalid packet boundary
            if (*packet_length as usize) < packet_size {
                return Err(tokio::io::Error::new(
                    tokio::io::ErrorKind::InvalidData,
                    "Consumed more data than packet length indicates",
                ));
            }

            let remaining = (*packet_length as usize).saturating_sub(packet_size);
            self.read_exact(&mut vec![0u8; remaining]).await?;
        }

        Ok(packet)
    }
}

pub trait WritePacketExt
where
    Self: AsyncWriteExt + Unpin,
{
    async fn write_packet<P: WritePacket>(&mut self, packet: &P) -> tokio::io::Result<()>;
}

impl<W: AsyncWriteExt + Unpin> WritePacketExt for W {
    async fn write_packet<P: WritePacket>(&mut self, packet: &P) -> tokio::io::Result<()> {
        let mut byte_size = packet.byte_size();
        if P::PACKET_ID.is_some() {
            byte_size += 1; // Packet ID size
        }
        let packet_size = VarInt::new(byte_size as i32)
            .map_err(|e| tokio::io::Error::new(tokio::io::ErrorKind::InvalidData, e))?;

        let mut buffer = Vec::with_capacity(byte_size + packet_size.byte_size());

        packet_size.write(&mut buffer).await?;
        if let Some(packet_id) = P::PACKET_ID {
            buffer.write_u8(packet_id).await?;
        }
        packet.write(&mut buffer).await?;

        self.write_all(&buffer).await
    }
}
