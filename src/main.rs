use std::{net::SocketAddr, path::Path};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

use tracing::{
    Instrument, Level, debug, error, info, level_filters::LevelFilter, span, trace, warn,
};

use crate::{
    config::{ConfigError, TomlConfig},
    packets::{
        Handshake, LoginStart, ReadPacket, VelocityLoginPluginRequest, VelocityLoginPluginResponse,
        WritePacket,
    },
    types::{MCData, MCString, VarInt},
};

mod config;
mod packets;
mod types;

static CONFIG_PATH: &str = "Config.toml";

// TODO: Investigate something like https://github.com/belohnung/minecraft-varint/tree/master for varint decoding
// TODO: Don't forget logging and ctrl-c handling
#[tokio::main]
async fn main() {
    let debug_filter = if cfg!(debug_assertions) {
        LevelFilter::TRACE
    } else {
        LevelFilter::INFO
    };

    tracing_subscriber::fmt::fmt()
        .with_max_level(debug_filter)
        .init();

    let config = match TomlConfig::at_location(Path::new(CONFIG_PATH)).await {
        Ok(config) => config,
        Err(e) => {
            match e {
                ConfigError::Creation(_)
                | ConfigError::Read(_)
                | ConfigError::Write(_)
                | ConfigError::Parse(_)
                | ConfigError::NoSecret => {
                    error!("{e}");
                }
                ConfigError::CreatedNew(_) => info!("{e}"),
            };
            return;
        }
    };

    let client_listener = match TcpListener::bind(config.bind_address).await {
        Ok(listener) => {
            info!(
                "Listening for client connections on {}",
                config.bind_address
            );
            listener
        }
        Err(e) => {
            error!("Failed to bind to {}: {e}", config.bind_address);
            return;
        }
    };

    let mut connection_id = 0i32;

    // Wait for connections
    while let Ok((client_connection, client_adress)) = client_listener.accept().await {
        let connection_span = span!(Level::TRACE, "client_connection", client = %client_adress);
        trace!(parent: &connection_span, "New client connection from {client_adress}");

        if !config.trusted_addresses.is_empty()
            && !config.trusted_addresses.contains(&client_adress)
        {
            warn!(parent: &connection_span, "Rejected connection from untrusted address {client_adress}");
            continue;
        }

        let Ok(backend_connection) = TcpStream::connect(config.backend_address).await else {
            error!(parent: &connection_span, "Failed to connect to backend server");
            continue;
        };

        let (Ok(()), Ok(())) = (
            client_connection.set_nodelay(true),
            backend_connection.set_nodelay(true),
        ) else {
            error!(parent: &connection_span, "Failed to disable TCP Delay for connection");
            continue;
        };

        tokio::task::spawn(
            handle_connection(
                client_connection,
                client_adress,
                backend_connection,
                connection_id,
            )
            .instrument(connection_span),
        );
        connection_id = connection_id.wrapping_add(1);
    }
}

