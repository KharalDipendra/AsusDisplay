use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{Kind, Monitor, Result};

const CORE_VCP_CODES: [u8; 7] = [0xDC, 0x10, 0x12, 0x14, 0x16, 0x18, 0x1A];
const PIXEL_CLEANING_BIT: u16 = 0x0010;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MonitorProfile {
    pub name: String,
    pub model: String,
    /// Map of hex VCP code string (e.g. "0xDC", "0x10", "0x12", "0x14", "0x16", "0x18", "0x1A") to value.
    pub vcp_values: BTreeMap<String, u16>,
    /// ASUS DisplayWidget Center compatible export fields ("0xDC", "0x10", "0x12", "0x14", "0x16", "0x18", "0x1A").
    #[serde(default)]
    pub asus_compat: BTreeMap<String, i32>,
}

pub fn profiles_dir() -> PathBuf {
    let dir = crate::config_dir().join("profiles");
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn sanitize_profile_name(name: &str) -> String {
    let mapped: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = mapped.trim_matches(|c| c == ' ' || c == '_');
    if trimmed.is_empty() {
        "profile".to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn list_profiles() -> Vec<MonitorProfile> {
    let dir = profiles_dir();
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut profiles = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        if let Ok(profile) = serde_json::from_str::<MonitorProfile>(&text) {
            profiles.push(profile);
        }
    }
    profiles.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.name.cmp(&b.name))
    });
    profiles
}

pub fn load_profile(name: &str) -> Result<MonitorProfile> {
    let dir = profiles_dir();
    let direct = dir.join(format!("{}.json", sanitize_profile_name(name)));
    if direct.is_file() {
        let text = fs::read_to_string(&direct).map_err(|e| format!("{}: {e}", direct.display()))?;
        return serde_json::from_str(&text).map_err(|e| format!("{}: {e}", direct.display()));
    }

    list_profiles()
        .into_iter()
        .find(|p| p.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| format!("profile '{name}' not found"))
}

