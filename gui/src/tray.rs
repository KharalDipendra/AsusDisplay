use crate::worker::MonitorInfo;

#[derive(Debug, Clone)]
pub enum TrayAction {
    ToggleWindow,
    ShowWindow,
    SelectMonitor(usize),
    /// `input` as the CLI takes it: a number or a choice name.
    Write { key: &'static str, input: String },
    StepBrightness(i16),
    ApplyProfile(String),
    SetLightBarPower(bool),
    SetLightBarBrightness(u8),
    Refresh,
    Quit,
}

#[derive(Debug, Clone, Default)]
pub struct TrayStateSnapshot {
    pub monitors: Vec<MonitorInfo>,
    pub active_monitor: usize,
    pub model_name: String,
    pub connector: String,
    pub is_oled: bool,
    pub brightness: Option<u16>,
    pub contrast: Option<u16>,
    /// `(choice name, label)` pairs; `current_*` is a choice name.
    pub current_mode: Option<String>,
    pub available_modes: Vec<(String, String)>,
    pub current_input: Option<String>,
    pub available_inputs: Vec<(String, String)>,
    pub profiles: Vec<String>,
    pub lightbar_connected: bool,
    pub lightbar_on: bool,
    pub window_visible: bool,
}

fn format_header(snapshot: &TrayStateSnapshot) -> String {
    if snapshot.model_name.is_empty() {
        return "AsusDisplay — No monitor detected".to_string();
    }
    let brightness = snapshot
        .brightness
        .map_or_else(|| "--".to_string(), |b| format!("{b}%"));
    let panel = if snapshot.is_oled { " OLED" } else { "" };
    if snapshot.connector.is_empty() {
        format!("ASUS {}{panel} — Brightness: {brightness}", snapshot.model_name)
    } else {
        format!(
            "ASUS {}{panel} ({}) — Brightness: {brightness}",
            snapshot.model_name, snapshot.connector
        )
    }
}

fn format_tooltip(snapshot: &TrayStateSnapshot) -> String {
    if snapshot.model_name.is_empty() {
        return if snapshot.window_visible {
            "AsusDisplay (Window Open)".to_string()
        } else {
            "AsusDisplay (Background)".to_string()
        };
    }
    let mut parts = Vec::new();
    let panel = if snapshot.is_oled { " [OLED]" } else { "" };
    if snapshot.connector.is_empty() {
        parts.push(format!("ASUS {}{panel}", snapshot.model_name));
    } else {
        parts.push(format!("ASUS {}{panel} ({})", snapshot.model_name, snapshot.connector));
    }
    if let Some(b) = snapshot.brightness {
        parts.push(format!("Brightness: {b}%"));
    }
    if let Some(c) = snapshot.contrast {
        parts.push(format!("Contrast: {c}%"));
    }
    if let Some(mode_val) = &snapshot.current_mode
        && let Some((_, mode_name)) = snapshot.available_modes.iter().find(|(v, _)| v == mode_val)
    {
        parts.push(format!("Mode: {mode_name}"));
    }
    parts.join(" | ")
}

/// The 32x32 tray icon in RGBA byte order.
#[cfg_attr(not(windows), allow(dead_code))]
fn generate_icon_rgba() -> (u32, u32, Vec<u8>) {
    (32, 32, crate::assets::app_icon(32).into_raw())
}

/// The tray icon in ARGB32 network byte order (`[a, r, g, b]`) for `ksni`.
#[cfg(target_os = "linux")]
fn generate_icon_argb32() -> (i32, i32, Vec<u8>) {
    let (w, h, mut data) = generate_icon_rgba();
    for px in data.as_chunks_mut::<4>().0 {
        px.rotate_right(1);
    }
    (w as i32, h as i32, data)
}

#[cfg(target_os = "linux")]
mod platform {
    use std::sync::mpsc::Sender;

    use ksni::blocking::{Handle, TrayMethods};
    use ksni::menu::{CheckmarkItem, MenuItem, RadioGroup, RadioItem, StandardItem, SubMenu};

    use super::{TrayAction, TrayStateSnapshot, format_header, format_tooltip, generate_icon_argb32};

    struct LinuxTray {
        snapshot: TrayStateSnapshot,
        action_tx: Sender<TrayAction>,
    }

