use std::collections::HashSet;
use std::fs;

use rs_common::sanitize_control_chars;

use crate::system::{ModuleEntry, ModuleId};

/// Parsed CPU information from `/proc/cpuinfo` and cpufreq.
#[derive(Debug, Clone, PartialEq)]
pub struct CpuInfo {
    pub model: String,
    pub physical_cores: usize,
    pub logical_cores: usize,
    pub max_freq_ghz: Option<f64>,
}

/// Cleans and normalizes a CPU model string.
fn clean_cpu_model(raw: &str) -> String {
    let sanitized = sanitize_control_chars(raw);
    sanitized.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Parses `/proc/cpuinfo` content and an optional `cpuinfo_max_freq` (in kHz).
pub fn parse_cpuinfo(content: &str, max_freq_khz: Option<u64>) -> Option<CpuInfo> {
    let mut model: Option<String> = None;
    let mut logical_cores: usize = 0;
    let mut physical_pairs: HashSet<(String, String)> = HashSet::new();
    let mut current_phys_id: Option<String> = None;
    let mut current_core_id: Option<String> = None;
    let mut cpu_cores_field: Option<usize> = None;
    let mut max_cpu_mhz: f64 = 0.0;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if let (Some(p), Some(c)) = (current_phys_id.take(), current_core_id.take()) {
                physical_pairs.insert((p, c));
            }
            current_phys_id = None;
            current_core_id = None;
            continue;
        }

        let Some((key, val)) = trimmed.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let val = val.trim();

        match key {
            "processor" => logical_cores += 1,
            "model name" | "Hardware" | "Processor" if model.is_none() => {
                let cleaned = clean_cpu_model(val);
                if !cleaned.is_empty() {
                    model = Some(cleaned);
                }
            }
            "physical id" => current_phys_id = Some(val.to_string()),
            "core id" => current_core_id = Some(val.to_string()),
            "cpu cores" if cpu_cores_field.is_none() => {
                cpu_cores_field = val.parse().ok();
            }
            "cpu MHz" => {
                if let Ok(mhz) = val.parse::<f64>() {
                    if mhz > max_cpu_mhz {
                        max_cpu_mhz = mhz;
                    }
                }
            }
            _ => {}
        }
    }

    if let (Some(p), Some(c)) = (current_phys_id, current_core_id) {
        physical_pairs.insert((p, c));
    }

    let model = model?;
    let logical_cores = logical_cores.max(1);
    let physical_cores = if !physical_pairs.is_empty() {
        physical_pairs.len()
    } else {
        cpu_cores_field.unwrap_or(logical_cores)
    };

    let max_freq_ghz = max_freq_khz
        .filter(|&khz| khz > 0)
        .map(|khz| khz as f64 / 1_000_000.0)
        .or_else(|| (max_cpu_mhz > 0.0).then_some(max_cpu_mhz / 1000.0));

    Some(CpuInfo {
        model,
        physical_cores,
        logical_cores,
        max_freq_ghz,
    })
}

/// Formats [`CpuInfo`] into a display string.
pub fn format_cpu_info(info: &CpuInfo) -> String {
    match info.max_freq_ghz {
        Some(ghz) => format!("{} ({}) @ {:.2} GHz", info.model, info.logical_cores, ghz),
        None => format!("{} ({})", info.model, info.logical_cores),
    }
}

/// Collects the CPU module entry.
pub fn collect() -> Option<ModuleEntry> {
    let raw = fs::read_to_string("/proc/cpuinfo").ok()?;
    let max_khz = fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok());

    let info = parse_cpuinfo(&raw, max_khz)?;
    let value = format_cpu_info(&info);

    Some(ModuleEntry {
        id: ModuleId::Cpu,
        label: ModuleId::Cpu.label().to_string(),
        value,
        json_value: serde_json::json!({
            "model": info.model,
            "physical_cores": info.physical_cores,
            "logical_cores": info.logical_cores,
            "max_freq_ghz": info.max_freq_ghz,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_x86_cpuinfo_with_cpufreq() {
        let sample = "\
processor\t: 0
model name\t: AMD Ryzen 7 7800X3D 8-Core Processor
physical id\t: 0
core id\t\t: 0
cpu cores\t: 8
cpu MHz\t\t: 4200.000

processor\t: 1
model name\t: AMD Ryzen 7 7800X3D 8-Core Processor
physical id\t: 0
core id\t\t: 0
cpu cores\t: 8
cpu MHz\t\t: 4200.000
";
        let info = parse_cpuinfo(sample, Some(5_050_000)).expect("cpuinfo should parse");
        assert_eq!(info.model, "AMD Ryzen 7 7800X3D 8-Core Processor");
        assert_eq!(info.logical_cores, 2);
        assert_eq!(info.physical_cores, 1);
        assert_eq!(
            format_cpu_info(&info),
            "AMD Ryzen 7 7800X3D 8-Core Processor (2) @ 5.05 GHz"
        );
    }

    #[test]
    fn falls_back_to_cpu_mhz_when_cpufreq_missing() {
        let sample = "\
processor\t: 0
Hardware\t: BCM2711
cpu MHz\t\t: 1500.000
";
        let info = parse_cpuinfo(sample, None).expect("arm cpuinfo should parse");
        assert_eq!(info.model, "BCM2711");
        assert_eq!(format_cpu_info(&info), "BCM2711 (1) @ 1.50 GHz");
    }
}
