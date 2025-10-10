use tokio::io::AsyncWriteExt;

use crate::{
    packets::{
        Packet,
        id::{AsId, Managed, Manual, VersionDependent},
    },
    types::{MCData, VarInt},
};

pub trait WritePacket {
    async fn write<W: AsyncWriteExt + Unpin + Sized>(
        &self,
        writer: &mut W,
    ) -> tokio::io::Result<()>;
}

pub trait WritePacketExt<ID, P>
where
    Self: AsyncWriteExt + Unpin,
    P: WritePacket + Packet<ID>,
    ID: AsId,
{
    async fn write_packet(&mut self, packet: &P) -> tokio::io::Result<()>;
}

pub trait WriteVersionedPacketExt<P>
where
    Self: AsyncWriteExt + Unpin,
    P: WritePacket + Packet<VersionDependent>,
{
    async fn write_packet_versioned(
        &mut self,
        packet: &P,
        protocol: i32,
    ) -> Result<(), WriteVersionedPacketError>;
}

pub enum WriteVersionedPacketError {
    Io(tokio::io::Error),
    InvalidPacketId { protocol: i32 },
}

impl From<tokio::io::Error> for WriteVersionedPacketError {
    fn from(e: tokio::io::Error) -> Self {
        WriteVersionedPacketError::Io(e)
    }
}

async fn write_packet_general<W: AsyncWriteExt + Unpin, P: WritePacket>(
    writer: &mut W,
    packet: &P,
    byte_size: usize,
    packet_id: Option<u8>,
) -> tokio::io::Result<()> {
    let packet_size = VarInt::new(byte_size as i32)
        .map_err(|e| tokio::io::Error::new(tokio::io::ErrorKind::InvalidData, e))?;

    let mut buffer =
        Vec::with_capacity(byte_size + packet_size.byte_size() + packet_id.map_or(0, |_| 1));

    // Packet Size
    packet_size.write(&mut buffer).await?;
    // Maybe a packet id
    if let Some(id) = packet_id {
        buffer.write_u8(id).await?;
    }
    // The Packet data
    packet.write(&mut buffer).await?;
    // Write it all out to the writer
    writer.write_all(&buffer).await?;

    Ok(())
}

impl<W: AsyncWriteExt + Unpin, P: WritePacket + Packet<Manual>> WritePacketExt<Manual, P> for W {
    async fn write_packet(&mut self, packet: &P) -> tokio::io::Result<()> {
        write_packet_general(self, packet, packet.byte_size(), None).await
    }
}

impl<W: AsyncWriteExt + Unpin, P: WritePacket + Packet<Managed>> WritePacketExt<Managed, P> for W {
    async fn write_packet(&mut self, packet: &P) -> tokio::io::Result<()> {
        write_packet_general(self, packet, packet.byte_size() + 1, Some(*P::PACKET_ID)).await
    }
}

impl<W: AsyncWriteExt + Unpin, P: WritePacket + Packet<VersionDependent>> WriteVersionedPacketExt<P>
    for W
{
    async fn write_packet_versioned(
        &mut self,
        packet: &P,
        protocol: i32,
    ) -> Result<(), WriteVersionedPacketError> {
        let packet_id = P::PACKET_ID
            .get(protocol)
            .ok_or(WriteVersionedPacketError::InvalidPacketId { protocol })?;
        write_packet_general(self, packet, packet.byte_size() + 1, Some(packet_id)).await?;
        Ok(())
    }
}
