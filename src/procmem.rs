//! Video memory per process, from the Windows performance counters `GPU Process Memory` (Local Usage).
//!
//! `nvidia-smi` reports `N/A` per process under WDDM, but these counters do not need administrator rights.
//!
//! `Local Usage` is the memory a process holds in the adapter's own (dedicated) video memory. It is the counter to use:
//! `Dedicated Usage` looks similar but counts far more than the adapter has (measured: 22.8 GB over 5.6 GB used), and the
//! desktop window manager alone reported 15 GB. The sum of `Local Usage` came to about 80% of the adapter's `memory.used`
//! (4.6 of 5.6 GB); the rest is memory the driver and the system hold for themselves. It answers "who", not "exactly how much".

use crate::model::Process;

/// At most this many processes are reported, largest first.
pub const MAX_PROCESSES: usize = 20;
/// A process holding less than this is left out.
pub const MIN_MIB: u64 = 16;

/// `pid_1804_luid_0x00000000_0x00015f3b_phys_0` gives the pid and the adapter's luid (`0x00000000_0x00015f3b`).
pub fn parse_instance(name: &str) -> Option<(u32, String)> {
    let rest = name.strip_prefix("pid_")?;
    let (pid, rest) = rest.split_once("_luid_")?;
    let luid = rest.rsplit_once("_phys_").map(|(luid, _)| luid).unwrap_or(rest);
    Some((pid.parse().ok()?, luid.to_string()))
}

/// Keeps the adapter whose processes hold the most (version 1 reports one list, for the first adapter), drops the small
/// ones, sorts largest first and cuts the list. `readings` are `(pid, luid, bytes)`.
pub fn summarise(readings: &[(u32, String, u64)], name_of: impl Fn(u32) -> String) -> Vec<Process> {
    use std::collections::HashMap;
    let mut totals: HashMap<&str, u64> = HashMap::new();
    for (_, luid, bytes) in readings {
        *totals.entry(luid.as_str()).or_default() += bytes;
    }
    let Some((luid, _)) = totals.into_iter().max_by_key(|(_, total)| *total) else {
        return Vec::new();
    };
    // The same process can appear more than once for one adapter (one instance per engine); its memory is added up.
    let mut by_pid: HashMap<u32, u64> = HashMap::new();
    for (pid, l, bytes) in readings {
        if l == luid {
            *by_pid.entry(*pid).or_default() += bytes;
        }
    }
    let mut processes: Vec<Process> = by_pid
        .into_iter()
        .map(|(pid, bytes)| Process { pid, name: name_of(pid), memory_mib: bytes / (1024 * 1024) })
        .filter(|process| process.memory_mib >= MIN_MIB)
        .collect();
    processes.sort_by(|a, b| b.memory_mib.cmp(&a.memory_mib).then(a.pid.cmp(&b.pid)));
    processes.truncate(MAX_PROCESSES);
    processes
}

#[cfg(windows)]
pub use windows_impl::read;

/// Not Windows: there is no such counter, so no list.
#[cfg(not(windows))]
pub fn read() -> Vec<Process> {
    Vec::new()
}

#[cfg(windows)]
mod windows_impl {
    use std::collections::HashMap;

    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Performance::{
        PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW,
        PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_LARGE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
    };

    use super::{parse_instance, summarise};
    use crate::model::Process;

