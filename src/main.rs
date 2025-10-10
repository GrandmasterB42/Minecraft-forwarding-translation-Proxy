use std::path::Path;

use tokio::net::{TcpListener, TcpStream};

use tokio_util::sync::CancellationToken;
use tracing::{
    Instrument, Level, debug, error, info, level_filters::LevelFilter, span, trace, warn,
};
use tracing_subscriber::{fmt, layer::SubscriberExt, reload, util::SubscriberInitExt};

use crate::{
    config::{ConfigError, TomlConfig},
    connection::{handle_connection, reject_untrusted},
};

mod config;
mod connection;
mod packets;
mod types;

static CONFIG_PATH: &str = "Config.toml";

// TODO: Investigate something like https://github.com/belohnung/minecraft-varint/tree/master for varint decoding
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

    // Setup shutdown signal
    let cancel = CancellationToken::new();
    tokio::spawn(shutdown_signal(cancel.clone()));

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
    loop {
        // Wait for cancellation or accept new connection
        let (mut client_connection, client_adress) = tokio::select! {
            _ = cancel.cancelled() => {
                trace!("Shutting down connection listener");
                break;
            }
            accept_result = client_listener.accept() => {
                match accept_result {
                    Ok((connection, adress)) => (connection, adress),
                    Err(e) => {
                        error!("Failed to accept new client connection: {e:?}");
                        continue;
                    }
                }
            }
        };

        let connection_span = span!(Level::TRACE, "client_connection", client = %client_adress);
        trace!(parent: &connection_span, "New client connection from {client_adress}");

        // Reject untrusted connections
        if !config.trusted_ips.is_empty() && !config.trusted_ips.contains(&client_adress.ip()) {
            warn!(parent: &connection_span, "Rejecting connection from untrusted address {client_adress}");
            reject_untrusted(&mut client_connection, client_adress).await;
            continue;
        }

        // Create a backend connection for this connection
        let Ok(backend_connection) = TcpStream::connect(config.backend_address).await else {
            error!(parent: &connection_span, "Failed to connect to backend server");
            continue;
        };

        // Packets should be forwarded immediately
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
                backend_connection,
                client_adress,
                connection_id,
                config.forwarding_secret.clone(),
                cancel.clone(),
            )
            .instrument(connection_span),
        );
        connection_id = connection_id.wrapping_add(1);
    }

    info!("Successfully shut down");
}

async fn shutdown_signal(cancel: CancellationToken) {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => (),
        _ = terminate => (),
    };

    info!("Starting to shut down...");

    cancel.cancel();
}
