use std::fs;

use rs_common::sanitize_control_chars;

use crate::modules::kernel;
use crate::system::{ModuleEntry, ModuleId};

const OEM_PLACEHOLDERS: &[&str] = &[
    "to be filled by o.e.m.",
    "default string",
    "not specified",
    "system product name",
    "system version",
    "system manufacturer",
    "base board serial number",
    "chassis serial number",
    "undefined",
    "none",
    "n/a",
    "oem",
    "invalid",
    "unknown",
    "0",
];

/// Returns `true` if a DMI string is empty or a known OEM placeholder.
pub fn is_placeholder(s: &str) -> bool {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lower = trimmed.to_lowercase();
    OEM_PLACEHOLDERS.iter().any(|&p| lower == p)
}

fn read_dmi_field(field: &str) -> Option<String> {
    for base in ["/sys/devices/virtual/dmi/id", "/sys/class/dmi/id"] {
        let path = format!("{base}/{field}");
        if let Ok(raw) = fs::read_to_string(path) {
            let cleaned = sanitize_control_chars(raw.trim());
            if !is_placeholder(&cleaned) {
                return Some(cleaned);
            }
        }
    }
    None
}

/// Formats host hardware string from DMI fields, filtering OEM placeholders.
pub fn format_dmi_host(
    vendor: Option<&str>,
    product: Option<&str>,
    version: Option<&str>,
) -> Option<String> {
    let clean = |opt: Option<&str>| -> Option<String> {
        opt.map(|s| sanitize_control_chars(s.trim()))
            .filter(|s| !is_placeholder(s))
    };

    let vendor = clean(vendor);
    let product = clean(product);
    let version = clean(version);

    let mut parts: Vec<String> = Vec::new();
    match (vendor.as_ref(), product.as_ref()) {
        (Some(v), Some(p)) => {
            if p.to_lowercase().starts_with(&v.to_lowercase()) {
                parts.push(p.clone());
            } else {
                parts.push(v.clone());
                parts.push(p.clone());
            }
        }
        (None, Some(p)) => parts.push(p.clone()),
        (Some(v), None) => parts.push(v.clone()),
        (None, None) => {}
    }

    if let Some(ver) = version {
        let joined = parts.join(" ").to_lowercase();
        if !joined.contains(&ver.to_lowercase()) {
            parts.push(ver);
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

/// Returns the sanitized system hostname via `uname` or `/proc/sys/kernel/hostname`.
pub fn get_hostname() -> String {
    if let Some(uts) = kernel::uname() {
        if !uts.nodename.is_empty() {
            return uts.nodename;
        }
    }
    if let Ok(raw) = fs::read_to_string("/proc/sys/kernel/hostname") {
        let cleaned = sanitize_control_chars(raw.trim());
        if !cleaned.is_empty() {
            return cleaned;
        }
    }
    "unknown".to_string()
}

/// Collects the Host module entry.
pub fn collect() -> Option<ModuleEntry> {
    let vendor = read_dmi_field("sys_vendor");
    let product = read_dmi_field("product_name");
    let version = read_dmi_field("product_version");

    let value = format_dmi_host(vendor.as_deref(), product.as_deref(), version.as_deref())
        .or_else(|| {
            fs::read_to_string("/sys/firmware/devicetree/base/model")
                .ok()
                .map(|s| sanitize_control_chars(s.trim_end_matches('\0').trim()))
                .filter(|s| !s.is_empty())
        })?;

    Some(ModuleEntry {
        id: ModuleId::Host,
        label: ModuleId::Host.label().to_string(),
        value: value.clone(),
        json_value: serde_json::json!({
            "name": value,
            "vendor": vendor,
            "product": product,
            "version": version,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_oem_placeholders() {
        assert_eq!(
            format_dmi_host(
                Some("To Be Filled By O.E.M."),
                Some("System Product Name"),
                Some("Default string"),
            ),
            None
        );
    }

    #[test]
    fn formats_vendor_product_version_without_duplication() {
        assert_eq!(
            format_dmi_host(
                Some("LENOVO"),
                Some("21HMCTO1WW"),
                Some("ThinkPad X1 Carbon Gen 11"),
            )
            .as_deref(),
            Some("LENOVO 21HMCTO1WW ThinkPad X1 Carbon Gen 11")
        );
        assert_eq!(
            format_dmi_host(Some("Framework"), Some("Framework Laptop 13"), Some("A7"),).as_deref(),
            Some("Framework Laptop 13 A7")
        );
    }
}
