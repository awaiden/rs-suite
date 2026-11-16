use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::Path;

use rs_common::sanitize_control_chars;

use crate::system::{ModuleEntry, ModuleId};

/// Built-in fallback vendor names for common GPU PCI vendor IDs.
pub fn fallback_vendor_name(vendor_id: &str) -> &'static str {
    match vendor_id {
        "1002" => "AMD/ATI",
        "10de" => "NVIDIA",
        "8086" => "Intel",
        "1af4" => "VirtIO",
        "15ad" => "VMware",
        "1b36" => "QEMU",
        "80ee" => "VirtualBox",
        _ => "Unknown GPU",
    }
}

fn normalize_hex_id(raw: &str) -> Option<String> {
    let s = raw.trim().trim_start_matches("0x").trim_start_matches("0X");
    if s.len() == 4 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some(s.to_lowercase())
    } else {
        None
    }
}

/// Resolves `(vendor_id, device_id)` pairs using `pci.ids` content.
pub fn resolve_from_pci_ids<R: BufRead>(
    reader: R,
    targets: &[(String, String)],
) -> HashMap<(String, String), String> {
    let mut resolved = HashMap::new();
    let mut vendor_names: HashMap<String, String> = HashMap::new();
    let mut current_vendor: Option<String> = None;

    for line in reader.lines().map_while(Result::ok) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with("\t\t") {
            continue;
        }
        if let Some(rest) = line.strip_prefix('\t') {
            let Some(ref v) = current_vendor else {
                continue;
            };
            if rest.len() >= 6 && rest.as_bytes()[4] == b' ' {
                let dev_id = rest[..4].to_lowercase();
                let key = (v.clone(), dev_id);
                if targets.contains(&key) {
                    let dev_name = sanitize_control_chars(rest[4..].trim());
                    let v_name = vendor_names
                        .get(v)
                        .map(String::as_str)
                        .unwrap_or_else(|| fallback_vendor_name(v));
                    resolved.insert(key, format!("{v_name} {dev_name}"));
                }
            }
        } else if line.len() >= 6 && line.as_bytes()[4] == b' ' {
            let v_id = line[..4].to_lowercase();
            let needed = targets.iter().any(|(tv, _)| *tv == v_id);
            if needed {
                let v_name = sanitize_control_chars(line[4..].trim());
                vendor_names.insert(v_id.clone(), v_name);
                current_vendor = Some(v_id);
            } else {
                current_vendor = None;
            }
        }
    }

    resolved
}

fn scan_drm_cards() -> Vec<(String, String)> {
    let mut cards = Vec::new();
    let Ok(entries) = fs::read_dir(Path::new("/sys/class/drm")) else {
        return cards;
    };

    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if !name_str.starts_with("card") || name_str.contains('-') {
            continue;
        }
        let dev_dir = entry.path().join("device");
        let Ok(vendor_raw) = fs::read_to_string(dev_dir.join("vendor")) else {
            continue;
        };
        let Ok(device_raw) = fs::read_to_string(dev_dir.join("device")) else {
            continue;
        };
        if let (Some(v), Some(d)) = (normalize_hex_id(&vendor_raw), normalize_hex_id(&device_raw)) {
            cards.push((v, d));
        }
    }
    cards.sort();
    cards.dedup();
    cards
}

/// Collects the GPU module entry.
pub fn collect() -> Option<ModuleEntry> {
    let cards = scan_drm_cards();
    if cards.is_empty() {
        return None;
    }

    let mut pci_names = HashMap::new();
    for path in ["/usr/share/hwdata/pci.ids", "/usr/share/misc/pci.ids"] {
        if let Ok(file) = File::open(path) {
            pci_names = resolve_from_pci_ids(BufReader::new(file), &cards);
            break;
        }
    }

    let mut gpus_json = Vec::with_capacity(cards.len());
    let mut names = Vec::with_capacity(cards.len());

    for (v, d) in &cards {
        let name = pci_names
            .get(&(v.clone(), d.clone()))
            .cloned()
            .unwrap_or_else(|| format!("{} [0x{d}]", fallback_vendor_name(v)));
        names.push(name.clone());
        gpus_json.push(serde_json::json!({
            "vendor_id": v,
            "device_id": d,
            "name": name,
        }));
    }

    Some(ModuleEntry {
        id: ModuleId::Gpu,
        label: ModuleId::Gpu.label().to_string(),
        value: names.join(", "),
        json_value: serde_json::json!(gpus_json),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn resolves_gpu_from_pci_ids_and_falls_back() {
        let pci_ids = "\
# PCI IDs
10de  NVIDIA Corporation
\t2684  AD102 [GeForce RTX 4090]
1002  Advanced Micro Devices, Inc. [AMD/ATI]
\t744c  Navi 31 [Radeon RX 7900 XT]
";
        let targets = vec![
            ("10de".to_string(), "2684".to_string()),
            ("8086".to_string(), "9a49".to_string()),
        ];
        let map = resolve_from_pci_ids(Cursor::new(pci_ids), &targets);
        assert_eq!(
            map.get(&("10de".to_string(), "2684".to_string()))
                .map(String::as_str),
            Some("NVIDIA Corporation AD102 [GeForce RTX 4090]")
        );
        assert!(!map.contains_key(&("8086".to_string(), "9a49".to_string())));
        assert_eq!(fallback_vendor_name("8086"), "Intel");
    }
}
