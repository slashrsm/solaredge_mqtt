use std::collections::BTreeMap;

use crate::modbus::Value;

/// Maps each scale-factor key to the value keys it applies to.
///
/// This is a direct port of `SCALE_FACTOR_MAP` from the Python version.
static SCALE_FACTOR_MAP: &[(&str, &[&str])] = &[
    (
        "current_scale",
        &["current", "l1_current", "l2_current", "l3_current"],
    ),
    (
        "voltage_scale",
        &[
            "l1_voltage",
            "l2_voltage",
            "l3_voltage",
            "l1n_voltage",
            "l2n_voltage",
            "l3n_voltage",
        ],
    ),
    ("power_ac_scale", &["power_ac"]),
    ("frequency_scale", &["frequency"]),
    ("power_apparent_scale", &["power_apparent"]),
    ("power_reactive_scale", &["power_reactive"]),
    ("power_factor_scale", &["power_factor"]),
    ("energy_total_scale", &["energy_total"]),
    ("current_dc_scale", &["current_dc"]),
    ("voltage_dc_scale", &["voltage_dc"]),
    ("power_dc_scale", &["power_dc"]),
    ("temperature_scale", &["temperature"]),
];

/// Extract the numeric (integer) value from a `Value`, if possible.
fn as_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Int(n) => Some(*n),
        Value::Float(f) => Some(*f as i64),
        _ => None,
    }
}

/// Extract a numeric value as f64.
fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Int(n) => Some(*n as f64),
        Value::Float(f) => Some(*f),
        _ => None,
    }
}

/// Apply SunSpec scale factors to raw register values.
///
/// Each scaled value is computed as:
///   `real_value = raw_value × 10^(scale_factor)`
///
/// Scale-factor keys are removed from the returned map so they are not
/// published to MQTT.
pub fn apply_scale_factors(values: &mut BTreeMap<String, Value>) {
    for &(scale_key, value_keys) in SCALE_FACTOR_MAP {
        // Look up the scale factor; skip if absent or sentinel.
        let scale = match values.get(scale_key).and_then(as_i64) {
            Some(s) if s != 0x8000_i64 && s != -32768_i64 => s,
            _ => continue,
        };

        let factor = 10_f64.powi(scale as i32);

        for &vk in value_keys {
            if let Some(raw) = values.get(vk).and_then(as_f64) {
                let scaled = (raw * factor * 1_000_000.0).round() / 1_000_000.0;
                values.insert(vk.to_string(), Value::Float(scaled));
            }
        }

        // Remove the scale factor key so it is not published.
        values.remove(scale_key);
    }
}