    /// The names of the running processes, by pid (the counters give only the pid).
    fn process_names() -> HashMap<u32, String> {
        let mut names = HashMap::new();
        unsafe {
            let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
                return names;
            };
            let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
            let mut more = Process32FirstW(snapshot, &mut entry).is_ok();
            while more {
                let length = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                names.insert(entry.th32ProcessID, String::from_utf16_lossy(&entry.szExeFile[..length]));
                more = Process32NextW(snapshot, &mut entry).is_ok();
            }
            let _ = CloseHandle(snapshot);
        }
        names
    }

    /// `(pid, luid, bytes)` of every instance of `\GPU Process Memory(*)\Local Usage`.
    fn readings() -> Vec<(u32, String, u64)> {
        let mut out = Vec::new();
        unsafe {
            // A new query each time: the instances (processes) come and go, and a wildcard is expanded when the counter is added.
            let mut query = PDH_HQUERY::default();
            if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != 0 {
                return out;
            }
            let mut counter = PDH_HCOUNTER::default();
            let path = w!("\\GPU Process Memory(*)\\Local Usage");
            if PdhAddEnglishCounterW(query, path, 0, &mut counter) == 0 && PdhCollectQueryData(query) == 0 {
                let mut size = 0u32;
                let mut count = 0u32;
                let status = PdhGetFormattedCounterArrayW(counter, PDH_FMT_LARGE, &mut size, &mut count, None);
                if status == PDH_MORE_DATA && size > 0 {
                    // Aligned storage for PDH_FMT_COUNTERVALUE_ITEM_W (it holds pointers and 64-bit values).
                    let words = (size as usize).div_ceil(8);
                    let mut buffer = vec![0u64; words];
                    let items = buffer.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
                    let status = PdhGetFormattedCounterArrayW(counter, PDH_FMT_LARGE, &mut size, &mut count, Some(items));
                    if status == 0 {
                        for item in std::slice::from_raw_parts(items, count as usize) {
                            let Ok(name) = item.szName.to_string() else { continue };
                            let Some((pid, luid)) = parse_instance(&name) else { continue };
                            let value = item.FmtValue.Anonymous.largeValue;
                            out.push((pid, luid, value.max(0) as u64));
                        }
                    }
                }
            }
            let _ = PdhCloseQuery(query);
        }
        out
    }

    /// The processes holding video memory, largest first (see the module note on what the numbers mean).
    pub fn read() -> Vec<Process> {
        let readings = readings();
        if readings.is_empty() {
            return Vec::new();
        }
        let names = process_names();
        summarise(&readings, |pid| names.get(&pid).cloned().unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_pid_and_the_luid() {
        assert_eq!(
            parse_instance("pid_1804_luid_0x00000000_0x00015f3b_phys_0"),
            Some((1804, "0x00000000_0x00015f3b".to_string()))
        );
        assert_eq!(parse_instance("garbage"), None);
        assert_eq!(parse_instance("pid_x_luid_0x0_0x1_phys_0"), None);
    }

    const MIB: u64 = 1024 * 1024;

    #[test]
    fn sorts_largest_first_drops_small_ones_and_adds_up_one_process() {
        let readings = vec![
            (1, "a".to_string(), 100 * MIB),
            (2, "a".to_string(), 300 * MIB),
            (1, "a".to_string(), 50 * MIB),
            (3, "a".to_string(), 4 * MIB),
        ];
        let processes = summarise(&readings, |pid| format!("p{pid}.exe"));
        assert_eq!(
            processes,
            vec![
                Process { pid: 2, name: "p2.exe".into(), memory_mib: 300 },
                Process { pid: 1, name: "p1.exe".into(), memory_mib: 150 },
            ]
        );
    }

    #[test]
    fn keeps_the_adapter_that_holds_the_most() {
        let readings = vec![(1, "small".to_string(), 100 * MIB), (2, "big".to_string(), 900 * MIB)];
        let processes = summarise(&readings, |_| String::new());
        assert_eq!(processes.iter().map(|p| p.pid).collect::<Vec<_>>(), vec![2]);
    }

    #[test]
    fn cuts_the_list() {
        let readings: Vec<_> = (0..40u32).map(|pid| (pid, "a".to_string(), (100 + u64::from(pid)) * MIB)).collect();
        assert_eq!(summarise(&readings, |_| String::new()).len(), MAX_PROCESSES);
    }

    #[test]
    fn nothing_read_gives_nothing() {
        assert!(summarise(&[], |_| String::new()).is_empty());
    }
}
