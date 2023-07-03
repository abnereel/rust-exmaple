use std::net::SocketAddr;

use serde::{Deserialize, Serialize};
use tokio::fs;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ServerConfig {
    pub network: NetworkConfig,
    pub params: ParamsConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ParamsConfig {
    pub min_size: u32,
    pub max_size: u32,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".into(),
            port: 3000,
        }
    }
}

impl From<NetworkConfig> for SocketAddr {
    fn from(value: NetworkConfig) -> Self {
        format!("{}:{}", value.host, value.port).parse().unwrap()
    }
}

impl Default for ParamsConfig {
    fn default() -> Self {
        Self {
            max_size: 10,
            min_size: 3,
        }
    }
}

impl ServerConfig {
    pub async fn load() -> Self {
        if let Ok(content) = fs::read_to_string("fixtures/config.yml").await {
            serde_yaml::from_str(&content).unwrap()
        } else {
            Self::default()
        }
    }
}
