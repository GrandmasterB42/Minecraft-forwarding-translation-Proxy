use tokio::io::AsyncReadExt;

use crate::{
    packets::{Packet, ReadPacket},
    types::{MCData, MCString, Uuid, VarInt},
};

pub struct VelocityLoginPluginResponse {
    pub connection_id: VarInt,
    pub version: VarInt,
    pub signature: [u8; 32],
    pub client_address: MCString,
    pub player_uuid: Uuid,
    pub username: MCString, // Max length 16
    pub properties_length: VarInt,
    pub properties: Vec<Property>,
}

pub struct Property {
    pub name: MCString,
    pub value: MCString,
    pub signature: Option<MCString>,
}

impl Property {
    fn byte_size(&self) -> usize {
        self.name.byte_size() // name
        + self.value.byte_size() // value
        + 1 // is signature bool
        + match &self.signature { None => 0, Some(s) => s.byte_size() } // maybe a signature
    }
}

impl Packet for VelocityLoginPluginResponse {
    const PACKET_ID: Option<u8> = Some(0x02);

    fn byte_size(&self) -> usize {
        self.connection_id.byte_size() // Message ID
        + 1 // Has Payload (boolean)
        + 32 // Signature
        + self.version.byte_size() // version
        + self.client_address.byte_size() // Client Address
        + self.player_uuid.byte_size() // Player ID
        + self.username.byte_size() // Username
        + self.properties_length.byte_size() // Length of properties array
        + self.properties.iter().fold(0usize, |acc, e| acc + e.byte_size())
    }
}

impl ReadPacket for VelocityLoginPluginResponse {
    async fn read<R: AsyncReadExt + Unpin>(
        reader: &mut R,
        _expected_length: VarInt,
    ) -> tokio::io::Result<Self> {
        let connection_id = VarInt::read(reader).await?;

        let has_payload = reader.read_u8().await? == 0x01;

        if !has_payload {
            return Err(tokio::io::Error::new(
                tokio::io::ErrorKind::InvalidData,
                "A Login Plugin Response Packet is expected to have a payload, but none was found",
            ));
        }
        // Start of Custom Payload

        let mut signature = [0u8; 32];
        reader.read_exact(&mut signature).await?;

        let version = VarInt::read(reader).await?;

        let client_address = MCString::read(reader).await?;

        let player_uuid = Uuid::read(reader).await?;

        let username = MCString::read(reader).await?;

        let properties_length = VarInt::read(reader).await?;

        let mut properties = Vec::with_capacity(*properties_length as usize);
        for _ in 0..*properties_length {
            let name = MCString::read(reader).await?;
            let value = MCString::read(reader).await?;
            let signature = if reader.read_u8().await? == 0x01 {
                Some(MCString::read(reader).await?)
            } else {
                None
            };
            properties.push(Property {
                name,
                value,
                signature,
            });
        }

        Ok(VelocityLoginPluginResponse {
            connection_id,
            version,
            signature,
            client_address,
            player_uuid,
            username,
            properties_length,
            properties,
        })
    }
}
