use crate::{
    packets::{Packet, WritePacket},
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

impl Packet for Disconnect {
    const PACKET_ID: Option<u8> = Some(0x00);

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
