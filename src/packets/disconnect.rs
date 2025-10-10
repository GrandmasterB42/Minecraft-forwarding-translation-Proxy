use crate::{
    packets::{
        Packet,
        id::{Managed, VersionDependent},
        packet_write::WritePacket,
    },
    types::{MCData, MCString},
};

pub struct Disconnect {
    pub reason: MCString<32767>, // A JSON text component
}

impl Disconnect {
    pub fn reason(reason: &str) -> Self {
        Self {
            reason: MCString::new(format!(r#"{{"text": "{reason}", "color": "red"}}"#)).unwrap(),
        }
    }
}

impl Packet<Managed> for Disconnect {
    const PACKET_ID: Managed = Managed(0x00);

    fn byte_size(&self) -> usize {
        self.reason.byte_size()
    }
}

impl WritePacket for Disconnect {
    async fn write<W: tokio::io::AsyncWriteExt + Unpin>(
        &self,
        writer: &mut W,
    ) -> tokio::io::Result<()> {
        self.reason.write(writer).await
    }
}

pub struct PlayDisconnect {
    inner: Disconnect,
}

impl PlayDisconnect {
    pub fn reason(reason: &str) -> Self {
        Self {
            inner: Disconnect::reason(reason),
        }
    }

    const fn protocol_id(protocol: i32) -> Option<u8> {
        match protocol {
            0..67 => Some(0x40),
            67..80 => Some(0x19),
            80..318 => Some(0x1A),
            318..332 => Some(0x1B),
            332..=340 => Some(0x1A),
            // Version 340 is 1.12.2, the earliest last non-modern forwarding version
            _ => None,
        }
    }
}

impl Packet<VersionDependent> for PlayDisconnect {
    // TODO: This is version dependent, 0x40 is valid for Protocol Version < 67
    // This needs to be accounted for, for Versions 1.12.2 and below
    const PACKET_ID: VersionDependent = VersionDependent(Self::protocol_id);

    fn byte_size(&self) -> usize {
        self.inner.byte_size()
    }
}

impl WritePacket for PlayDisconnect {
    async fn write<W: tokio::io::AsyncWriteExt + Unpin>(
        &self,
        writer: &mut W,
    ) -> tokio::io::Result<()> {
        self.inner.write(writer).await
    }
}
