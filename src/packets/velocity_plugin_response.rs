use hmac::{Hmac, Mac};
use sha2::Sha256;
use tokio::io::{AsyncReadExt, BufReader};

use crate::{
    packets::{Packet, id::Managed, packet_read::ReadPacket},
    types::{MCData, MCString, Uuid, VarInt},
};

pub struct VelocityLoginPluginResponse {
    pub connection_id: VarInt,
    pub version: VarInt,
    pub signature: [u8; 32],
    raw_remaining_data: Vec<u8>, // Store any remaining data again for validation
    pub client_address: MCString<32767>,
    pub player_uuid: Uuid,
    pub username: MCString<16>,
    pub properties_length: VarInt,
    pub properties: Vec<Property>,
}

pub struct Property {
    pub name: MCString<32767>,
    pub value: MCString<32767>,
    pub signature: Option<MCString<32767>>,
}

impl Property {
    fn byte_size(&self) -> usize {
        self.name.byte_size() // name
        + self.value.byte_size() // value
        + 1 // is signature bool
        + match &self.signature { None => 0, Some(s) => s.byte_size() } // maybe a signature
    }
}

impl VelocityLoginPluginResponse {
    pub fn validate(&self, secret: &str) -> bool {
        Hmac::<Sha256>::new_from_slice(secret.as_bytes())
            .map(|mut hmac| {
                hmac.update(&self.raw_remaining_data);
                hmac.verify((&self.signature).into()).is_ok()
            })
            .unwrap_or(false)
    }
}

impl Packet<Managed> for VelocityLoginPluginResponse {
    const PACKET_ID: Managed = Managed(0x02);

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
        expected_length: VarInt,
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

        // Read all the extra data into a buffer for validation later
        let bytes_read_so_far = connection_id.byte_size() + 1 + 32; // connection_id + has_payload + signature
        let remaining_bytes = *expected_length as usize - bytes_read_so_far;
        let mut raw_remaining_data = vec![0u8; remaining_bytes];

        reader.read_exact(&mut raw_remaining_data).await?;
        let reader = &mut BufReader::new(&raw_remaining_data[..]);

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
        tracing::trace!("Read packet");

        Ok(VelocityLoginPluginResponse {
            connection_id,
            version,
            signature,
            raw_remaining_data,
            client_address,
            player_uuid,
            username,
            properties_length,
            properties,
        })
    }
}
