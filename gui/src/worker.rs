use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use asusdisplay::appsync::{self, AppSyncConfig};
use asusdisplay::lightbar::{self, Effect};
use asusdisplay::profiles::{self, MonitorProfile};
use asusdisplay::{Feature, Kind, Monitor, Options, ProductLine};

pub type RgbEffect = Effect;

const HERO_VCP_CODES: [u8; 4] = [0x10, 0x12, 0xDC, 0x60];
const VCP_BRIGHTNESS: u8 = 0x10;
const VCP_COLOR_TEMP: u8 = 0x14;
const VCP_RED_GAIN: u8 = 0x16;
const VCP_GREEN_GAIN: u8 = 0x18;
const VCP_BLUE_GAIN: u8 = 0x1A;
#[cfg(target_os = "linux")]
const VCP_ORIENTATION: u8 = 0xAA;
const VCP_PICTURE_MODE: u8 = 0xDC;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorInfo {
    pub id: String,
    pub model_name: String,
    pub connector: String,
    pub serial: Option<String>,
    pub is_asus: bool,
    pub is_oled: bool,
    pub product_line: ProductLine,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightBarState {
    pub on: bool,
    pub brightness: u8,
    pub cct_step: u8,
    pub kelvin: u16,
    pub is_rgb: bool,
    pub effect: RgbEffect,
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub speed: u8,
}

impl Default for LightBarState {
    fn default() -> Self {
        Self {
            on: false,
            brightness: 70,
            cct_step: 2,
            kelvin: 4600,
            is_rgb: false,
            effect: RgbEffect::Static,
            r: 255,
            g: 255,
            b: 255,
            speed: 3,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FeatureState {
    pub code: u8,
    pub name: String,
    pub cli_name: String,
    pub section: String,
    pub group: String,
    pub description: String,
    pub current: u16,
    pub max: u16,
    pub raw_sh: u8,
    pub raw_sl: u8,
    pub read_only: bool,
    pub write_only: bool,
    pub supported: bool,
    pub values: Vec<(u16, String)>,
}

#[derive(Debug, Clone)]
pub struct LightBarSnapshot {
    pub connected: bool,
    pub product: String,
    pub path: String,
    pub state: LightBarState,
}

#[derive(Debug, Clone)]
pub enum WorkerCommand {
    Discover,
    LoadMonitor { index: usize },
    SetVcp { index: usize, code: u8, value: u16 },
    ApplyColorPreset { index: usize, preset_val: u16, r: Option<u16>, g: Option<u16>, b: Option<u16> },
    StepBrightness { delta: i16 },
    SaveProfile { index: usize, name: String },
    ApplyProfile { index: usize, name: String },
    DeleteProfile { name: String },
    ExportProfile { index: usize, name: String, path: PathBuf },
    ImportProfile { path: PathBuf },
    SaveAppSyncConfig { config: AppSyncConfig },
    SetAutoRotate { enabled: bool },
    GenerateReport { index: usize },
    RefreshLightBar,
    SetLightBarPower { on: bool },
    SetLightBarBrightness { level: u8 },
    SetLightBarCct { step: u8 },
    SetLightBarEffect { effect: RgbEffect, r: u8, g: u8, b: u8, speed: u8 },
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum WorkerEvent {
    MonitorsDiscovered {
        monitors: Vec<MonitorInfo>,
    },
    MonitorHeroLoaded {
        index: usize,
        features: Vec<FeatureState>,
    },
    MonitorLoaded {
        index: usize,
        info: MonitorInfo,
        features: Vec<FeatureState>,
    },
    VcpUpdated {
        index: usize,
        code: u8,
        current: u16,
    },
    LightBarLoaded {
        snapshot: Option<LightBarSnapshot>,
    },
    ProfilesUpdated {
        profiles: Vec<MonitorProfile>,
    },
    AppSyncUpdated {
        config: AppSyncConfig,
        active_rule: Option<String>,
    },
    ReportGenerated {
        markdown: String,
    },
    StatusMessage {
        message: String,
        is_error: bool,
    },
}

#[derive(Clone)]
pub struct WorkerHandle {
    pub cmd_tx: Sender<WorkerCommand>,
}

impl WorkerHandle {
    pub fn send(&self, cmd: WorkerCommand) {
        let _ = self.cmd_tx.send(cmd);
    }
}

fn monitor_info(m: &Monitor) -> MonitorInfo {
    MonitorInfo {
        id: m.id.clone(),
        model_name: m.model.clone(),
        connector: m.connector.clone().unwrap_or_default(),
        serial: m.edid.as_ref().and_then(|e| e.serial.clone()),
        is_asus: m.is_asus(),
        is_oled: m.is_oled(),
        product_line: m.product_line(),
    }
}

fn feature_to_state(
    f: &Feature,
    current: u16,
    max: u16,
    unavailable: bool,
    supported: bool,
    declared_values: Option<&[u16]>,
    reason_override: Option<String>,
) -> FeatureState {
    let values: Vec<(u16, String)> = if let Some(declared) = declared_values
        && !declared.is_empty()
    {
        let filtered: Vec<(u16, String)> = f
            .choices
            .iter()
            .filter(|c| {
                let v = c.raw;
                declared.contains(&v)
                    || (v > 0xFF
                        && (declared.contains(&(v & 0xFF)) || declared.contains(&(v >> 8))))
            })
            .map(|c| (c.raw, c.label.clone()))
            .collect();
        if filtered.is_empty() {
            f.choices.iter().map(|c| (c.raw, c.label.clone())).collect()
        } else {
            filtered
        }
    } else {
        f.choices.iter().map(|c| (c.raw, c.label.clone())).collect()
    };

    FeatureState {
        code: f.code,
        name: f.label.to_string(),
        cli_name: f.key.to_string(),
        section: f.group.to_string(),
        group: f.group.to_string(),
        description: reason_override.unwrap_or_else(|| f.note.unwrap_or("").to_string()),
        current,
        max,
        raw_sh: (current >> 8) as u8,
        raw_sl: (current & 0xFF) as u8,
        read_only: f.kind == Kind::Info || unavailable,
        write_only: f.kind == Kind::Action,
        supported,
        values,
    }
}

fn step_to_kelvin(step: u8) -> u16 {
    match step.min(4) {
        0 => 2700,
        1 => 3650,
        2 => 4600,
        3 => 5550,
        _ => 6500,
    }
}

fn parse_vcp_hex(key: &str) -> Option<u8> {
    let trimmed = key.trim();
    let hex = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    u8::from_str_radix(hex, 16).ok()
}

struct WorkerState {
    monitors: Vec<Monitor>,
    monitor_infos: Vec<MonitorInfo>,
    active_monitor: Option<usize>,
    cached_brightness: Option<(usize, u16)>,
    cached_features: HashMap<usize, Vec<FeatureState>>,
    appsync_config: AppSyncConfig,
    last_matched_rule: Option<String>,
    auto_rotate_enabled: bool,
    last_orientation: Option<u16>,
    lightbar_state: LightBarState,
    ev_tx: Sender<WorkerEvent>,
}

impl WorkerState {
    fn new(ev_tx: Sender<WorkerEvent>) -> Self {
        let appsync_config = appsync::load_appsync_config();
        Self {
            monitors: Vec::new(),
            monitor_infos: Vec::new(),
            active_monitor: None,
            cached_brightness: None,
            cached_features: HashMap::new(),
            appsync_config,
            last_matched_rule: None,
            auto_rotate_enabled: false,
            last_orientation: None,
            lightbar_state: LightBarState::default(),
            ev_tx,
        }
    }

    fn emit(&self, event: WorkerEvent) {
        let _ = self.ev_tx.send(event);
    }

    fn emit_status(&self, message: impl Into<String>, is_error: bool) {
        self.emit(WorkerEvent::StatusMessage {
            message: message.into(),
            is_error,
        });
    }

    fn send_initial_state(&mut self) {
        let profiles = profiles::list_profiles();
        self.emit(WorkerEvent::ProfilesUpdated { profiles });

        self.emit(WorkerEvent::AppSyncUpdated {
            config: self.appsync_config.clone(),
            active_rule: self.last_matched_rule.clone(),
        });

        self.refresh_lightbar();
        self.discover();
    }

    fn discover(&mut self) {
        let monitors = asusdisplay::enumerate(&Options::default());
        let infos: Vec<MonitorInfo> = monitors.iter().map(monitor_info).collect();
        self.monitors = monitors;
        self.monitor_infos = infos.clone();
        self.cached_features.clear();
        self.cached_brightness = None;
        self.active_monitor = if self.monitors.is_empty() { None } else { Some(0) };

        self.emit(WorkerEvent::MonitorsDiscovered { monitors: infos });
    }

    fn load_monitor(&mut self, index: usize) {
        if index >= self.monitors.len() {
            self.emit_status(format!("Monitor index {index} out of range"), true);
            return;
        }
        self.active_monitor = Some(index);

        let caps_map = self.monitors[index]
            .capabilities()
            .ok()
            .map(|c| c.vcp.clone());

        // Stage 1: Fast read of the 4 hero VCP codes so the Quick-Control Deck populates quickly.
        let table_defs = self.monitors[index].features(true);
        let mut hero_features = Vec::with_capacity(HERO_VCP_CODES.len());
        let mut hero_by_code: HashMap<u8, FeatureState> = HashMap::new();

        for &code in &HERO_VCP_CODES {
            if let Some(f) = table_defs.iter().find(|f| f.code == code) {
                let declared = caps_map.as_ref().and_then(|m| m.get(&f.code)).map(Vec::as_slice);
                let state = match self.monitors[index].read(f) {
                    Ok(reading) => {
                        if code == VCP_BRIGHTNESS && !reading.unavailable {
                            self.cached_brightness = Some((index, reading.current));
                        }
                        feature_to_state(
                            f,
                            reading.current,
                            reading.max,
                            reading.unavailable,
                            true,
                            declared,
                            None,
                        )
                    }
                    Err(_) => feature_to_state(f, 0, 100, false, false, declared, None),
                };
                hero_by_code.insert(code, state.clone());
                hero_features.push(state);
            }
        }

        self.emit(WorkerEvent::MonitorHeroLoaded {
            index,
            features: hero_features,
        });

        // Stage 2: Read all remaining supported features, plus unsupported diagnostics.
        let supported_defs = self.monitors[index].features(false);
        let unsupported_defs = self.monitors[index].unsupported();
        let mut all_features =
            Vec::with_capacity(supported_defs.len() + unsupported_defs.len());

        for f in &supported_defs {
            if f.kind != Kind::Toggle
                && let Some(existing) = hero_by_code.get(&f.code)
            {
                all_features.push(existing.clone());
                continue;
            }

            let declared = caps_map.as_ref().and_then(|m| m.get(&f.code)).map(Vec::as_slice);
            let state = if f.kind == Kind::Action {
                let max = f.choices.iter().map(|c| c.raw).max().unwrap_or(1);
                feature_to_state(f, 0, max, false, true, declared, None)
            } else {
                match self.monitors[index].read(f) {
                    Ok(reading) => feature_to_state(
                        f,
                        reading.current,
                        reading.max,
                        reading.unavailable,
                        true,
                        declared,
                        None,
                    ),
                    Err(_) => feature_to_state(f, 0, 0, false, false, declared, None),
                }
            };
            all_features.push(state);
        }

        for sup in unsupported_defs {
            let declared = caps_map
                .as_ref()
                .and_then(|m| m.get(&sup.feature.code))
                .map(Vec::as_slice);
            all_features.push(feature_to_state(
                &sup.feature,
                0,
                0,
                true,
                false,
                declared,
                Some(sup.reason),
            ));
        }

        let info = monitor_info(&self.monitors[index]);
        self.monitor_infos[index] = info.clone();
        self.cached_features.insert(index, all_features.clone());
        self.emit(WorkerEvent::MonitorLoaded {
            index,
            info,
            features: all_features,
        });
    }

    fn update_cached_feature(&mut self, index: usize, code: u8, current: u16) {
        if code == VCP_BRIGHTNESS {
            self.cached_brightness = Some((index, current));
        }
        if let Some(list) = self.cached_features.get_mut(&index) {
            for feat in list.iter_mut() {
                if feat.code == code {
                    feat.current = current;
                    feat.raw_sh = (current >> 8) as u8;
                    feat.raw_sl = (current & 0xFF) as u8;
                }
            }
        }
    }

    fn set_vcp(&mut self, index: usize, code: u8, value: u16) {
        let Some(monitor) = self.monitors.get_mut(index) else {
            self.emit_status(format!("Monitor index {index} not available"), true);
            return;
        };

        match monitor.set_vcp(code, value) {
            Ok(()) => {
                self.update_cached_feature(index, code, value);
                self.emit(WorkerEvent::VcpUpdated {
                    index,
                    code,
                    current: value,
                });
            }
            Err(e) => {
                self.emit_status(format!("Failed to set VCP 0x{code:02X}: {e}"), true);
            }
        }
    }

    fn apply_color_preset(
        &mut self,
        index: usize,
        preset_val: u16,
        r: Option<u16>,
        g: Option<u16>,
        b: Option<u16>,
    ) {
        self.set_vcp(index, VCP_COLOR_TEMP, preset_val);
        if let Some(rv) = r {
            self.set_vcp(index, VCP_RED_GAIN, rv);
        }
        if let Some(gv) = g {
            self.set_vcp(index, VCP_GREEN_GAIN, gv);
        }
        if let Some(bv) = b {
            self.set_vcp(index, VCP_BLUE_GAIN, bv);
        }
    }

    fn step_brightness(&mut self, delta: i16) {
        let index = self.active_monitor.unwrap_or(0);
        if index >= self.monitors.len() {
            return;
        }

        let current = match self.cached_brightness {
            Some((cached_idx, val)) if cached_idx == index => val,
            _ => match self.monitors[index].get_vcp(VCP_BRIGHTNESS) {
                Ok(vcp) => {
                    self.cached_brightness = Some((index, vcp.current));
                    vcp.current
                }
                Err(_) => 50,
            },
        };

        let new_brightness = (current as i16 + delta).clamp(0, 100) as u16;
        self.set_vcp(index, VCP_BRIGHTNESS, new_brightness);
    }

    fn save_profile(&mut self, index: usize, name: String) {
        let Some(monitor) = self.monitors.get_mut(index) else {
            self.emit_status(format!("Monitor index {index} not available"), true);
            return;
        };
        match profiles::capture_profile(monitor, &name) {
            Ok(profile) => match profiles::save_profile_to_disk(&profile) {
                Ok(_) => {
                    let profiles = profiles::list_profiles();
                    self.emit(WorkerEvent::ProfilesUpdated { profiles });
                    self.emit_status(format!("Saved profile \"{name}\""), false);
                }
                Err(e) => self.emit_status(format!("Failed to save profile: {e}"), true),
            },
            Err(e) => self.emit_status(format!("Failed to capture profile: {e}"), true),
        }
    }

    fn apply_profile(&mut self, index: usize, name: &str) {
        let Some(monitor) = self.monitors.get_mut(index) else {
            self.emit_status(format!("Monitor index {index} not available"), true);
            return;
        };
        let profile = match profiles::load_profile(name) {
            Ok(p) => p,
            Err(e) => {
                self.emit_status(e, true);
                return;
            }
        };

        match profiles::apply_profile(monitor, &profile) {
            Ok(count) => {
                let mut updates = Vec::new();
                for (key, &val) in &profile.vcp_values {
                    if let Some(code) = parse_vcp_hex(key) {
                        updates.push((code, val));
                    }
                }
                for (code, val) in updates {
                    self.update_cached_feature(index, code, val);
                    self.emit(WorkerEvent::VcpUpdated {
                        index,
                        code,
                        current: val,
                    });
                }
                self.emit_status(
                    format!("Applied profile \"{name}\" ({count} settings)"),
                    false,
                );
            }
            Err(e) => self.emit_status(format!("Failed to apply profile \"{name}\": {e}"), true),
        }
    }

    fn delete_profile(&mut self, name: &str) {
        match profiles::delete_profile(name) {
            Ok(()) => {
                let profiles = profiles::list_profiles();
                self.emit(WorkerEvent::ProfilesUpdated { profiles });
                self.emit_status(format!("Deleted profile \"{name}\""), false);
            }
            Err(e) => self.emit_status(format!("Failed to delete profile: {e}"), true),
        }
    }

    fn export_profile(&mut self, index: usize, name: String, path: PathBuf) {
        let profile = match profiles::load_profile(&name) {
            Ok(p) => Ok(p),
            Err(_) => match self.monitors.get_mut(index) {
                Some(mon) => profiles::capture_profile(mon, &name),
                None => Err(format!("Monitor index {index} not available")),
            },
        };

        match profile {
            Ok(p) => match profiles::export_profile(&p, &path) {
                Ok(()) => {
                    self.emit_status(format!("Exported profile to {}", path.display()), false)
                }
                Err(e) => self.emit_status(format!("Export failed: {e}"), true),
            },
            Err(e) => self.emit_status(e, true),
        }
    }

    fn import_profile(&mut self, path: PathBuf) {
        match profiles::import_profile(&path) {
            Ok(profile) => {
                let name = profile.name;
                let profiles = profiles::list_profiles();
                self.emit(WorkerEvent::ProfilesUpdated { profiles });
                self.emit_status(format!("Imported profile \"{name}\""), false);
            }
            Err(e) => self.emit_status(format!("Import failed: {e}"), true),
        }
    }

    fn save_appsync_config(&mut self, config: AppSyncConfig) {
        match appsync::save_appsync_config(&config) {
            Ok(()) => {
                self.appsync_config = config.clone();
                self.last_matched_rule = None;
                self.emit(WorkerEvent::AppSyncUpdated {
                    config,
                    active_rule: None,
                });
                self.emit_status("Saved AppSync rules", false);
            }
            Err(e) => self.emit_status(format!("Failed to save AppSync config: {e}"), true),
        }
    }

    fn generate_report(&mut self, index: usize) {
        let Some(monitor) = self.monitors.get_mut(index) else {
            self.emit_status(format!("Monitor index {index} not available"), true);
            return;
        };
        let markdown = asusdisplay::report::generate_device_report(monitor);
        self.emit(WorkerEvent::ReportGenerated { markdown });
    }

    fn refresh_lightbar(&mut self) {
        let bars = lightbar::find();
        let snapshot = bars.into_iter().next().map(|bar| LightBarSnapshot {
            connected: true,
            product: "ASUS Screen Light Bar".to_string(),
            path: bar.path().to_string(),
            state: self.lightbar_state.clone(),
        });
        self.emit(WorkerEvent::LightBarLoaded { snapshot });
    }

    fn emit_lightbar_snapshot(&self, path: &str) {
        self.emit(WorkerEvent::LightBarLoaded {
            snapshot: Some(LightBarSnapshot {
                connected: true,
                product: "ASUS Screen Light Bar".to_string(),
                path: path.to_string(),
                state: self.lightbar_state.clone(),
            }),
        });
    }

    fn apply_lightbar_state(&mut self) {
        let mut bars = lightbar::find();
        let Some(bar) = bars.first_mut() else {
            self.emit(WorkerEvent::LightBarLoaded { snapshot: None });
            self.emit_status("No ASUS Light Bar detected", true);
            return;
        };
        let path = bar.path().to_string();
        let res = if !self.lightbar_state.on {
            bar.off()
        } else if self.lightbar_state.is_rgb {
            bar.set_rgb(
                self.lightbar_state.effect,
                self.lightbar_state.r,
                self.lightbar_state.g,
                self.lightbar_state.b,
            )
        } else {
            bar.set_white(self.lightbar_state.brightness, self.lightbar_state.kelvin)
        };

        match res {
            Ok(()) => self.emit_lightbar_snapshot(&path),
            Err(e) => self.emit_status(format!("Light Bar update failed: {e}"), true),
        }
    }

    fn set_lightbar_power(&mut self, on: bool) {
        self.lightbar_state.on = on;
        self.apply_lightbar_state();
    }

    fn set_lightbar_brightness(&mut self, level: u8) {
        self.lightbar_state.on = level > 0;
        self.lightbar_state.brightness = level.min(100);
        self.lightbar_state.is_rgb = false;
        self.apply_lightbar_state();
    }

    fn set_lightbar_cct(&mut self, step: u8) {
        self.lightbar_state.on = true;
        self.lightbar_state.cct_step = step.min(4);
        self.lightbar_state.kelvin = step_to_kelvin(step);
        self.lightbar_state.is_rgb = false;
        self.apply_lightbar_state();
    }

    fn set_lightbar_effect(&mut self, effect: RgbEffect, r: u8, g: u8, b: u8, speed: u8) {
        self.lightbar_state.on = true;
        self.lightbar_state.is_rgb = true;
        self.lightbar_state.effect = effect;
        self.lightbar_state.r = r;
        self.lightbar_state.g = g;
        self.lightbar_state.b = b;
        self.lightbar_state.speed = speed;
        self.apply_lightbar_state();
    }

    fn poll_appsync(&mut self) {
        if !self.appsync_config.enabled || self.monitors.is_empty() {
            return;
        }
        let procs = appsync::list_running_processes();
        let matched = appsync::match_active_rule(&self.appsync_config, &procs).cloned();
        let matched_name = matched.as_ref().map(|r| r.name.clone());

        if matched_name == self.last_matched_rule {
            return;
        }
        self.last_matched_rule = matched_name.clone();

        let index = self.active_monitor.unwrap_or(0);
        if let Some(rule) = matched {
            if let Some(ref prof_name) = rule.target_profile {
                self.apply_profile(index, prof_name);
            } else if let Some(mode_val) = rule.target_mode {
                self.set_vcp(index, VCP_PICTURE_MODE, mode_val);
            }
        } else if let Some(default_mode) = self.appsync_config.default_mode {
            self.set_vcp(index, VCP_PICTURE_MODE, default_mode);
        }

        self.emit(WorkerEvent::AppSyncUpdated {
            config: self.appsync_config.clone(),
            active_rule: matched_name,
        });
    }

    #[cfg(target_os = "linux")]
    fn poll_auto_rotate(&mut self) {
        if !self.auto_rotate_enabled {
            return;
        }
        let index = self.active_monitor.unwrap_or(0);
        let Some(monitor) = self.monitors.get_mut(index) else {
            return;
        };
        let Ok(vcp) = monitor.get_vcp(VCP_ORIENTATION) else {
            return;
        };
        let orient = vcp.current & 0xFF;
        if !matches!(orient, 1..=4) || self.last_orientation == Some(orient) {
            return;
        }
        self.last_orientation = Some(orient);

        let connector = self
            .monitor_infos
            .get(index)
            .map(|i| i.connector.as_str())
            .unwrap_or("");
        if connector.is_empty() {
            return;
        }

        let (xrandr_rot, kscreen_rot, hypr_transform, deg_label) = match orient {
            1 => ("normal", "none", "0", "0°"),
            2 => ("right", "right", "1", "90°"),
            3 => ("inverted", "inverted", "2", "180°"),
            4 => ("left", "left", "3", "270°"),
            _ => return,
        };

        let rotated = std::process::Command::new("xrandr")
            .args(["--output", connector, "--rotate", xrandr_rot])
            .output()
            .is_ok_and(|o| o.status.success())
            || std::process::Command::new("kscreen-doctor")
                .arg(format!("output.{connector}.rotation.{kscreen_rot}"))
                .output()
                .is_ok_and(|o| o.status.success())
            || std::process::Command::new("hyprctl")
                .args([
                    "keyword",
                    "monitor",
                    &format!("{connector},preferred,auto,1,transform,{hypr_transform}"),
                ])
                .output()
                .is_ok_and(|o| o.status.success());

        if rotated {
            self.emit_status(format!("Auto-rotated {connector} to {deg_label}"), false);
        } else {
            self.emit_status(
                format!("Orientation changed to {deg_label} (compositor tool unavailable)"),
                false,
            );
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn poll_auto_rotate(&mut self) {}
}

pub fn spawn_worker() -> (WorkerHandle, Receiver<WorkerEvent>) {
    let (cmd_tx, cmd_rx) = mpsc::channel::<WorkerCommand>();
    let (ev_tx, ev_rx) = mpsc::channel::<WorkerEvent>();

    let _ = thread::Builder::new()
        .name("asusdisplay-worker".into())
        .spawn(move || {
            let mut state = WorkerState::new(ev_tx);
            state.send_initial_state();

            let mut pending: VecDeque<WorkerCommand> = VecDeque::new();
            let mut last_appsync = Instant::now();
            let mut last_rotate = Instant::now();

            loop {
                let cmd = if let Some(next) = pending.pop_front() {
                    Some(next)
                } else {
                    match cmd_rx.recv_timeout(Duration::from_millis(500)) {
                        Ok(c) => Some(c),
                        Err(mpsc::RecvTimeoutError::Timeout) => None,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                };

                if let Some(cmd) = cmd {
                    match cmd {
                        WorkerCommand::Discover => state.discover(),
                        WorkerCommand::LoadMonitor { index } => state.load_monitor(index),
                        WorkerCommand::SetVcp {
                            index,
                            code,
                            mut value,
                        } => {
                            while let Ok(next) = cmd_rx.try_recv() {
                                pending.push_back(next);
                            }
                            let mut retained = VecDeque::with_capacity(pending.len());
                            for queued in pending.drain(..) {
                                match queued {
                                    WorkerCommand::SetVcp {
                                        index: qi,
                                        code: qc,
                                        value: qv,
                                    } if qi == index && qc == code => {
                                        value = qv;
                                    }
                                    other => retained.push_back(other),
                                }
                            }
                            pending = retained;
                            state.set_vcp(index, code, value);
                        }
                        WorkerCommand::ApplyColorPreset {
                            index,
                            preset_val,
                            r,
                            g,
                            b,
                        } => state.apply_color_preset(index, preset_val, r, g, b),
                        WorkerCommand::StepBrightness { mut delta } => {
                            while let Ok(next) = cmd_rx.try_recv() {
                                pending.push_back(next);
                            }
                            let mut retained = VecDeque::with_capacity(pending.len());
                            for queued in pending.drain(..) {
                                match queued {
                                    WorkerCommand::StepBrightness { delta: d } => {
                                        delta = delta.saturating_add(d);
                                    }
                                    other => retained.push_back(other),
                                }
                            }
                            pending = retained;
                            state.step_brightness(delta);
                        }
                        WorkerCommand::SaveProfile { index, name } => {
                            state.save_profile(index, name)
                        }
                        WorkerCommand::ApplyProfile { index, name } => {
                            state.apply_profile(index, &name)
                        }
                        WorkerCommand::DeleteProfile { name } => state.delete_profile(&name),
                        WorkerCommand::ExportProfile { index, name, path } => {
                            state.export_profile(index, name, path)
                        }
                        WorkerCommand::ImportProfile { path } => state.import_profile(path),
                        WorkerCommand::SaveAppSyncConfig { config } => {
                            state.save_appsync_config(config)
                        }
                        WorkerCommand::SetAutoRotate { enabled } => {
                            state.auto_rotate_enabled = enabled;
                            if !enabled {
                                state.last_orientation = None;
                            }
                        }
                        WorkerCommand::GenerateReport { index } => state.generate_report(index),
                        WorkerCommand::RefreshLightBar => state.refresh_lightbar(),
                        WorkerCommand::SetLightBarPower { on } => state.set_lightbar_power(on),
                        WorkerCommand::SetLightBarBrightness { level } => {
                            state.set_lightbar_brightness(level)
                        }
                        WorkerCommand::SetLightBarCct { step } => state.set_lightbar_cct(step),
                        WorkerCommand::SetLightBarEffect {
                            effect,
                            r,
                            g,
                            b,
                            speed,
                        } => state.set_lightbar_effect(effect, r, g, b, speed),
                        WorkerCommand::Shutdown => break,
                    }
                }

                if last_appsync.elapsed() >= Duration::from_secs(2) {
                    last_appsync = Instant::now();
                    state.poll_appsync();
                }

                if last_rotate.elapsed() >= Duration::from_secs(3) {
                    last_rotate = Instant::now();
                    state.poll_auto_rotate();
                }
            }
        });

    (WorkerHandle { cmd_tx }, ev_rx)
}
