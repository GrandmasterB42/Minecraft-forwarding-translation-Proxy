use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::debug;

use crate::{
    packets::{Packet, ReadPacket, WritePacket, velocity_plugin_response::Property},
    types::{MCData, MCString, Uuid, VarInt},
};

pub struct Handshake {
    pub protocol_version: VarInt,
    pub server_address: MCString,
    pub server_port: u16,
    pub next_state: VarInt,
}

impl Handshake {
    pub async fn insert_forwarding_data(
        &mut self,
        client_address: MCString,
        player_uuid: Uuid,
        properties: &[Property],
    ) {
        let mut forwarding_data = format!(
            "{}\0{}\0{:x}",
            self.server_address.as_str(),
            client_address.as_str(),
            *player_uuid
        );

        if !properties.is_empty() {
            forwarding_data.push_str("\0[");

            for property in properties {
                forwarding_data.push('{');

                forwarding_data.push_str(r#""name":""#);
                forwarding_data.push_str(&property.name.to_string());
                forwarding_data.push_str("\",");

                forwarding_data.push_str(r#""value":""#);
                forwarding_data.push_str(&property.value.to_string());
                forwarding_data.push('"');

                if let Some(signature) = &property.signature {
                    forwarding_data.push_str(r#","signature":""#);
                    forwarding_data.push_str(&signature.to_string());
                    forwarding_data.push('\"');
                }
                forwarding_data.push_str("},");
            }
            forwarding_data.pop(); // Remove the last ',' at the end of the list
            forwarding_data.push(']');
        }

        debug!("{}", forwarding_data.replace('\0', "\\0"));
        self.server_address = MCString::new(forwarding_data).unwrap();
    }
}

impl Packet for Handshake {
    const PACKET_ID: Option<u8> = Some(0x00);

    fn byte_size(&self) -> usize {
        self.protocol_version.byte_size()
            + self.server_address.byte_size()
            + 2 // u16
            + self.next_state.byte_size()
    }
}

impl ReadPacket for Handshake {
    async fn read<R: AsyncReadExt + Unpin>(
        reader: &mut R,
        _expected_length: VarInt,
    ) -> tokio::io::Result<Self> {
        Ok(Handshake {
            protocol_version: VarInt::read(reader).await?,
            server_address: MCString::read(reader).await?,
            server_port: reader.read_u16().await?,
            next_state: VarInt::read(reader).await?,
        })
    }
}

impl WritePacket for Handshake {
    async fn write<W: AsyncWriteExt + Unpin>(&self, writer: &mut W) -> tokio::io::Result<()> {
        self.protocol_version.write(writer).await?;
        self.server_address.write(writer).await?;
        writer.write_u16(self.server_port).await?;
        self.next_state.write(writer).await
    }
}
