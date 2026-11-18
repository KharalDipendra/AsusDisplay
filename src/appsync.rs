use std::collections::HashSet;
use std::fs;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppRule {
    pub id: String,
    pub name: String,
    /// Case-insensitive process executable or window substring match (e.g. "cs2", "mpv", "blender", "code")
    pub process_match: String,
    /// Target VCP 0xDC picture mode value (e.g. 2 = Game/FPS, 3 = Cinema, 1 = sRGB)
    pub target_mode: Option<u16>,
    /// Optional saved profile name to apply instead of just 0xDC
    pub target_profile: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppSyncConfig {
    pub enabled: bool,
    pub poll_interval_ms: u64,
    /// Fallback mode when no rule matches
    pub default_mode: Option<u16>,
    pub rules: Vec<AppRule>,
}

impl Default for AppSyncConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            poll_interval_ms: 2000,
            default_mode: None,
            rules: Vec::new(),
        }
    }
}

pub fn load_appsync_config() -> AppSyncConfig {
    let path = crate::config_dir().join("appsync.json");
    let Ok(text) = fs::read_to_string(path) else {
        return AppSyncConfig::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

pub fn save_appsync_config(cfg: &AppSyncConfig) -> Result<(), String> {
    let path = crate::config_dir().join("appsync.json");
    let json = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(target_os = "linux")]
pub fn list_running_processes() -> Vec<String> {
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid_str) = name.to_str() else {
            continue;
        };
        if !pid_str.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Ok(comm) = fs::read_to_string(entry.path().join("comm")) else {
            continue;
        };
        let proc_name = comm.trim().to_ascii_lowercase();
        if !proc_name.is_empty() && seen.insert(proc_name.clone()) {
            out.push(proc_name);
        }
    }
    out
}

#[cfg(windows)]
pub fn list_running_processes() -> Vec<String> {
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId,
    };

    // SAFETY: GetForegroundWindow and GetWindowThreadProcessId take a valid HWND or null.
    let mut fg_pid = 0u32;
    unsafe {
        let hwnd = GetForegroundWindow();
        if !hwnd.is_null() {
            GetWindowThreadProcessId(hwnd, &mut fg_pid);
        }
    }

    // SAFETY: TH32CS_SNAPPROCESS snapshot handle is checked against INVALID_HANDLE_VALUE and closed.
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snap == INVALID_HANDLE_VALUE {
        return Vec::new();
    }

    let mut entry: PROCESSENTRY32W = unsafe { zeroed() };
    entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;

    let mut fg_exe: Option<String> = None;
    let mut others = Vec::new();

    // SAFETY: `entry` is properly initialized with `dwSize` set.
    let mut has_next = unsafe { Process32FirstW(snap, &mut entry) } != 0;
    while has_next {
        let len = entry
            .szExeFile
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(entry.szExeFile.len());
        let exe = String::from_utf16_lossy(&entry.szExeFile[..len]).to_ascii_lowercase();
        if !exe.is_empty() {
            if fg_pid != 0 && entry.th32ProcessID == fg_pid {
                fg_exe = Some(exe);
            } else {
                others.push(exe);
            }
        }
        has_next = unsafe { Process32NextW(snap, &mut entry) } != 0;
    }

    // SAFETY: `snap` is a valid snapshot handle.
    unsafe { CloseHandle(snap) };

    let mut seen = HashSet::new();
    let mut out = Vec::new();
    if let Some(fg) = fg_exe
        && seen.insert(fg.clone())
    {
        out.push(fg);
    }
    for exe in others {
        if seen.insert(exe.clone()) {
            out.push(exe);
        }
    }
    out
}

pub fn match_active_rule<'a>(config: &'a AppSyncConfig, processes: &[String]) -> Option<&'a AppRule> {
    for rule in config.rules.iter().filter(|r| r.enabled) {
        let needle = rule.process_match.trim().to_ascii_lowercase();
        if needle.is_empty() {
            continue;
        }
        let needle_stem = needle.strip_suffix(".exe").unwrap_or(&needle);
        let matched = processes.iter().any(|proc_name| {
            let p = proc_name.trim().to_ascii_lowercase();
            if p.is_empty() {
                return false;
            }
            let stem = p.strip_suffix(".exe").unwrap_or(&p);
            p == needle || stem == needle || stem == needle_stem || p.contains(&needle)
        });
        if matched {
            return Some(rule);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(id: &str, process_match: &str, enabled: bool) -> AppRule {
        AppRule {
            id: id.into(),
            name: id.into(),
            process_match: process_match.into(),
            target_mode: Some(5),
            target_profile: None,
            enabled,
        }
    }

    #[test]
    fn match_active_rule_respects_enabled_and_matching_modes() {
        let cfg = AppSyncConfig {
            enabled: true,
            poll_interval_ms: 2000,
            default_mode: Some(4),
            rules: vec![
                rule("disabled", "cs2", false),
                rule("game", "  CS2 ", true),
                rule("editor", "code", true),
                rule("video", "mpv.exe", true),
            ],
        };

        // Stem match: "cs2" matches "cs2.exe" and skips disabled rule.
        let procs = vec!["explorer.exe".into(), "cs2.exe".into()];
        assert_eq!(match_active_rule(&cfg, &procs).map(|r| r.id.as_str()), Some("game"));

        // Substring match: "code" matches "code-insiders.exe".
        let procs = vec!["Code-Insiders.exe".into()];
        assert_eq!(match_active_rule(&cfg, &procs).map(|r| r.id.as_str()), Some("editor"));

        // Cross-platform stem match: "mpv.exe" rule matches Linux "mpv".
        let procs = vec!["mpv".into()];
        assert_eq!(match_active_rule(&cfg, &procs).map(|r| r.id.as_str()), Some("video"));

        // Empty process_match never matches.
        let empty_rule_cfg = AppSyncConfig {
            rules: vec![rule("empty", "   ", true)],
            ..Default::default()
        };
        assert!(match_active_rule(&empty_rule_cfg, &procs).is_none());

        // No match returns None.
        let none_procs = vec!["firefox.exe".into()];
        assert!(match_active_rule(&cfg, &none_procs).is_none());
    }
}
