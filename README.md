# SolarEdge Modbus → MQTT Bridge

Polls a SolarEdge inverter via Modbus TCP and publishes each metric as an
individual MQTT topic (e.g. `solaredge/inverter/power_ac`,
`solaredge/inverter/temperature`, etc.).

## Quick start

```bash
# 1. Create a virtual environment
python3 -m venv env
source env/bin/activate

# 2. Install dependencies
pip install -r requirements.txt

# 3. Copy and edit configuration
cp config.example.yaml config.yaml
# Edit config.yaml with your values

# 4. Run
python main.py
```

## Configuration

All settings live in `config.yaml` (default path, override with `-c`):

```yaml
modbus:
  host: 10.2.0.11      # Inverter IP
  port: 1502            # Modbus TCP port
  unit: 1               # Modbus unit/slave ID
  timeout: 5            # Connection timeout (seconds)

mqtt:
  host: 10.2.0.29       # MQTT broker IP
  port: 1883            # MQTT broker port
  username: admin        # MQTT username (optional)
  password: "secret"     # MQTT password (optional)
  base_topic: solaredge/inverter   # Base MQTT topic
  client_id: solaredge_mqtt        # MQTT client ID

poll_interval_seconds: 10   # How often to poll the inverter
```

## MQTT topics

Each inverter metric is published to its own topic under the configured
`base_topic`. For example:

| Topic | Example value |
|---|---|
| `solaredge/inverter/power_ac` | `15574` |
| `solaredge/inverter/power_ac_scale` | `-1` |
| `solaredge/inverter/temperature` | `3501` |
| `solaredge/inverter/energy_total` | `18580756` |
| `solaredge/inverter/_timestamp` | `2026-12-03T11:30:00+0100` |

All messages are published with the **retain** flag so the last known value
is always available to new subscribers.

## Command-line options

```
usage: main.py [-h] [-c CONFIG] [-v]

  -c, --config CONFIG   Path to YAML config file (default: config.yaml)
  -v, --verbose         Enable debug logging
```

## Running as a service

Create a systemd unit (Linux) or launchd plist (macOS) to keep the bridge
running in the background. Example systemd unit:

```ini
[Unit]
Description=SolarEdge MQTT Bridge
After=network.target

[Service]
Type=simple
User=solaredge
WorkingDirectory=/opt/solaredge_mqtt
ExecStart=/opt/solaredge_mqtt/env/bin/python main.py
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```
