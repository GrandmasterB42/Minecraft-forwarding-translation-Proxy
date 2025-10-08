use tokio::io::AsyncWriteExt;

use crate::{
    packets::{Packet, WritePacket},
    types::{MCData, MCString, VarInt},
};

pub struct VelocityLoginPluginRequest {
    connection_id: VarInt,
}

impl VelocityLoginPluginRequest {
    pub fn new(connection_id: i32) -> Self {
        Self {
            connection_id: VarInt::new(connection_id).unwrap(),
        }
    }
}

impl Packet for VelocityLoginPluginRequest {
    const PACKET_ID: Option<u8> = Some(0x04);

    fn byte_size(&self) -> usize {
        self.connection_id.byte_size() // Message ID
            + {
                let channel = "velocity:player_info";
                VarInt::new(channel.len() as i32).unwrap().byte_size() + channel.len()
            } // Channel (VarInt length + string)
            + 1 // Protocol version u8
    }
}

impl WritePacket for VelocityLoginPluginRequest {
    async fn write<W: AsyncWriteExt + Unpin>(&self, mut writer: &mut W) -> tokio::io::Result<()> {
        self.connection_id.write(&mut writer).await?; // Message ID
        MCString::new(String::from_utf8_lossy(b"velocity:player_info").into_owned())
            .unwrap()
            .write(&mut writer)
            .await?; // Channel
        writer.write_u8(0x01).await // Protocol version
    }
}
