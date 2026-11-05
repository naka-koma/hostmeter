//! The shapes of the HTTP contract (docs/contract.md, version 1).

use serde::Serialize;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Process {
    pub pid: u32,
    pub name: String,
    pub memory_mib: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Memory {
    pub used_mib: u64,
    pub total_mib: u64,
}

/// One adapter as the latest sample reports it.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Adapter {
    pub index: u32,
    pub name: String,
    pub utilization_percent: Option<u32>,
    pub memory: Memory,
    pub temperature_c: Option<i32>,
    pub power_watts: Option<f64>,
    pub processes: Vec<Process>,
}

/// One reading of the machine: what the driver says about each adapter, and the per-process video memory.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub at: time::OffsetDateTime,
    pub adapters: Vec<Adapter>,
}

#[derive(Serialize)]
pub struct Latest<'a> {
    pub schema: u32,
    pub sampled_at: String,
    pub interval_ms: u64,
    pub available: bool,
    pub adapters: &'a [Adapter],
}

#[derive(Serialize)]
pub struct HistoryAdapter<'a> {
    pub index: u32,
    pub utilization_percent: Option<u32>,
    pub memory_used_mib: u64,
    pub temperature_c: Option<i32>,
    pub power_watts: Option<f64>,
    pub processes: &'a [Process],
}

#[derive(Serialize)]
pub struct HistorySample<'a> {
    pub t: String,
    pub adapters: Vec<HistoryAdapter<'a>>,
}

#[derive(Serialize)]
pub struct History<'a> {
    pub schema: u32,
    pub interval_ms: u64,
    pub retention_seconds: u64,
    pub samples: Vec<HistorySample<'a>>,
}

pub fn rfc3339(at: time::OffsetDateTime) -> String {
    at.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}
