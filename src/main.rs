//! SolarEdge Modbus → MQTT bridge.
//!
//! Polls a SolarEdge inverter via Modbus TCP and publishes each metric
//! as an individual retained MQTT topic.

mod config;
mod modbus;
mod mqtt;
mod scale;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use chrono::Local;
use clap::Parser;
use tracing::{error, info, warn};

use config::load_config;
use modbus::{InverterConnection, Value};
use mqtt::MqttConnection;
use scale::apply_scale_factors;

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser, Debug)]
#[command(about = "SolarEdge Modbus → MQTT bridge")]
struct Cli {
    /// Path to YAML configuration file.
    #[arg(short, long, default_value = "config.yaml")]
    config: PathBuf,

    /// Enable debug logging.
    #[arg(short, long)]
    verbose: bool,
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialise logging.
    let level = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| level.into()),
        )
        .init();

    // Load configuration.
    let cfg = load_config(&cli.config)?;
    info!(
        "Configuration loaded – Modbus {}:{}, MQTT {}:{}, poll every {}s",
        cfg.modbus.host,
        cfg.modbus.port,
        cfg.mqtt.host,
        cfg.mqtt.port,
        cfg.poll_interval_seconds,
    );

    // Connect to MQTT broker.
    let mqtt_conn = MqttConnection::connect(&cfg.mqtt)?;

    // Connect to inverter (persistent connection).
    let mut inverter = InverterConnection::connect(&cfg.modbus).await?;

    // Main polling loop.
    let mut consecutive_errors: u32 = 0;
    let max_backoff = Duration::from_secs(60);
    let poll_interval = Duration::from_secs(cfg.poll_interval_seconds);

    loop {
        // Check for shutdown signal (Ctrl-C / SIGTERM).
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("Received shutdown signal – shutting down…");
                break;
            }
            result = inverter.read_all() => {
                match result {
                    Ok(mut values) => {
                        consecutive_errors = 0;

                        // Apply SunSpec scale factors.
                        apply_scale_factors(&mut values);

                        // Add a timestamp (ISO 8601 with timezone offset).
                        let ts = Local::now().format("%Y-%m-%dT%H:%M:%S%z").to_string();
                        values.insert("_timestamp".to_string(), Value::Str(ts));

                        let count = values.len();

                        if let Err(e) = mqtt_conn.publish_values(&cfg.mqtt.base_topic, &values).await {
                            warn!("Failed to publish values: {e:#}");
                        } else {
                            info!("Published {count} metrics to MQTT");
                        }
                    }
                    Err(e) => {
                        consecutive_errors += 1;
                        let backoff = Duration::from_secs(2u64.pow(consecutive_errors)).min(max_backoff);
                        error!(
                            "Error reading inverter (attempt #{consecutive_errors}): {e:#}. Retrying in {backoff:?}…"
                        );

                        // Try to reconnect before next attempt.
                        if let Err(re) = inverter.reconnect(&cfg.modbus).await {
                            warn!("Reconnect failed: {re:#}");
                        }

                        tokio::time::sleep(backoff).await;
                        continue;
                    }
                }
            }
        }

        // Sleep until next poll, but remain interruptible by Ctrl-C.
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("Received shutdown signal – shutting down…");
                break;
            }
            _ = tokio::time::sleep(poll_interval) => {}
        }
    }

    // Graceful shutdown.
    info!("Shutting down…");
    if let Err(e) = mqtt_conn.disconnect().await {
        warn!("MQTT disconnect error: {e:#}");
    }
    info!("Done.");

    Ok(())
}
