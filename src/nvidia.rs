//! The driver's own view of each adapter, read with `nvidia-smi` (utilisation, memory, temperature, power).

use std::process::Command;

use crate::model::{Adapter, Memory};

const QUERY: &str = "index,name,utilization.gpu,memory.used,memory.total,temperature.gpu,power.draw";

/// `[N/A]`, `[Not Supported]` and an empty field are "not reported".
fn field<T: std::str::FromStr>(text: &str) -> Option<T> {
    let text = text.trim();
    if text.is_empty() || text.starts_with('[') {
        return None;
    }
    text.parse().ok()
}

/// Parses the output of `nvidia-smi --query-gpu=... --format=csv,noheader,nounits`. A line that cannot be read is left out.
pub fn parse(output: &str) -> Vec<Adapter> {
    output
        .lines()
        .filter_map(|line| {
            let columns: Vec<&str> = line.split(',').collect();
            if columns.len() < 7 {
                return None;
            }
            Some(Adapter {
                index: field(columns[0])?,
                name: columns[1].trim().to_string(),
                utilization_percent: field(columns[2]),
                memory: Memory {
                    used_mib: field(columns[3])?,
                    total_mib: field(columns[4])?,
                },
                temperature_c: field(columns[5]),
                power_watts: field(columns[6]),
                processes: Vec::new(),
            })
        })
        .collect()
}

/// The adapters, or an empty list when `nvidia-smi` is missing or fails (no driver, no GPU).
pub fn read() -> Vec<Adapter> {
    let mut command = Command::new("nvidia-smi");
    command.args([format!("--query-gpu={QUERY}"), "--format=csv,noheader,nounits".to_string()]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: a console window must not flash up every interval when this runs under a launcher.
        command.creation_flags(0x0800_0000);
    }
    match command.output() {
        Ok(output) if output.status.success() => parse(&String::from_utf8_lossy(&output.stdout)),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_one_adapter() {
        let adapters = parse("0, NVIDIA GeForce RTX 5070 Ti, 3, 5560, 16303, 40, 37.37\n");
        assert_eq!(adapters.len(), 1);
        assert_eq!(adapters[0].name, "NVIDIA GeForce RTX 5070 Ti");
        assert_eq!(adapters[0].utilization_percent, Some(3));
        assert_eq!(adapters[0].memory.used_mib, 5560);
        assert_eq!(adapters[0].memory.total_mib, 16303);
        assert_eq!(adapters[0].temperature_c, Some(40));
        assert_eq!(adapters[0].power_watts, Some(37.37));
    }

    #[test]
    fn a_value_the_driver_does_not_report_is_none() {
        let adapters = parse("0, GPU, [N/A], 100, 200, [Not Supported], [N/A]\n");
        assert_eq!(adapters[0].utilization_percent, None);
        assert_eq!(adapters[0].temperature_c, None);
        assert_eq!(adapters[0].power_watts, None);
        assert_eq!(adapters[0].memory.used_mib, 100);
    }

    #[test]
    fn a_line_without_memory_is_left_out() {
        assert!(parse("0, GPU, 1, [N/A], 200, 1, 1\n").is_empty());
        assert!(parse("garbage\n").is_empty());
        assert!(parse("").is_empty());
    }

    #[test]
    fn two_adapters_keep_their_order() {
        let adapters = parse("0, A, 1, 1, 2, 3, 4\n1, B, 5, 6, 7, 8, 9\n");
        assert_eq!(adapters.iter().map(|a| a.index).collect::<Vec<_>>(), vec![0, 1]);
    }
}