    impl ksni::Tray for LinuxTray {
        fn id(&self) -> String {
            "asusdisplay".into()
        }

        fn title(&self) -> String {
            if self.snapshot.model_name.is_empty() {
                "AsusDisplay".into()
            } else if let Some(b) = self.snapshot.brightness {
                format!("ASUS {} — {}%", self.snapshot.model_name, b)
            } else {
                format!("ASUS {}", self.snapshot.model_name)
            }
        }

        fn icon_name(&self) -> String {
            "asusdisplay".into()
        }

        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            let (width, height, data) = generate_icon_argb32();
            vec![ksni::Icon {
                width,
                height,
                data,
            }]
        }

        fn tool_tip(&self) -> ksni::ToolTip {
            let title = if self.snapshot.model_name.is_empty() {
                "AsusDisplay".into()
            } else {
                format!("ASUS {}", self.snapshot.model_name)
            };
            ksni::ToolTip {
                icon_name: "video-display".into(),
                icon_pixmap: Vec::new(),
                title,
                description: format_tooltip(&self.snapshot),
            }
        }

        fn activate(&mut self, _x: i32, _y: i32) {
            let _ = self.action_tx.send(TrayAction::ToggleWindow);
        }

        fn scroll(&mut self, delta: i32, _dir: ksni::Orientation) {
            let step = if delta > 0 { 5 } else { -5 };
            let _ = self.action_tx.send(TrayAction::StepBrightness(step));
        }

