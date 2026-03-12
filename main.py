#!/usr/bin/env python3
"""SolarEdge Modbus → MQTT bridge.

Polls a SolarEdge inverter via Modbus TCP and publishes each metric
as an individual MQTT topic.
"""

import argparse
import json
import logging
import signal
import sys
import time
from pathlib import Path

import paho.mqtt.client as mqtt
import solaredge_modbus
import yaml

# ---------------------------------------------------------------------------
# Logging
# ---------------------------------------------------------------------------
logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] %(message)s",
)
log = logging.getLogger("solaredge_mqtt")

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

_STOP = False


def _handle_signal(signum, _frame):
    global _STOP
    log.info("Received signal %s – shutting down …", signum)
    _STOP = True


def load_config(path: str) -> dict:
    """Load YAML configuration file and return as dict."""
    with open(path, "r") as fh:
        cfg = yaml.safe_load(fh)
    # Apply defaults
    cfg.setdefault("modbus", {})
    cfg["modbus"].setdefault("host", "10.2.0.11")
    cfg["modbus"].setdefault("port", 1502)
    cfg["modbus"].setdefault("unit", 1)
    cfg["modbus"].setdefault("timeout", 5)

    cfg.setdefault("mqtt", {})
    cfg["mqtt"].setdefault("host", "localhost")
    cfg["mqtt"].setdefault("port", 1883)
    cfg["mqtt"].setdefault("username", None)
    cfg["mqtt"].setdefault("password", None)
    cfg["mqtt"].setdefault("base_topic", "solaredge/inverter")
    cfg["mqtt"].setdefault("client_id", "solaredge_mqtt")

    cfg.setdefault("poll_interval_seconds", 10)
    return cfg


# ---------------------------------------------------------------------------
# MQTT
# ---------------------------------------------------------------------------

def create_mqtt_client(cfg: dict) -> mqtt.Client:
    """Create, configure and connect an MQTT client."""
    client = mqtt.Client(client_id=cfg["client_id"])
    if cfg.get("username"):
        client.username_pw_set(cfg["username"], cfg.get("password"))

    def on_connect(client, _userdata, _flags, rc):
        if rc == 0:
            log.info("Connected to MQTT broker %s:%s", cfg["host"], cfg["port"])
        else:
            log.error("MQTT connection failed with code %s", rc)

    def on_disconnect(client, _userdata, rc):
        if rc != 0:
            log.warning("Unexpected MQTT disconnect (rc=%s). Will auto-reconnect.", rc)

    client.on_connect = on_connect
    client.on_disconnect = on_disconnect
    client.reconnect_delay_set(min_delay=1, max_delay=60)
    client.connect(cfg["host"], cfg["port"], keepalive=60)
    client.loop_start()
    return client


def publish_values(client: mqtt.Client, base_topic: str, values: dict, prefix: str = ""):
    """Recursively publish each value to its own MQTT topic.

    Nested dicts (e.g. meters, batteries) are flattened into sub-topics.
    """
    for key, value in values.items():
        topic = f"{base_topic}/{prefix}{key}" if prefix else f"{base_topic}/{key}"
        if isinstance(value, dict):
            publish_values(client, base_topic, value, prefix=f"{prefix}{key}/")
        else:
            payload = json.dumps(value) if not isinstance(value, (int, float, str)) else str(value)
            client.publish(topic, payload, retain=True)


# ---------------------------------------------------------------------------
# Scaling
# ---------------------------------------------------------------------------

# Maps each scale factor key to the list of value keys it applies to.
SCALE_FACTOR_MAP = {
    "current_scale": ["current", "l1_current", "l2_current", "l3_current"],
    "voltage_scale": [
        "l1_voltage", "l2_voltage", "l3_voltage",
        "l1n_voltage", "l2n_voltage", "l3n_voltage",
    ],
    "power_ac_scale": ["power_ac"],
    "frequency_scale": ["frequency"],
    "power_apparent_scale": ["power_apparent"],
    "power_reactive_scale": ["power_reactive"],
    "power_factor_scale": ["power_factor"],
    "energy_total_scale": ["energy_total"],
    "current_dc_scale": ["current_dc"],
    "voltage_dc_scale": ["voltage_dc"],
    "power_dc_scale": ["power_dc"],
    "temperature_scale": ["temperature"],
}


