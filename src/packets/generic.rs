use crate::{
    packets::{Packet, ReadPacket, WritePacket},
    types::VarInt,
};
use tokio::io::{AsyncReadExt, BufReader};

pub struct GenericPacket {
    pub data: Vec<u8>,
}

#[derive(Debug)]
pub enum InterpretError {
    InvalidPacketId,
    PacketIdMismatch(u8),
    PacketLengthMismatch,
    IoError(tokio::io::Error),
}

// A Generic packet that just holds the data as its contents
impl GenericPacket {
    // Reinterpret the generic packet as a specific packet type, this currently requires the packet to have a packet ID
    pub async fn try_interpret_as<P: Packet + ReadPacket>(&self) -> Result<P, InterpretError> {
        let mut reader = BufReader::new(&self.data[..]);

        // Only parse into packets with a packet ID
        let Some(packet_id) = P::PACKET_ID else {
            return Err(InterpretError::InvalidPacketId);
        };

        // Check packet id
        let read_packet_id = reader.read_u8().await.map_err(InterpretError::IoError)?;
        if read_packet_id != packet_id {
            return Err(InterpretError::PacketIdMismatch(read_packet_id));
        }

        // Read the generic data as the target packet
        let packet = P::read(
            &mut reader,
            VarInt::new(self.data.len() as i32 - 1).unwrap(),
        )
        .await
        .map_err(InterpretError::IoError)?;

        if self.data.len() != packet.byte_size() + 1 {
            return Err(InterpretError::PacketLengthMismatch);
        }

        Ok(packet)
    }
}

impl Packet for GenericPacket {
    const PACKET_ID: Option<u8> = None;

    fn byte_size(&self) -> usize {
        self.data.len()
    }
}

impl ReadPacket for GenericPacket {
    async fn read<R: tokio::io::AsyncReadExt + Unpin + Sized>(
        reader: &mut R,
        expected_length: VarInt,
    ) -> tokio::io::Result<Self> {
        let mut data = vec![0u8; *expected_length as usize];
        reader.read_exact(&mut data).await?;
        Ok(GenericPacket { data })
    }
}

impl WritePacket for GenericPacket {
    async fn write<W: tokio::io::AsyncWriteExt + Unpin + Sized>(
        &self,
        writer: &mut W,
    ) -> tokio::io::Result<()> {
        writer.write_all(&self.data).await
    }
}
