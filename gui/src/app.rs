use std::path::PathBuf;

use gpui::{
    AnyElement, ClipboardItem, Context, Entity, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Subscription, Window,
    div, px, rgb,
};

use asusdisplay::appsync::{self, AppRule, AppSyncConfig};
use asusdisplay::profiles::MonitorProfile;

use crate::autostart;
use crate::components::{
    ButtonVariant, TextInputState, action_button, badge, code_badge, pill_selector,
    precision_slider, section_card, text_input_box, toggle_switch,
};
use crate::theme::Theme;
use crate::tray::{SystemTrayHandle, TrayAction, TrayStateSnapshot};
use crate::worker::{
    FeatureState, LightBarSnapshot, MonitorInfo, RgbEffect, WorkerCommand, WorkerEvent,
    WorkerHandle,
};

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

const LIGHTBAR_EFFECTS: [(&str, RgbEffect); 5] = [
    ("Static", RgbEffect::Static),
    ("Breathing", RgbEffect::Breath),
    ("Strobe", RgbEffect::Strobe),
    ("Color Cycle", RgbEffect::Cycle),
    ("Rainbow", RgbEffect::Rainbow),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NavTab {
    DisplayPicture,
    ColorCalibration,
    GamingOled,
    InputsPipKvm,
    LightBar,
    ProfilesAppSync,
    SystemDaemon,
}

impl NavTab {
    const ALL: [NavTab; 7] = [
        NavTab::DisplayPicture,
        NavTab::ColorCalibration,
        NavTab::GamingOled,
        NavTab::InputsPipKvm,
        NavTab::LightBar,
        NavTab::ProfilesAppSync,
        NavTab::SystemDaemon,
    ];

    fn id(self) -> &'static str {
        match self {
            NavTab::DisplayPicture => "tab-display",
            NavTab::ColorCalibration => "tab-color",
            NavTab::GamingOled => "tab-gaming",
            NavTab::InputsPipKvm => "tab-inputs",
            NavTab::LightBar => "tab-lightbar",
            NavTab::ProfilesAppSync => "tab-profiles",
            NavTab::SystemDaemon => "tab-system",
        }
    }

    fn label(self) -> &'static str {
        match self {
            NavTab::DisplayPicture => "Display & Picture",
            NavTab::ColorCalibration => "Color & Calibration",
            NavTab::GamingOled => "Gaming & OLED Care",
            NavTab::InputsPipKvm => "Inputs, PIP & KVM",
            NavTab::LightBar => "Light Bar",
            NavTab::ProfilesAppSync => "Profiles & AppSync",
            NavTab::SystemDaemon => "System & Daemon",
        }
    }

    fn subtitle(self) -> &'static str {
        match self {
            NavTab::DisplayPicture => {
                "GameVisual / Splendid modes, luminance, sharpness, and eye care"
            }
            NavTab::ColorCalibration => {
                "Color temperature presets, RGB gain channels, and 6-axis calibration"
            }
            NavTab::GamingOled => {
                "GamePlus overlays, variable overdrive, and OLED panel protection"
            }
            NavTab::InputsPipKvm => {
                "Active video inputs, multi-frame PIP/PBP layout, and KVM switching"
            }
            NavTab::LightBar => "ASUS ROG Monitor Light Bar USB HID white CCT & Aura RGB effects",
            NavTab::ProfilesAppSync => {
                "Monitor VCP snapshots, JSON import/export, and automatic AppSync rules"
            }
            NavTab::SystemDaemon => {
                "Tray daemon autostart, auto-rotation, OSD controls, and diagnostics"
            }
        }
    }
}

#[derive(Clone)]
pub struct PendingConfirm {
    pub title: String,
    pub description: String,
    pub confirm_label: String,
    pub code: u8,
    pub value: u16,
}

/// Persistent application state that outlives individual GPUI windows when hidden to the tray.
pub struct DaemonState {
    pub worker: WorkerHandle,
    pub tray: Option<SystemTrayHandle>,
    pub window_visible: bool,
    pub monitors: Vec<MonitorInfo>,
    pub active_monitor: usize,
    pub loading_monitors: bool,
    pub loading_features: bool,
    pub features: Vec<FeatureState>,
    pub lightbar: Option<LightBarSnapshot>,
    pub lightbar_on_local: bool,
    pub lightbar_brightness_local: u8,
    pub lightbar_cct_local: u8,
    pub profiles: Vec<MonitorProfile>,
    pub appsync_config: AppSyncConfig,
    pub active_appsync_rule: Option<String>,
    pub running_processes: Vec<String>,
    pub report_markdown: Option<String>,
    pub status_message: Option<(String, bool)>,
    pub auto_rotate_enabled: bool,
    pub autostart_enabled: bool,
}

impl DaemonState {
    pub fn new(
        worker: WorkerHandle,
        tray: Option<SystemTrayHandle>,
        window_visible: bool,
        _cx: &mut Context<Self>,
    ) -> Self {
        let initial_monitor = std::env::args()
            .skip_while(|a| a != "--monitor" && a != "-m")
            .nth(1)
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);