def apply_scale_factors(values: dict) -> dict:
    """Apply SunSpec scale factors to raw register values.

    Each scaled value is computed as: real_value = raw_value × 10^(scale_factor)
    Scale factor keys are removed from the returned dict.
    """
    scaled = dict(values)

    for scale_key, value_keys in SCALE_FACTOR_MAP.items():
        if scale_key not in scaled:
            continue
        scale = scaled[scale_key]
        if not isinstance(scale, (int, float)) or scale == 0x8000:
            # Scale factor not implemented / invalid – skip
            continue
        factor = 10 ** scale
        for vk in value_keys:
            if vk in scaled and isinstance(scaled[vk], (int, float)):
                scaled[vk] = round(scaled[vk] * factor, 6)
        del scaled[scale_key]

    return scaled


# ---------------------------------------------------------------------------
# Modbus
# ---------------------------------------------------------------------------

def read_inverter(cfg: dict) -> dict:
    """Read all values from the SolarEdge inverter via Modbus TCP."""
    inverter = solaredge_modbus.Inverter(
        host=cfg["host"],
        port=cfg["port"],
        timeout=cfg["timeout"],
        unit=cfg["unit"],
    )
    values = inverter.read_all()
    return values


# ---------------------------------------------------------------------------
# Main loop
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(description="SolarEdge Modbus → MQTT bridge")
    parser.add_argument(
        "-c", "--config",
        default="config.yaml",
        help="Path to YAML configuration file (default: config.yaml)",
    )
    parser.add_argument(
        "-v", "--verbose",
        action="store_true",
        help="Enable debug logging",
    )
    args = parser.parse_args()

    if args.verbose:
        logging.getLogger().setLevel(logging.DEBUG)

    # Load configuration
    config_path = Path(args.config)
    if not config_path.exists():
        log.error("Configuration file not found: %s", config_path)
        sys.exit(1)

    cfg = load_config(str(config_path))
    log.info(
        "Configuration loaded – Modbus %s:%s, MQTT %s:%s, poll every %ss",
        cfg["modbus"]["host"],
        cfg["modbus"]["port"],
        cfg["mqtt"]["host"],
        cfg["mqtt"]["port"],
        cfg["poll_interval_seconds"],
    )

    # Handle graceful shutdown
    signal.signal(signal.SIGINT, _handle_signal)
    signal.signal(signal.SIGTERM, _handle_signal)

    # Connect MQTT
    mqtt_client = create_mqtt_client(cfg["mqtt"])

    # Main polling loop
    consecutive_errors = 0
    max_backoff = 60

    while not _STOP:
        try:
            log.debug("Polling inverter …")
            values = read_inverter(cfg["modbus"])
            consecutive_errors = 0

            # Apply scale factors so published values are in real-world units
            values = apply_scale_factors(values)

            # Add a timestamp
            values["_timestamp"] = time.strftime("%Y-%m-%dT%H:%M:%S%z")

            log.debug("Publishing %d values …", len(values))
            publish_values(mqtt_client, cfg["mqtt"]["base_topic"], values)
            log.info("Published %d metrics to MQTT", len(values))

        except Exception:
            consecutive_errors += 1
            backoff = min(2 ** consecutive_errors, max_backoff)
            log.exception("Error reading inverter (attempt #%d). Retrying in %ds …", consecutive_errors, backoff)
            time.sleep(backoff)
            continue

        # Sleep until next poll (interruptible)
        deadline = time.monotonic() + cfg["poll_interval_seconds"]
        while not _STOP and time.monotonic() < deadline:
            time.sleep(0.5)

    # Cleanup
    log.info("Shutting down …")
    mqtt_client.loop_stop()
    mqtt_client.disconnect()
    log.info("Done.")


if __name__ == "__main__":
    main()
