use tokio::io::AsyncReadExt;

use crate::{
    packets::{
        GenericPacket, Packet,
        id::{AsId, Managed, Manual, VersionDependent},
    },
    types::{MCData, VarInt},
};

pub trait ReadPacket {
    async fn read<R: AsyncReadExt + Unpin + Sized>(
        reader: &mut R,
        expected_length: VarInt,
    ) -> tokio::io::Result<Self>
    where
        Self: Sized;
}

pub trait ReadPacketExt<Id>
where
    Self: AsyncReadExt + Unpin + Sized,
    Id: AsId,
{
    async fn read_packet<P: ReadPacket + Packet<Id>>(&mut self) -> Result<P, ReadPacketError>;
}

#[allow(dead_code)]
pub trait ReadVersionedPacketExt<Id>
where
    Self: AsyncReadExt + Unpin + Sized,
    Id: AsId,
{
    async fn read_packet_versioned<P: ReadPacket + Packet<Id>>(
        &mut self,
        protocol: i32,
    ) -> Result<P, ReadVersionedPacketError>;
}

pub enum ReadPacketError {
    // The reader errored
    Io(tokio::io::Error),
    // This just means you didn't get what you expected
    InvalidPacketId {
        #[allow(dead_code)]
        expected: u8,
        got: u8,
        packet: GenericPacket,
    },
    // If got > expected, we have a serious problem as we are now at an invalid packet boundary
    PacketSizeMismatch {
        expected: usize,
        got: usize,
    },
}

impl From<tokio::io::Error> for ReadPacketError {
    fn from(e: tokio::io::Error) -> Self {
        ReadPacketError::Io(e)
    }
}

#[allow(dead_code)]
pub enum ReadVersionedPacketError {
    Read(ReadPacketError),
    UnknownVersionedPacketId { protocol: i32 },
}

impl From<ReadPacketError> for ReadVersionedPacketError {
    fn from(e: ReadPacketError) -> Self {
        ReadVersionedPacketError::Read(e)
    }
}

async fn read_packet_general<R: AsyncReadExt + Unpin, P: ReadPacket + Packet<Id>, Id: AsId>(
    reader: &mut R,
    packet_id: Option<u8>,
) -> Result<P, ReadPacketError> {
    let packet_length = VarInt::read(reader).await?;

    // Account for the packet ID if specified
    if let Some(packet_id) = packet_id {
        let read_packet_id = reader.read_u8().await?;
        if read_packet_id != packet_id {
            // Read the rest into a buffer to give the generic packet the entire data
            let mut buffer = vec![0u8; *packet_length as usize];
            buffer.push(read_packet_id);
            reader.read_exact(&mut buffer).await?;

            return Err(ReadPacketError::InvalidPacketId {
                expected: packet_id,
                got: read_packet_id,
                packet: GenericPacket::read(&mut buffer.as_slice(), packet_length).await?,
            });
        }
    }

    // Remove one from the packet length if we already read the packet ID
    let packet_length = VarInt::new(*packet_length - packet_id.map_or(0, |_| 1)).unwrap();

    // Read the packet data
    let packet = P::read(reader, packet_length).await?;

    let packet_size = packet.byte_size();

    if *packet_length as usize != packet_size {
        // This is bad, as we are now at an invalid packet boundary
        if (*packet_length as usize) < packet_size {
            return Err(ReadPacketError::PacketSizeMismatch {
                expected: *packet_length as usize,
                got: packet_size,
            });
        }

        let remaining = (*packet_length as usize).saturating_sub(packet_size);
        reader.read_exact(&mut vec![0u8; remaining]).await?;
    }

    Ok(packet)
}

impl<R: AsyncReadExt + Unpin + Sized> ReadPacketExt<Manual> for R {
    async fn read_packet<P: ReadPacket + Packet<Manual>>(&mut self) -> Result<P, ReadPacketError> {
        read_packet_general(self, None).await
    }
}

impl<R: AsyncReadExt + Unpin + Sized> ReadPacketExt<Managed> for R {
    async fn read_packet<P: ReadPacket + Packet<Managed>>(&mut self) -> Result<P, ReadPacketError> {
        read_packet_general(self, Some(*P::PACKET_ID)).await
    }
}

impl<R: AsyncReadExt + Unpin + Sized> ReadVersionedPacketExt<VersionDependent> for R {
    async fn read_packet_versioned<P: ReadPacket + Packet<VersionDependent>>(
        &mut self,
        protocol: i32,
    ) -> Result<P, ReadVersionedPacketError> {
        let packet_id = P::PACKET_ID
            .get(protocol)
            .ok_or(ReadVersionedPacketError::UnknownVersionedPacketId { protocol })?;
        let packet = read_packet_general(self, Some(packet_id)).await?;
        Ok(packet)
    }
}
