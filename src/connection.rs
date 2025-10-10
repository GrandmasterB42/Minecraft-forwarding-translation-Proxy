use std::sync::Arc;

use tokio::{io::AsyncWriteExt, net::TcpStream};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, trace, warn};

use crate::{
    packets::{
        Disconnect, GenericPacket, Handshake, LoginStart, PlayDisconnect,
        VelocityLoginPluginRequest, VelocityLoginPluginResponse,
        packet_read::{ReadPacketError, ReadPacketExt},
        packet_write::{WritePacketExt, WriteVersionedPacketError, WriteVersionedPacketExt},
    },
    types::NextState,
};

pub struct Connection {
    client: TcpStream,
    backend: TcpStream,
    connection_id: i32,
}

impl Connection {
    pub fn initiate(client: TcpStream) -> Result<ParitalConnection, &'static str> {
        // Packets should be forwarded immediately
        if client.set_nodelay(true).is_err() {
            return Err("Failed to disable TCP Delay for client connection");
        };

        Ok(ParitalConnection { client })
    }

    pub async fn handle(&mut self, secret: Arc<str>, cancel: CancellationToken) {
        // First, read the handshake from the client
        let Ok(mut handshake) = self
            .client
            .read_packet::<Handshake>()
            .await
            .map_err(|e| match e {
                ReadPacketError::Io(error) => {
                    error!("Failed to read handshake from client: {error}",);
                }
                ReadPacketError::InvalidPacketId { got, .. } => {
                    warn!("Client sent invalid packet id {got:x} for handshake");
                }
                ReadPacketError::PacketSizeMismatch { .. } => {
                    warn!("Client sent handshake with invalid length");
                }
            })
        else {
            return;
        };

        let protocol = *handshake.protocol_version;

        match handshake.next_state {
            NextState::Status => {
                trace!("Client is requesting status");
                self.forward_status(&handshake).await;
            }
            NextState::Login => {
                trace!("Client is requesting login");
                info!(
                    "Client from is attempting to log in with protocol: {}",
                    *handshake.protocol_version
                );

                // Read the Login Start Packet
                let Ok(mut login_start) = self.client.read_packet::<LoginStart>().await else {
                    error!("Failed to read login start packet from client");
                    return;
                };

                trace!("Sending login plugin request to proxy");
                if let Err(e) = self
                    .client
                    .write_packet(&VelocityLoginPluginRequest::new(self.connection_id))
                    .await
                {
                    warn!("Failed to send login plugin request to proxy: {e}");
                    return;
                };

                trace!("Waiting for login plugin response from proxy");
                let (buffer, response) = match self.buffer_until_response().await {
                    Ok((buffer, response)) => (buffer, response),
                    Err(e) => {
                        error!("Failed to read login plugin response from client: {e}");
                        return;
                    }
                };
                trace!("Received login plugin response from client");

                // Validate the response
                trace!("Validating login plugin response from proxy");
                if *response.connection_id != self.connection_id {
                    warn!("Client sent invalid connection id in login plugin response");
                    return;
                }

                let is_valid = response.validate(&secret);
                if !is_valid {
                    warn!("Client sent invalid signature in login plugin response");

                    if let Err(e) = self
                        .client
                        .write_packet(&Disconnect::reason(
                            "Failed to verify your identity, please rejoin the server",
                        ))
                        .await
                    {
                        warn!("Failed to send disconnect packet to client");
                        debug!("Error: {e}");
                    }

                    return;
                }
                trace!("Forwarding data was valid, continuing with modified handshake");

                // Sending modified Handshake
                handshake
                    .insert_forwarding_data(
                        response.client_address,
                        response.player_uuid,
                        &response.properties,
                    )
                    .await;

                login_start.username = response.username;

                if let Err(e) = self.backend.write_packet(&handshake).await {
                    warn!("Failed to forward handshake to backend: {e}");
                    return;
                }

                if let Err(e) = self.backend.write_packet(&login_start).await {
                    warn!("Failed to forward login start to backend: {e}");
                    return;
                }

                // Forward the buffered packets
                trace!("Forwarding {} buffered packets to backend", buffer.len());
                for packet in &buffer {
                    trace!("Forwarding buffered packet with id {:x}", packet.data[0]);
                    if let Err(e) = self.backend.write_packet(packet).await {
                        warn!("Failed to forward buffered packet to backend: {e}");
                        return;
                    }
                }

                info!("Client authenticated successfully, now forwarding...");
                self.forward_connection(
                    cancel,
                    PlayDisconnect::reason("The Proxy is shutting down"),
                    protocol,
                )
                .await;
                info!("Client disconnected");
            }
            NextState::Transfer => {
                trace!("Client is requesting transfer");
                return;
            }
        }

        trace!("Connection closed");
    }

    async fn forward_connection(
        &mut self,
        cancel: CancellationToken,
        disconnect_packet: PlayDisconnect,
        protocol_version: i32,
    ) {
        tokio::select! {
            result = tokio::io::copy_bidirectional(&mut self.client, &mut self.backend) => {
                match result {
                    Ok((from_client, from_backend)) => {
                        trace!("Connection closed, forwarded {from_client} bytes from client and {from_backend} bytes from backend");
                    }
                    Err(e) => {
                        error!("Failed while forwarding normal server-client interaction: {e}");
                    }
                }
            }
            _ = cancel.cancelled() => {
                trace!("Shutting down active connection");
                match self.client.write_packet_versioned(&disconnect_packet, protocol_version).await
                {
                    Ok(()) => trace!("Sent disconnect packet to client"),
                    Err(e) => match e {
                        WriteVersionedPacketError::Io(error) =>  error!("Failed while forwarding normal server-client interaction: {error}"),
                        WriteVersionedPacketError::InvalidPacketId { protocol } => {
                            warn!("Could not resolve disconnect packet id for protocol verison {protocol}")
                        },
                    },
                };
            },
        }
    }

    async fn forward_status(&mut self, handshake: &Handshake) {
        if let Err(e) = self.backend.write_packet(handshake).await {
            warn!("Failed to forward status handshake to backend: {e}");
            return;
        };

        // Let them to the status exchange normally
        if let Err(e) = tokio::io::copy_bidirectional(&mut self.client, &mut self.backend).await {
            warn!("Failed to forward status data between client and backend");
            debug!("Error: {e}");
        };
    }

    async fn buffer_until_response(
        &mut self,
    ) -> tokio::io::Result<(Vec<GenericPacket>, VelocityLoginPluginResponse)> {
        let mut serverbound_buffer = Vec::new();
        loop {
            trace!("Reading next packet from client while waiting for login plugin response");
            let packet = self
                .client
                .read_packet::<VelocityLoginPluginResponse>()
                .await;

            match packet {
                Ok(response) => {
                    trace!("Found login plugin response packet");
                    return Ok((serverbound_buffer, response));
                }
                Err(e) => match e {
                    ReadPacketError::Io(error) => return Err(error),
                    ReadPacketError::InvalidPacketId {
                        expected: _,
                        got,
                        packet,
                    } => {
                        trace!(
                            "Buffering packet with id {got:x} while waiting for login plugin response"
                        );
                        serverbound_buffer.push(packet);
                    }
                    ReadPacketError::PacketSizeMismatch { expected, got } => {
                        if expected < got {
                            error!(
                                "Packet size mismatch while waiting for login plugin response, expected {expected} bytes but got {got} bytes"
                            );
                            return Err(tokio::io::Error::new(
                                tokio::io::ErrorKind::InvalidData,
                                "Now at invalid packet boundary",
                            ));
                        } else {
                            warn!(
                                "Packet size mismatch while waiting for login plugin response, expected {expected} bytes but got {got} bytes, read remaining bytes and skipping packet"
                            );
                        }
                    }
                },
            }
        }
    }
}

