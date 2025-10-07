use serde::Deserialize;
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};
use tokio::{
    fs::{File, OpenOptions},
    io::{AsyncReadExt, AsyncWriteExt},
};
use toml_example::TomlExample;
use tracing::{info, level_filters::LevelFilter, trace};

#[derive(TomlExample, Deserialize)]
pub struct TomlConfig {
    /// The Address this proxy will try to listen to
    #[toml_example(default = "127.0.0.1:45565")]
    pub bind_address: SocketAddr,
    /// The Address this proxy will try to forward the traffic to
    #[toml_example(default = "127.0.0.1:35565")]
    pub backend_address: SocketAddr,
    /// The Velocity forwarding secret, alternatively you can set the FORWARDING_SECRET environment variable
    pub forwarding_secret: String,
    /// The trusted addresses that are allowed to connect, keep this empty to allow all connections
    #[toml_example(default = [])]
    pub trusted_addresses: Vec<SocketAddr>,
    /// The logging verbosity of this proxy, it can be one of: "off", "error", "warn", "info", "debug" or "trace"
    #[toml_example(default = "info")]
    pub log_level: ConfigLevelFilter,
}

impl TomlConfig {
    pub async fn at_location(location: &Path) -> Result<Self, ConfigError> {
        let display_location = location.display();

        if !location.exists() {
            info!("Could not find config at {display_location}, creating default config");

            let mut file = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(location)
                .await
                .map_err(ConfigError::Creation)?;

            file.write_all(TomlConfig::toml_example().as_bytes())
                .await
                .map_err(ConfigError::Write)?;

            return Err(ConfigError::CreatedNew(location.to_path_buf()));
        }

        trace!("Loading config at {display_location}");
        let mut contents = String::new();
        File::open(location)
            .await
            .map_err(ConfigError::Read)?
            .read_to_string(&mut contents)
            .await
            .map_err(ConfigError::Read)?;

        trace!("Trying to parse config");
        let mut config = toml::from_str::<TomlConfig>(&contents).map_err(ConfigError::Parse)?;

        config.forwarding_secret = if config.forwarding_secret.is_empty() {
            trace!("Using FORWARDING_SECRET from environment");
            std::env::var("FORWARDING_SECRET").map_err(|_| ConfigError::NoSecret)?
        } else {
            config.forwarding_secret
        };

        Ok(config)
    }
}

pub enum ConfigError {
    Creation(tokio::io::Error),
    Read(tokio::io::Error),
    Write(tokio::io::Error),
    Parse(toml::de::Error),
    NoSecret,
    CreatedNew(PathBuf),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Creation(e) => write!(f, "Failed while creating config file: {e}"),
            ConfigError::Read(e) => write!(f, "Failed while reading config file: {e}"),
            ConfigError::Write(e) => write!(f, "Failed while writing to config file: {e}"),
            ConfigError::Parse(e) => write!(f, "Failed while parsing config file: {e}"),
            ConfigError::NoSecret => write!(
                f,
                "No forwarding secret provided, please set it in the config or in the FORWARDING_SECRET environment variable"
            ),
            ConfigError::CreatedNew(path) => write!(
                f,
                "Created new config file at \"{}\", please edit it and restart the proxy",
                path.canonicalize()
                    .expect("Failed to get canonicalized path of config file")
                    .display()
            ),
        }
    }
}

// A Wrapper around LevelFilter for deserializing
pub enum ConfigLevelFilter {
    Off,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl<'de> Deserialize<'de> for ConfigLevelFilter {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct CaseInsensitiveVisitor;

        impl<'de> serde::de::Visitor<'de> for CaseInsensitiveVisitor {
            type Value = ConfigLevelFilter;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a string matching any case-insensitive variant of MyEnum")
            }

            fn visit_str<E>(self, value: &str) -> Result<ConfigLevelFilter, E>
            where
                E: serde::de::Error,
            {
                match value.to_lowercase().as_str() {
                    "off" => Ok(ConfigLevelFilter::Off),
                    "error" => Ok(ConfigLevelFilter::Error),
                    "warn" => Ok(ConfigLevelFilter::Warn),
                    "info" => Ok(ConfigLevelFilter::Info),
                    "debug" => Ok(ConfigLevelFilter::Debug),
                    "trace" => Ok(ConfigLevelFilter::Trace),
                    _ => Err(serde::de::Error::unknown_variant(
                        value,
                        &["off", "error", "warn", "info", "debug", "trace"],
                    )),
                }
            }
        }

        deserializer.deserialize_str(CaseInsensitiveVisitor)
    }
}

impl From<ConfigLevelFilter> for LevelFilter {
    fn from(level: ConfigLevelFilter) -> Self {
        match level {
            ConfigLevelFilter::Off => LevelFilter::OFF,
            ConfigLevelFilter::Error => LevelFilter::ERROR,
            ConfigLevelFilter::Warn => LevelFilter::WARN,
            ConfigLevelFilter::Info => LevelFilter::INFO,
            ConfigLevelFilter::Debug => LevelFilter::DEBUG,
            ConfigLevelFilter::Trace => LevelFilter::TRACE,
        }
    }
}