        let state = Self {
            worker,
            tray,
            window_visible,
            monitors: Vec::new(),
            active_monitor: initial_monitor,
            loading_monitors: true,
            loading_features: false,
            features: Vec::new(),
            lightbar: None,
            lightbar_on_local: false,
            lightbar_brightness_local: 75,
            lightbar_cct_local: 2,
            profiles: Vec::new(),
            appsync_config: AppSyncConfig::default(),
            active_appsync_rule: None,
            running_processes: appsync::list_running_processes(),
            report_markdown: None,
            status_message: None,
            auto_rotate_enabled: false,
            autostart_enabled: autostart::is_autostart_enabled(),
        };
        state.sync_tray();
        state
    }

    pub fn sync_window_visible(&mut self, visible: bool) {
        if self.window_visible != visible {
            self.window_visible = visible;
            self.sync_tray();
        }
    }

    pub fn feature(&self, code: u8) -> Option<&FeatureState> {
        self.features.iter().find(|f| f.code == code)
    }

    pub fn discover(&mut self, cx: &mut Context<Self>) {
        self.loading_monitors = true;
        self.worker.send(WorkerCommand::Discover);
        cx.notify();
    }

    pub fn select_monitor(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.monitors.len() {
            self.active_monitor = index;
            self.features.clear();
            self.loading_features = true;
            self.report_markdown = None;
            self.worker.send(WorkerCommand::LoadMonitor { index });
            self.sync_tray();
            cx.notify();
        }
    }

    pub fn set_vcp(&mut self, code: u8, value: u16, cx: &mut Context<Self>) {
        if self.monitors.is_empty() {
            return;
        }
        // Optimistically reflect responsive updates in the UI immediately.
        for feat in &mut self.features {
            if feat.code == code {
                feat.current = value;
                feat.raw_sh = (value >> 8) as u8;
                feat.raw_sl = (value & 0xFF) as u8;
            }
        }
        self.worker.send(WorkerCommand::SetVcp {
            index: self.active_monitor,
            code,
            value,
        });
        self.sync_tray();
        cx.notify();
    }

    pub fn apply_color_preset(
        &mut self,
        preset_val: u16,
        r: Option<u16>,
        g: Option<u16>,
        b: Option<u16>,
        cx: &mut Context<Self>,
    ) {
        if self.monitors.is_empty() {
            return;
        }
        self.worker.send(WorkerCommand::ApplyColorPreset {
            index: self.active_monitor,
            preset_val,
            r,
            g,
            b,
        });
        self.set_status("Applied color preset", false, cx);
    }

    pub fn set_status(&mut self, message: impl Into<String>, is_error: bool, cx: &mut Context<Self>) {
        self.status_message = Some((message.into(), is_error));
        cx.notify();
    }

    pub fn refresh_processes(&mut self, cx: &mut Context<Self>) {
        self.running_processes = appsync::list_running_processes();
        cx.notify();
    }

    pub fn set_autostart(&mut self, enabled: bool, cx: &mut Context<Self>) {
        match autostart::set_autostart_enabled(enabled) {
            Ok(()) => {
                self.autostart_enabled = autostart::is_autostart_enabled();
                let msg = if self.autostart_enabled {
                    "Enabled launch at login (--tray)"
                } else {
                    "Disabled launch at login"
                };
                self.set_status(msg, false, cx);
            }
            Err(err) => {
                self.set_status(format!("Autostart failed: {err}"), true, cx);
            }
        }
    }

    pub fn set_auto_rotate(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.auto_rotate_enabled = enabled;
        self.worker.send(WorkerCommand::SetAutoRotate { enabled });
        self.set_status(
            if enabled {
                "Auto-rotation polling enabled (VCP 0xAA)"
            } else {
                "Auto-rotation polling disabled"
            },
            false,
            cx,
        );
    }

    pub fn handle_worker_event(&mut self, event: WorkerEvent, cx: &mut Context<Self>) {
        match event {
            WorkerEvent::MonitorsDiscovered { monitors } => {
                self.monitors = monitors;
                self.loading_monitors = false;
                self.features.clear();
                if self.monitors.is_empty() {
                    self.active_monitor = 0;
                    self.loading_features = false;
                } else {
                    if self.active_monitor >= self.monitors.len() {
                        self.active_monitor = 0;
                    }
                    self.loading_features = true;
                    self.worker.send(WorkerCommand::LoadMonitor {
                        index: self.active_monitor,
                    });
                }
                self.sync_tray();
                cx.notify();
            }
            WorkerEvent::MonitorHeroLoaded { index, features } => {
                if index == self.active_monitor {
                    for hero in features {
                        if let Some(existing) =
                            self.features.iter_mut().find(|f| f.code == hero.code)
                        {
                            *existing = hero;
                        } else {
                            self.features.push(hero);
                        }
                    }
                    self.sync_tray();
                    cx.notify();
                }
            }
            WorkerEvent::MonitorLoaded {
                index,
                info,
                features,
            } => {
                if index == self.active_monitor {
                    if let Some(slot) = self.monitors.get_mut(index) {
                        *slot = info;
                    }
                    self.features = features;
                    self.loading_features = false;
                    self.sync_tray();
                    cx.notify();
                }
            }
            WorkerEvent::VcpUpdated {
                index,
                code,
                current,
            } => {
                if index == self.active_monitor {
                    for feat in &mut self.features {
                        if feat.code == code {
                            feat.current = current;
                            feat.raw_sh = (current >> 8) as u8;
                            feat.raw_sl = (current & 0xFF) as u8;
                        }
                    }
                    self.sync_tray();
                    cx.notify();
                }
            }
            WorkerEvent::LightBarLoaded { snapshot } => {
                if let Some(snap) = &snapshot {
                    self.lightbar_on_local = snap.state.on;
                    self.lightbar_brightness_local = snap.state.brightness;
                    self.lightbar_cct_local = snap.state.cct_step;
                }
                self.lightbar = snapshot;
                self.sync_tray();
                cx.notify();
            }
            WorkerEvent::ProfilesUpdated { profiles } => {
                self.profiles = profiles;
                self.sync_tray();
                cx.notify();
            }
            WorkerEvent::AppSyncUpdated {
                config,
                active_rule,
            } => {
                self.appsync_config = config;
                self.active_appsync_rule = active_rule;
                cx.notify();
            }
            WorkerEvent::ReportGenerated { markdown } => {
                self.report_markdown = Some(markdown);
                self.status_message = Some(("Device support report ready".into(), false));
                cx.notify();
            }
            WorkerEvent::StatusMessage { message, is_error } => {
                self.status_message = Some((message, is_error));
                cx.notify();
            }
        }
    }

    pub fn handle_tray_action(&mut self, action: TrayAction, cx: &mut Context<Self>) {
        match action {
            TrayAction::SelectMonitor(idx) => self.select_monitor(idx, cx),
            TrayAction::SetVcp { code, value } => self.set_vcp(code, value, cx),
            TrayAction::StepBrightness(delta) => {
                self.worker.send(WorkerCommand::StepBrightness { delta });
            }
            TrayAction::ApplyProfile(name) => {
                self.worker.send(WorkerCommand::ApplyProfile {
                    index: self.active_monitor,
                    name,
                });
            }
            TrayAction::SetLightBarPower(on) => {
                self.lightbar_on_local = on;
                self.worker.send(WorkerCommand::SetLightBarPower { on });
                self.sync_tray();
                cx.notify();
            }
            TrayAction::SetLightBarBrightness(level) => {
                self.lightbar_on_local = level > 0;
                self.lightbar_brightness_local = level;
                self.worker
                    .send(WorkerCommand::SetLightBarBrightness { level });
                self.sync_tray();
                cx.notify();
            }
            TrayAction::Refresh => self.discover(cx),
            TrayAction::ToggleWindow | TrayAction::ShowWindow | TrayAction::Quit => {}
        }
    }

    fn sync_tray(&self) {
        let Some(tray) = &self.tray else {
            return;
        };

        let active_info = self.monitors.get(self.active_monitor);
        let brightness = self
            .feature(0x10)
            .filter(|f| f.supported)
            .map(|f| f.current);
        let contrast = self
            .feature(0x12)
            .filter(|f| f.supported)
            .map(|f| f.current);
        let mode_feat = self.feature(0xDC).filter(|f| f.supported);
        let input_feat = self.feature(0x60).filter(|f| f.supported);

        let snapshot = TrayStateSnapshot {
            monitors: self.monitors.clone(),
            active_monitor: self.active_monitor,
            model_name: active_info
                .map(|m| m.model_name.clone())
                .unwrap_or_default(),
            connector: active_info.map(|m| m.connector.clone()).unwrap_or_default(),
            is_oled: active_info.is_some_and(|m| m.is_oled),
            brightness,
            contrast,
            current_mode: mode_feat.map(|f| f.current),
            available_modes: mode_feat.map(|f| f.values.clone()).unwrap_or_default(),
            current_input: input_feat.map(|f| f.current),
            available_inputs: input_feat.map(|f| f.values.clone()).unwrap_or_default(),
            profiles: self.profiles.iter().map(|p| p.name.clone()).collect(),
            lightbar_connected: self.lightbar.as_ref().is_some_and(|b| b.connected),
            lightbar_on: self.lightbar_on_local,
            window_visible: self.window_visible,
        };

        tray.update(snapshot);
    }

    fn clone_snapshot_for_render(&self) -> DaemonState {
        DaemonState {
            worker: self.worker.clone(),
            tray: None,
            window_visible: self.window_visible,
            monitors: self.monitors.clone(),
            active_monitor: self.active_monitor,
            loading_monitors: self.loading_monitors,
            loading_features: self.loading_features,
            features: self.features.clone(),
            lightbar: self.lightbar.clone(),
            lightbar_on_local: self.lightbar_on_local,
            lightbar_brightness_local: self.lightbar_brightness_local,
            lightbar_cct_local: self.lightbar_cct_local,
            profiles: self.profiles.clone(),
            appsync_config: self.appsync_config.clone(),
            active_appsync_rule: self.active_appsync_rule.clone(),
            running_processes: self.running_processes.clone(),
            report_markdown: self.report_markdown.clone(),
            status_message: self.status_message.clone(),
            auto_rotate_enabled: self.auto_rotate_enabled,
            autostart_enabled: self.autostart_enabled,
        }
    }
}

/// Main desktop window view bound to the persistent `Entity<DaemonState>`.
pub struct MainView {
    daemon: Entity<DaemonState>,
    _daemon_sub: Subscription,
    active_tab: NavTab,
    show_unsupported: bool,
    search_input: TextInputState,
    profile_name_input: TextInputState,
    import_path_input: TextInputState,
    rule_name_input: TextInputState,
    rule_process_input: TextInputState,
    rule_target_mode: u16,
    lb_effect_idx: usize,
    lb_r: u8,
    lb_g: u8,
    lb_b: u8,
    lb_speed: u8,
    pending_confirm: Option<PendingConfirm>,
}

impl MainView {
    pub fn new(daemon: Entity<DaemonState>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let daemon_sub = cx.observe(&daemon, |_, _, cx| cx.notify());
        let show_unsupported = std::env::args().any(|a| a == "--all-vcp");

        Self {
            daemon,
            _daemon_sub: daemon_sub,
            active_tab: NavTab::DisplayPicture,
            show_unsupported,
            search_input: TextInputState::new("Filter features by name or VCP hex (e.g. 0x10)...", cx),
            profile_name_input: TextInputState::new("New profile name (e.g. Night Coding)", cx),
            import_path_input: TextInputState::new("Path to profile .json file to import", cx),
            rule_name_input: TextInputState::new("Rule label (e.g. Counter-Strike 2)", cx),
            rule_process_input: TextInputState::new("Process match (e.g. cs2, mpv, code)", cx),
            rule_target_mode: 2,
            lb_effect_idx: 0,
            lb_r: 59,
            lb_g: 130,
            lb_b: 246,
            lb_speed: 128,
            pending_confirm: None,
        }
    }

    fn tab_for_feature(feat: &FeatureState) -> NavTab {
        let g = feat.group.to_ascii_lowercase();
        let s = feat.section.to_ascii_lowercase();
        let cli = feat.cli_name.to_ascii_lowercase();

        if matches!(
            feat.code,
            0x14 | 0x16 | 0x18 | 0x1A | 0x59..=0x5E | 0x6C | 0x6E | 0x70
        ) || (feat.code == 0xE4 && cli.contains("range"))
            || g.contains("color")
            || s.contains("color")
        {
            return NavTab::ColorCalibration;
        }

        if matches!(feat.code, 0x60 | 0xF3 | 0xF4 | 0xF5 | 0xF6)
            || g.contains("pip")
            || s.contains("pip")
            || cli.contains("kvm")
            || cli.contains("input")
        {
            return NavTab::InputsPipKvm;
        }

        if matches!(feat.code, 0xE0 | 0xE3 | 0xE4 | 0xE7 | 0xE8 | 0xEA | 0xF8 | 0xF9)
            || (feat.code == 0xFD
                && (cli.contains("oled")
                    || cli.contains("pixel")
                    || cli.contains("screen")
                    || cli.contains("logo")
                    || cli.contains("uniform")
                    || cli.contains("taskbar")
                    || cli.contains("boundary")
                    || cli.contains("dimming")
                    || cli.contains("freesync")
                    || cli.contains("target")))
            || g.contains("gaming")
            || g.contains("gameplus")
            || g.contains("oled")
            || s.contains("gaming")
            || s.contains("oled")
        {
            return NavTab::GamingOled;
        }

        if matches!(
            feat.code,
            0x04 | 0x62 | 0x79 | 0xAA | 0xC0 | 0xCC | 0xD5 | 0xD6 | 0xD8 | 0xD9 | 0xEB | 0xEC
                | 0xED | 0xEE | 0xEF | 0xFA | 0xFB | 0xFF
        ) || (feat.code == 0xE1 && cli.contains("power"))
            || g.contains("system")
            || g.contains("osd")
            || g.contains("info")
            || s.contains("system")
            || s.contains("osd")
            || s.contains("info")
        {
            return NavTab::SystemDaemon;
        }

        NavTab::DisplayPicture
    }

    fn tab_badge_count(&self, tab: NavTab, state: &DaemonState) -> usize {
        match tab {
            NavTab::LightBar => usize::from(state.lightbar.as_ref().is_some_and(|b| b.connected)),
            NavTab::ProfilesAppSync => state.profiles.len() + state.appsync_config.rules.len(),
            _ => state
                .features
                .iter()
                .filter(|f| {
                    (f.supported || self.show_unsupported) && Self::tab_for_feature(f) == tab
                })
                .count(),
        }
    }