pub fn save_profile_to_disk(profile: &MonitorProfile) -> Result<PathBuf> {
    let dir = profiles_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(format!("{}.json", sanitize_profile_name(&profile.name)));
    let json = serde_json::to_string_pretty(profile).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

pub fn delete_profile(name: &str) -> Result<()> {
    let dir = profiles_dir();
    let direct = dir.join(format!("{}.json", sanitize_profile_name(name)));
    if direct.is_file() {
        return fs::remove_file(&direct).map_err(|e| format!("{}: {e}", direct.display()));
    }

    let profile = load_profile(name)?;
    let path = dir.join(format!("{}.json", sanitize_profile_name(&profile.name)));
    fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn export_profile(profile: &MonitorProfile, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(profile).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn import_profile(path: &Path) -> Result<MonitorProfile> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let fallback_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(sanitize_profile_name)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "imported".to_string());

    let profile = parse_imported_profile(&text, &fallback_name)?;
    save_profile_to_disk(&profile)?;
    Ok(profile)
}

fn parse_imported_profile(text: &str, fallback_name: &str) -> Result<MonitorProfile> {
    if let Ok(mut profile) = serde_json::from_str::<MonitorProfile>(text) {
        if profile.name.trim().is_empty() {
            profile.name = fallback_name.to_string();
        }
        return Ok(profile);
    }

    let map: BTreeMap<String, serde_json::Value> =
        serde_json::from_str(text).map_err(|e| format!("invalid profile JSON: {e}"))?;

    let mut vcp_values = BTreeMap::new();
    let mut asus_compat = BTreeMap::new();
    let mut model = String::from("ASUS");

    for (key, value) in &map {
        if key.eq_ignore_ascii_case("model") {
            if let Some(s) = value.as_str().filter(|s| !s.trim().is_empty()) {
                model = s.trim().to_string();
            }
            continue;
        }
        let Some(code) = parse_vcp_code(key) else {
            continue;
        };
        let Some(num) = value.as_i64() else {
            continue;
        };
        let hex = format!("0x{code:02X}");
        if let Ok(v16) = u16::try_from(num) {
            vcp_values.insert(hex.clone(), v16);
        }
        if let Ok(v32) = i32::try_from(num) {
            asus_compat.insert(hex, v32);
        }
    }

    if vcp_values.is_empty() {
        return Err("no VCP values found in profile JSON".into());
    }

    Ok(MonitorProfile {
        name: fallback_name.to_string(),
        model,
        vcp_values,
        asus_compat,
    })
}

pub fn capture_profile(mon: &mut Monitor, name: &str) -> Result<MonitorProfile> {
    let mut codes = BTreeSet::new();
    for feat in mon.features(false) {
        if matches!(feat.kind, Kind::Action | Kind::Info) || feat.force {
            continue;
        }
        if matches!(feat.code, 0xD6 | 0xEC | 0xE2) {
            continue;
        }
        codes.insert(feat.code);
    }

    let is_asus = mon.is_asus() || mon.is_asus_vcp();
    for code in CORE_VCP_CODES {
        if code == 0xDC && !is_asus {
            continue;
        }
        codes.insert(code);
    }

    let mut vcp_values = BTreeMap::new();
    let mut asus_compat = BTreeMap::new();
    let mut last_err = None;

    for code in codes {
        match mon.get_vcp(code) {
            Ok(vcp) => {
                // 254/254 (0xFE) indicates a locked/unavailable control in the current mode.
                if code != 0xFD && vcp.current == 0xFE && vcp.max == 0xFE {
                    continue;
                }
                let value = if code == 0xFD {
                    vcp.current & !PIXEL_CLEANING_BIT
                } else {
                    vcp.current
                };
                let hex = format!("0x{code:02X}");
                vcp_values.insert(hex.clone(), value);
                if CORE_VCP_CODES.contains(&code) {
                    asus_compat.insert(hex, i32::from(value));
                }
            }
            Err(e) => {
                last_err = Some(e);
            }
        }
    }

    if vcp_values.is_empty()
        && let Some(err) = last_err
    {
        return Err(err);
    }

    Ok(MonitorProfile {
        name: name.to_string(),
        model: mon.model.clone(),
        vcp_values,
        asus_compat,
    })
}

pub fn apply_profile(mon: &mut Monitor, profile: &MonitorProfile) -> Result<usize> {
    let mut parsed = BTreeMap::new();
    for (key, &val) in &profile.vcp_values {
        if let Some(code) = parse_vcp_code(key) {
            parsed.insert(code, val);
        }
    }

    // Never write disruptive/unsafe or write-only action codes during profile apply.
    for blocked in [0x04u8, 0xD6, 0xE2, 0xE8, 0xEB, 0xEC] {
        parsed.remove(&blocked);
    }

    let mut applied = 0usize;

    // ASUS firmware resets brightness/contrast/RGB registers on picture-mode changes (0xDC).
    if let Some(mode) = parsed.remove(&0xDC) {
        mon.set_vcp(0xDC, mode)?;
        applied += 1;
        sleep(Duration::from_millis(150));
    }

    // Write Color Temperature (0x14) before RGB gains (0x16/0x18/0x1A).
    if let Some(temp) = parsed.remove(&0x14) {
        mon.set_vcp(0x14, temp)?;
        applied += 1;
    }

    for (code, value) in parsed {
        let safe_val = if code == 0xFD {
            value & !PIXEL_CLEANING_BIT
        } else {
            value
        };
        mon.set_vcp(code, safe_val)?;
        applied += 1;
    }

    Ok(applied)
}

fn parse_vcp_code(key: &str) -> Option<u8> {
    let trimmed = key.trim();
    let hex = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    if hex.is_empty() || hex.len() > 2 {
        return None;
    }
    u8::from_str_radix(hex, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_names() {
        assert_eq!(sanitize_profile_name("FPS Gaming"), "FPS Gaming");
        assert_eq!(sanitize_profile_name("my_profile-1"), "my_profile-1");
        assert_eq!(sanitize_profile_name("../../etc/passwd"), "etc_passwd");
        assert_eq!(sanitize_profile_name("...///"), "profile");
        assert_eq!(sanitize_profile_name(""), "profile");
    }

    #[test]
    fn round_trip_json() {
        let mut vcp_values = BTreeMap::new();
        vcp_values.insert("0xDC".into(), 4);
        vcp_values.insert("0x10".into(), 75);
        vcp_values.insert("0x12".into(), 80);

        let mut asus_compat = BTreeMap::new();
        asus_compat.insert("0xDC".into(), 4);
        asus_compat.insert("0x10".into(), 75);

        let profile = MonitorProfile {
            name: "Coding".into(),
            model: "XG27ACDMS".into(),
            vcp_values,
            asus_compat,
        };

        let json = serde_json::to_string_pretty(&profile).unwrap();
        let decoded: MonitorProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, profile);
    }

    #[test]
    fn import_raw_asus_displaywidget_json() {
        let raw = r#"{"0xDC": 4, "0x10": 80, "0x12": 80}"#;
        let profile = parse_imported_profile(raw, "dw_export").unwrap();
        assert_eq!(profile.name, "dw_export");
        assert_eq!(profile.vcp_values.get("0xDC"), Some(&4));
        assert_eq!(profile.vcp_values.get("0x10"), Some(&80));
        assert_eq!(profile.vcp_values.get("0x12"), Some(&80));
        assert_eq!(profile.asus_compat.get("0xDC"), Some(&4));
        assert_eq!(profile.asus_compat.get("0x10"), Some(&80));
        assert_eq!(profile.asus_compat.get("0x12"), Some(&80));
    }

    #[test]
    fn capture_and_apply_order_and_safety() {
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};
        use crate::{Ddc, Edid, Vcp};

        struct RecordingDdc {
            values: HashMap<u8, Vcp>,
            writes: Arc<Mutex<Vec<(u8, u16)>>>,
        }

        impl Ddc for RecordingDdc {
            fn get(&mut self, code: u8) -> Result<Vcp> {
                self.values
                    .get(&code)
                    .copied()
                    .ok_or_else(|| format!("0x{code:02X} unsupported"))
            }

            fn set(&mut self, code: u8, value: u16) -> Result<()> {
                self.writes.lock().unwrap().push((code, value));
                if let Some(v) = self.values.get_mut(&code) {
                    v.current = value;
                }
                Ok(())
            }

            fn capabilities(&mut self) -> Result<String> {
                Ok("(prot(monitor)type(LCD)model(XG27ACDMS)vcp(10 12 14(05 0B) 16 18 1A D6(01 05) DC(04 0A) E2(00 01) EC(00 01) EF FD(7879) FF))".into())
            }
        }

        let writes = Arc::new(Mutex::new(Vec::new()));
        let mut values = HashMap::new();
        values.insert(0x10, Vcp { current: 70, max: 100 });
        values.insert(0x12, Vcp { current: 80, max: 100 });
        values.insert(0x14, Vcp { current: 0x0B, max: 0x0B });
        values.insert(0x16, Vcp { current: 50, max: 100 });
        values.insert(0x18, Vcp { current: 51, max: 100 });
        values.insert(0x1A, Vcp { current: 52, max: 100 });
        values.insert(0xD6, Vcp { current: 1, max: 5 });
        values.insert(0xDC, Vcp { current: 4, max: 10 });
        values.insert(0xE2, Vcp { current: 0, max: 4 });
        values.insert(0xEC, Vcp { current: 0, max: 1 });
        values.insert(0xEF, Vcp { current: 0xB1, max: 0x0217 });
        values.insert(0xFD, Vcp { current: 0x7879, max: 0x7879 });
        values.insert(0xFF, Vcp { current: 0x30, max: 102 });

        let edid = Edid {
            manufacturer: "AUS".into(),
            name: Some("XG27ACDMS".into()),
            serial: None,
        };
        let mut mon = Monitor::new(
            Box::new(RecordingDdc {
                values,
                writes: Arc::clone(&writes),
            }),
            "fake".into(),
            None,
            Some(edid),
            None,
        );

        let mut captured = capture_profile(&mut mon, "Work").unwrap();
        assert!(!captured.vcp_values.contains_key("0xD6"));
        assert!(!captured.vcp_values.contains_key("0xEC"));
        assert!(!captured.vcp_values.contains_key("0xE2"));
        assert_eq!(captured.vcp_values.get("0xDC"), Some(&4));
        assert_eq!(captured.vcp_values.get("0xFD"), Some(&(0x7879 & !PIXEL_CLEANING_BIT)));

        // Inject forbidden codes to verify apply_profile blocks them and strips pixel-cleaning bit.
        captured.vcp_values.insert("0xD6".into(), 5);
        captured.vcp_values.insert("0x04".into(), 1);
        captured.vcp_values.insert("0xEC".into(), 1);
        captured.vcp_values.insert("0xE2".into(), 1);
        captured.vcp_values.insert("0xFD".into(), 0x7879);

        let count = apply_profile(&mut mon, &captured).unwrap();
        let recorded = writes.lock().unwrap();
        assert_eq!(count, recorded.len());
        assert_eq!(recorded[0], (0xDC, 4), "0xDC must be written first");
        assert_eq!(recorded[1], (0x14, 0x0B), "0x14 must be written before RGB gains");
        assert!(!recorded.iter().any(|&(c, _)| matches!(c, 0x04 | 0xD6 | 0xE2 | 0xEC)));
        let fd_write = recorded.iter().find(|&&(c, _)| c == 0xFD).unwrap();
        assert_eq!(fd_write.1 & PIXEL_CLEANING_BIT, 0, "pixel cleaning bit must be masked off");
    }
}
