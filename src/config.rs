use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

/// Top-level configuration, loaded from a YAML file.
#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub modbus: ModbusConfig,
    #[serde(default)]
    pub mqtt: MqttConfig,
    #[serde(default = "default_poll_interval")]
    pub poll_interval_seconds: u64,
}

#[derive(Debug, Deserialize)]
pub struct ModbusConfig {
    #[serde(default = "default_modbus_host")]
    pub host: String,
    #[serde(default = "default_modbus_port")]
    pub port: u16,
    #[serde(default = "default_modbus_unit")]
    pub unit: u8,
    #[serde(default = "default_modbus_timeout")]
    pub timeout: u64,
}

#[derive(Debug, Deserialize)]
pub struct MqttConfig {
    #[serde(default = "default_mqtt_host")]
    pub host: String,
    #[serde(default = "default_mqtt_port")]
    pub port: u16,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default = "default_base_topic")]
    pub base_topic: String,
    #[serde(default = "default_client_id")]
    pub client_id: String,
}

// --- Defaults (matching the Python version) ---

fn default_modbus_host() -> String {
    "10.2.0.11".into()
}
fn default_modbus_port() -> u16 {
    1502
}
fn default_modbus_unit() -> u8 {
    1
}
fn default_modbus_timeout() -> u64 {
    5
}

fn default_mqtt_host() -> String {
    "localhost".into()
}
fn default_mqtt_port() -> u16 {
    1883
}
fn default_base_topic() -> String {
    "solaredge/inverter".into()
}
fn default_client_id() -> String {
    "solaredge_mqtt".into()
}

fn default_poll_interval() -> u64 {
    10
}

impl Default for ModbusConfig {
    fn default() -> Self {
        Self {
            host: default_modbus_host(),
            port: default_modbus_port(),
            unit: default_modbus_unit(),
            timeout: default_modbus_timeout(),
        }
    }
}

impl Default for MqttConfig {
    fn default() -> Self {
        Self {
            host: default_mqtt_host(),
            port: default_mqtt_port(),
            username: None,
            password: None,
            base_topic: default_base_topic(),
            client_id: default_client_id(),
        }
    }
}

/// Load configuration from a YAML file.
pub fn load_config(path: &Path) -> Result<Config> {
    let content =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let cfg: Config =
        serde_yaml::from_str(&content).with_context(|| format!("parsing {}", path.display()))?;
    Ok(cfg)
}