    fn is_destructive_feature(feat: &FeatureState, next_value: u16) -> Option<PendingConfirm> {
        let cli = feat.cli_name.to_ascii_lowercase();
        if feat.code == 0x04 || cli.contains("factory") {
            return Some(PendingConfirm {
                title: "Confirm Factory Reset".into(),
                description: "VCP 0x04 resets all monitor picture, color, and system settings back to factory defaults.".into(),
                confirm_label: "Reset Monitor".into(),
                code: feat.code,
                value: next_value,
            });
        }
        if feat.code == 0xEC || cli == "reset" {
            return Some(PendingConfirm {
                title: "Reset Current Picture Mode?".into(),
                description: "VCP 0xEC restores every setting inside the active GameVisual / Splendid mode to factory defaults.".into(),
                confirm_label: "Reset Mode".into(),
                code: feat.code,
                value: next_value,
            });
        }
        if (feat.code == 0xE2 && cli.contains("clean"))
            || (feat.code == 0xFD && cli.contains("pixel-clean"))
        {
            return Some(PendingConfirm {
                title: "Start OLED Pixel Cleaning?".into(),
                description: "The panel will turn dark for ~6 minutes while running OLED pixel compensation. Do not unplug the monitor.".into(),
                confirm_label: "Start Pixel Cleaning".into(),
                code: feat.code,
                value: next_value,
            });
        }
        if feat.code == 0xD6 && next_value != 1 {
            return Some(PendingConfirm {
                title: "Power Down / Standby Monitor?".into(),
                description: "Sending VCP 0xD6 standby/off will blank the display. You may need the monitor power button to wake it.".into(),
                confirm_label: "Confirm Standby".into(),
                code: feat.code,
                value: next_value,
            });
        }
        None
    }

    fn request_vcp_write(&mut self, feat: &FeatureState, value: u16, cx: &mut Context<Self>) {
        if let Some(confirm) = Self::is_destructive_feature(feat, value) {
            self.pending_confirm = Some(confirm);
            cx.notify();
            return;
        }
        self.daemon
            .update(cx, |state, cx| state.set_vcp(feat.code, value, cx));
    }

    fn render_sidebar(&self, state: &DaemonState, cx: &mut Context<Self>) -> impl IntoElement {
        let daemon = self.daemon.clone();
        let mut monitor_cards = div().flex().flex_col().gap(px(6.0));

        if state.monitors.is_empty() {
            monitor_cards = monitor_cards.child(
                div()
                    .p(px(10.0))
                    .rounded(px(6.0))
                    .bg(Theme::bg_surface())
                    .border_1()
                    .border_color(Theme::border())
                    .text_size(px(11.5))
                    .text_color(Theme::text_muted())
                    .child(if state.loading_monitors {
                        "Scanning DDC/CI buses..."
                    } else {
                        "No DDC/CI monitors detected"
                    }),
            );
        } else {
            for (idx, mon) in state.monitors.iter().enumerate() {
                let active = idx == state.active_monitor;
                let mon_id = SharedString::from(format!("mon-card-{idx}"));
                let connector = if mon.connector.is_empty() {
                    "DDC".to_string()
                } else {
                    mon.connector.clone()
                };
                let bus_label = if mon.id.is_empty() {
                    format!("#{idx}")
                } else {
                    mon.id.clone()
                };

                let mut badges_row = div()
                    .flex()
                    .flex_wrap()
                    .gap(px(4.0))
                    .child(badge(
                        connector,
                        Theme::text_secondary(),
                        Theme::bg_elevated(),
                        Theme::border(),
                    ))
                    .child(if mon.is_oled {
                        badge(
                            "OLED",
                            Theme::amber(),
                            Theme::amber_subtle(),
                            Theme::amber_border(),
                        )
                    } else {
                        badge(
                            "LCD",
                            Theme::text_secondary(),
                            Theme::bg_elevated(),
                            Theme::border(),
                        )
                    });

                if mon.is_asus {
                    badges_row = badges_row.child(badge(
                        "ASUS",
                        Theme::accent(),
                        Theme::accent_subtle(),
                        Theme::accent_border(),
                    ));
                }

                let card = div()
                    .id(mon_id)
                    .p(px(9.0))
                    .rounded(px(7.0))
                    .bg(if active {
                        Theme::bg_surface()
                    } else {
                        Theme::bg_sidebar()
                    })
                    .border_1()
                    .border_color(if active {
                        Theme::accent_border()
                    } else {
                        Theme::border()
                    })
                    .flex()
                    .flex_col()
                    .gap(px(5.0))
                    .cursor_pointer()
                    .hover(|s| s.bg(Theme::bg_surface()).border_color(Theme::border_active()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.daemon.update(cx, |s, cx| s.select_monitor(idx, cx));
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(if active {
                                        Theme::text_primary()
                                    } else {
                                        Theme::text_secondary()
                                    })
                                    .child(mon.model_name.clone()),
                            )
                            .child(
                                div()
                                    .w(px(7.0))
                                    .h(px(7.0))
                                    .rounded(px(99.0))
                                    .bg(if active {
                                        Theme::emerald()
                                    } else {
                                        Theme::bg_hover()
                                    }),
                            ),
                    )
                    .child(badges_row)
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(Theme::text_muted())
                            .child(bus_label),
                    );

                monitor_cards = monitor_cards.child(card);
            }
        }

