use std::collections::HashSet;

use rayon::prelude::*;
use rs_common::sanitize_control_chars;

use crate::cli::CliArgs;
use crate::modules::{
    battery, colors, cpu, de_wm_res, disk, gpu, host, kernel, local_ip, memory, os, packages,
    shell, terminal, uptime,
};

/// Identifiers for all supported system information modules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ModuleId {
    Os,
    Host,
    Kernel,
    Uptime,
    Packages,
    Shell,
    Resolution,
    De,
    Wm,
    Terminal,
    Cpu,
    Gpu,
    Memory,
    Disk,
    LocalIp,
    Battery,
    Colors,
}

impl ModuleId {
    /// All supported modules in canonical display order.
    pub const ALL: &'static [ModuleId] = &[
        ModuleId::Os,
        ModuleId::Host,
        ModuleId::Kernel,
        ModuleId::Uptime,
        ModuleId::Packages,
        ModuleId::Shell,
        ModuleId::Resolution,
        ModuleId::De,
        ModuleId::Wm,
        ModuleId::Terminal,
        ModuleId::Cpu,
        ModuleId::Gpu,
        ModuleId::Memory,
        ModuleId::Disk,
        ModuleId::LocalIp,
        ModuleId::Battery,
        ModuleId::Colors,
    ];

    /// Default modules enabled when neither `--modules` nor `--all` is specified.
    pub const DEFAULT: &'static [ModuleId] = &[
        ModuleId::Os,
        ModuleId::Host,
        ModuleId::Kernel,
        ModuleId::Uptime,
        ModuleId::Packages,
        ModuleId::Shell,
        ModuleId::Resolution,
        ModuleId::De,
        ModuleId::Wm,
        ModuleId::Terminal,
        ModuleId::Cpu,
        ModuleId::Gpu,
        ModuleId::Memory,
        ModuleId::Disk,
        ModuleId::Colors,
    ];

    /// Parses a CLI module name (case-insensitive, supporting common aliases).
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_lowercase().as_str() {
            "os" => Some(Self::Os),
            "host" => Some(Self::Host),
            "kernel" => Some(Self::Kernel),
            "uptime" => Some(Self::Uptime),
            "packages" | "pkgs" => Some(Self::Packages),
            "shell" => Some(Self::Shell),
            "resolution" | "res" => Some(Self::Resolution),
            "de" => Some(Self::De),
            "wm" => Some(Self::Wm),
            "terminal" | "term" => Some(Self::Terminal),
            "cpu" => Some(Self::Cpu),
            "gpu" => Some(Self::Gpu),
            "memory" | "mem" => Some(Self::Memory),
            "disk" => Some(Self::Disk),
            "local-ip" | "local_ip" | "localip" | "ip" => Some(Self::LocalIp),
            "battery" | "bat" => Some(Self::Battery),
            "colors" | "color" => Some(Self::Colors),
            _ => None,
        }
    }

    /// Canonical CLI name for this module.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Os => "os",
            Self::Host => "host",
            Self::Kernel => "kernel",
            Self::Uptime => "uptime",
            Self::Packages => "packages",
            Self::Shell => "shell",
            Self::Resolution => "resolution",
            Self::De => "de",
            Self::Wm => "wm",
            Self::Terminal => "terminal",
            Self::Cpu => "cpu",
            Self::Gpu => "gpu",
            Self::Memory => "memory",
            Self::Disk => "disk",
            Self::LocalIp => "local-ip",
            Self::Battery => "battery",
            Self::Colors => "colors",
        }
    }

    /// Human-readable label for terminal output.
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Os => "OS",
            Self::Host => "Host",
            Self::Kernel => "Kernel",
            Self::Uptime => "Uptime",
            Self::Packages => "Packages",
            Self::Shell => "Shell",
            Self::Resolution => "Resolution",
            Self::De => "DE",
            Self::Wm => "WM",
            Self::Terminal => "Terminal",
            Self::Cpu => "CPU",
            Self::Gpu => "GPU",
            Self::Memory => "Memory",
            Self::Disk => "Disk",
            Self::LocalIp => "Local IP",
            Self::Battery => "Battery",
            Self::Colors => "Colors",
        }
    }
}

/// A single collected system information entry.
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleEntry {
    pub id: ModuleId,
    pub label: String,
    pub value: String,
    pub json_value: serde_json::Value,
}

/// Complete collected system report ready for rendering or JSON serialization.
#[derive(Debug, Clone, PartialEq)]
pub struct SystemReport {
    pub user: String,
    pub hostname: String,
    pub distro_id: String,
    pub distro_id_like: Vec<String>,
    pub entries: Vec<ModuleEntry>,
}

