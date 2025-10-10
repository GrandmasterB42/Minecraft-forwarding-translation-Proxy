mod handshake;
pub use handshake::Handshake;

mod login_start;
pub use login_start::LoginStart;

mod velocity_plugin_request;
pub use velocity_plugin_request::VelocityLoginPluginRequest;

mod velocity_plugin_response;
pub use velocity_plugin_response::VelocityLoginPluginResponse;

mod disconnect;
pub use disconnect::{Disconnect, PlayDisconnect};

mod generic;
pub use generic::GenericPacket;

pub mod packet_read;
pub mod packet_write;

pub mod id {
    pub trait AsId {}

    pub struct Manual;
    impl AsId for Manual {}

    pub struct Managed(pub u8);
    impl AsId for Managed {}

    impl std::ops::Deref for Managed {
        type Target = u8;

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    pub struct VersionDependent(pub fn(i32) -> Option<u8>);
    impl AsId for VersionDependent {}

    impl VersionDependent {
        pub fn get(&self, protocol: i32) -> Option<u8> {
            (self.0)(protocol)
        }
    }
}

pub trait Packet<ID: id::AsId> {
    // None for packets without a packet ID, meaning all data gets forwarded to the read call in ReadPacket
    const PACKET_ID: ID;

    // The size in bytes of the packet when serialized, meaning the data read and written by ReadPacket and WritePacket respectively
    // When the PACKET_ID is Manual, this does include the size of the packet ID byte, otherwise not
    fn byte_size(&self) -> usize;
}
