use std::{net::SocketAddr, sync::Arc};

use tokio::{io::AsyncWriteExt, net::TcpStream};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, trace, warn};

use crate::packets::{
    Disconnect, GenericPacket, Handshake, LoginStart, PlayDisconnect, VelocityLoginPluginRequest,
    VelocityLoginPluginResponse,
    packet_read::{ReadPacketError, ReadPacketExt},
    packet_write::{WritePacketExt, WriteVersionedPacketError, WriteVersionedPacketExt},
};

pub async fn handle_connection(
    mut client_connection: TcpStream,
    mut backend_connection: TcpStream,
    client_adress: SocketAddr,
    connection_id: i32,
    secret: Arc<str>,
    cancel: CancellationToken,
) {
    // First, read the handshake from the client
    let mut handshake = match client_connection.read_packet::<Handshake>().await {
        Ok(handshake) => handshake,
        Err(e) => {
            match e {
                ReadPacketError::Io(error) => {
                    error!("Failed to read handshake from client {client_adress}: {error}");
                    return;
                }
                ReadPacketError::InvalidPacketId { got, .. } => {
                    warn!("Client {client_adress} sent invalid packet id {got:x} for handshake");
                }
                ReadPacketError::PacketSizeMismatch { .. } => {
                    warn!("Client {client_adress} sent handshake with invalid length");
                }
            }
            return;
        }
    };

    match *handshake.next_state {
        1 => {
            trace!("Client {client_adress} is requesting status");
            if let Err(e) = backend_connection.write_packet(&handshake).await {
                warn!("Failed to forward status handshake to backend: {e}");
                return;
            };

            // Let them to the status exchange normally
            if let Err(e) =
                tokio::io::copy_bidirectional(&mut client_connection, &mut backend_connection).await
            {
                warn!("Failed to forward status data between client and backend");
                debug!("Error: {e}");
                return;
            };
        }
        2 => {
            trace!("Client {client_adress} is requesting login");
            info!(
                "Client from {client_adress} is attempting to log in with protocol: {}",
                *handshake.protocol_version
            );

            // Read the Login Start Packet
            let Ok(mut login_start) = client_connection.read_packet::<LoginStart>().await else {
                error!("Failed to read login start packet from client");
                return;
            };

            trace!("Sending login plugin request to proxy");
            if let Err(e) = client_connection
                .write_packet(&VelocityLoginPluginRequest::new(connection_id))
                .await
            {
                warn!("Failed to send login plugin request to proxy: {e}");
                return;
            };

            trace!("Waiting for login plugin response from proxy");
            let (buffer, response) = match buffer_until_response(&mut client_connection).await {
                Ok((buffer, response)) => (buffer, response),
                Err(e) => {
                    error!("Failed to read login plugin response from client {client_adress}: {e}");
                    return;
                }
            };
            trace!("Received login plugin response from client");

            // Validate the response
            trace!("Validating login plugin response from proxy");
            if *response.connection_id != connection_id {
                warn!("Client {client_adress} sent invalid connection id in login plugin response");
                return;
            }

            let is_valid = response.validate(&secret);
            if !is_valid {
                warn!("Client {client_adress} sent invalid signature in login plugin response");

                if let Err(e) = client_connection
                    .write_packet(&Disconnect::reason(
                        "Failed to verify your identity, please rejoin the server",
                    ))
                    .await
                {
                    warn!("Failed to send disconnect packet to client {client_adress}");
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

            if let Err(e) = backend_connection.write_packet(&handshake).await {
                warn!("Failed to forward status handshake to backend: {e}");
                return;
            };

            login_start.username = response.username;
            // Forward the captured login start packet
            if let Err(e) = backend_connection.write_packet(&login_start).await {
                warn!("Failed to forward login start packet to backend: {e}");
                return;
            };

            // Forward the buffered packets
            if !buffer.is_empty() {
                trace!(
                    "Forwarding {} bytes of buffered packets to backend",
                    buffer.len()
                );

                for packet in &buffer {
                    trace!("Forwarding buffered packet with id {:x}", packet.data[0]);
                    if let Err(e) = backend_connection.write_packet(packet).await {
                        warn!("Failed to forward buffered packet to backend: {e}");
                        return;
                    }
                }
            }

            info!("Client {client_adress} authenticated successfully, now forwarding...");
            if let Err(e) = forward_connection(
                client_connection,
                backend_connection,
                &client_adress.to_string(),
                cancel,
                PlayDisconnect::reason("The Proxy is shutting down"),
                *handshake.protocol_version,
            )
            .await
            {
                warn!("Failed while forwarding normal server-client interaction: {e}");
                return;
            };
            info!("Client {client_adress} disconnected");
        }
        3 => {
            trace!("Client {client_adress} is requesting transfer");
            return;
        }
        _ => {
            error!("Client {client_adress} sent invalid handshake packet");
            return;
        }
    }

    trace!("Connection from {client_adress} closed");
}

pub async fn reject_untrusted(client_connection: &mut TcpStream, client_adress: SocketAddr) {
    let handshake = match client_connection.read_packet::<Handshake>().await {
        Ok(handshake) => handshake,
        Err(e) => {
            match e {
                ReadPacketError::Io(error) => {
                    error!("Failed to read handshake from client {client_adress}: {error}");
                    return;
                }
                ReadPacketError::InvalidPacketId { got, .. } => {
                    warn!("Client {client_adress} sent invalid packet id {got:x} for handshake");
                }
                ReadPacketError::PacketSizeMismatch { .. } => {
                    warn!("Client {client_adress} sent handshake with invalid length");
                }
            }
            return;
        }
    };
    match *handshake.next_state {
        1 => trace!("Rejecting untrusted connection from {client_adress} for a status request"),
        2 => {
            warn!("Rejecting untrusted connection from {client_adress} for a login request");

            if let Err(e) = client_connection
                .write_packet(&Disconnect::reason(
                    "You are not allowed to connect to this server directly!",
                ))
                .await
            {
                warn!("Failed to send disconnect packet to untrusted client {client_adress}");
                debug!("Error: {e}");
                return;
            };
        }
        3 => warn!("Rejecting untrusted connection from {client_adress} for a transfer request"),
        _ => (),
    }
    client_connection.shutdown().await.ok();
}

async fn forward_connection(
    mut client: TcpStream,
    mut backend: TcpStream,
    client_adress: &str,
    cancel: CancellationToken,
    disconnect_packet: PlayDisconnect,
    protocol_version: i32,
) -> tokio::io::Result<()> {
    tokio::select! {
        result = tokio::io::copy_bidirectional(&mut client, &mut backend) => {
            match result {
                Ok((from_client, from_backend)) => {
                    trace!("Connection closed, forwarded {from_client} bytes from client and {from_backend} bytes from backend");
                    Ok(())
                }
                Err(e) => {
                    Err(e)
                }
            }
        }
        _ = cancel.cancelled() => {
            trace!("Shutting down active connection from {client_adress}");
            match client.write_packet_versioned(&disconnect_packet, protocol_version).await
            {
                Ok(()) => trace!("Sent disconnect packet to client {client_adress}"),
                Err(e) => match e {
                    WriteVersionedPacketError::Io(error) => return Err(error),
                    WriteVersionedPacketError::InvalidPacketId { protocol } => {
                        warn!("Could not resolve disconnect packet id for protocol verison {protocol}")
                    },
                },
            };
            Ok(())
        },
    }
}

async fn buffer_until_response(
    client_connection: &mut TcpStream,
) -> tokio::io::Result<(Vec<GenericPacket>, VelocityLoginPluginResponse)> {
    let mut serverbound_buffer = Vec::new();
    loop {
        trace!("Reading next packet from client while waiting for login plugin response");
        let packet = client_connection
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
