use packet_markers::{PacketReadable, PacketWritable};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::warn;

use crate::types::{MCData, MCString, Uuid, VarInt};

// TODO: Enforce max length

#[derive(PacketReadable)]
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

impl MCData for VelocityLoginPluginResponse {
    async fn read<R>(reader: &mut R) -> tokio::io::Result<Self>
    where
        R: AsyncReadExt + Unpin,
        Self: std::marker::Sized,
    {
        let packet_id = reader.read_u8().await?;

        if packet_id != 0x02 {
            return Err(tokio::io::Error::new(
                tokio::io::ErrorKind::InvalidData,
                format!("Invalid packet ID for login plugin response packet: {packet_id}"),
            ));
        }

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

    async fn write<W>(&self, _writer: &mut W) -> tokio::io::Result<()>
    where
        W: AsyncWriteExt + Unpin,
    {
        todo!("This is not writeable")
    }

    fn byte_size(&self) -> usize {
        1 // Packet ID
        + self.connection_id.byte_size() // Message ID
        + 32 // Signature
        + self.version.byte_size() // version
        + self.client_address.byte_size() // Client Address
        + self.player_uuid.byte_size() // Player ID
        + self.username.byte_size() // Username
        + self.properties_length.byte_size() // Length of properties array
        + self.properties.iter().fold(0usize, |acc, e| acc + e.byte_size())
    }
}

#[derive(PacketWritable)]
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

impl MCData for VelocityLoginPluginRequest {
    async fn read<R: AsyncReadExt + Unpin>(_reader: &mut R) -> tokio::io::Result<Self> {
        panic!("This is not supposed to ever be read")
    }

    async fn write<W: AsyncWriteExt + Unpin>(&self, mut writer: &mut W) -> tokio::io::Result<()> {
        writer.write_u8(0x04).await?; // Packet ID
        self.connection_id.write(&mut writer).await?; // Message ID
        MCString::new(String::from_utf8_lossy(b"velocity:player_info").into_owned())
            .unwrap()
            .write(&mut writer)
            .await?; // Channel
        writer.write_u8(0x01).await // Protocol version
    }

    fn byte_size(&self) -> usize {
        1 // Packet ID
            + self.connection_id.byte_size() // Message ID
            + {
                let channel = "velocity:player_info";
                VarInt::new(channel.len() as i32).unwrap().byte_size() + channel.len()
            } // Channel (VarInt length + string)
            + 1 // Protocol version u8
    }
}

#[derive(PacketReadable, PacketWritable)]
pub struct LoginStart {
    pub username: MCString, // Max length 16
                            // It seems the UUID is only sent in later versions?
}

impl MCData for LoginStart {
    async fn read<R: AsyncReadExt + Unpin>(reader: &mut R) -> tokio::io::Result<Self> {
        let packet_id = reader.read_u8().await?;
        if packet_id != 0x00 {
            return Err(tokio::io::Error::new(
                tokio::io::ErrorKind::InvalidData,
                format!("Invalid packet ID for login start: {packet_id}"),
            ));
        }

        Ok(LoginStart {
            username: MCString::read(reader).await?,
        })
    }

    async fn write<W: AsyncWriteExt + Unpin>(&self, writer: &mut W) -> tokio::io::Result<()> {
        writer.write_u8(0x00).await?;
        self.username.write(writer).await
    }

    fn byte_size(&self) -> usize {
        1 // Packet ID
            + self.username.byte_size()
    }
}

#[derive(PacketReadable, PacketWritable)]
pub struct Handshake {
    pub protocol_version: VarInt,
    pub server_address: MCString, // Max length 255? -> Maybe not with forwarding
    pub server_port: u16,
    pub next_state: VarInt,
}

impl MCData for Handshake {
    async fn read<R: AsyncReadExt + Unpin>(reader: &mut R) -> tokio::io::Result<Self> {
        let packet_id = reader.read_u8().await?;
        if packet_id != 0x00 {
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

pub trait PacketReadable {}
pub trait PacketWritable {}

pub trait ReadPacket
where
    Self: AsyncReadExt + Unpin + Sized,
{
    async fn read_packet<P: MCData + PacketReadable>(&mut self) -> tokio::io::Result<P>;
}

impl<R: AsyncReadExt + Unpin + Sized> ReadPacket for R {
    async fn read_packet<P: MCData + PacketReadable>(&mut self) -> tokio::io::Result<P> {
        let packet_length = VarInt::read(self).await?;
        let packet = P::read(self).await?;

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
    async fn write_packet<P: MCData + PacketWritable>(
        &mut self,
        packet: P,
    ) -> tokio::io::Result<()>;
}

impl<W: AsyncWriteExt + Unpin> WritePacket for W {
    async fn write_packet<P: MCData + PacketWritable>(
        &mut self,
        packet: P,
    ) -> tokio::io::Result<()> {
        let packet_size = VarInt::new(packet.byte_size() as i32)
            .map_err(|e| tokio::io::Error::new(tokio::io::ErrorKind::InvalidData, e))?;

        let mut buffer = Vec::with_capacity(packet.byte_size() + packet_size.byte_size());

        packet_size.write(&mut buffer).await?;
        packet.write(&mut buffer).await?;

        self.write_all(&buffer).await
    }
}
