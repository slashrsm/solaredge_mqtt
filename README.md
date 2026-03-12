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
`base_topic`. Scale factors are automatically applied before publishing,
so all numeric values are in real-world units (watts, volts, amps, etc.).
Scale factor topics (e.g. `*_scale`) are not published. For example:

| Topic | Example value | Unit |
|---|---|---|
| `solaredge/inverter/power_ac` | `1557.4` | W |
| `solaredge/inverter/temperature` | `35.01` | °C |
| `solaredge/inverter/energy_total` | `1858075.6` | Wh |
| `solaredge/inverter/l1n_voltage` | `230.5` | V |
| `solaredge/inverter/current` | `6.77` | A |
| `solaredge/inverter/frequency` | `50.01` | Hz |
| `solaredge/inverter/_timestamp` | `2026-12-03T11:30:00+0100` | — |

All messages are published with the **retain** flag so the last known value
is always available to new subscribers.

## Command-line options

```
usage: main.py [-h] [-c CONFIG] [-v]

  -c, --config CONFIG   Path to YAML config file (default: config.yaml)
  -v, --verbose         Enable debug logging
```

## Running with Docker

```bash
# Build the image
docker build -t solaredge-mqtt .

# Run (mount your config file)
docker run -d --name solaredge-mqtt \
  -v ./config.yaml:/app/config.yaml \
  solaredge-mqtt

# Run in debug mode (verbose logging)
docker run -d --name solaredge-mqtt \
  -e DEBUG=1 \
  -v ./config.yaml:/app/config.yaml \
  solaredge-mqtt

# View logs
docker logs -f solaredge-mqtt
```

## Home Assistant integration

An example Home Assistant configuration is provided in
[`homeassistant.example.yaml`](homeassistant.example.yaml). It includes:

- **MQTT sensors** for every inverter metric with proper `device_class`,
  `unit_of_measurement`, and `state_class` (values are pre-scaled by the
  bridge, so no template math is needed)
- Human-readable `value_template` mappings for status and configuration
  registers
- All sensors grouped under a single **SolarEdge Inverter** device in HA

The file is structured as a Home Assistant
[package](https://www.home-assistant.io/docs/configuration/packages/), so
you can include it directly without copying anything into
`configuration.yaml`:

```yaml
# In your configuration.yaml
homeassistant:
  packages:
    solaredge: !include solaredge.yaml
```

1. Copy `homeassistant.example.yaml` into your HA config directory
   (e.g. as `solaredge.yaml`)
2. Add the `packages:` lines above to your `configuration.yaml`
3. If you changed `mqtt.base_topic` in `config.yaml`, update all
   `state_topic` values in `solaredge.yaml` to match
4. Restart Home Assistant

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
