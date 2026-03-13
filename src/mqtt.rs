use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{Context, Result};
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use crate::config::MqttConfig;
use crate::modbus::Value;

/// Wrapper around an async MQTT client with background event loop.
pub struct MqttConnection {
    client: AsyncClient,
    _event_loop_handle: JoinHandle<()>,
}

impl MqttConnection {
    /// Create a new MQTT connection and start the background event loop.
    pub fn connect(cfg: &MqttConfig) -> Result<Self> {
        let mut opts = MqttOptions::new(&cfg.client_id, &cfg.host, cfg.port);
        opts.set_keep_alive(Duration::from_secs(60));

        if let Some(ref username) = cfg.username {
            opts.set_credentials(username, cfg.password.as_deref().unwrap_or(""));
        }

        let (client, mut event_loop) = AsyncClient::new(opts, 256);

        info!("Connecting to MQTT broker {}:{}…", cfg.host, cfg.port);

        // Spawn a background task that drives the MQTT event loop.
        // rumqttc handles reconnection automatically.
        let broker_host = cfg.host.clone();
        let broker_port = cfg.port;
        let handle = tokio::spawn(async move {
            loop {
                match event_loop.poll().await {
                    Ok(Event::Incoming(Packet::ConnAck(_))) => {
                        info!("Connected to MQTT broker {broker_host}:{broker_port}");
                    }
                    Ok(_) => {}
                    Err(e) => {
                        warn!("MQTT event loop error: {e}. Will auto-reconnect.");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        });

        Ok(Self {
            client,
            _event_loop_handle: handle,
        })
    }

    /// Recursively publish each value to its own MQTT topic (retained).
    ///
    /// Nested maps (if any) are flattened into sub-topic paths, matching
    /// the Python version's `publish_values()` behaviour.
    pub async fn publish_values(
        &self,
        base_topic: &str,
        values: &BTreeMap<String, Value>,
    ) -> Result<()> {
        for (key, value) in values {
            let topic = format!("{base_topic}/{key}");
            let payload = value.to_string();

            debug!("Publishing {topic} = {payload}");

            self.client
                .publish(&topic, QoS::AtLeastOnce, true, payload.as_bytes())
                .await
                .with_context(|| format!("publishing to {topic}"))?;
        }
        Ok(())
    }

    /// Disconnect from the MQTT broker.
    pub async fn disconnect(&self) -> Result<()> {
        self.client
            .disconnect()
            .await
            .context("MQTT disconnect")?;
        Ok(())
    }
}