        let mut nav_list = div().flex().flex_col().gap(px(2.0));
        for tab in NavTab::ALL {
            let active = self.active_tab == tab;
            let count = self.tab_badge_count(tab, state);
            let item = div()
                .id(tab.id())
                .px(px(10.0))
                .py(px(7.0))
                .rounded(px(6.0))
                .bg(if active {
                    Theme::bg_elevated()
                } else {
                    Theme::bg_sidebar()
                })
                .border_1()
                .border_color(if active {
                    Theme::border_active()
                } else {
                    Theme::bg_sidebar()
                })
                .flex()
                .items_center()
                .justify_between()
                .cursor_pointer()
                .hover(|s| s.bg(Theme::bg_surface()).text_color(Theme::text_primary()))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.active_tab = tab;
                    cx.notify();
                }))
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(if active {
                            gpui::FontWeight::SEMIBOLD
                        } else {
                            gpui::FontWeight::MEDIUM
                        })
                        .text_color(if active {
                            Theme::text_primary()
                        } else {
                            Theme::text_secondary()
                        })
                        .child(tab.label()),
                )
                .child(
                    div()
                        .px(px(6.0))
                        .py(px(1.0))
                        .rounded(px(4.0))
                        .bg(if active {
                            Theme::accent_subtle()
                        } else {
                            Theme::bg_surface()
                        })
                        .text_size(px(10.5))
                        .text_color(if active {
                            Theme::accent()
                        } else {
                            Theme::text_muted()
                        })
                        .child(count.to_string()),
                );
            nav_list = nav_list.child(item);
        }

        let rescan_daemon = daemon.clone();
        div()
            .w(px(230.0))
            .h_full()
            .bg(Theme::bg_sidebar())
            .border_r_1()
            .border_color(Theme::border())
            .flex()
            .flex_col()
            .justify_between()
            .child(
                div()
                    .p(px(12.0))
                    .flex()
                    .flex_col()
                    .gap(px(14.0))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.0))
                                    .child(
                                        div()
                                            .text_size(px(14.5))
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(Theme::text_primary())
                                            .child("AsusDisplay"),
                                    )
                                    .child(badge(
                                        format!("v{APP_VERSION}"),
                                        Theme::text_muted(),
                                        Theme::bg_elevated(),
                                        Theme::border(),
                                    )),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.0))
                                    .child(
                                        div()
                                            .w(px(6.0))
                                            .h(px(6.0))
                                            .rounded(px(99.0))
                                            .bg(Theme::emerald()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .text_color(Theme::emerald())
                                            .child("StatusBar Active (Close hides to tray)"),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(Theme::text_muted())
                                            .child("DISPLAYS"),
                                    )
                                    .child(action_button(
                                        "btn-rescan-monitors",
                                        if state.loading_monitors {
                                            "Scanning..."
                                        } else {
                                            "Rescan"
                                        },
                                        ButtonVariant::Ghost,
                                        state.loading_monitors,
                                        move |_, cx| {
                                            rescan_daemon.update(cx, |s, cx| s.discover(cx));
                                        },
                                    )),
                            )
                            .child(monitor_cards),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(Theme::text_muted())
                                    .child("CONTROLS"),
                            )
                            .child(nav_list),
                    ),
            )
            .child(
                div()
                    .p(px(12.0))
                    .border_t_1()
                    .border_color(Theme::border())
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(Theme::text_muted())
                            .child(if state.loading_features {
                                "Syncing VCP..."
                            } else {
                                "DDC/CI Ready"
                            }),
                    )
                    .child(action_button(
                        "btn-hide-to-tray",
                        "Hide to Tray",
                        ButtonVariant::Secondary,
                        false,
                        move |window, cx| {
                            daemon.update(cx, |s, _| s.sync_window_visible(false));
                            window.remove_window();
                        },
                    )),
            )
    }

    fn render_toolbar(
        &self,
        state: &DaemonState,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let view = cx.entity();
        let clear_view = view.clone();

        let model_label = state
            .monitors
            .get(state.active_monitor)
            .map(|m| m.model_name.as_str())
            .unwrap_or("No Monitor");

        div()
            .px(px(18.0))
            .py(px(10.0))
            .bg(Theme::bg_header())
            .border_b_1()
            .border_color(Theme::border())
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .text_size(px(15.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(Theme::text_primary())
                                    .child(self.active_tab.label()),
                            )
                            .child(badge(
                                model_label.to_string(),
                                Theme::text_secondary(),
                                Theme::bg_elevated(),
                                Theme::border(),
                            )),
                    )
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(Theme::text_secondary())
                            .child(self.active_tab.subtitle()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(div().w(px(270.0)).child(text_input_box(
                        "toolbar-search-input",
                        &self.search_input,
                        window,
                        cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                            this.search_input.handle_key_down(ev);
                            cx.notify();
                        }),
                        Some(Box::new(move |_, cx| {
                            clear_view.update(cx, |this, cx| {
                                this.search_input.value.clear();
                                cx.notify();
                            });
                        })),
                    )))
                    .child(action_button(
                        "btn-toggle-unsupported",
                        if self.show_unsupported {
                            "All VCP"
                        } else {
                            "Supported"
                        },
                        if self.show_unsupported {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Secondary
                        },
                        false,
                        move |_, cx| {
                            view.update(cx, |this, cx| {
                                this.show_unsupported = !this.show_unsupported;
                                cx.notify();
                            });
                        },
                    )),
            )
    }

    /// Always-visible top Quick-Control Deck: Brightness (0x10), Contrast (0x12),
    /// Picture Mode (0xDC), Input Source (0x60), plus sRGB lock notice.
    fn render_hero_deck(&self, state: &DaemonState, cx: &mut Context<Self>) -> impl IntoElement {
        let daemon = self.daemon.clone();
        let brightness = state.feature(0x10);
        let contrast = state.feature(0x12);
        let mode = state.feature(0xDC);
        let input = state.feature(0x60);

        let b_cur = brightness.map(|f| f.current).unwrap_or(0);
        let b_max = brightness.map(|f| f.max.max(1)).unwrap_or(100);
        let b_locked =
            brightness.is_none_or(|f| !f.supported || (f.raw_sl == 0xFE && f.max == 0xFE));

        let c_cur = contrast.map(|f| f.current).unwrap_or(0);
        let c_max = contrast.map(|f| f.max.max(1)).unwrap_or(100);
        let c_locked =
            contrast.is_none_or(|f| !f.supported || (f.raw_sl == 0xFE && f.max == 0xFE));

        let is_srgb_locked = mode.is_some_and(|m| {
            m.supported
                && (m.current == 1
                    || m.values
                        .iter()
                        .any(|(v, l)| *v == m.current && l.eq_ignore_ascii_case("srgb")))
        });

        let mut quick_presets = div().flex().items_center().gap(px(4.0));
        for pct in [25u16, 50, 75, 100] {
            let target = ((u32::from(pct) * u32::from(b_max) + 50) / 100) as u16;
            let active = !b_locked && b_cur == target;
            quick_presets = quick_presets.child(
                div()
                    .id(SharedString::from(format!("hero-b-preset-{pct}")))
                    .px(px(6.0))
                    .py(px(2.0))
                    .rounded(px(4.0))
                    .bg(if active {
                        Theme::accent_subtle()
                    } else {
                        Theme::bg_elevated()
                    })
                    .border_1()
                    .border_color(if active {
                        Theme::accent_border()
                    } else {
                        Theme::border()
                    })
                    .text_size(px(10.5))
                    .text_color(if active {
                        Theme::text_primary()
                    } else {
                        Theme::text_secondary()
                    })
                    .cursor_pointer()
                    .hover(|s| s.bg(Theme::bg_hover()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.daemon.update(cx, |s, cx| s.set_vcp(0x10, target, cx));
                    }))
                    .child(format!("{pct}%")),
            );
        }

        let b_daemon = daemon.clone();
        let c_daemon = daemon.clone();

        let mut deck = div()
            .mx(px(18.0))
            .mt(px(14.0))
            .p(px(12.0))
            .rounded(px(8.0))
            .bg(Theme::bg_surface())
            .border_1()
            .border_color(Theme::border())
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(16.0))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(6.0))
                                            .child(
                                                div()
                                                    .text_size(px(12.0))
                                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                                    .text_color(Theme::text_primary())
                                                    .child("Brightness"),
                                            )
                                            .child(code_badge(0x10)),
                                    )
                                    .child(quick_presets),
                            )
                            .child(precision_slider(
                                "hero-brightness",
                                b_cur,
                                b_max,
                                b_locked,
                                Theme::accent(),
                                None,
                                move |val, _, cx| {
                                    b_daemon.update(cx, |s, cx| s.set_vcp(0x10, val, cx));
                                },
                            )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.0))
                                    .child(
                                        div()
                                            .text_size(px(12.0))
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(Theme::text_primary())
                                            .child("Contrast"),
                                    )
                                    .child(code_badge(0x12)),
                            )
                            .child(precision_slider(
                                "hero-contrast",
                                c_cur,
                                c_max,
                                c_locked,
                                Theme::accent(),
                                None,
                                move |val, _, cx| {
                                    c_daemon.update(cx, |s, cx| s.set_vcp(0x12, val, cx));
                                },
                            )),
                    ),
            );

        if mode.is_some() || input.is_some() {
            let mut row2 = div()
                .pt(px(8.0))
                .border_t_1()
                .border_color(Theme::border())
                .flex()
                .items_start()
                .gap(px(16.0));

            if let Some(m) = mode {
                let m_daemon = daemon.clone();
                row2 = row2.child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(px(5.0))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.0))
                                .child(
                                    div()
                                        .text_size(px(11.5))
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(Theme::text_secondary())
                                        .child("Picture Mode"),
                                )
                                .child(code_badge(0xDC)),
                        )
                        .child(pill_selector(
                            "hero-mode",
                            &m.values,
                            m.current,
                            !m.supported,
                            move |val, _, cx| {
                                m_daemon.update(cx, |s, cx| s.set_vcp(0xDC, val, cx));
                            },
                        )),
                );
            }

            if let Some(inp) = input {
                let i_daemon = daemon.clone();
                row2 = row2.child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(px(5.0))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.0))
                                .child(
                                    div()
                                        .text_size(px(11.5))
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(Theme::text_secondary())
                                        .child("Input Source"),
                                )
                                .child(code_badge(0x60)),
                        )
                        .child(pill_selector(
                            "hero-input",
                            &inp.values,
                            inp.current,
                            !inp.supported,
                            move |val, _, cx| {
                                i_daemon.update(cx, |s, cx| s.set_vcp(0x60, val, cx));
                            },
                        )),
                );
            }

            deck = deck.child(row2);
        }

        if is_srgb_locked {
            deck = deck.child(
                div()
                    .px(px(10.0))
                    .py(px(6.0))
                    .rounded(px(5.0))
                    .bg(Theme::amber_subtle())
                    .border_1()
                    .border_color(Theme::amber_border())
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(badge(
                        "sRGB LOCK",
                        Theme::amber(),
                        Theme::amber_subtle(),
                        Theme::amber_border(),
                    ))
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(Theme::text_primary())
                            .child("sRGB mode locks brightness and color calibration on some ASUS firmware. Switch Picture Mode to User or Racing to unlock full adjustments."),
                    ),
            );
        }

        deck
    }

    fn render_feature_row(&self, feat: &FeatureState, cx: &mut Context<Self>) -> AnyElement {
        let view = cx.entity();
        let feat_clone = feat.clone();
        let locked_by_firmware = !feat.supported
            || (feat.code != 0xFD && feat.raw_sl == 0xFE && feat.max == 0xFE)
            || (feat.max == 0 && feat.values.is_empty() && !feat.read_only && !feat.write_only);

        let mut title_row = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(
                div()
                    .text_size(px(12.5))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(if feat.supported {
                        Theme::text_primary()
                    } else {
                        Theme::text_muted()
                    })
                    .child(feat.name.clone()),
            )
            .child(code_badge(feat.code));

        if !feat.supported {
            title_row = title_row.child(badge(
                "Unsupported",
                Theme::text_muted(),
                Theme::bg_elevated(),
                Theme::border(),
            ));
        } else if locked_by_firmware && !feat.read_only && !feat.write_only {
            title_row = title_row.child(badge(
                "Locked in mode",
                Theme::amber(),
                Theme::amber_subtle(),
                Theme::amber_border(),
            ));
        }

        let mut left = div().flex().flex_col().gap(px(3.0)).child(title_row);
        if !feat.description.is_empty() {
            left = left.child(
                div()
                    .text_size(px(11.0))
                    .text_color(Theme::text_secondary())
                    .child(feat.description.clone()),
            );
        }

        let control: AnyElement = if feat.read_only {
            let formatted = if !feat.supported {
                "Unavailable".to_string()
            } else if let Some((_, label)) = feat.values.iter().find(|(v, _)| *v == feat.current) {
                label.clone()
            } else if feat.code == 0xC0 {
                format!("{} hrs", feat.current)
            } else if feat.code == 0xD5 {
                format!("{}%", feat.current)
            } else {
                format!("{} (0x{:04X})", feat.current, feat.current)
            };

            div()
                .px(px(9.0))
                .py(px(4.0))
                .rounded(px(5.0))
                .bg(Theme::bg_elevated())
                .border_1()
                .border_color(Theme::border())
                .text_size(px(11.5))
                .text_color(Theme::text_secondary())
                .child(formatted)
                .into_any_element()
        } else if feat.write_only {
            let mut btns = div().flex().flex_wrap().gap(px(6.0));
            let choices: Vec<(u16, String)> = if feat.values.is_empty() {
                vec![(1, "Execute".to_string())]
            } else {
                feat.values.clone()
            };
            let danger = Self::is_destructive_feature(&feat_clone, 1).is_some();
            for (val, label) in choices {
                let f_c = feat_clone.clone();
                let v_c = view.clone();
                btns = btns.child(action_button(
                    SharedString::from(format!("act-{}-{}", feat.cli_name, val)),
                    label,
                    if danger {
                        ButtonVariant::Danger
                    } else {
                        ButtonVariant::Secondary
                    },
                    !feat.supported,
                    move |_, cx| {
                        v_c.update(cx, |this, cx| this.request_vcp_write(&f_c, val, cx));
                    },
                ));
            }
            btns.into_any_element()
        } else if !feat.values.is_empty() {
            let is_binary_toggle = feat.values.len() == 2
                && feat.values.iter().any(|(v, _)| *v == 0)
                && feat.values.iter().any(|(v, _)| *v == 1);

            if is_binary_toggle {
                let is_on = feat.current != 0;
                let f_c = feat_clone.clone();
                let v_c = view.clone();
                let next_val = if is_on { 0 } else { 1 };
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(if is_on {
                                Theme::text_primary()
                            } else {
                                Theme::text_muted()
                            })
                            .child(if is_on { "On" } else { "Off" }),
                    )
                    .child(toggle_switch(
                        SharedString::from(format!("tgl-{}-{}", feat.code, feat.cli_name)),
                        is_on,
                        locked_by_firmware,
                        move |_, cx| {
                            v_c.update(cx, |this, cx| this.request_vcp_write(&f_c, next_val, cx));
                        },
                    ))
                    .into_any_element()
            } else {
                let f_c = feat_clone.clone();
                let v_c = view.clone();
                pill_selector(
                    SharedString::from(format!("pill-{}-{}", feat.code, feat.cli_name)),
                    &feat.values,
                    feat.current,
                    locked_by_firmware,
                    move |val, _, cx| {
                        v_c.update(cx, |this, cx| this.request_vcp_write(&f_c, val, cx));
                    },
                )
                .into_any_element()
            }
        } else {
            let f_c = feat_clone.clone();
            let v_c = view.clone();
            let max = feat.max.max(1);
            div()
                .w(px(360.0))
                .child(precision_slider(
                    SharedString::from(format!("sldr-{}-{}", feat.code, feat.cli_name)),
                    feat.current,
                    max,
                    locked_by_firmware,
                    Theme::accent(),
                    None,
                    move |val, _, cx| {
                        v_c.update(cx, |this, cx| this.request_vcp_write(&f_c, val, cx));
                    },
                ))
                .into_any_element()
        };

        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.0))
            .child(div().flex_1().child(left))
            .child(control)
            .into_any_element()
    }

    fn render_standard_feature_tab(
        &self,
        tab: NavTab,
        state: &DaemonState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut items: Vec<&FeatureState> = state
            .features
            .iter()
            .filter(|f| (f.supported || self.show_unsupported) && Self::tab_for_feature(f) == tab)
            .collect();

        if items.is_empty() {
            return section_card(
                tab.label(),
                Some(SharedString::from(
                    "No matching DDC/CI features reported for this monitor.",
                )),
                None,
                vec![
                    div()
                        .text_size(px(12.0))
                        .text_color(Theme::text_muted())
                        .child("Try toggling 'All VCP' in the toolbar to inspect unadvertised features, or rescan displays.")
                        .into_any_element(),
                ],
            )
            .into_any_element();
        }

        items.sort_by_key(|f| (f.group.clone(), f.code));
        let mut grouped: Vec<(String, Vec<&FeatureState>)> = Vec::new();
        for feat in items {
            let grp = if !feat.group.is_empty() {
                feat.group.clone()
            } else if !feat.section.is_empty() {
                feat.section.clone()
            } else {
                tab.label().to_string()
            };
            if let Some((_, list)) = grouped.iter_mut().find(|(g, _)| g == &grp) {
                list.push(feat);
            } else {
                grouped.push((grp, vec![feat]));
            }
        }

        let mut stack = div().flex().flex_col().gap(px(12.0));

        if tab == NavTab::ColorCalibration {
            let d1 = self.daemon.clone();
            let d2 = self.daemon.clone();
            let reset_btn = action_button(
                "btn-color-reset-neutral",
                "Reset Color (6500K / 50-50-50)",
                ButtonVariant::Secondary,
                state.monitors.is_empty(),
                move |_, cx| {
                    d1.update(cx, |s, cx| {
                        s.apply_color_preset(0x0B, Some(50), Some(50), Some(50), cx);
                    });
                },
            );
            let srgb_btn = action_button(
                "btn-color-preset-user-warm",
                "Warm User (50/46/40)",
                ButtonVariant::Ghost,
                state.monitors.is_empty(),
                move |_, cx| {
                    d2.update(cx, |s, cx| {
                        s.apply_color_preset(0x0B, Some(50), Some(46), Some(40), cx);
                    });
                },
            );
            stack = stack.child(section_card(
                "Calibration Actions",
                Some(SharedString::from(
                    "Quickly apply neutral User RGB gains or warm white-point presets.",
                )),
                Some(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(srgb_btn)
                        .child(reset_btn)
                        .into_any_element(),
                ),
                Vec::new(),
            ));
        }

        for (group_name, group_feats) in grouped {
            let rows: Vec<AnyElement> = group_feats
                .into_iter()
                .map(|f| self.render_feature_row(f, cx))
                .collect();
            stack = stack.child(section_card(group_name, None, None, rows));
        }

        stack.into_any_element()
    }

    fn render_lightbar_tab(&self, state: &DaemonState, cx: &mut Context<Self>) -> AnyElement {
        let daemon = self.daemon.clone();
        let view = cx.entity();
        let lb = state.lightbar.as_ref();
        let connected = lb.is_some_and(|b| b.connected);
        let product = lb
            .map(|b| b.product.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("ASUS ROG Monitor Light Bar (0B05:1AC8)");
        let path = lb
            .map(|b| b.path.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("Not attached");

        let d_refresh = daemon.clone();
        let status_row = div()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(Theme::text_primary())
                                    .child(product.to_string()),
                            )
                            .child(if connected {
                                badge(
                                    "USB HID Connected",
                                    Theme::emerald(),
                                    Theme::emerald_subtle(),
                                    Theme::emerald_border(),
                                )
                            } else {
                                badge(
                                    "Disconnected",
                                    Theme::text_muted(),
                                    Theme::bg_elevated(),
                                    Theme::border(),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(Theme::text_muted())
                            .child(format!("HID Path: {path}")),
                    ),
            )
            .child(action_button(
                "btn-refresh-lightbar",
                "Refresh USB HID",
                ButtonVariant::Secondary,
                false,
                move |_, cx| {
                    d_refresh.update(cx, |s, _| s.worker.send(WorkerCommand::RefreshLightBar));
                },
            ))
            .into_any_element();

        let d_power = daemon.clone();
        let power_row = div()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .text_size(px(12.5))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(Theme::text_primary())
                            .child("Light Bar Power"),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(Theme::text_secondary())
                            .child("Toggle task light & rear Aura illumination"),
                    ),
            )
            .child(toggle_switch(
                "lb-power-switch",
                state.lightbar_on_local,
                !connected,
                move |_, cx| {
                    d_power.update(cx, |s, cx| {
                        let next = !s.lightbar_on_local;
                        s.lightbar_on_local = next;
                        s.worker.send(WorkerCommand::SetLightBarPower { on: next });
                        s.sync_tray();
                        cx.notify();
                    });
                },
            ))
            .into_any_element();

        let d_bright = daemon.clone();
        let brightness_row = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.0))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .text_size(px(12.5))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(Theme::text_primary())
                            .child("White Task Light Brightness"),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(Theme::text_secondary())
                            .child("Front asymmetric desk illumination (0–100%)"),
                    ),
            )
            .child(div().w(px(360.0)).child(precision_slider(
                "lb-brightness",
                u16::from(state.lightbar_brightness_local),
                100,
                !connected,
                Theme::amber(),
                Some(format!("{}%", state.lightbar_brightness_local)),
                move |val, _, cx| {
                    let level = val.min(100) as u8;
                    d_bright.update(cx, |s, cx| {
                        s.lightbar_on_local = level > 0;
                        s.lightbar_brightness_local = level;
                        s.worker
                            .send(WorkerCommand::SetLightBarBrightness { level });
                        s.sync_tray();
                        cx.notify();
                    });
                },
            )))
            .into_any_element();

        let cct_steps: [(u16, String); 5] = [
            (0, "2700K Warm".into()),
            (1, "3650K Soft".into()),
            (2, "4600K Neutral".into()),
            (3, "5550K Cool".into()),
            (4, "6500K Daylight".into()),
        ];

        let d_cct = daemon.clone();
        let cct_row = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .text_size(px(12.5))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(Theme::text_primary())
                            .child("Color Temperature (Kelvin)"),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(Theme::text_secondary())
                            .child("Warm-to-cool LED balance"),
                    ),
            )
            .child(pill_selector(
                "lb-cct",
                &cct_steps,
                u16::from(state.lightbar_cct_local),
                !connected,
                move |val, _, cx| {
                    let step = val as u8;
                    d_cct.update(cx, |s, cx| {
                        s.lightbar_cct_local = step;
                        s.worker.send(WorkerCommand::SetLightBarCct { step });
                        cx.notify();
                    });
                },
            ))
            .into_any_element();

        let effect_pills: Vec<(u16, String)> = LIGHTBAR_EFFECTS
            .iter()
            .enumerate()
            .map(|(i, (l, _))| (i as u16, (*l).to_string()))
            .collect();

        let rgb_presets: [(&str, u8, u8, u8); 7] = [
            ("ROG Red", 255, 32, 56),
            ("Studio Blue", 59, 130, 246),
            ("Emerald", 16, 185, 129),
            ("Amber", 245, 158, 11),
            ("Violet", 139, 92, 246),
            ("Cyan", 6, 182, 212),
            ("Pure White", 255, 255, 255),
        ];

        let mut swatch_row = div().flex().flex_wrap().gap(px(6.0));
        for (name, pr, pg, pb) in rgb_presets {
            let color_u32 = (u32::from(pr) << 16) | (u32::from(pg) << 8) | u32::from(pb);
            let active = self.lb_r == pr && self.lb_g == pg && self.lb_b == pb;
            let eff = LIGHTBAR_EFFECTS[self.lb_effect_idx].1;
            let spd = self.lb_speed;
            swatch_row = swatch_row.child(
                div()
                    .id(SharedString::from(format!("lb-swatch-{name}")))
                    .px(px(8.0))
                    .py(px(4.0))
                    .rounded(px(5.0))
                    .bg(if active {
                        Theme::accent_subtle()
                    } else {
                        Theme::bg_elevated()
                    })
                    .border_1()
                    .border_color(if active {
                        Theme::accent_border()
                    } else {
                        Theme::border()
                    })
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .cursor_pointer()
                    .hover(|s| s.bg(Theme::bg_hover()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.lb_r = pr;
                        this.lb_g = pg;
                        this.lb_b = pb;
                        this.daemon.update(cx, |s, _| {
                            s.worker.send(WorkerCommand::SetLightBarEffect {
                                effect: eff,
                                r: pr,
                                g: pg,
                                b: pb,
                                speed: spd,
                            });
                        });
                        cx.notify();
                    }))
                    .child(div().w(px(10.0)).h(px(10.0)).rounded(px(3.0)).bg(rgb(color_u32)))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(Theme::text_primary())
                            .child(name),
                    ),
            );
        }

        let v_eff = view.clone();
        let v_spd = view;
        let aura_row = div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .child(
                        div()
                            .text_size(px(12.5))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(Theme::text_primary())
                            .child("Aura RGB Effect"),
                    )
                    .child(pill_selector(
                        "lb-effect",
                        &effect_pills,
                        self.lb_effect_idx as u16,
                        !connected,
                        move |val, _, cx| {
                            v_eff.update(cx, |this, cx| {
                                let idx = (val as usize).min(LIGHTBAR_EFFECTS.len() - 1);
                                this.lb_effect_idx = idx;
                                let eff = LIGHTBAR_EFFECTS[idx].1;
                                let (r, g, b, speed) =
                                    (this.lb_r, this.lb_g, this.lb_b, this.lb_speed);
                                this.daemon.update(cx, |s, _| {
                                    s.worker.send(WorkerCommand::SetLightBarEffect {
                                        effect: eff,
                                        r,
                                        g,
                                        b,
                                        speed,
                                    });
                                });
                                cx.notify();
                            });
                        },
                    )),
            )
            .child(swatch_row)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(Theme::text_secondary())
                            .child("Effect Speed"),
                    )
                    .child(div().w(px(360.0)).child(precision_slider(
                        "lb-speed",
                        u16::from(self.lb_speed),
                        255,
                        !connected,
                        Theme::accent(),
                        None,
                        move |val, _, cx| {
                            v_spd.update(cx, |this, cx| {
                                this.lb_speed = val.min(255) as u8;
                                let eff = LIGHTBAR_EFFECTS[this.lb_effect_idx].1;
                                let (r, g, b, speed) =
                                    (this.lb_r, this.lb_g, this.lb_b, this.lb_speed);
                                this.daemon.update(cx, |s, _| {
                                    s.worker.send(WorkerCommand::SetLightBarEffect {
                                        effect: eff,
                                        r,
                                        g,
                                        b,
                                        speed,
                                    });
                                });
                                cx.notify();
                            });
                        },
                    ))),
            )
            .into_any_element();

        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(section_card("Device Connection", None, None, vec![status_row]))
            .child(section_card(
                "Task Lighting",
                None,
                None,
                vec![power_row, brightness_row, cct_row],
            ))
            .child(section_card(
                "Backlight Aura RGB",
                None,
                None,
                vec![aura_row],
            ))
            .into_any_element()
    }

    fn render_profiles_appsync_tab(
        &self,
        state: &DaemonState,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let view = cx.entity();
        let daemon = self.daemon.clone();

        let v_save_clr = view.clone();
        let v_save_btn = view.clone();
        let save_row = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(div().flex_1().child(text_input_box(
                "profile-save-name",
                &self.profile_name_input,
                window,
                cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                    if this.profile_name_input.handle_key_down(ev) {
                        let name = this.profile_name_input.value.trim().to_string();
                        if !name.is_empty() {
                            this.daemon.update(cx, |s, _| {
                                s.worker.send(WorkerCommand::SaveProfile {
                                    index: s.active_monitor,
                                    name,
                                });
                            });
                            this.profile_name_input.value.clear();
                        }
                    }
                    cx.notify();
                }),
                Some(Box::new(move |_, cx| {
                    v_save_clr.update(cx, |this, cx| {
                        this.profile_name_input.value.clear();
                        cx.notify();
                    });
                })),
            )))
            .child(action_button(
                "btn-save-profile",
                "Save Current Profile",
                ButtonVariant::Primary,
                state.monitors.is_empty(),
                move |_, cx| {
                    v_save_btn.update(cx, |this, cx| {
                        let name = this.profile_name_input.value.trim().to_string();
                        if name.is_empty() {
                            this.daemon.update(cx, |s, cx| {
                                s.set_status("Enter a profile name first", true, cx)
                            });
                            return;
                        }
                        this.daemon.update(cx, |s, _| {
                            s.worker.send(WorkerCommand::SaveProfile {
                                index: s.active_monitor,
                                name,
                            });
                        });
                        this.profile_name_input.value.clear();
                        cx.notify();
                    });
                },
            ))
            .into_any_element();

        let v_imp_clr = view.clone();
        let v_imp_btn = view.clone();
        let import_row = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(div().flex_1().child(text_input_box(
                "profile-import-path",
                &self.import_path_input,
                window,
                cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                    this.import_path_input.handle_key_down(ev);
                    cx.notify();
                }),
                Some(Box::new(move |_, cx| {
                    v_imp_clr.update(cx, |this, cx| {
                        this.import_path_input.value.clear();
                        cx.notify();
                    });
                })),
            )))
            .child(action_button(
                "btn-import-profile",
                "Import JSON",
                ButtonVariant::Secondary,
                false,
                move |_, cx| {
                    v_imp_btn.update(cx, |this, cx| {
                        let raw = this.import_path_input.value.trim().to_string();
                        if raw.is_empty() {
                            this.daemon.update(cx, |s, cx| {
                                s.set_status("Enter a JSON file path to import", true, cx)
                            });
                            return;
                        }
                        this.daemon.update(cx, |s, _| {
                            s.worker
                                .send(WorkerCommand::ImportProfile { path: PathBuf::from(raw) });
                        });
                        this.import_path_input.value.clear();
                        cx.notify();
                    });
                },
            ))
            .into_any_element();

        let mut profile_rows = vec![save_row, import_row];
        if state.profiles.is_empty() {
            profile_rows.push(
                div()
                    .text_size(px(12.0))
                    .text_color(Theme::text_muted())
                    .child("No saved profiles yet. Enter a name above to snapshot the monitor's current VCP values.")
                    .into_any_element(),
            );
        } else {
            for (idx, prof) in state.profiles.iter().enumerate() {
                let p_apply = prof.name.clone();
                let p_export = prof.name.clone();
                let p_delete = prof.name.clone();
                let d_apply = daemon.clone();
                let d_export = daemon.clone();
                let d_delete = daemon.clone();

                profile_rows.push(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(12.0))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.0))
                                .child(
                                    div()
                                        .text_size(px(12.5))
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(Theme::text_primary())
                                        .child(prof.name.clone()),
                                )
                                .child(badge(
                                    prof.model.clone(),
                                    Theme::text_secondary(),
                                    Theme::bg_elevated(),
                                    Theme::border(),
                                ))
                                .child(badge(
                                    format!("{} VCPs", prof.vcp_values.len()),
                                    Theme::accent(),
                                    Theme::accent_subtle(),
                                    Theme::accent_border(),
                                )),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.0))
                                .child(action_button(
                                    SharedString::from(format!("prof-apply-{idx}")),
                                    "Apply",
                                    ButtonVariant::Primary,
                                    state.monitors.is_empty(),
                                    move |_, cx| {
                                        let name = p_apply.clone();
                                        d_apply.update(cx, |s, _| {
                                            s.worker.send(WorkerCommand::ApplyProfile {
                                                index: s.active_monitor,
                                                name,
                                            });
                                        });
                                    },
                                ))
                                .child(action_button(
                                    SharedString::from(format!("prof-export-{idx}")),
                                    "Export JSON",
                                    ButtonVariant::Secondary,
                                    false,
                                    move |_, cx| {
                                        let name = p_export.clone();
                                        let safe: String = name
                                            .chars()
                                            .map(|c| {
                                                if c.is_ascii_alphanumeric()
                                                    || c == '-'
                                                    || c == '_'
                                                {
                                                    c
                                                } else {
                                                    '_'
                                                }
                                            })
                                            .collect();
                                        let export_dir = asusdisplay::config_dir().join("exports");
                                        let _ = std::fs::create_dir_all(&export_dir);
                                        let path = export_dir.join(format!("{safe}.json"));
                                        d_export.update(cx, |s, _| {
                                            s.worker.send(WorkerCommand::ExportProfile {
                                                index: s.active_monitor,
                                                name,
                                                path,
                                            });
                                        });
                                    },
                                ))
                                .child(action_button(
                                    SharedString::from(format!("prof-delete-{idx}")),
                                    "Delete",
                                    ButtonVariant::Danger,
                                    false,
                                    move |_, cx| {
                                        let name = p_delete.clone();
                                        d_delete.update(cx, |s, _| {
                                            s.worker.send(WorkerCommand::DeleteProfile { name });
                                        });
                                    },
                                )),
                        )
                        .into_any_element(),
                );
            }
        }

        let cfg = &state.appsync_config;
        let available_modes = state
            .feature(0xDC)
            .map(|f| f.values.clone())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| {
                vec![
                    (1, "Cinema".into()),
                    (2, "Scenery".into()),
                    (3, "sRGB".into()),
                    (4, "User".into()),
                    (5, "Racing".into()),
                    (7, "FPS".into()),
                ]
            });

        let d_master = daemon.clone();
        let appsync_master_row = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(Theme::text_primary())
                                    .child("Enable AppSync Automatic Mode Switching"),
                            )
                            .child(if let Some(rule) = &state.active_appsync_rule {
                                badge(
                                    format!("Matched: {rule}"),
                                    Theme::emerald(),
                                    Theme::emerald_subtle(),
                                    Theme::emerald_border(),
                                )
                            } else {
                                badge(
                                    "Idle (No rule matched)",
                                    Theme::text_muted(),
                                    Theme::bg_elevated(),
                                    Theme::border(),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(Theme::text_secondary())
                            .child("Polls active foreground/running processes every 2s with near-zero CPU overhead"),
                    ),
            )
            .child(toggle_switch(
                "appsync-master-toggle",
                cfg.enabled,
                false,
                move |_, cx| {
                    d_master.update(cx, |s, cx| {
                        let mut next = s.appsync_config.clone();
                        next.enabled = !next.enabled;
                        s.appsync_config = next.clone();
                        s.worker.send(WorkerCommand::SaveAppSyncConfig { config: next });
                        cx.notify();
                    });
                },
            ))
            .into_any_element();

        let d_fallback = daemon.clone();
        let fallback_row = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .text_size(px(12.5))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(Theme::text_primary())
                            .child("Fallback Picture Mode"),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(Theme::text_secondary())
                            .child("Mode restored when no AppSync rule matches"),
                    ),
            )
            .child(pill_selector(
                "appsync-fallback-mode",
                &available_modes,
                cfg.default_mode.unwrap_or(0),
                false,
                move |val, _, cx| {
                    d_fallback.update(cx, |s, cx| {
                        let mut next = s.appsync_config.clone();
                        next.default_mode = if next.default_mode == Some(val) {
                            None
                        } else {
                            Some(val)
                        };
                        s.appsync_config = next.clone();
                        s.worker.send(WorkerCommand::SaveAppSyncConfig { config: next });
                        cx.notify();
                    });
                },
            ))
            .into_any_element();

        let mut proc_chips = div().flex().flex_wrap().gap(px(4.0));
        for (p_idx, proc_name) in state.running_processes.iter().take(12).enumerate() {
            let p_fill = proc_name.clone();
            proc_chips = proc_chips.child(
                div()
                    .id(SharedString::from(format!("proc-chip-{p_idx}")))
                    .px(px(7.0))
                    .py(px(2.0))
                    .rounded(px(4.0))
                    .bg(Theme::bg_elevated())
                    .border_1()
                    .border_color(Theme::border())
                    .text_size(px(10.5))
                    .text_color(Theme::text_secondary())
                    .cursor_pointer()
                    .hover(|s| s.bg(Theme::bg_hover()).text_color(Theme::text_primary()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.rule_process_input.value = p_fill.clone();
                        if this.rule_name_input.value.trim().is_empty() {
                            this.rule_name_input.value =
                                p_fill.trim_end_matches(".exe").to_string();
                        }
                        cx.notify();
                    }))
                    .child(proc_name.clone()),
            );
        }

        let d_procs = daemon.clone();
        let live_procs_row = div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(Theme::text_secondary())
                            .child("Live Detected Processes (click to pre-fill rule matcher)"),
                    )
                    .child(action_button(
                        "btn-refresh-procs",
                        "Refresh Processes",
                        ButtonVariant::Ghost,
                        false,
                        move |_, cx| {
                            d_procs.update(cx, |s, cx| s.refresh_processes(cx));
                        },
                    )),
            )
            .child(proc_chips)
            .into_any_element();

        let v_rname_clr = view.clone();
        let v_rproc_clr = view.clone();
        let v_add_rule = view.clone();
        let v_rule_mode = view;
        let add_rule_row = div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(div().flex_1().child(text_input_box(
                        "appsync-rule-name",
                        &self.rule_name_input,
                        window,
                        cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                            this.rule_name_input.handle_key_down(ev);
                            cx.notify();
                        }),
                        Some(Box::new(move |_, cx| {
                            v_rname_clr.update(cx, |this, cx| {
                                this.rule_name_input.value.clear();
                                cx.notify();
                            });
                        })),
                    )))
                    .child(div().flex_1().child(text_input_box(
                        "appsync-rule-process",
                        &self.rule_process_input,
                        window,
                        cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                            this.rule_process_input.handle_key_down(ev);
                            cx.notify();
                        }),
                        Some(Box::new(move |_, cx| {
                            v_rproc_clr.update(cx, |this, cx| {
                                this.rule_process_input.value.clear();
                                cx.notify();
                            });
                        })),
                    )))
                    .child(action_button(
                        "btn-add-appsync-rule",
                        "Add Rule",
                        ButtonVariant::Primary,
                        false,
                        move |_, cx| {
                            v_add_rule.update(cx, |this, cx| {
                                let proc_match =
                                    this.rule_process_input.value.trim().to_string();
                                if proc_match.is_empty() {
                                    this.daemon.update(cx, |s, cx| {
                                        s.set_status("Enter a process name to match", true, cx)
                                    });
                                    return;
                                }
                                let name = {
                                    let raw = this.rule_name_input.value.trim().to_string();
                                    if raw.is_empty() {
                                        proc_match.clone()
                                    } else {
                                        raw
                                    }
                                };
                                let target_mode = this.rule_target_mode;
                                this.daemon.update(cx, |s, cx| {
                                    let mut next = s.appsync_config.clone();
                                    let id = format!("rule-{}", next.rules.len() + 1);
                                    next.rules.push(AppRule {
                                        id,
                                        name,
                                        process_match: proc_match,
                                        target_mode: Some(target_mode),
                                        target_profile: None,
                                        enabled: true,
                                    });
                                    s.appsync_config = next.clone();
                                    s.worker
                                        .send(WorkerCommand::SaveAppSyncConfig { config: next });
                                    cx.notify();
                                });
                                this.rule_name_input.value.clear();
                                this.rule_process_input.value.clear();
                                cx.notify();
                            });
                        },
                    )),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(Theme::text_secondary())
                            .child("Target Mode:"),
                    )
                    .child(pill_selector(
                        "appsync-new-rule-mode",
                        &available_modes,
                        self.rule_target_mode,
                        false,
                        move |val, _, cx| {
                            v_rule_mode.update(cx, |this, cx| {
                                this.rule_target_mode = val;
                                cx.notify();
                            });
                        },
                    )),
            )
            .into_any_element();

        let mut appsync_rows = vec![appsync_master_row, fallback_row, live_procs_row, add_rule_row];
        for (idx, rule) in cfg.rules.iter().enumerate() {
            let mode_text = rule
                .target_profile
                .as_ref()
                .map(|p| format!("Profile: {p}"))
                .or_else(|| {
                    rule.target_mode.map(|m| {
                        available_modes
                            .iter()
                            .find(|(v, _)| *v == m)
                            .map(|(_, l)| format!("Mode: {l}"))
                            .unwrap_or_else(|| format!("Mode: 0x{m:02X}"))
                    })
                })
                .unwrap_or_else(|| "No action".into());

            let d_tgl = daemon.clone();
            let d_del = daemon.clone();
            appsync_rows.push(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(Theme::text_primary())
                                    .child(rule.name.clone()),
                            )
                            .child(badge(
                                rule.process_match.clone(),
                                Theme::text_secondary(),
                                Theme::bg_elevated(),
                                Theme::border(),
                            ))
                            .child(badge(
                                mode_text,
                                Theme::accent(),
                                Theme::accent_subtle(),
                                Theme::accent_border(),
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(toggle_switch(
                                SharedString::from(format!("rule-tgl-{idx}")),
                                rule.enabled,
                                false,
                                move |_, cx| {
                                    d_tgl.update(cx, |s, cx| {
                                        let mut next = s.appsync_config.clone();
                                        if let Some(r) = next.rules.get_mut(idx) {
                                            r.enabled = !r.enabled;
                                        }
                                        s.appsync_config = next.clone();
                                        s.worker
                                            .send(WorkerCommand::SaveAppSyncConfig { config: next });
                                        cx.notify();
                                    });
                                },
                            ))
                            .child(action_button(
                                SharedString::from(format!("rule-del-{idx}")),
                                "Remove",
                                ButtonVariant::Danger,
                                false,
                                move |_, cx| {
                                    d_del.update(cx, |s, cx| {
                                        let mut next = s.appsync_config.clone();
                                        if idx < next.rules.len() {
                                            next.rules.remove(idx);
                                        }
                                        s.appsync_config = next.clone();
                                        s.worker
                                            .send(WorkerCommand::SaveAppSyncConfig { config: next });
                                        cx.notify();
                                    });
                                },
                            )),
                    )
                    .into_any_element(),
            );
        }

        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(section_card(
                "Saved Monitor Profiles",
                Some(SharedString::from("Capture, apply, and share VCP state snapshots")),
                None,
                profile_rows,
            ))
            .child(section_card(
                "AppSync Process Rules",
                Some(SharedString::from(
                    "Zero-bloat background mode switching per running app",
                )),
                None,
                appsync_rows,
            ))
            .into_any_element()
    }

    fn render_system_daemon_tab(&self, state: &DaemonState, cx: &mut Context<Self>) -> AnyElement {
        let daemon = self.daemon.clone();
        let d_autostart = daemon.clone();
        let autostart_row = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .text_size(px(12.5))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(Theme::text_primary())
                            .child("Launch at Login in StatusBar (--tray)"),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(Theme::text_secondary())
                            .child("Starts AsusDisplay silently in the system tray when you sign in"),
                    ),
            )
            .child(toggle_switch(
                "sys-autostart-toggle",
                state.autostart_enabled,
                false,
                move |_, cx| {
                    d_autostart.update(cx, |s, cx| {
                        let next = !s.autostart_enabled;
                        s.set_autostart(next, cx);
                    });
                },
            ))
            .into_any_element();

        let d_autorotate = daemon.clone();
        let autorotate_row = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(Theme::text_primary())
                                    .child("Auto-Rotation Sync"),
                            )
                            .child(code_badge(0xAA)),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(Theme::text_secondary())
                            .child("Polls monitor G-sensor orientation and rotates desktop output on supported compositors"),
                    ),
            )
            .child(toggle_switch(
                "sys-autorotate-toggle",
                state.auto_rotate_enabled,
                false,
                move |_, cx| {
                    d_autorotate.update(cx, |s, cx| {
                        let next = !s.auto_rotate_enabled;
                        s.set_auto_rotate(next, cx);
                    });
                },
            ))
            .into_any_element();

        let d_gen = daemon.clone();
        let d_copy = daemon;
        let mut report_rows: Vec<AnyElement> = vec![
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.0))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .text_size(px(12.5))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(Theme::text_primary())
                                .child("Device Support & VCP Diagnostic Report"),
                        )
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(Theme::text_secondary())
                                .child("Captures model identity, MCCS capabilities string, and VCP register table"),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(action_button(
                            "btn-generate-report",
                            "Generate Device Support Report",
                            ButtonVariant::Primary,
                            state.monitors.is_empty(),
                            move |_, cx| {
                                d_gen.update(cx, |s, _| {
                                    s.worker.send(WorkerCommand::GenerateReport {
                                        index: s.active_monitor,
                                    });
                                });
                            },
                        ))
                        .child(action_button(
                            "btn-copy-report",
                            "Copy to Clipboard",
                            ButtonVariant::Secondary,
                            state.report_markdown.is_none(),
                            move |_, cx| {
                                if let Some(md) = d_copy.read(cx).report_markdown.clone() {
                                    cx.write_to_clipboard(ClipboardItem::new_string(md));
                                    d_copy.update(cx, |s, cx| {
                                        s.set_status("Copied device report to clipboard", false, cx)
                                    });
                                }
                            },
                        )),
                )
                .into_any_element(),
        ];

        if let Some(md) = &state.report_markdown {
            let preview: String = md.lines().take(18).collect::<Vec<_>>().join("\n");
            report_rows.push(
                div()
                    .p(px(10.0))
                    .rounded(px(6.0))
                    .bg(Theme::bg_input())
                    .border_1()
                    .border_color(Theme::border())
                    .text_size(px(11.0))
                    .text_color(Theme::text_secondary())
                    .child(preview)
                    .into_any_element(),
            );
        }

        let std_features = self.render_standard_feature_tab(NavTab::SystemDaemon, state, cx);

        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(section_card(
                "Background Daemon & Integration",
                None,
                None,
                vec![autostart_row, autorotate_row],
            ))
            .child(section_card(
                "Diagnostics & Support",
                None,
                None,
                report_rows,
            ))
            .child(std_features)
            .into_any_element()
    }

    fn render_search_results(&self, state: &DaemonState, cx: &mut Context<Self>) -> AnyElement {
        let view = cx.entity();
        let q = self.search_input.value.trim().to_ascii_lowercase();
        let q_hex = q.strip_prefix("0x").unwrap_or(&q);

        let matches: Vec<&FeatureState> = state
            .features
            .iter()
            .filter(|f| f.supported || self.show_unsupported)
            .filter(|f| {
                let code_hex = format!("{:02x}", f.code);
                f.name.to_ascii_lowercase().contains(&q)
                    || f.cli_name.to_ascii_lowercase().contains(&q)
                    || f.description.to_ascii_lowercase().contains(&q)
                    || f.group.to_ascii_lowercase().contains(&q)
                    || code_hex.contains(q_hex)
            })
            .collect();

        if matches.is_empty() {
            return section_card(
                format!("Search Results for \"{}\"", self.search_input.value.trim()),
                None,
                None,
                vec![
                    div()
                        .text_size(px(12.0))
                        .text_color(Theme::text_muted())
                        .child("No VCP features matched your filter.")
                        .into_any_element(),
                ],
            )
            .into_any_element();
        }

        let rows: Vec<AnyElement> = matches
            .into_iter()
            .map(|f| self.render_feature_row(f, cx))
            .collect();

        section_card(
            format!("Filtered Features ({})", rows.len()),
            Some(SharedString::from("Matching across all VCP categories")),
            Some(
                action_button(
                    "btn-clear-search",
                    "Clear Filter",
                    ButtonVariant::Ghost,
                    false,
                    move |_, cx| {
                        view.update(cx, |this, cx| {
                            this.search_input.value.clear();
                            cx.notify();
                        });
                    },
                )
                .into_any_element(),
            ),
            rows,
        )
        .into_any_element()
    }

    fn render_confirm_modal(
        &self,
        confirm: &PendingConfirm,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let view_cancel = cx.entity();
        let view_confirm = view_cancel.clone();
        let code = confirm.code;
        let value = confirm.value;

        div()
            .id("confirm-modal-backdrop")
            .absolute()
            .top(px(0.0))
            .left(px(0.0))
            .size_full()
            .bg(Theme::modal_backdrop())
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(430.0))
                    .p(px(18.0))
                    .rounded(px(10.0))
                    .bg(Theme::bg_surface())
                    .border_1()
                    .border_color(Theme::red_border())
                    .flex()
                    .flex_col()
                    .gap(px(12.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(badge(
                                "SAFETY CHECK",
                                Theme::red(),
                                Theme::red_subtle(),
                                Theme::red_border(),
                            ))
                            .child(code_badge(confirm.code)),
                    )
                    .child(
                        div()
                            .text_size(px(14.5))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(Theme::text_primary())
                            .child(confirm.title.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(Theme::text_secondary())
                            .child(confirm.description.clone()),
                    )
                    .child(
                        div()
                            .pt(px(4.0))
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(8.0))
                            .child(action_button(
                                "btn-modal-cancel",
                                "Cancel",
                                ButtonVariant::Secondary,
                                false,
                                move |_, cx| {
                                    view_cancel.update(cx, |this, cx| {
                                        this.pending_confirm = None;
                                        cx.notify();
                                    });
                                },
                            ))
                            .child(action_button(
                                "btn-modal-confirm",
                                confirm.confirm_label.clone(),
                                ButtonVariant::Danger,
                                false,
                                move |_, cx| {
                                    view_confirm.update(cx, |this, cx| {
                                        this.pending_confirm = None;
                                        this.daemon.update(cx, |s, cx| s.set_vcp(code, value, cx));
                                        cx.notify();
                                    });
                                },
                            )),
                    ),
            )
    }
}

