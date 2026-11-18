use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

use asusdisplay::appsync::{self, AppSyncConfig};
use asusdisplay::lightbar::{self, Effect};
use asusdisplay::profiles::{self, MonitorProfile};
use asusdisplay::{Choice, Feature, Kind, Monitor, Options, Reading};

pub type RgbEffect = Effect;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorInfo {
    pub model_name: String,
    pub connector: String,
    pub is_oled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightBarState {
    pub on: bool,
    pub brightness: u8,
    pub cct_step: u8,
    pub is_rgb: bool,
    pub effect: RgbEffect,
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Default for LightBarState {
    fn default() -> Self {
        Self { on: false, brightness: 70, cct_step: 2, is_rgb: false, effect: RgbEffect::Static, r: 255, g: 255, b: 255 }
    }
}

/// One monitor setting as the GUI shows it: the library's `Feature` plus its current `Reading`.
#[derive(Debug, Clone)]
pub struct FeatureState {
    pub key: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub kind: Kind,
    pub note: Option<&'static str>,
    /// Disruptive (power, reset, pixel cleaning): ask before writing.
    pub force: bool,
    /// Only the choices this monitor advertises.
    pub choices: Vec<Choice>,
    pub current: u16,
    pub max: u16,
    pub text: String,
    pub on: bool,
    /// `Choice::name` of the current value.
    pub selected: Option<String>,
    /// Why it can't be changed right now.
    pub blocked: Option<String>,
    pub supported: bool,
}

impl FeatureState {
    fn new(f: &Feature, declared: Option<&[u16]>, reading: Result<Reading, String>) -> Self {
        let mut s = FeatureState {
            key: f.key,
            label: f.label,
            group: f.group,
            kind: f.kind,
            note: f.note,
            force: f.force,
            choices: declared_choices(&f.choices, declared),
            current: 0,
            max: 0,
            text: String::new(),
            on: false,
            selected: None,
            blocked: None,
            supported: true,
        };
        match reading {
            Ok(r) => {
                s.blocked = r.unavailable.then(|| match r.text.as_str() {
                    "n/a in current mode" => "Not available in this picture mode".to_string(),
                    other => capitalize(other),
                });
                s.current = r.current;
                s.max = r.max;
                s.on = r.on.unwrap_or(false);
                s.selected = r.selected;
                s.text = r.text;
            }
            // Actions are write-only, so there's nothing to read.
            Err(_) if f.kind == Kind::Action => {}
            Err(reason) => {
                s.supported = false;
                s.blocked = Some(reason);
            }
        }
        s
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|first| first.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// The choices the monitor lists in its capabilities string. 16-bit values are listed by their low
/// byte (HDR, PIP) or high byte (ProArt presets). Falls back to all choices if none match.
fn declared_choices(all: &[Choice], declared: Option<&[u16]>) -> Vec<Choice> {
    let Some(declared) = declared.filter(|d| !d.is_empty()) else {
        return all.to_vec();
    };
    let listed = |v: u16| declared.contains(&v) || (v > 0xFF && (declared.contains(&(v & 0xFF)) || declared.contains(&(v >> 8))));
    let filtered: Vec<Choice> = all.iter().filter(|c| listed(c.raw)).cloned().collect();
    if filtered.is_empty() { all.to_vec() } else { filtered }
}

#[derive(Debug, Clone)]
pub struct LightBarSnapshot {
    pub state: LightBarState,
}

#[derive(Debug, Clone)]
pub enum WorkerCommand {
    Discover,
    LoadMonitor { index: usize },
    /// `input` is what the CLI takes: a number, a choice name, or on/off.
    Write { index: usize, key: &'static str, input: String, force: bool },
    StepBrightness { index: usize, delta: i16 },
    SaveProfile { index: usize, name: String },
    ApplyProfile { index: usize, name: String },
    DeleteProfile { name: String },
    ExportProfile { name: String, path: PathBuf },
    ImportProfile { path: PathBuf },
    SaveAppSyncConfig { config: AppSyncConfig },
    SetActiveMonitor { index: usize },
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    SetAutoRotate { enabled: bool },
    GenerateReport { index: usize },
    RefreshLightBar,
    SetLightBarPower { on: bool },
    SetLightBarBrightness { level: u8 },
    SetLightBarCct { step: u8 },
    SetLightBarEffect { effect: RgbEffect, r: u8, g: u8, b: u8 },
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum WorkerEvent {
    MonitorsDiscovered { monitors: Vec<MonitorInfo> },
    MonitorLoaded { index: usize, features: Vec<FeatureState> },
    /// The monitor stopped answering, even after detecting monitors again.
    MonitorFailed { index: usize },
    FeatureUpdated { index: usize, feature: FeatureState },
    LightBarLoaded { snapshot: Option<LightBarSnapshot> },
    ProfilesUpdated { profiles: Vec<MonitorProfile> },
    AppSyncUpdated { config: AppSyncConfig, active_rule: Option<String> },
    ReportGenerated { markdown: String },
    StatusMessage { message: String, is_error: bool },
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
        model_name: m.model.clone(),
        connector: m.connector.clone().unwrap_or_default(),
        is_oled: m.is_oled(),
    }
}

fn step_to_kelvin(step: u8) -> u16 {
    [2700, 3650, 4600, 5550, 6500][usize::from(step.min(4))]
}

struct WorkerState {
    monitors: Vec<Monitor>,
    /// Monitors whose features have been read at least once (the rest get pre-read when idle).
    loaded: HashSet<usize>,
    /// Monitor ids that already got an automatic re-detect after going silent.
    retried: HashSet<String>,
    active_monitor: usize,
    show_unsupported: bool,
    appsync_config: AppSyncConfig,
    last_matched_rule: Option<String>,
    auto_rotate_enabled: bool,
    last_orientation: Option<u16>,
    lightbar_state: LightBarState,
    ev_tx: async_channel::Sender<WorkerEvent>,
}

impl WorkerState {
    fn new(ev_tx: async_channel::Sender<WorkerEvent>) -> Self {
        Self {
            monitors: Vec::new(),
            loaded: HashSet::new(),
            retried: HashSet::new(),
            active_monitor: 0,
            show_unsupported: std::env::args().any(|a| a == "--all-vcp"),
            appsync_config: appsync::load_appsync_config(),
            last_matched_rule: None,
            auto_rotate_enabled: false,
            last_orientation: None,
            lightbar_state: LightBarState::default(),
            ev_tx,
        }
    }

    fn emit(&self, event: WorkerEvent) {
        let _ = self.ev_tx.try_send(event);
    }

    fn emit_status(&self, message: impl Into<String>, is_error: bool) {
        self.emit(WorkerEvent::StatusMessage { message: message.into(), is_error });
    }

    fn send_initial_state(&mut self) {
        self.emit(WorkerEvent::ProfilesUpdated { profiles: profiles::list_profiles() });
        self.emit(WorkerEvent::AppSyncUpdated {
            config: self.appsync_config.clone(),
            active_rule: self.last_matched_rule.clone(),
        });
        self.refresh_lightbar();
        self.discover();
    }

    fn discover(&mut self) {
        self.set_monitors(asusdisplay::enumerate(&Options::default()));
    }

    fn set_monitors(&mut self, monitors: Vec<Monitor>) {
        self.monitors = monitors;
        self.loaded.clear();
        let monitors = self.monitors.iter().map(monitor_info).collect();
        self.emit(WorkerEvent::MonitorsDiscovered { monitors });
    }

    /// Picks up monitors that were plugged in or unplugged, or that came back after a graphics
    /// driver reset. Enumerating costs no DDC/CI traffic (Windows asks the OS, Linux reads sysfs
    /// and EDIDs), so this is cheap enough to run every few seconds.
    fn check_displays(&mut self) {
        let fresh = asusdisplay::enumerate(&Options::default());
        let key = |m: &Monitor| (m.id.clone(), m.model.clone());
        if !fresh.iter().map(key).eq(self.monitors.iter().map(key)) {
            return self.set_monitors(fresh);
        }
        // Same monitors. One that went silent gets its fresh handle as soon as it answers again.
        for (index, mut m) in fresh.into_iter().enumerate() {
            if self.retried.contains(&m.id) && m.get_vcp(0x10).is_ok() {
                self.retried.remove(&m.id);
                self.monitors[index] = m;
                self.loaded.remove(&index);
            }
        }
    }

    /// Nothing on the monitor answers. A graphics driver reset leaves Windows' monitor handles dead,
    /// so detect monitors again once; if that doesn't help, tell the GUI.
    fn recover(&mut self, index: usize) {
        let Some(id) = self.monitors.get(index).map(|m| m.id.clone()) else { return };
        if self.retried.insert(id) {
            self.discover();
        } else {
            self.emit(WorkerEvent::MonitorFailed { index });
        }
    }

    fn read_state(m: &mut Monitor, f: &Feature) -> FeatureState {
        let declared = m.capabilities().ok().and_then(|c| c.vcp.get(&f.code).cloned());
        let reading = if f.kind == Kind::Action { Err(String::new()) } else { m.read(f) };
        FeatureState::new(f, declared.as_deref(), reading)
    }

    fn load_monitor(&mut self, index: usize) {
        let show_unsupported = self.show_unsupported;
        let Some(m) = self.monitors.get_mut(index) else { return };
        let defs = m.features(false);
        let readable = defs.iter().filter(|f| f.kind != Kind::Action).count();
        // Settings that fail to read are left out; the monitor didn't really offer them.
        let mut features: Vec<FeatureState> = defs.iter().map(|f| Self::read_state(m, f)).filter(|s| s.supported).collect();
        let answered = features.iter().filter(|s| s.kind != Kind::Action).count();
        if show_unsupported {
            for s in m.unsupported() {
                features.push(FeatureState::new(&s.feature, None, Err(s.reason)));
            }
        }
        let id = m.id.clone();
        self.loaded.insert(index);
        if readable > 0 && answered == 0 {
            return self.recover(index);
        }
        self.retried.remove(&id);
        self.emit(WorkerEvent::MonitorLoaded { index, features });
    }

    fn prewarm_next(&mut self) -> bool {
        match (0..self.monitors.len()).find(|i| !self.loaded.contains(i)) {
            Some(index) => {
                self.load_monitor(index);
                true
            }
            None => false,
        }
    }

    /// Re-reads one setting; false if the monitor didn't answer.
    fn reload_feature(&mut self, index: usize, f: &Feature) -> bool {
        let Some(m) = self.monitors.get_mut(index) else { return false };
        let feature = Self::read_state(m, f);
        let ok = feature.supported;
        if ok {
            self.emit(WorkerEvent::FeatureUpdated { index, feature });
        }
        ok
    }

    fn write(&mut self, index: usize, key: &str, input: &str, force: bool) {
        let Some(m) = self.monitors.get_mut(index) else { return };
        let Some(f) = m.feature(key) else { return };
        if let Err(e) = m.write(&f, input, force) {
            let model = m.model.clone();
            if self.reload_feature(index, &f) {
                self.emit_status(format!("Couldn't change {}: {e}", f.label), true);
            } else {
                self.emit_status(format!("Lost connection to {model}. Looking for monitors again…"), true);
                self.recover(index);
            }
            return;
        }
        match f.kind {
            // A picture mode, colour temperature or reset change re-locks other controls.
            _ if matches!(key, "mode" | "color-temp" | "reset") => self.load_monitor(index),
            // The GUI already shows the new value; re-reading mid-drag would make the slider jump.
            Kind::Continuous | Kind::Action => {}
            _ => {
                self.reload_feature(index, &f);
            }
        }
    }

    fn step_brightness(&mut self, index: usize, delta: i16) {
        self.write(index, "brightness", &format!("{delta:+}"), false);
        let Some(m) = self.monitors.get_mut(index) else { return };
        if let Some(f) = m.feature("brightness") {
            self.reload_feature(index, &f);
        }
    }

    fn save_profile(&mut self, index: usize, name: String) {
        let Some(monitor) = self.monitors.get_mut(index) else { return };
        let result = profiles::capture_profile(monitor, &name).and_then(|p| profiles::save_profile_to_disk(&p));
        match result {
            Ok(_) => {
                self.emit(WorkerEvent::ProfilesUpdated { profiles: profiles::list_profiles() });
                self.emit_status(format!("Saved profile \"{name}\""), false);
            }
            Err(e) => self.emit_status(format!("Couldn't save profile: {e}"), true),
        }
    }

    fn apply_profile(&mut self, index: usize, name: &str) {
        let Some(monitor) = self.monitors.get_mut(index) else { return };
        match profiles::load_profile(name).and_then(|p| profiles::apply_profile(monitor, &p)) {
            Ok(_) => {
                self.emit_status(format!("Applied profile \"{name}\""), false);
                self.load_monitor(index);
            }
            Err(e) => self.emit_status(format!("Couldn't apply profile \"{name}\": {e}"), true),
        }
    }

    fn delete_profile(&mut self, name: &str) {
        match profiles::delete_profile(name) {
            Ok(()) => {
                self.emit(WorkerEvent::ProfilesUpdated { profiles: profiles::list_profiles() });
                self.emit_status(format!("Deleted profile \"{name}\""), false);
            }
            Err(e) => self.emit_status(format!("Couldn't delete profile: {e}"), true),
        }
    }

    fn export_profile(&mut self, name: &str, path: PathBuf) {
        match profiles::load_profile(name).and_then(|p| profiles::export_profile(&p, &path)) {
            Ok(()) => self.emit_status(format!("Exported \"{name}\" to {}", path.display()), false),
            Err(e) => self.emit_status(format!("Export failed: {e}"), true),
        }
    }

    fn import_profile(&mut self, path: PathBuf) {
        match profiles::import_profile(&path) {
            Ok(profile) => {
                self.emit(WorkerEvent::ProfilesUpdated { profiles: profiles::list_profiles() });
                self.emit_status(format!("Imported profile \"{}\"", profile.name), false);
            }
            Err(e) => self.emit_status(format!("Import failed: {e}"), true),
        }
    }

    fn save_appsync_config(&mut self, config: AppSyncConfig) {
        match appsync::save_appsync_config(&config) {
            Ok(()) => {
                self.appsync_config = config.clone();
                self.last_matched_rule = None;
                self.emit(WorkerEvent::AppSyncUpdated { config, active_rule: None });
            }
            Err(e) => self.emit_status(format!("Couldn't save app rules: {e}"), true),
        }
    }

    fn generate_report(&mut self, index: usize) {
        let Some(monitor) = self.monitors.get_mut(index) else { return };
        let markdown = asusdisplay::report::generate_device_report(monitor);
        self.emit(WorkerEvent::ReportGenerated { markdown });
    }

    fn refresh_lightbar(&mut self) {
        let snapshot = lightbar::find().into_iter().next().map(|_| LightBarSnapshot { state: self.lightbar_state.clone() });
        self.emit(WorkerEvent::LightBarLoaded { snapshot });
    }

    fn apply_lightbar_state(&mut self) {
        let mut bars = lightbar::find();
        let Some(bar) = bars.first_mut() else {
            self.emit(WorkerEvent::LightBarLoaded { snapshot: None });
            self.emit_status("Light bar not found", true);
            return;
        };
        let s = &self.lightbar_state;
        let res = if !s.on {
            bar.off()
        } else if s.is_rgb {
            bar.set_rgb(s.effect, s.r, s.g, s.b)
        } else {
            bar.set_white(s.brightness, step_to_kelvin(s.cct_step))
        };
        match res {
            Ok(()) => self.emit(WorkerEvent::LightBarLoaded { snapshot: Some(LightBarSnapshot { state: s.clone() }) }),
            Err(e) => self.emit_status(format!("Light bar update failed: {e}"), true),
        }
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

        let index = self.active_monitor;
        match matched {
            Some(rule) => {
                if let Some(profile) = rule.target_profile {
                    self.apply_profile(index, &profile);
                } else if let Some(mode) = rule.target_mode {
                    self.write(index, "mode", &mode.to_string(), false);
                }
            }
            None => {
                if let Some(mode) = self.appsync_config.default_mode {
                    self.write(index, "mode", &mode.to_string(), false);
                }
            }
        }
        self.emit(WorkerEvent::AppSyncUpdated { config: self.appsync_config.clone(), active_rule: matched_name });
    }

    #[cfg(target_os = "linux")]
    fn poll_auto_rotate(&mut self) {
        if !self.auto_rotate_enabled {
            return;
        }
        let Some(monitor) = self.monitors.get_mut(self.active_monitor) else { return };
        let Ok(vcp) = monitor.get_vcp(0xAA) else { return };
        let orient = vcp.current & 0xFF;
        if !matches!(orient, 1..=4) || self.last_orientation == Some(orient) {
            return;
        }
        self.last_orientation = Some(orient);

        let Some(connector) = monitor.connector.clone() else { return };
        let (xrandr_rot, kscreen_rot, hypr_transform, deg_label) = match orient {
            1 => ("normal", "none", "0", "0°"),
            2 => ("right", "right", "1", "90°"),
            3 => ("inverted", "inverted", "2", "180°"),
            _ => ("left", "left", "3", "270°"),
        };
        let run = |cmd: &str, args: &[&str]| {
            std::process::Command::new(cmd).args(args).output().is_ok_and(|o| o.status.success())
        };
        let rotated = run("xrandr", &["--output", &connector, "--rotate", xrandr_rot])
            || run("kscreen-doctor", &[&format!("output.{connector}.rotation.{kscreen_rot}")])
            || run("hyprctl", &["keyword", "monitor", &format!("{connector},preferred,auto,1,transform,{hypr_transform}")]);
        if rotated {
            self.emit_status(format!("Rotated {connector} to {deg_label}"), false);
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn poll_auto_rotate(&mut self) {}
}

/// Pulls every queued command into `pending`, dropping writes that a later write to the same
/// setting supersedes (slider drags send dozens).
fn coalesce(rx: &mpsc::Receiver<WorkerCommand>, pending: &mut VecDeque<WorkerCommand>) {
    pending.extend(rx.try_iter());
    let mut out: VecDeque<WorkerCommand> = VecDeque::with_capacity(pending.len());
    for cmd in pending.drain(..) {
        if let WorkerCommand::Write { index, key, .. } = &cmd
            && let Some(pos) = out.iter().position(|c| matches!(c, WorkerCommand::Write { index: i, key: k, .. } if i == index && k == key))
        {
            out.remove(pos);
        }
        out.push_back(cmd);
    }
    *pending = out;
}

pub fn spawn_worker() -> (WorkerHandle, async_channel::Receiver<WorkerEvent>) {
    let (cmd_tx, cmd_rx) = mpsc::channel::<WorkerCommand>();
    let (ev_tx, ev_rx) = async_channel::unbounded::<WorkerEvent>();

    let _ = thread::Builder::new().name("asusdisplay-worker".into()).spawn(move || {
        let mut state = WorkerState::new(ev_tx);
        state.send_initial_state();

        let mut pending: VecDeque<WorkerCommand> = VecDeque::new();
        let mut last_appsync = Instant::now();
        let mut last_rotate = Instant::now();
        let mut last_display_check = Instant::now();

        loop {
            coalesce(&cmd_rx, &mut pending);
            let cmd = match pending.pop_front() {
                Some(cmd) => Some(cmd),
                None if state.prewarm_next() => None,
                None => match cmd_rx.recv_timeout(Duration::from_millis(500)) {
                    Ok(c) => {
                        pending.push_back(c);
                        continue;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                },
            };

            if let Some(cmd) = cmd {
                match cmd {
                    WorkerCommand::Discover => {
                        // A manual rescan gives every monitor a fresh chance.
                        state.retried.clear();
                        state.discover();
                    }
                    // Only the first time; writes that change other settings reload on their own.
                    WorkerCommand::LoadMonitor { index } => {
                        if !state.loaded.contains(&index) {
                            state.load_monitor(index);
                        }
                    }
                    WorkerCommand::Write { index, key, input, force } => state.write(index, key, &input, force),
                    WorkerCommand::StepBrightness { index, mut delta } => {
                        // Merge queued scroll steps into one write.
                        pending.retain(|c| match c {
                            WorkerCommand::StepBrightness { index: i, delta: d } if *i == index => {
                                delta = delta.saturating_add(*d);
                                false
                            }
                            _ => true,
                        });
                        state.step_brightness(index, delta);
                    }
                    WorkerCommand::SaveProfile { index, name } => state.save_profile(index, name),
                    WorkerCommand::ApplyProfile { index, name } => state.apply_profile(index, &name),
                    WorkerCommand::DeleteProfile { name } => state.delete_profile(&name),
                    WorkerCommand::ExportProfile { name, path } => state.export_profile(&name, path),
                    WorkerCommand::ImportProfile { path } => state.import_profile(path),
                    WorkerCommand::SaveAppSyncConfig { config } => state.save_appsync_config(config),
                    WorkerCommand::SetActiveMonitor { index } => state.active_monitor = index,
                    WorkerCommand::SetAutoRotate { enabled } => {
                        state.auto_rotate_enabled = enabled;
                        state.last_orientation = None;
                    }
                    WorkerCommand::GenerateReport { index } => state.generate_report(index),
                    WorkerCommand::RefreshLightBar => state.refresh_lightbar(),
                    WorkerCommand::SetLightBarPower { on } => {
                        state.lightbar_state.on = on;
                        state.apply_lightbar_state();
                    }
                    WorkerCommand::SetLightBarBrightness { level } => {
                        let s = &mut state.lightbar_state;
                        (s.on, s.brightness, s.is_rgb) = (level > 0, level.min(100), false);
                        state.apply_lightbar_state();
                    }
                    WorkerCommand::SetLightBarCct { step } => {
                        let s = &mut state.lightbar_state;
                        (s.on, s.cct_step, s.is_rgb) = (true, step.min(4), false);
                        state.apply_lightbar_state();
                    }
                    WorkerCommand::SetLightBarEffect { effect, r, g, b } => {
                        let s = &mut state.lightbar_state;
                        (s.on, s.is_rgb, s.effect, s.r, s.g, s.b) = (true, true, effect, r, g, b);
                        state.apply_lightbar_state();
                    }
                    WorkerCommand::Shutdown => break,
                }
            }

            if last_appsync.elapsed() >= Duration::from_secs(2) {
                last_appsync = Instant::now();
                state.poll_appsync();
            }
            // Windows only: on Linux each pass would probe idle I2C buses for EDIDs every few seconds.
            if cfg!(windows) && pending.is_empty() && last_display_check.elapsed() >= Duration::from_secs(5) {
                last_display_check = Instant::now();
                state.check_displays();
            }
            if last_rotate.elapsed() >= Duration::from_secs(3) {
                last_rotate = Instant::now();
                state.poll_auto_rotate();
            }
        }
    });

    (WorkerHandle { cmd_tx }, ev_rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(raw: u16, name: &str) -> Choice {
        Choice { raw, name: name.into(), label: name.into() }
    }

    #[test]
    fn declared_choices_filter() {
        let all = [choice(0, "off"), choice(1, "red-dot"), choice(7, "blue-dot"), choice(0x0F00, "dp1-b"), choice(0x05FF, "srgb")];
        let names = |d: &[u16]| declared_choices(&all, Some(d)).into_iter().map(|c| c.name).collect::<Vec<_>>();
        assert_eq!(names(&[1, 7]), ["red-dot", "blue-dot"]);
        // 16-bit values match on either byte.
        assert_eq!(names(&[0x0F]), ["dp1-b"]);
        assert_eq!(names(&[0x05]), ["srgb"]);
        // Nothing listed that we know: show everything rather than nothing.
        assert_eq!(declared_choices(&all, Some(&[0x42])).len(), all.len());
        assert_eq!(declared_choices(&all, None).len(), all.len());
    }

    #[test]
    fn coalesce_keeps_last_write_per_setting() {
        let (tx, rx) = mpsc::channel();
        let w = |key, input: &str| WorkerCommand::Write { index: 0, key, input: input.into(), force: false };
        for cmd in [w("brightness", "10"), w("contrast", "5"), w("brightness", "40"), WorkerCommand::RefreshLightBar] {
            tx.send(cmd).unwrap();
        }
        let mut pending = VecDeque::new();
        coalesce(&rx, &mut pending);
        let got: Vec<String> = pending
            .iter()
            .map(|c| match c {
                WorkerCommand::Write { key, input, .. } => format!("{key}={input}"),
                _ => "other".into(),
            })
            .collect();
        assert_eq!(got, ["contrast=5", "brightness=40", "other"]);
    }
}
