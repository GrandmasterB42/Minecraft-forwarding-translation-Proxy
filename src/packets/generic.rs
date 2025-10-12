use std::sync::Arc;

use tokio::io::{AsyncReadExt, BufReader};

use crate::{
    packets::{
        Packet,
        id::{AsId, Managed, Manual, VersionDependent},
        packet_read::ReadPacket,
        packet_write::WritePacket,
    },
    types::VarInt,
};

pub struct GenericPacket {
    pub data: Arc<[u8]>,
}

#[allow(dead_code)]
#[derive(Debug)]
pub enum InterpretError {
    InvalidPacketId,
    PacketIdMismatch(u8),
    PacketLengthMismatch,
    IoError(tokio::io::Error),
}

impl From<tokio::io::Error> for InterpretError {
    fn from(e: tokio::io::Error) -> Self {
        InterpretError::IoError(e)
    }
}

#[allow(dead_code)]
impl GenericPacket {
    pub async fn try_interpret_as<P: Packet<Managed> + ReadPacket>(
        &self,
    ) -> Result<P, InterpretError> {
        self.internl_interpret_as::<P>(*P::PACKET_ID).await
    }

    pub async fn try_interpret_as_versioned<P: Packet<VersionDependent> + ReadPacket>(
        &self,
        protocol: i32,
    ) -> Result<P, InterpretError> {
        let packet_id = (P::PACKET_ID.0)(protocol).ok_or(InterpretError::InvalidPacketId)?;
        self.internl_interpret_as::<P>(packet_id).await
    }

    async fn internl_interpret_as<P: ReadPacket + Packet<impl AsId>>(
        &self,
        packet_id: u8,
    ) -> Result<P, InterpretError> {
        let byte_size = self.data.len() - 1; // Account for packet id
        let mut reader = BufReader::new(&self.data[..]);

        // Check packet id
        let read_packet_id = reader.read_u8().await?;
        if read_packet_id != packet_id {
            return Err(InterpretError::PacketIdMismatch(read_packet_id));
        }

        // Read the generic data as the target packet
        let packet = P::read(&mut reader, VarInt::new(byte_size as i32).unwrap()).await?;

        if byte_size != packet.byte_size() {
            return Err(InterpretError::PacketLengthMismatch);
        }

        Ok(packet)
    }
}

impl Packet<Manual> for GenericPacket {
    const PACKET_ID: Manual = Manual;

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
        Ok(GenericPacket {
            data: Arc::from(data),
        })
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