pub struct ParitalConnection {
    client: TcpStream,
}

impl ParitalConnection {
    pub fn with_backend(
        self,
        backend: TcpStream,
        connection_id: i32,
    ) -> Result<Connection, &'static str> {
        // Packets should be forwarded immediately
        if backend.set_nodelay(true).is_err() {
            return Err("Failed to disable TCP Delay for backend connection");
        };

        Ok(Connection {
            client: self.client,
            backend,
            connection_id,
        })
    }

    pub async fn reject_untrusted(mut self) {
        let handshake = match self.client.read_packet::<Handshake>().await {
            Ok(handshake) => handshake,
            Err(e) => {
                match e {
                    ReadPacketError::Io(error) => {
                        error!("Failed to read handshake from client: {error}",);
                        return;
                    }
                    ReadPacketError::InvalidPacketId { got, .. } => {
                        warn!("Client sent invalid packet id {got:x} for handshake",);
                    }
                    ReadPacketError::PacketSizeMismatch { .. } => {
                        warn!("Clientsent handshake with invalid length",);
                    }
                }
                return;
            }
        };
        match handshake.next_state {
            NextState::Status => trace!("Rejecting untrusted connection for a status request",),
            NextState::Login => {
                warn!("Rejecting untrusted connection for a login request",);

                if let Err(e) = self
                    .client
                    .write_packet(&Disconnect::reason(
                        "You are not allowed to connect to this server directly!",
                    ))
                    .await
                {
                    warn!("Failed to send disconnect packet to untrusted client",);
                    debug!("Error: {e}");
                    return;
                };
            }
            NextState::Transfer => {
                warn!("Rejecting untrusted connection for a transfer request",);
            }
        }
        self.client.shutdown().await.ok();
    }
}