        fn menu(&self) -> Vec<MenuItem<Self>> {
            let mut items: Vec<MenuItem<Self>> = Vec::new();

            // Header
            items.push(
                StandardItem {
                    label: format_header(&self.snapshot),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            );

            // Toggle window
            items.push(
                StandardItem {
                    label: "Open / Hide AsusDisplay".into(),
                    activate: Box::new(|this: &mut Self| {
                        let _ = this.action_tx.send(TrayAction::ToggleWindow);
                    }),
                    ..Default::default()
                }
                .into(),
            );

            items.push(MenuItem::Separator);

            // Monitor selector
            if self.snapshot.monitors.len() > 1 {
                let options = self
                    .snapshot
                    .monitors
                    .iter()
                    .map(|m| {
                        let label = if m.connector.is_empty() {
                            m.model_name.clone()
                        } else {
                            format!("{} ({})", m.model_name, m.connector)
                        };
                        RadioItem {
                            label,
                            ..Default::default()
                        }
                    })
                    .collect();
                items.push(
                    SubMenu {
                        label: "Select Monitor".into(),
                        submenu: vec![
                            RadioGroup {
                                selected: self.snapshot.active_monitor,
                                select: Box::new(|this: &mut Self, idx| {
                                    let _ = this.action_tx.send(TrayAction::SelectMonitor(idx));
                                }),
                                options,
                            }
                            .into(),
                        ],
                        ..Default::default()
                    }
                    .into(),
                );
            }

            // Brightness submenu
            let mut bright_items: Vec<MenuItem<Self>> = [100u16, 80, 60, 40, 20, 0]
                .into_iter()
                .map(|val| {
                    let is_current = self.snapshot.brightness == Some(val);
                    let label = if is_current {
                        format!("{val}%  ✓")
                    } else {
                        format!("{val}%")
                    };
                    StandardItem {
                        label,
                        activate: Box::new(move |this: &mut Self| {
                            let _ = this.action_tx.send(TrayAction::Write {
                                key: "brightness",
                                input: val.to_string(),
                            });
                        }),
                        ..Default::default()
                    }
                    .into()
                })
                .collect();
            bright_items.push(MenuItem::Separator);
            bright_items.push(
                StandardItem {
                    label: "+10%".into(),
                    activate: Box::new(|this: &mut Self| {
                        let _ = this.action_tx.send(TrayAction::StepBrightness(10));
                    }),
                    ..Default::default()
                }
                .into(),
            );
            bright_items.push(
                StandardItem {
                    label: "-10%".into(),
                    activate: Box::new(|this: &mut Self| {
                        let _ = this.action_tx.send(TrayAction::StepBrightness(-10));
                    }),
                    ..Default::default()
                }
                .into(),
            );
            items.push(
                SubMenu {
                    label: "Brightness".into(),
                    submenu: bright_items,
                    ..Default::default()
                }
                .into(),
            );

            // Picture Mode submenu
            if !self.snapshot.available_modes.is_empty() {
                let modes = self.snapshot.available_modes.clone();
                let selected = self
                    .snapshot
                    .current_mode
                    .as_ref()
                    .and_then(|cur| modes.iter().position(|(v, _)| v == cur))
                    .unwrap_or(0);
                let options = modes
                    .iter()
                    .map(|(_, name)| RadioItem {
                        label: name.clone(),
                        ..Default::default()
                    })
                    .collect();
                items.push(
                    SubMenu {
                        label: "Picture Mode".into(),
                        submenu: vec![
                            RadioGroup {
                                selected,
                                select: Box::new(move |this: &mut Self, idx| {
                                    if let Some((val, _)) = modes.get(idx) {
                                        let _ = this.action_tx.send(TrayAction::Write {
                                            key: "mode",
                                            input: val.clone(),
                                        });
                                    }
                                }),
                                options,
                            }
                            .into(),
                        ],
                        ..Default::default()
                    }
                    .into(),
                );
            }

            // Input Source submenu
            if !self.snapshot.available_inputs.is_empty() {
                let inputs = self.snapshot.available_inputs.clone();
                let selected = self
                    .snapshot
                    .current_input
                    .as_ref()
                    .and_then(|cur| inputs.iter().position(|(v, _)| v == cur))
                    .unwrap_or(0);
                let options = inputs
                    .iter()
                    .map(|(_, name)| RadioItem {
                        label: name.clone(),
                        ..Default::default()
                    })
                    .collect();
                items.push(
                    SubMenu {
                        label: "Input Source".into(),
                        submenu: vec![
                            RadioGroup {
                                selected,
                                select: Box::new(move |this: &mut Self, idx| {
                                    if let Some((val, _)) = inputs.get(idx) {
                                        let _ = this.action_tx.send(TrayAction::Write {
                                            key: "input",
                                            input: val.clone(),
                                        });
                                    }
                                }),
                                options,
                            }
                            .into(),
                        ],
                        ..Default::default()
                    }
                    .into(),
                );
            }

            // Saved Profiles submenu
            if !self.snapshot.profiles.is_empty() {
                let prof_items = self
                    .snapshot
                    .profiles
                    .iter()
                    .map(|name| {
                        let profile_name = name.clone();
                        StandardItem {
                            label: name.clone(),
                            activate: Box::new(move |this: &mut Self| {
                                let _ = this
                                    .action_tx
                                    .send(TrayAction::ApplyProfile(profile_name.clone()));
                            }),
                            ..Default::default()
                        }
                        .into()
                    })
                    .collect();
                items.push(
                    SubMenu {
                        label: "Saved Profiles".into(),
                        submenu: prof_items,
                        ..Default::default()
                    }
                    .into(),
                );
            }

            // Light Bar submenu
            if self.snapshot.lightbar_connected {
                let next_power = !self.snapshot.lightbar_on;
                let mut lb_items: Vec<MenuItem<Self>> = vec![
                    CheckmarkItem {
                        label: "Light Bar Power".into(),
                        checked: self.snapshot.lightbar_on,
                        activate: Box::new(move |this: &mut Self| {
                            let _ = this
                                .action_tx
                                .send(TrayAction::SetLightBarPower(next_power));
                        }),
                        ..Default::default()
                    }
                    .into(),
                    MenuItem::Separator,
                ];
                for level in [100u8, 50, 10] {
                    lb_items.push(
                        StandardItem {
                            label: format!("{level}%"),
                            activate: Box::new(move |this: &mut Self| {
                                let _ = this
                                    .action_tx
                                    .send(TrayAction::SetLightBarBrightness(level));
                            }),
                            ..Default::default()
                        }
                        .into(),
                    );
                }
                items.push(
                    SubMenu {
                        label: "Light Bar".into(),
                        submenu: lb_items,
                        ..Default::default()
                    }
                    .into(),
                );
            }

            items.push(MenuItem::Separator);

            items.push(
                StandardItem {
                    label: "Rescan Monitors".into(),
                    activate: Box::new(|this: &mut Self| {
                        let _ = this.action_tx.send(TrayAction::Refresh);
                    }),
                    ..Default::default()
                }
                .into(),
            );

            items.push(
                StandardItem {
                    label: "Quit AsusDisplay".into(),
                    activate: Box::new(|this: &mut Self| {
                        let _ = this.action_tx.send(TrayAction::Quit);
                    }),
                    ..Default::default()
                }
                .into(),
            );

            items
        }
    }

