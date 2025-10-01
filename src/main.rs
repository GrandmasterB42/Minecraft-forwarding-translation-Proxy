use std::net::SocketAddr;

use tokio::net::{TcpListener, TcpStream};
use tracing::{Instrument, Level, error, info, span, trace, warn};

use crate::packets::{Handshake, ReadPacket, WritePacket};

mod packets;
mod types;

// TODO: Investigate something like https://github.com/belohnung/minecraft-varint/tree/master for varint decoding
// TODO: Don't forget logging and ctrl-c handling
#[tokio::main]
async fn main() {
    // TODO: Adjust tracing level based on compilation profile
    tracing_subscriber::fmt::fmt()
        .with_max_level(tracing::Level::TRACE)
        .init();

    let backend_address = "127.0.0.1:35565";
    let listen_address = "127.0.0.1:45565";

    let client_listener = match TcpListener::bind(listen_address).await {
        Ok(listener) => {
            info!("Listening for client connections on {listen_address}");
            listener
        }
        Err(e) => {
            error!("Failed to bind to {listen_address}: {e}");
            return;
        }
    };

    // Wait for connections
    while let Ok((client_connection, client_adress)) = client_listener.accept().await {
        let connection_span = span!(Level::TRACE, "client_connection", client = %client_adress);

        // TODO: Make this configurable to only trust a certain adress
        trace!(parent: &connection_span, "New client connection from {client_adress}");

        let Ok(backend_connection) = TcpStream::connect(backend_address).await else {
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
            handle_connection(client_connection, client_adress, backend_connection)
                .instrument(connection_span),
        );
    }
}

async fn handle_connection(
    mut client_connection: TcpStream,
    client_adress: SocketAddr,
    mut backend_connection: TcpStream,
) {
    // TODO: Support old handshakes
    // First, read the handshake from the client
    let handshake = match client_connection.read_packet::<Handshake>().await {
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

            let Ok(_) = backend_connection.write_packet(handshake).await else {
                warn!("Failed to forward status handshake to backend");
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
