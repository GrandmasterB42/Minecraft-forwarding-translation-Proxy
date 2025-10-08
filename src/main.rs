use std::{net::SocketAddr, path::Path, sync::Arc};

use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
};

use tracing::{
    Instrument, Level, debug, error, info, level_filters::LevelFilter, span, trace, warn,
};
use tracing_subscriber::{fmt, layer::SubscriberExt, reload, util::SubscriberInitExt};

use crate::{
    config::{ConfigError, TomlConfig},
    packets::{
        Disconnect, GenericPacket, Handshake, InterpretError, LoginStart, ReadPacketExt,
        VelocityLoginPluginRequest, VelocityLoginPluginResponse, WritePacketExt,
    },
};

mod config;
mod packets;
mod types;

static CONFIG_PATH: &str = "Config.toml";

// TODO: Investigate something like https://github.com/belohnung/minecraft-varint/tree/master for varint decoding
// TODO: Don't forget logging and ctrl-c handling
#[tokio::main]
async fn main() {
    // Setup all the logging
    let default_filter = if cfg!(debug_assertions) {
        LevelFilter::TRACE
    } else {
        LevelFilter::INFO
    };

    let (filter, reload_handle) = reload::Layer::new(default_filter);
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::Layer::default())
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

    trace!("Now updating the log level according to the config");
    if let Err(e) = reload_handle.modify(|filter| *filter = config.log_level.into()) {
        error!("Failed to update log level");
        debug!("Error: {e}");
        return;
    };

    // Start listening for clients
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
    while let Ok((mut client_connection, client_adress)) = client_listener.accept().await {
        let connection_span = span!(Level::TRACE, "client_connection", client = %client_adress);
        trace!(parent: &connection_span, "New client connection from {client_adress}");

        // Reject untrusted connections
        if !config.trusted_ips.is_empty() && !config.trusted_ips.contains(&client_adress.ip()) {
            warn!(parent: &connection_span, "Rejecting connection from untrusted address {client_adress}");
            let handshake = match client_connection.read_packet::<Handshake>().await {
                Ok(handshake) => handshake,
                Err(e) => {
                    error!("Failed to read handshake from client {client_adress}: {e}");
                    return;
                }
            };
            match *handshake.next_state {
                1 => trace!(
                    "Rejecting untrusted connection from {client_adress} for a status request"
                ),
                2 => {
                    warn!(
                        "Rejecting untrusted connection from {client_adress} for a login request"
                    );

                    if let Err(e) = client_connection
                        .write_packet(&Disconnect::reason(
                            "You are not allowed to connect to this server directly!",
                        ))
                        .await
                    {
                        warn!(
                            "Failed to send disconnect packet to untrusted client {client_adress}"
                        );
                        debug!("Error: {e}");
                        return;
                    };
                }
                3 => warn!(
                    "Rejecting untrusted connection from {client_adress} for a transfer request"
                ),
                _ => (),
            }
            client_connection.shutdown().await.ok();
            continue;
        }

        // Create a backend connection for this connection
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
                config.forwarding_secret.clone(),
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
    secret: Arc<str>,
) {
    // TODO: Support old handshakes, is this necessary?
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
                warn!("Failed to read login start packet from client");
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
                    warn!("Failed to read login plugin response from client {client_adress}: {e}");
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
            if let Err(e) =
                tokio::io::copy_bidirectional(&mut client_connection, &mut backend_connection).await
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

async fn buffer_until_response(
    client_connection: &mut TcpStream,
) -> tokio::io::Result<(Vec<GenericPacket>, VelocityLoginPluginResponse)> {
    let mut serverbound_buffer = Vec::new();
    loop {
        trace!("Reading next packet from client while waiting for login plugin response");
        let packet = client_connection.read_packet::<GenericPacket>().await?;

        trace!("Now looking if it was the login plugin response");
        match packet
            .try_interpret_as::<VelocityLoginPluginResponse>()
            .await
        {
            Ok(response) => {
                trace!("Found login plugin response packet");
                return Ok((serverbound_buffer, response));
            }
            Err(e) => match e {
                InterpretError::PacketIdMismatch(id) => {
                    trace!(
                        "Buffering packet with id {id:x} while waiting for login plugin response"
                    );
                    serverbound_buffer.push(packet);
                }
                InterpretError::IoError(e) => {
                    return Err(e);
                }
                _ => {
                    warn!(
                        "Encountered unexpected error while trying to read Velocity Plugin Response packet: {e:?}, continuing anyway"
                    );
                    continue;
                }
            },
        };
    }
}