    pub struct SystemTrayHandle {
        handle: Handle<LinuxTray>,
    }

    impl SystemTrayHandle {
        pub fn spawn(action_tx: Sender<TrayAction>) -> Option<Self> {
            let tray = LinuxTray {
                snapshot: TrayStateSnapshot::default(),
                action_tx,
            };
            let handle = tray.assume_sni_available(true).spawn().ok()?;
            Some(Self { handle })
        }

        pub fn update(&self, snapshot: TrayStateSnapshot) {
            self.handle.update(|tray| {
                tray.snapshot = snapshot;
            });
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::Sender;

    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

    use super::{TrayAction, TrayStateSnapshot, format_header, format_tooltip, generate_icon_rgba};

    pub struct SystemTrayHandle {
        tray: TrayIcon,
        lightbar_on: Arc<AtomicBool>,
    }

    fn build_menu(snapshot: &TrayStateSnapshot) -> Menu {
        let menu = Menu::new();

        let header = MenuItem::with_id("header", format_header(snapshot), false, None);
        let toggle = MenuItem::with_id("toggle", "Open / Hide AsusDisplay", true, None);
        let _ = menu.append(&header);
        let _ = menu.append(&toggle);
        let _ = menu.append(&PredefinedMenuItem::separator());

        if snapshot.monitors.len() > 1 {
            let sub = Submenu::new("Select Monitor", true);
            for (idx, m) in snapshot.monitors.iter().enumerate() {
                let label = if m.connector.is_empty() {
                    m.model_name.clone()
                } else {
                    format!("{} ({})", m.model_name, m.connector)
                };
                let item = CheckMenuItem::with_id(
                    format!("mon:{idx}"),
                    label,
                    true,
                    idx == snapshot.active_monitor,
                    None,
                );
                let _ = sub.append(&item);
            }
            let _ = menu.append(&sub);
        }

        let bright_sub = Submenu::new("Brightness", true);
        for val in [100u16, 80, 60, 40, 20, 0] {
            let item = CheckMenuItem::with_id(
                format!("bright:{val}"),
                format!("{val}%"),
                true,
                snapshot.brightness == Some(val),
                None,
            );
            let _ = bright_sub.append(&item);
        }
        let _ = bright_sub.append(&PredefinedMenuItem::separator());
        let _ = bright_sub.append(&MenuItem::with_id("bright_step:+10", "+10%", true, None));
        let _ = bright_sub.append(&MenuItem::with_id("bright_step:-10", "-10%", true, None));
        let _ = menu.append(&bright_sub);

        if !snapshot.available_modes.is_empty() {
            let mode_sub = Submenu::new("Picture Mode", true);
            for (val, name) in &snapshot.available_modes {
                let item = CheckMenuItem::with_id(
                    format!("mode:{val}"),
                    name,
                    true,
                    snapshot.current_mode.as_ref() == Some(val),
                    None,
                );
                let _ = mode_sub.append(&item);
            }
            let _ = menu.append(&mode_sub);
        }

        if !snapshot.available_inputs.is_empty() {
            let input_sub = Submenu::new("Input Source", true);
            for (val, name) in &snapshot.available_inputs {
                let item = CheckMenuItem::with_id(
                    format!("input:{val}"),
                    name,
                    true,
                    snapshot.current_input.as_ref() == Some(val),
                    None,
                );
                let _ = input_sub.append(&item);
            }
            let _ = menu.append(&input_sub);
        }

        if !snapshot.profiles.is_empty() {
            let prof_sub = Submenu::new("Saved Profiles", true);
            for name in &snapshot.profiles {
                let item = MenuItem::with_id(format!("profile:{name}"), name, true, None);
                let _ = prof_sub.append(&item);
            }
            let _ = menu.append(&prof_sub);
        }

        if snapshot.lightbar_connected {
            let lb_sub = Submenu::new("Light Bar", true);
            let power = CheckMenuItem::with_id(
                "lb_power",
                "Light Bar Power",
                true,
                snapshot.lightbar_on,
                None,
            );
            let _ = lb_sub.append(&power);
            let _ = lb_sub.append(&PredefinedMenuItem::separator());
            for lvl in [100u8, 50, 10] {
                let item = MenuItem::with_id(format!("lb_bright:{lvl}"), format!("{lvl}%"), true, None);
                let _ = lb_sub.append(&item);
            }
            let _ = menu.append(&lb_sub);
        }

        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&MenuItem::with_id("refresh", "Rescan Monitors", true, None));
        let _ = menu.append(&MenuItem::with_id("quit", "Quit AsusDisplay", true, None));

        menu
    }