impl Render for MainView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.daemon.read(cx).clone_snapshot_for_render();

        let body_content: AnyElement = if !self.search_input.value.trim().is_empty() {
            self.render_search_results(&state, cx)
        } else {
            match self.active_tab {
                NavTab::DisplayPicture
                | NavTab::ColorCalibration
                | NavTab::GamingOled
                | NavTab::InputsPipKvm => {
                    self.render_standard_feature_tab(self.active_tab, &state, cx)
                }
                NavTab::LightBar => self.render_lightbar_tab(&state, cx),
                NavTab::ProfilesAppSync => self.render_profiles_appsync_tab(&state, window, cx),
                NavTab::SystemDaemon => self.render_system_daemon_tab(&state, cx),
            }
        };

        let mut workspace = div()
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .bg(Theme::bg_window())
            .child(self.render_toolbar(&state, window, cx))
            .child(self.render_hero_deck(&state, cx))
            .child(
                div()
                    .id("main-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .p(px(18.0))
                    .child(body_content),
            );

        if let Some((msg, is_err)) = &state.status_message {
            let d_dismiss = self.daemon.clone();
            workspace = workspace.child(
                div()
                    .px(px(18.0))
                    .py(px(7.0))
                    .bg(Theme::bg_sidebar())
                    .border_t_1()
                    .border_color(Theme::border())
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(badge(
                                if *is_err { "ERROR" } else { "STATUS" },
                                if *is_err {
                                    Theme::red()
                                } else {
                                    Theme::emerald()
                                },
                                if *is_err {
                                    Theme::red_subtle()
                                } else {
                                    Theme::emerald_subtle()
                                },
                                if *is_err {
                                    Theme::red_border()
                                } else {
                                    Theme::emerald_border()
                                },
                            ))
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(Theme::text_secondary())
                                    .child(msg.clone()),
                            ),
                    )
                    .child(action_button(
                        "btn-dismiss-status",
                        "Dismiss",
                        ButtonVariant::Ghost,
                        false,
                        move |_, cx| {
                            d_dismiss.update(cx, |s, cx| {
                                s.status_message = None;
                                cx.notify();
                            });
                        },
                    )),
            );
        }

        let mut root = div()
            .relative()
            .size_full()
            .bg(Theme::bg_window())
            .text_color(Theme::text_primary())
            .flex()
            .child(self.render_sidebar(&state, cx))
            .child(workspace);

        if let Some(confirm) = &self.pending_confirm {
            root = root.child(self.render_confirm_modal(confirm, cx));
        }

        root
    }
}