async fn handle_connection(
    mut client_connection: TcpStream,
    client_adress: SocketAddr,
    mut backend_connection: TcpStream,
    connection_id: i32,
) {
    // TODO: Support old handshakes
    // First, read the handshake from the client
    let mut handshake = match client_connection.read_packet::<Handshake>().await {
        Ok(handshake) => handshake,
        Err(e) => {
            error!("Failed to read handshake from client {client_adress}: {e}");
            return;
        }
    };

    match *handshake.next_state {
        1 => {
            trace!("Client {client_adress} is requesting status");
            let Ok(_) = backend_connection.write_packet(handshake).await else {
                warn!("Failed to forward status handshake to backend");
                return;
            };

            // Let them to the status exchange normally
            let Ok(_) =
                tokio::io::copy_bidirectional(&mut client_connection, &mut backend_connection)
                    .await
            else {
                warn!("Failed to forward status data between client and backend");
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
                warn!("Failed to read login start packet from client");
                return;
            };

            // Send Request to the proxy
            let plugin_request = VelocityLoginPluginRequest::new(connection_id);
            let Ok(_) = client_connection.write_packet(plugin_request).await else {
                warn!("Failed to send login plugin request to proxy");
                return;
            };

            trace!("Sending login plugin request to proxy");
            let Ok(_) = client_connection.flush().await else {
                warn!("Failed to flush login plugin request to proxy");
                return;
            };

            trace!("Waiting for login plugin response from proxy");
            let (buffer, response) = match buffer_until_response(&mut client_connection).await {
                Ok((buffer, response)) => (buffer, response),
                Err(e) => {
                    warn!("Failed to read login plugin response from client {client_adress}: {e}");
                    return;
                }
            };
            trace!("Received login plugin response from client");

            // TODO: Validate response
            trace!("Validating login plugin response from proxy");
            if *response.connection_id != connection_id {
                warn!("Client {client_adress} sent invalid connection id in login plugin response");
                return;
            }

            // Sending modified Handshake

            let mut forwarding_data = format!(
                "{}\0{}\0{:x}",
                handshake.server_address.as_str(),
                response.client_address.as_str(),
                response.player_uuid.0
            );

            if !response.properties.is_empty() {
                forwarding_data.push('\0');
                forwarding_data.push('[');

                for property in response.properties {
                    forwarding_data.push('{');

                    forwarding_data.push_str("\"name\":\"");
                    forwarding_data.push_str(&property.name.to_string());
                    forwarding_data.push_str("\",");

                    forwarding_data.push_str("\"value\":\"");
                    forwarding_data.push_str(&property.value.to_string().replace('=', "\\u003d"));
                    forwarding_data.push('"');

                    if let Some(signature) = property.signature {
                        forwarding_data.push_str(",\"signature\":\"");
                        forwarding_data.push_str(&signature.to_string().replace('=', "\\u003d"));
                        forwarding_data.push('\"');
                    }
                    forwarding_data.push_str("},");
                }
                forwarding_data.pop(); // Remove the last ',' at the end of the list
                forwarding_data.push(']');
            }

            debug!("{}", forwarding_data.replace('\0', "\\0"));
            handshake.server_address = MCString::new(forwarding_data).unwrap();

            let Ok(_) = backend_connection.write_packet(handshake).await else {
                warn!("Failed to forward status handshake to backend");
                return;
            };

            login_start.username = response.username;
            // Forward the captured login start packet
            let Ok(_) = backend_connection.write_packet(login_start).await else {
                warn!("Failed to forward login start packet to backend");
                return;
            };

            // Forward the buffered packets
            if !buffer.is_empty() {
                trace!(
                    "Forwarding {} bytes of buffered packets to backend",
                    buffer.len()
                );
                if let Err(e) = backend_connection.write_all(&buffer).await {
                    warn!("Failed to forward buffered packets to backend: {e}");
                    return;
                }
            }

            let Ok(_) = backend_connection.flush().await else {
                error!("failed to send all packets to the server before initiating connection");
                return;
            };

            info!("Client {client_adress} authenticated successfully, now forwarding...");
            let Ok(_) =
                tokio::io::copy_bidirectional(&mut client_connection, &mut backend_connection)
                    .await
            else {
                warn!("Failed while forwarding normal server-client interaction");
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

async fn buffer_until_response(
    client_connection: &mut TcpStream,
) -> tokio::io::Result<(Vec<u8>, VelocityLoginPluginResponse)> {
    let mut serverbound_buffer = Vec::new();
    loop {
        // Read length
        let packet_length = VarInt::read(client_connection).await?;
        trace!("Reading packet of length {}", *packet_length);

        // Peek if id matches the LoginPluginResponse
        let mut peek_buffer = [0u8; 1];
        client_connection.peek(&mut peek_buffer).await?;

        // If it does, read the packet fully and return
        if peek_buffer[0] == 0x02 {
            trace!("Trying to read login plugin response packet");
            match VelocityLoginPluginResponse::read(client_connection).await {
                Ok(response) => {
                    return Ok((serverbound_buffer, response));
                }
                Err(e) => {
                    error!("Failed to read login plugin response from client: {e}");
                    return Err(e);
                }
            };
        }

        trace!("buffering packet id: 0x{:02X}", peek_buffer[0]);
        // If not, read the packet fully into the buffer and continue
        let mut packet_buffer = vec![0u8; *packet_length as usize];
        client_connection.read_exact(&mut packet_buffer).await?;
        packet_length.write(&mut serverbound_buffer).await?;
        serverbound_buffer.extend(packet_buffer);
    }
}