    fn parse_menu_action(id: &str, lightbar_on: &AtomicBool) -> Option<TrayAction> {
        match id {
            "toggle" => Some(TrayAction::ToggleWindow),
            "refresh" => Some(TrayAction::Refresh),
            "quit" => Some(TrayAction::Quit),
            "lb_power" => {
                let next = !lightbar_on.load(Ordering::Relaxed);
                lightbar_on.store(next, Ordering::Relaxed);
                Some(TrayAction::SetLightBarPower(next))
            }
            "bright_step:+10" => Some(TrayAction::StepBrightness(10)),
            "bright_step:-10" => Some(TrayAction::StepBrightness(-10)),
            _ => {
                if let Some(rest) = id.strip_prefix("mon:") {
                    rest.parse::<usize>().ok().map(TrayAction::SelectMonitor)
                } else if let Some(rest) = id.strip_prefix("bright:") {
                    Some(TrayAction::Write { key: "brightness", input: rest.to_string() })
                } else if let Some(rest) = id.strip_prefix("mode:") {
                    Some(TrayAction::Write { key: "mode", input: rest.to_string() })
                } else if let Some(rest) = id.strip_prefix("input:") {
                    Some(TrayAction::Write { key: "input", input: rest.to_string() })
                } else if let Some(rest) = id.strip_prefix("profile:") {
                    Some(TrayAction::ApplyProfile(rest.to_string()))
                } else if let Some(rest) = id.strip_prefix("lb_bright:") {
                    rest.parse::<u8>().ok().map(TrayAction::SetLightBarBrightness)
                } else {
                    None
                }
            }
        }
    }

    impl SystemTrayHandle {
        pub fn spawn(action_tx: Sender<TrayAction>) -> Option<Self> {
            let (w, h, rgba) = generate_icon_rgba();
            let icon = Icon::from_rgba(rgba, w, h).ok()?;
            let initial_snapshot = TrayStateSnapshot::default();
            let menu = build_menu(&initial_snapshot);
            let lightbar_on = Arc::new(AtomicBool::new(false));

            let click_tx = action_tx.clone();
            TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    let _ = click_tx.send(TrayAction::ToggleWindow);
                }
            }));

            let lb_state = Arc::clone(&lightbar_on);
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                if let Some(action) = parse_menu_action(event.id().as_ref(), &lb_state) {
                    let _ = action_tx.send(action);
                }
            }));

            let tray = TrayIconBuilder::new()
                .with_id("asusdisplay")
                .with_tooltip("AsusDisplay")
                .with_icon(icon)
                .with_menu(Box::new(menu))
                .with_menu_on_left_click(false)
                .build()
                .ok()?;

            Some(Self { tray, lightbar_on })
        }

        pub fn update(&self, snapshot: TrayStateSnapshot) {
            self.lightbar_on
                .store(snapshot.lightbar_on, Ordering::Relaxed);
            let tooltip = format_tooltip(&snapshot);
            let menu = build_menu(&snapshot);
            self.tray.set_menu(Some(Box::new(menu)));
            let _ = self.tray.set_tooltip(Some(tooltip));
        }
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod platform {
    use std::sync::mpsc::Sender;

    use super::{TrayAction, TrayStateSnapshot};

    pub struct SystemTrayHandle;

    impl SystemTrayHandle {
        pub fn spawn(_action_tx: Sender<TrayAction>) -> Option<Self> {
            None
        }

        pub fn update(&self, _snapshot: TrayStateSnapshot) {}
    }
}

pub use platform::SystemTrayHandle;