/// Resolves the ordered list of modules to collect based on CLI flags.
pub fn resolve_modules(args: &CliArgs) -> Vec<ModuleId> {
    let base: Vec<ModuleId> = if !args.modules.is_empty() {
        let mut seen = HashSet::new();
        args.modules
            .iter()
            .filter_map(|name| ModuleId::from_name(name))
            .filter(|id| seen.insert(*id))
            .collect()
    } else if args.all {
        ModuleId::ALL.to_vec()
    } else {
        ModuleId::DEFAULT.to_vec()
    };

    if args.hide.is_empty() {
        return base;
    }

    let hidden: HashSet<ModuleId> = args
        .hide
        .iter()
        .filter_map(|name| ModuleId::from_name(name))
        .collect();

    base.into_iter().filter(|id| !hidden.contains(id)).collect()
}

fn current_username() -> String {
    for var in ["USER", "LOGNAME"] {
        if let Ok(val) = std::env::var(var) {
            let clean = sanitize_control_chars(val.trim());
            if !clean.is_empty() {
                return clean;
            }
        }
    }
    "user".to_string()
}

fn collect_single(id: ModuleId, no_color: bool) -> Option<ModuleEntry> {
    match id {
        ModuleId::Os => os::collect(),
        ModuleId::Host => host::collect(),
        ModuleId::Kernel => kernel::collect(),
        ModuleId::Uptime => uptime::collect(),
        ModuleId::Packages => packages::collect(),
        ModuleId::Shell => shell::collect(),
        ModuleId::Resolution => de_wm_res::collect_resolution(),
        ModuleId::De => de_wm_res::collect_de(),
        ModuleId::Wm => de_wm_res::collect_wm(),
        ModuleId::Terminal => terminal::collect(),
        ModuleId::Cpu => cpu::collect(),
        ModuleId::Gpu => gpu::collect(),
        ModuleId::Memory => memory::collect(),
        ModuleId::Disk => disk::collect(),
        ModuleId::LocalIp => local_ip::collect(),
        ModuleId::Battery => battery::collect(),
        ModuleId::Colors => colors::collect(no_color),
    }
}

/// Runs enabled module collectors in parallel with `rayon` while preserving deterministic order.
pub fn collect_system_report(args: &CliArgs) -> SystemReport {
    let modules = resolve_modules(args);
    let no_color = args.no_color;

    let ((user, hostname, os_info), raw_entries) = rayon::join(
        || {
            let user = current_username();
            let hostname = host::get_hostname();
            let os_info = os::read_os_release();
            (user, hostname, os_info)
        },
        || {
            modules
                .into_par_iter()
                .map(|id| collect_single(id, no_color))
                .collect::<Vec<Option<ModuleEntry>>>()
        },
    );

    let entries = raw_entries.into_iter().flatten().collect();
    let distro_id = os_info
        .id
        .map(|s| sanitize_control_chars(&s))
        .unwrap_or_else(|| "linux".to_string());
    let distro_id_like = os_info
        .id_like
        .into_iter()
        .map(|s| sanitize_control_chars(&s))
        .collect();

    SystemReport {
        user,
        hostname,
        distro_id,
        distro_id_like,
        entries,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn resolve_modules_respects_order_all_and_hide() {
        let default_args = CliArgs::try_parse_from(["rsfetch"]).unwrap();
        let def = resolve_modules(&default_args);
        assert!(!def.contains(&ModuleId::Battery));
        assert!(!def.contains(&ModuleId::LocalIp));
        assert!(def.contains(&ModuleId::Os));

        let all_args = CliArgs::try_parse_from(["rsfetch", "--all"]).unwrap();
        let all = resolve_modules(&all_args);
        assert_eq!(all.len(), ModuleId::ALL.len());
        assert!(all.contains(&ModuleId::Battery));
        assert!(all.contains(&ModuleId::LocalIp));

        let custom_args = CliArgs::try_parse_from([
            "rsfetch",
            "--modules",
            "kernel,os,cpu,local-ip",
            "--hide",
            "cpu",
        ])
        .unwrap();
        assert_eq!(
            resolve_modules(&custom_args),
            vec![ModuleId::Kernel, ModuleId::Os, ModuleId::LocalIp]
        );
    }

    #[test]
    fn collect_system_report_preserves_requested_order() {
        let args =
            CliArgs::try_parse_from(["rsfetch", "--modules", "uptime,kernel,os,colors"]).unwrap();
        let report = collect_system_report(&args);
        let ids: Vec<ModuleId> = report.entries.iter().map(|e| e.id).collect();
        assert_eq!(
            ids,
            vec![
                ModuleId::Uptime,
                ModuleId::Kernel,
                ModuleId::Os,
                ModuleId::Colors
            ]
        );
        assert!(!report.user.is_empty());
        assert!(!report.hostname.is_empty());
    }
}
