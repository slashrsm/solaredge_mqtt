use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio_modbus::client::Reader;
use tokio_modbus::prelude::*;
use tracing::{debug, info};

use crate::config::ModbusConfig;

// ---------------------------------------------------------------------------
// Value type – mirrors what the Python version publishes
// ---------------------------------------------------------------------------

/// A value read from the inverter.
#[derive(Debug, Clone)]
pub enum Value {
    Str(String),
    Int(i64),
    Float(f64),
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Str(s) => write!(f, "{s}"),
            Value::Int(n) => write!(f, "{n}"),
            Value::Float(v) => write!(f, "{v}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Register data types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
enum DataType {
    /// UTF-8 string spanning `len` registers (2 bytes per register).
    String,
    Uint16,
    Int16,
    Uint32,
    Acc32,
    Int32,
    Uint64,
    Float32,
}

// ---------------------------------------------------------------------------
// Register definitions
// ---------------------------------------------------------------------------

/// A single register definition (name, start address, length in registers,
/// data type).
struct RegDef {
    name: &'static str,
    addr: u16,
    len: u16,
    dtype: DataType,
}

// Macro to keep the table compact.
macro_rules! r {
    ($name:expr, $addr:expr, $len:expr, $dt:ident) => {
        RegDef {
            name: $name,
            addr: $addr,
            len: $len,
            dtype: DataType::$dt,
        }
    };
}

// Register map taken from solaredge_modbus 0.8.0 (Inverter class).
// Addresses are the raw hex values used by the library.

/// Batch 1 – Common Block (SunSpec header + device info)
static BATCH1: &[RegDef] = &[
    r!("c_id", 0x9c40, 2, String),
    r!("c_did", 0x9c42, 1, Uint16),
    r!("c_length", 0x9c43, 1, Uint16),
    r!("c_manufacturer", 0x9c44, 16, String),
    r!("c_model", 0x9c54, 16, String),
    // Note: gap between c_model end (0x9c64) and c_version (0x9c6c) – 8 registers of padding/options
    r!("c_version", 0x9c6c, 8, String),
    r!("c_serialnumber", 0x9c74, 16, String),
    r!("c_deviceaddress", 0x9c84, 1, Uint16),
];

/// Batch 2 – Inverter Block (measurements + status)
static BATCH2: &[RegDef] = &[
    r!("c_sunspec_did", 0x9c85, 1, Uint16),
    r!("c_sunspec_length", 0x9c86, 1, Uint16),
    // AC Current
    r!("current", 0x9c87, 1, Uint16),
    r!("l1_current", 0x9c88, 1, Uint16),
    r!("l2_current", 0x9c89, 1, Uint16),
    r!("l3_current", 0x9c8a, 1, Uint16),
    r!("current_scale", 0x9c8b, 1, Int16),
    // AC Voltage
    r!("l1_voltage", 0x9c8c, 1, Uint16),
    r!("l2_voltage", 0x9c8d, 1, Uint16),
    r!("l3_voltage", 0x9c8e, 1, Uint16),
    r!("l1n_voltage", 0x9c8f, 1, Uint16),
    r!("l2n_voltage", 0x9c90, 1, Uint16),
    r!("l3n_voltage", 0x9c91, 1, Uint16),
    r!("voltage_scale", 0x9c92, 1, Int16),
    // AC Power
    r!("power_ac", 0x9c93, 1, Int16),
    r!("power_ac_scale", 0x9c94, 1, Int16),
    // Frequency
    r!("frequency", 0x9c95, 1, Uint16),
    r!("frequency_scale", 0x9c96, 1, Int16),
    // Apparent / Reactive / Power Factor
    r!("power_apparent", 0x9c97, 1, Int16),
    r!("power_apparent_scale", 0x9c98, 1, Int16),
    r!("power_reactive", 0x9c99, 1, Int16),
    r!("power_reactive_scale", 0x9c9a, 1, Int16),
    r!("power_factor", 0x9c9b, 1, Int16),
    r!("power_factor_scale", 0x9c9c, 1, Int16),
    // Total Energy
    r!("energy_total", 0x9c9d, 2, Acc32),
    r!("energy_total_scale", 0x9c9f, 1, Int16),
    // DC
    r!("current_dc", 0x9ca0, 1, Uint16),
    r!("current_dc_scale", 0x9ca1, 1, Int16),
    r!("voltage_dc", 0x9ca2, 1, Uint16),
    r!("voltage_dc_scale", 0x9ca3, 1, Int16),
    r!("power_dc", 0x9ca4, 1, Int16),
    r!("power_dc_scale", 0x9ca5, 1, Int16),
    // Temperature (0x9ca7 – gap of 1 register after power_dc_scale)
    r!("temperature", 0x9ca7, 1, Int16),
    // Temperature scale (0x9caa – gap of 2 registers after temperature)
    r!("temperature_scale", 0x9caa, 1, Int16),
    // Status
    r!("status", 0x9cab, 1, Uint16),
    r!("vendor_status", 0x9cac, 1, Uint16),
];

/// Batch 3 – RRCR / Power Limit
static BATCH3: &[RegDef] = &[
    r!("rrcr_state", 0xf000, 1, Uint16),
    r!("active_power_limit", 0xf001, 1, Uint16),
    r!("cosphi", 0xf002, 2, Float32),
];

/// Batch 4 – Power Control Settings
static BATCH4: &[RegDef] = &[
    r!("commit_power_control_settings", 0xf100, 1, Int16),
    r!(
        "restore_power_control_default_settings",
        0xf101,
        1,
        Int16
    ),
    // gap: 0xf102 skipped
    r!("reactive_power_config", 0xf103, 2, Int32),
    r!("reactive_power_response_time", 0xf105, 2, Uint32),
    // large gap to 0xf142
    r!("advanced_power_control_enable", 0xf142, 2, Uint16),
];

/// Batch 5 – Export Control
static BATCH5: &[RegDef] = &[
    r!("export_control_mode", 0xf700, 1, Uint16),
    r!("export_control_limit_mode", 0xf701, 1, Uint16),
    r!("export_control_site_limit", 0xf702, 2, Float32),
];

/// All batches in order.
static BATCHES: &[&[RegDef]] = &[BATCH1, BATCH2, BATCH3, BATCH4, BATCH5];

// ---------------------------------------------------------------------------
// SunSpec "not implemented" sentinel detection
// ---------------------------------------------------------------------------

/// Returns `true` if the raw value represents "not implemented" for this type.
fn is_not_implemented(raw: &[u16], dtype: DataType) -> bool {
    match dtype {
        DataType::Uint16 => raw.first() == Some(&0xFFFF),
        DataType::Int16 => raw.first() == Some(&0x8000),
        DataType::Uint32 | DataType::Acc32 => {
            raw.len() >= 2 && raw[0] == 0xFFFF && raw[1] == 0xFFFF
        }
        DataType::Int32 => raw.len() >= 2 && raw[0] == 0x8000 && raw[1] == 0x0000,
        DataType::Uint64 => raw.len() >= 4 && raw.iter().all(|&r| r == 0xFFFF),
        DataType::Float32 => {
            if raw.len() >= 2 {
                let bits = ((raw[0] as u32) << 16) | (raw[1] as u32);
                bits == 0x7fc0_0000 || bits == 0xFFFF_FFFF || f32::from_bits(bits).is_nan()
            } else {
                true
            }
        }
        DataType::String => {
            // Empty / null-padded strings are treated as not-implemented
            // by the Python library, but we still publish them (as empty string)
            false
        }
    }
}

/// Decode a slice of registers into a `Value`.
fn decode(raw: &[u16], dtype: DataType) -> Value {
    match dtype {
        DataType::String => {
            let bytes: Vec<u8> = raw
                .iter()
                .flat_map(|r| r.to_be_bytes())
                .filter(|&b| b != 0)
                .collect();
            Value::Str(String::from_utf8_lossy(&bytes).trim().to_string())
        }
        DataType::Uint16 => Value::Int(raw[0] as i64),
        DataType::Int16 => Value::Int(raw[0] as i16 as i64),
        DataType::Uint32 | DataType::Acc32 => {
            let v = ((raw[0] as u32) << 16) | (raw[1] as u32);
            Value::Int(v as i64)
        }
        DataType::Int32 => {
            let v = ((raw[0] as u32) << 16) | (raw[1] as u32);
            Value::Int(v as i32 as i64)
        }
        DataType::Uint64 => {
            let v = ((raw[0] as u64) << 48)
                | ((raw[1] as u64) << 32)
                | ((raw[2] as u64) << 16)
                | (raw[3] as u64);
            Value::Int(v as i64)
        }
        DataType::Float32 => {
            let bits = ((raw[0] as u32) << 16) | (raw[1] as u32);
            Value::Float(f32::from_bits(bits) as f64)
        }
    }
}

// ---------------------------------------------------------------------------
// Persistent connection wrapper
// ---------------------------------------------------------------------------

/// A persistent Modbus TCP connection to a SolarEdge inverter.
pub struct InverterConnection {
    ctx: tokio_modbus::client::Context,
    addr: SocketAddr,
    slave: Slave,
}

impl InverterConnection {
    /// Open a new TCP connection to the inverter.
    pub async fn connect(cfg: &ModbusConfig) -> Result<Self> {
        let addr: SocketAddr = format!("{}:{}", cfg.host, cfg.port)
            .parse()
            .with_context(|| format!("invalid address {}:{}", cfg.host, cfg.port))?;
        let slave = Slave(cfg.unit);

        info!("Connecting to inverter at {addr} (unit {})…", cfg.unit);

        let timeout = Duration::from_secs(cfg.timeout);
        let ctx = tokio::time::timeout(
            timeout,
            tokio_modbus::client::tcp::connect_slave(addr, slave),
        )
        .await
        .with_context(|| format!("timeout connecting to {addr}"))?
        .with_context(|| format!("Modbus TCP connect to {addr}"))?;

        info!("Connected to inverter at {addr}");

        Ok(Self { ctx, addr, slave })
    }

    /// Reconnect (drops old connection, creates a new one).
    pub async fn reconnect(&mut self, cfg: &ModbusConfig) -> Result<()> {
        info!("Reconnecting to inverter at {}…", self.addr);

        let timeout = Duration::from_secs(cfg.timeout);
        let ctx = tokio::time::timeout(
            timeout,
            tokio_modbus::client::tcp::connect_slave(self.addr, self.slave),
        )
        .await
        .with_context(|| format!("timeout reconnecting to {}", self.addr))?
        .with_context(|| format!("Modbus TCP reconnect to {}", self.addr))?;

        self.ctx = ctx;
        info!("Reconnected to inverter at {}", self.addr);
        Ok(())
    }

    /// Read all inverter registers and return a flat map of name → value.
    ///
    /// This mirrors `inverter.read_all()` from the Python `solaredge_modbus`
    /// library: it reads five batches of holding registers (common block,
    /// inverter block, RRCR, power-control, export-control) and decodes
    /// every field, skipping any that report the SunSpec "not implemented"
    /// sentinel.
    pub async fn read_all(&mut self) -> Result<BTreeMap<String, Value>> {
        let mut values = BTreeMap::new();

        for (batch_idx, batch) in BATCHES.iter().enumerate() {
            match self.read_batch(batch).await {
                Ok(batch_values) => values.extend(batch_values),
                Err(e) => {
                    // Batches 3-5 (power control / export control) may not be
                    // available on all firmware versions – log and continue.
                    if batch_idx >= 2 {
                        debug!("Batch {batch_idx} read failed (non-critical): {e:#}");
                    } else {
                        return Err(e)
                            .with_context(|| format!("reading register batch {batch_idx}"));
                    }
                }
            }
        }

        Ok(values)
    }

    /// Read a single batch of contiguous-ish registers.
    async fn read_batch(&mut self, regs: &[RegDef]) -> Result<Vec<(String, Value)>> {
        if regs.is_empty() {
            return Ok(vec![]);
        }

        // Determine the address range to read.
        let addr_min = regs.first().unwrap().addr;
        let last = regs.last().unwrap();
        let addr_max = last.addr + last.len;
        let count = addr_max - addr_min;

        debug!(
            "Reading {} registers starting at 0x{:04x}",
            count, addr_min
        );

        let data = self
            .ctx
            .read_holding_registers(addr_min, count)
            .await
            .context("Modbus read_holding_registers I/O")?
            .context("Modbus read_holding_registers response")?;

        if data.len() < count as usize {
            bail!(
                "expected {} registers from 0x{:04x}, got {}",
                count,
                addr_min,
                data.len()
            );
        }

        let mut result = Vec::with_capacity(regs.len());

        for reg in regs {
            let offset = (reg.addr - addr_min) as usize;
            let end = offset + reg.len as usize;
            let raw = &data[offset..end];

            if is_not_implemented(raw, reg.dtype) {
                // The Python library returns `False` (→ "False" or 0) for
                // not-implemented values. We publish 0 for numeric types
                // and empty string for strings, matching the Python behaviour.
                let val = match reg.dtype {
                    DataType::String => Value::Str(String::new()),
                    DataType::Float32 => Value::Float(0.0),
                    _ => Value::Int(0),
                };
                result.push((reg.name.to_string(), val));
            } else {
                result.push((reg.name.to_string(), decode(raw, reg.dtype)));
            }
        }

        Ok(result)
    }
}
