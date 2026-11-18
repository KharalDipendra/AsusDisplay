use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnyElement, App, ClipboardItem, Context, Decorations, Div, Entity, FontWeight, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, ParentElement, PathPromptOptions, Render, SharedString, Stateful,
    StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::FluentBuilder, px, rgb,
};

use asusdisplay::Kind;
use asusdisplay::appsync::AppRule;
use asusdisplay::profiles::MonitorProfile;

use crate::assets::*;
use crate::autostart;
use crate::components::{Callback, TextInput, Variant, button, card, dropdown, row, segmented, slider, switch, toolbar_button};
use crate::theme;
use crate::tray::{SystemTrayHandle, TrayAction, TrayStateSnapshot};
use crate::worker::{FeatureState, LightBarSnapshot, MonitorInfo, RgbEffect, WorkerCommand, WorkerEvent, WorkerHandle};

const REPO_URL: &str = "https://github.com/KharalDipendra/Asus-Displaywidget-Linux";

/// Settings that mean nothing to most people (the OSD remote and the VCP table version).
const HIDDEN: [&str; 2] = ["vcp-version", "osd"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Picture,
    Color,
    Gaming,
    OledCare,
    Inputs,
    System,
    LightBar,
    Profiles,
    Settings,
}

impl Page {
    const ALL: [Page; 9] = [
        Page::Picture,
        Page::Color,
        Page::Gaming,
        Page::OledCare,
        Page::Inputs,
        Page::System,
        Page::LightBar,
        Page::Profiles,
        Page::Settings,
    ];

    fn label(self) -> &'static str {
        match self {
            Page::Picture => "Picture",
            Page::Color => "Color",
            Page::Gaming => "Gaming",
            Page::OledCare => "OLED Care",
            Page::Inputs => "Inputs",
            Page::System => "System",
            Page::LightBar => "Light Bar",
            Page::Profiles => "Profiles",
            Page::Settings => "Settings",
        }
    }

    fn icon(self) -> &'static [u8] {
        match self {
            Page::Picture => ICON_SUN,
            Page::Color => ICON_DROP,
            Page::Gaming => ICON_CROSSHAIR,
            Page::OledCare => ICON_SHIELD,
            Page::Inputs => ICON_PIP,
            Page::System => ICON_MONITOR,
            Page::LightBar => ICON_LIGHTBAR,
            Page::Profiles => ICON_LAYERS,
            Page::Settings => ICON_SLIDERS,
        }
    }
}

/// Which page a monitor setting lives on, from the library's group.
fn page_of(f: &FeatureState) -> Page {
    match (f.key, f.group) {
        ("input" | "kvm", _) | (_, "PIP") => Page::Inputs,
        (_, "Picture" | "HDR") => Page::Picture,
        (_, "Color") => Page::Color,
        (_, "Gaming" | "GamePlus") => Page::Gaming,
        (_, "OLED Care") => Page::OledCare,
        _ => Page::System,
    }
}

fn section_of(f: &FeatureState) -> &'static str {
    match (f.key, f.group) {
        ("input" | "kvm", _) => "Input",
        (_, "PIP") => "Picture in Picture",
        (_, "OSD") => "On-Screen Menu",
        (_, "Info") => "About This Monitor",
        (_, group) => group,
    }
}

/// App state that outlives the window while it's hidden in the tray.
pub struct DaemonState {
    pub worker: WorkerHandle,
    tray: Option<SystemTrayHandle>,
    window_visible: bool,
    monitors: Vec<MonitorInfo>,
    active: usize,
    scanning: bool,
    /// Per monitor index, once read.
    features: HashMap<usize, Vec<FeatureState>>,
    /// Monitors that stopped answering.
    failed: HashSet<usize>,
    lightbar: Option<LightBarSnapshot>,
    profiles: Vec<MonitorProfile>,
    appsync: asusdisplay::appsync::AppSyncConfig,
    appsync_match: Option<String>,
    status: Option<(String, bool)>,
    status_seq: u64,
    autostart: bool,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    auto_rotate: bool,
}

impl DaemonState {
    pub fn new(worker: WorkerHandle, tray: Option<SystemTrayHandle>, window_visible: bool, _cx: &mut Context<Self>) -> Self {
        let active = std::env::args()
            .skip_while(|a| a != "--monitor" && a != "-m")
            .nth(1)
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        Self {
            worker,
            tray,
            window_visible,
            monitors: Vec::new(),
            active,
            scanning: true,
            features: HashMap::new(),
            failed: HashSet::new(),
            lightbar: None,
            profiles: Vec::new(),
            appsync: Default::default(),
            appsync_match: None,
            status: None,
            status_seq: 0,
            autostart: autostart::is_autostart_enabled(),
            auto_rotate: false,
        }
    }

    pub fn sync_window_visible(&mut self, visible: bool) {
        if self.window_visible != visible {
            self.window_visible = visible;
            self.sync_tray();
        }
    }

    fn active_features(&self) -> &[FeatureState] {
        self.features.get(&self.active).map_or(&[], Vec::as_slice)
    }

    fn feature(&self, key: &str) -> Option<&FeatureState> {
        self.active_features().iter().find(|f| f.key == key && f.supported)
    }

    fn discover(&mut self, cx: &mut Context<Self>) {
        self.scanning = true;
        self.worker.send(WorkerCommand::Discover);
        self.worker.send(WorkerCommand::RefreshLightBar);
        cx.notify();
    }

    fn select_monitor(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.monitors.len() {
            return;
        }
        self.active = index;
        self.worker.send(WorkerCommand::SetActiveMonitor { index });
        self.worker.send(WorkerCommand::LoadMonitor { index });
        self.sync_tray();
        cx.notify();
    }

    /// Shows the new value right away and queues the write; the worker reports back if it fails.
    fn write(&mut self, key: &'static str, input: String, force: bool, cx: &mut Context<Self>) {
        let index = self.active;
        if let Some(f) = self.features.get_mut(&index).and_then(|l| l.iter_mut().find(|f| f.key == key)) {
            match f.kind {
                Kind::Continuous => f.current = input.parse().unwrap_or(f.current),
                Kind::Toggle => f.on = input == "on",
                Kind::Enum => f.selected = Some(input.clone()),
                _ => {}
            }
        }
        self.worker.send(WorkerCommand::Write { index, key, input, force });
        self.sync_tray();
        cx.notify();
    }

    fn update_lightbar(&mut self, cmd: WorkerCommand, change: impl FnOnce(&mut crate::worker::LightBarState), cx: &mut Context<Self>) {
        if let Some(lb) = &mut self.lightbar {
            change(&mut lb.state);
        }
        self.worker.send(cmd);
        self.sync_tray();
        cx.notify();
    }

    fn save_appsync(&mut self, change: impl FnOnce(&mut asusdisplay::appsync::AppSyncConfig), cx: &mut Context<Self>) {
        change(&mut self.appsync);
        self.worker.send(WorkerCommand::SaveAppSyncConfig { config: self.appsync.clone() });
        cx.notify();
    }

    fn set_status(&mut self, message: impl Into<String>, is_error: bool, cx: &mut Context<Self>) {
        self.status = Some((message.into(), is_error));
        self.status_seq += 1;
        let seq = self.status_seq;
        let secs = if is_error { 8 } else { 4 };
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(secs)).await;
            let _ = this.update(cx, |s, cx| {
                if s.status_seq == seq {
                    s.status = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn set_autostart(&mut self, enabled: bool, cx: &mut Context<Self>) {
        match autostart::set_autostart_enabled(enabled) {
            Ok(()) => self.autostart = autostart::is_autostart_enabled(),
            Err(e) => self.set_status(format!("Couldn't change launch at login: {e}"), true, cx),
        }
        cx.notify();
    }

    pub fn handle_worker_event(&mut self, event: WorkerEvent, cx: &mut Context<Self>) {
        match event {
            WorkerEvent::MonitorsDiscovered { monitors } => {
                // Stay on the same monitor when the list changes under us.
                let previous = self.monitors.get(self.active).map(|m| m.model_name.clone());
                self.monitors = monitors;
                self.scanning = false;
                self.features.clear();
                self.failed.clear();
                if let Some(i) = previous.and_then(|p| self.monitors.iter().position(|m| m.model_name == p)) {
                    self.active = i;
                }
                if self.active >= self.monitors.len() {
                    self.active = 0;
                }
                if !self.monitors.is_empty() {
                    self.worker.send(WorkerCommand::SetActiveMonitor { index: self.active });
                    self.worker.send(WorkerCommand::LoadMonitor { index: self.active });
                }
            }
            WorkerEvent::MonitorLoaded { index, features } => {
                self.failed.remove(&index);
                self.features.insert(index, features);
            }
            WorkerEvent::MonitorFailed { index } => {
                self.failed.insert(index);
            }
            WorkerEvent::FeatureUpdated { index, feature } => {
                if let Some(f) = self.features.get_mut(&index).and_then(|l| l.iter_mut().find(|f| f.key == feature.key)) {
                    *f = feature;
                }
            }
            WorkerEvent::LightBarLoaded { snapshot } => self.lightbar = snapshot,
            WorkerEvent::ProfilesUpdated { profiles } => self.profiles = profiles,
            WorkerEvent::AppSyncUpdated { config, active_rule } => {
                self.appsync = config;
                self.appsync_match = active_rule;
            }
            WorkerEvent::ReportGenerated { markdown } => {
                cx.write_to_clipboard(ClipboardItem::new_string(markdown));
                self.set_status("Support report copied to the clipboard", false, cx);
            }
            WorkerEvent::StatusMessage { message, is_error } => self.set_status(message, is_error, cx),
        }
        self.sync_tray();
        cx.notify();
    }

    pub fn handle_tray_action(&mut self, action: TrayAction, cx: &mut Context<Self>) {
        match action {
            TrayAction::SelectMonitor(index) => self.select_monitor(index, cx),
            TrayAction::Write { key, input } => self.write(key, input, false, cx),
            TrayAction::StepBrightness(delta) => self.worker.send(WorkerCommand::StepBrightness { index: self.active, delta }),
            TrayAction::ApplyProfile(name) => self.worker.send(WorkerCommand::ApplyProfile { index: self.active, name }),
            TrayAction::SetLightBarPower(on) => self.update_lightbar(WorkerCommand::SetLightBarPower { on }, |s| s.on = on, cx),
            TrayAction::SetLightBarBrightness(level) => self.update_lightbar(
                WorkerCommand::SetLightBarBrightness { level },
                |s| (s.on, s.brightness, s.is_rgb) = (level > 0, level, false),
                cx,
            ),
            TrayAction::Refresh => self.discover(cx),
            TrayAction::ToggleWindow | TrayAction::ShowWindow | TrayAction::Quit => {}
        }
    }

    fn sync_tray(&self) {
        let Some(tray) = &self.tray else { return };
        let info = self.monitors.get(self.active);
        let choices = |key| {
            self.feature(key).map_or_else(Vec::new, |f| f.choices.iter().map(|c| (c.name.clone(), c.label.clone())).collect())
        };
        let level = |key| self.feature(key).filter(|f| f.blocked.is_none()).map(|f| f.current);
        tray.update(TrayStateSnapshot {
            monitors: self.monitors.clone(),
            active_monitor: self.active,
            model_name: info.map(|m| m.model_name.clone()).unwrap_or_default(),
            connector: info.map(|m| m.connector.clone()).unwrap_or_default(),
            is_oled: info.is_some_and(|m| m.is_oled),
            brightness: level("brightness"),
            contrast: level("contrast"),
            current_mode: self.feature("mode").and_then(|f| f.selected.clone()),
            available_modes: choices("mode"),
            current_input: self.feature("input").and_then(|f| f.selected.clone()),
            available_inputs: choices("input"),
            profiles: self.profiles.iter().map(|p| p.name.clone()).collect(),
            lightbar_connected: self.lightbar.is_some(),
            lightbar_on: self.lightbar.as_ref().is_some_and(|l| l.state.on),
            window_visible: self.window_visible,
        });
    }
}

/// Asks before a disruptive write (power off, reset, pixel cleaning).
struct Confirm {
    key: &'static str,
    input: String,
    title: String,
    body: String,
}

pub struct MainView {
    daemon: Entity<DaemonState>,
    _sub: Subscription,
    page: Page,
    /// The dropdown that is open, by id.
    open_menu: Option<SharedString>,
    confirm: Option<Confirm>,
    profile_name: TextInput,
    rule_process: TextInput,
    rule_mode: Option<String>,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    move_pending: bool,
}

impl MainView {
    pub fn new(daemon: Entity<DaemonState>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            _sub: cx.observe(&daemon, |_, _, cx| cx.notify()),
            daemon,
            page: Page::Picture,
            open_menu: None,
            confirm: None,
            profile_name: TextInput::new("Profile name, e.g. Night", cx),
            rule_process: TextInput::new("App name, e.g. cs2 or mpv", cx),
            rule_mode: None,
            move_pending: false,
        }
    }

    fn write(&mut self, key: &'static str, input: String, cx: &mut Context<Self>) {
        let feature = self.daemon.read(cx).feature(key).cloned();
        match feature {
            Some(f) if f.force && !(f.kind == Kind::Toggle && input == "off") => {
                let choice = f.choices.iter().find(|c| c.name == input).map(|c| c.label.clone());
                self.confirm = Some(Confirm {
                    key,
                    input,
                    title: choice.map_or_else(|| format!("{}?", f.label), |c| format!("{}: {c}?", f.label)),
                    body: f.note.unwrap_or("This can interrupt what's on screen.").to_string(),
                });
            }
            _ => self.daemon.update(cx, |s, cx| s.write(key, input, false, cx)),
        }
        cx.notify();
    }

    fn save_profile(&mut self, cx: &mut Context<Self>) {
        let name = self.profile_name.value.trim().to_string();
        self.daemon.update(cx, |s, cx| {
            if name.is_empty() {
                s.set_status("Type a name for the profile first", true, cx);
            } else {
                s.worker.send(WorkerCommand::SaveProfile { index: s.active, name });
            }
        });
        self.profile_name.value.clear();
    }

    fn add_rule(&mut self, cx: &mut Context<Self>) {
        let process = self.rule_process.value.trim().to_string();
        let mode = self.rule_mode.as_deref().and_then(|m| m.parse::<u16>().ok());
        let (Some(mode), false) = (mode, process.is_empty()) else {
            self.daemon.update(cx, |s, cx| s.set_status("Type an app name and pick a picture mode", true, cx));
            return;
        };
        self.daemon.update(cx, |s, cx| {
            s.save_appsync(
                |c| {
                    let id = format!("rule-{}", c.rules.len() + 1);
                    let name = process.trim_end_matches(".exe").to_string();
                    c.rules.push(AppRule { id, name, process_match: process, target_mode: Some(mode), target_profile: None, enabled: true });
                },
                cx,
            )
        });
        self.rule_process.value.clear();
    }
}

/// Wraps a view update as a click handler.
fn on_view(view: &Entity<MainView>, f: impl Fn(&mut MainView, &mut Context<MainView>) + 'static) -> impl Fn(&mut Window, &mut App) + 'static {
    let view = view.clone();
    move |_, cx| {
        view.update(cx, |this, cx| {
            f(this, cx);
            cx.notify();
        })
    }
}

fn on_daemon(daemon: &Entity<DaemonState>, f: impl Fn(&mut DaemonState, &mut Context<DaemonState>) + 'static) -> impl Fn(&mut Window, &mut App) + 'static {
    let daemon = daemon.clone();
    move |_, cx| daemon.update(cx, |s, cx| f(s, cx))
}

fn hint(text: impl Into<String>) -> Option<(String, gpui::Rgba)> {
    Some((text.into(), theme::subtle()))
}

fn monitor_label(monitors: &[MonitorInfo], i: usize) -> String {
    let m = &monitors[i];
    let twins = monitors.iter().filter(|o| o.model_name == m.model_name).count();
    if twins > 1 { format!("{} ({})", m.model_name, i + 1) } else { m.model_name.clone() }
}

impl MainView {
    fn titlebar(&self, state: &DaemonState, view: &Entity<Self>, window: &Window) -> impl IntoElement {
        let client_decorated = matches!(window.window_decorations(), Decorations::Client { .. });
        let controls = cfg!(windows) || (cfg!(target_os = "linux") && client_decorated);

        let tabs = (0..state.monitors.len()).map(|i| {
            let active = i == state.active;
            div()
                .id(("monitor-tab", i))
                .h(px(28.))
                .px(px(12.))
                .rounded(px(6.))
                .flex()
                .items_center()
                .gap(px(8.))
                .text_size(px(13.))
                .text_color(if active { theme::text() } else { theme::subtle() })
                .when(active, |d| d.bg(theme::overlay()))
                .cursor_pointer()
                .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
                .on_click(on_daemon(&self.daemon, move |s, cx| s.select_monitor(i, cx)).pipe_click())
                .child(icon(ICON_MONITOR, 14., if active { theme::accent() } else { theme::muted() }))
                .child(monitor_label(&state.monitors, i))
        });

        div()
            .h(px(40.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .bg(theme::surface())
            .border_b_1()
            .border_color(theme::border())
            .child(
                drag_area(div().id("title"), view)
                    .h_full()
                    .pl(px(14.))
                    .pr(px(16.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(icon(ICON_LOGO, 18., theme::accent()))
                    .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child("AsusDisplay")),
            )
            .child(div().flex().items_center().gap(px(4.)).children(tabs))
            .child(drag_area(div().id("title-drag"), view).flex_1().h_full())
            .child(div().pr(px(6.)).child(toolbar_button(
                "rescan",
                ICON_REFRESH,
                if state.scanning { "Scanning…" } else { "Rescan" },
                on_daemon(&self.daemon, |s, cx| s.discover(cx)),
            )))
            .when(controls, |d| d.child(self.window_controls(window)))
    }

    fn window_controls(&self, window: &Window) -> impl IntoElement {
        let max_icon = if window.is_maximized() { ICON_WIN_RESTORE } else { ICON_WIN_MAX };
        let control = |id: &'static str, data: &'static [u8], close: bool| {
            div()
                .id(id)
                .w(px(46.))
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme::subtle())
                .hover(move |s| if close { s.bg(rgb(0xc42b1c)).text_color(theme::text()) } else { s.bg(theme::hover()) })
                .child(gpui::svg().data(data).size(px(14.)).text_color(theme::subtle()))
        };

        #[cfg(not(target_os = "linux"))]
        let buttons = [
            control("win-min", ICON_WIN_MIN, false).window_control_area(gpui::WindowControlArea::Min),
            control("win-max", max_icon, false).window_control_area(gpui::WindowControlArea::Max),
            control("win-close", ICON_WIN_CLOSE, true).window_control_area(gpui::WindowControlArea::Close),
        ];
        #[cfg(target_os = "linux")]
        let buttons = {
            let daemon = self.daemon.clone();
            [
                control("win-min", ICON_WIN_MIN, false).on_click(|_, w, _| w.minimize_window()),
                control("win-max", max_icon, false).on_click(|_, w, _| w.zoom_window()),
                control("win-close", ICON_WIN_CLOSE, true).on_click(move |_, w, cx| {
                    daemon.update(cx, |s, _| s.sync_window_visible(false));
                    w.remove_window();
                }),
            ]
        };
        div().h_full().flex().children(buttons)
    }

    fn sidebar(&self, pages: &[Page], view: &Entity<Self>) -> impl IntoElement {
        div()
            .w(px(200.))
            .flex_shrink_0()
            .h_full()
            .p(px(10.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .bg(theme::surface())
            .border_r_1()
            .border_color(theme::border())
            .children(pages.iter().map(|&p| {
                let active = p == self.page;
                div()
                    .id(p.label())
                    .h(px(36.))
                    .px(px(10.))
                    .rounded(px(6.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .text_size(px(14.))
                    .text_color(if active { theme::text() } else { theme::subtle() })
                    .when(active, |d| d.bg(theme::overlay()))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
                    .on_click(on_view(view, move |this, _| {
                        this.page = p;
                        this.open_menu = None;
                    })
                    .pipe_click())
                    .child(icon(p.icon(), 16., if active { theme::accent() } else { theme::muted() }))
                    .child(p.label())
            }))
    }

    /// Rows for one monitor setting.
    fn feature_row(&self, f: &FeatureState, view: &Entity<Self>) -> AnyElement {
        let key = f.key;
        let disabled = f.blocked.is_some();
        let write: Callback<String> = {
            let view = view.clone();
            Rc::new(move |input, _, cx| view.update(cx, |this, cx| this.write(key, input, cx)))
        };
        let id = SharedString::from(key);
        let control: AnyElement = match f.kind {
            Kind::Continuous => {
                let write = write.clone();
                slider(id, f.current, f.max, disabled, Rc::new(move |v: u16, w: &mut Window, cx: &mut App| write(v.to_string(), w, cx)))
                    .into_any_element()
            }
            Kind::Toggle => {
                let next = if f.on { "off" } else { "on" };
                switch(id, f.on, disabled, move |w, cx| write(next.into(), w, cx)).into_any_element()
            }
            Kind::Enum => {
                let options: Vec<(String, String)> = f.choices.iter().map(|c| (c.name.clone(), c.label.clone())).collect();
                let short = options.len() <= 4 && options.iter().map(|(_, l)| l.len()).sum::<usize>() <= 28;
                if short {
                    segmented(key, &options, f.selected.as_deref(), disabled, write).into_any_element()
                } else {
                    let open = self.open_menu.as_ref() == Some(&id);
                    let toggle: Callback<bool> = {
                        let (view, id) = (view.clone(), id.clone());
                        Rc::new(move |open, _, cx| {
                            view.update(cx, |this, cx| {
                                this.open_menu = open.then(|| id.clone());
                                cx.notify();
                            })
                        })
                    };
                    dropdown(key, &options, f.selected.as_deref(), open, disabled, toggle, write).into_any_element()
                }
            }
            Kind::Action => div()
                .flex()
                .flex_wrap()
                .justify_end()
                .gap(px(6.))
                .children(f.choices.iter().map(|c| {
                    let (write, name) = (write.clone(), c.name.clone());
                    let variant = if f.force { Variant::Danger } else { Variant::Normal };
                    button(SharedString::from(format!("{key}-{}", c.name)), c.label.clone(), variant, disabled, move |w, cx| {
                        write(name.clone(), w, cx)
                    })
                }))
                .into_any_element(),
            Kind::Info => div().text_size(px(14.)).text_color(theme::subtle()).child(f.text.clone()).into_any_element(),
        };
        let hint = match &f.blocked {
            Some(reason) => Some((reason.clone(), theme::gold())),
            None => f.note.map(|n| (n.to_string(), theme::subtle())),
        };
        row(f.label, hint, control).into_any_element()
    }

    fn feature_page(&self, state: &DaemonState, page: Page, view: &Entity<Self>) -> Vec<AnyElement> {
        let mut features: Vec<&FeatureState> =
            state.active_features().iter().filter(|f| page_of(f) == page && !HIDDEN.contains(&f.key)).collect();
        // Picture mode decides what everything else can do, so it goes first.
        features.sort_by_key(|f| f.key != "mode");

        let mut sections: Vec<(&str, Vec<AnyElement>)> = Vec::new();
        for f in features {
            let title = section_of(f);
            let el = self.feature_row(f, view);
            match sections.iter_mut().find(|(t, _)| *t == title) {
                Some((_, rows)) => rows.push(el),
                None => sections.push((title, vec![el])),
            }
        }
        // The page heading already names a section with the same title.
        sections.into_iter().map(|(title, rows)| card(if title == page.label() { "" } else { title }, rows).into_any_element()).collect()
    }

    fn lightbar_page(&self, state: &DaemonState) -> Vec<AnyElement> {
        let Some(lb) = &state.lightbar else { return Vec::new() };
        let s = lb.state.clone();
        let d = &self.daemon;

        let temps: Vec<(String, String)> =
            ["Warm", "Soft", "Neutral", "Cool", "Daylight"].iter().enumerate().map(|(i, l)| (i.to_string(), l.to_string())).collect();
        let set_temp: Callback<String> = {
            let d = d.clone();
            Rc::new(move |v: String, _, cx| {
                let step: u8 = v.parse().unwrap_or(2);
                d.update(cx, |s, cx| s.update_lightbar(WorkerCommand::SetLightBarCct { step }, |l| (l.on, l.cct_step, l.is_rgb) = (true, step, false), cx))
            })
        };
        let set_level: Callback<u16> = {
            let d = d.clone();
            Rc::new(move |v: u16, _, cx| {
                let level = v.min(100) as u8;
                d.update(cx, |s, cx| {
                    s.update_lightbar(WorkerCommand::SetLightBarBrightness { level }, |l| (l.on, l.brightness, l.is_rgb) = (level > 0, level, false), cx)
                })
            })
        };
        let on = s.on;
        let white = vec![
            row("Power", None, switch("lb-power", on, false, on_daemon(d, move |s, cx| {
                s.update_lightbar(WorkerCommand::SetLightBarPower { on: !on }, |l| l.on = !on, cx)
            })))
            .into_any_element(),
            row("Brightness", None, slider("lb-level", u16::from(s.brightness), 100, false, set_level)).into_any_element(),
            row("Color temperature", None, segmented("lb-temp", &temps, (!s.is_rgb).then(|| s.cct_step.to_string()).as_deref(), false, set_temp))
                .into_any_element(),
        ];

        const EFFECTS: [(&str, RgbEffect); 5] = [
            ("Static", RgbEffect::Static),
            ("Breathing", RgbEffect::Breath),
            ("Strobe", RgbEffect::Strobe),
            ("Color cycle", RgbEffect::Cycle),
            ("Rainbow", RgbEffect::Rainbow),
        ];
        const SWATCHES: [(u8, u8, u8); 7] =
            [(235, 111, 146), (246, 193, 119), (156, 207, 216), (49, 116, 143), (196, 167, 231), (235, 188, 186), (255, 255, 255)];
        let effects: Vec<(String, String)> = EFFECTS.iter().enumerate().map(|(i, (l, _))| (i.to_string(), l.to_string())).collect();
        let current_effect = EFFECTS.iter().position(|(_, e)| *e == s.effect).unwrap_or(0);
        let (r, g, b) = (s.r, s.g, s.b);
        let set_effect: Callback<String> = {
            let d = d.clone();
            Rc::new(move |v: String, _, cx| {
                let effect = EFFECTS[v.parse::<usize>().unwrap_or(0).min(4)].1;
                d.update(cx, |s, cx| {
                    s.update_lightbar(WorkerCommand::SetLightBarEffect { effect, r, g, b }, |l| (l.on, l.is_rgb, l.effect) = (true, true, effect), cx)
                })
            })
        };
        let effect = s.effect;
        let swatches = div().flex().gap(px(8.)).children(SWATCHES.iter().map(|&(r, g, b)| {
            let active = s.is_rgb && (s.r, s.g, s.b) == (r, g, b);
            div()
                .id(SharedString::from(format!("swatch-{r}-{g}-{b}")))
                .size(px(24.))
                .rounded_full()
                .bg(rgb(u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)))
                .border_2()
                .border_color(if active { theme::text() } else { theme::surface() })
                .cursor_pointer()
                .on_click(on_daemon(d, move |s, cx| {
                    s.update_lightbar(WorkerCommand::SetLightBarEffect { effect, r, g, b }, |l| (l.on, l.is_rgb, l.r, l.g, l.b) = (true, true, r, g, b), cx)
                })
                .pipe_click())
        }));
        let rgb_rows = vec![
            row("Effect", None, segmented("lb-effect", &effects, s.is_rgb.then(|| current_effect.to_string()).as_deref(), false, set_effect))
                .into_any_element(),
            row("Color", None, swatches).into_any_element(),
        ];
        vec![card("White Light", white).into_any_element(), card("RGB Lighting", rgb_rows).into_any_element()]
    }

    fn profiles_page(&self, state: &DaemonState, view: &Entity<Self>, window: &Window) -> Vec<AnyElement> {
        let no_monitor = state.monitors.is_empty();
        let save = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(16.))
            .py(px(12.))
            .child(self.profile_name.render("profile-name", window, {
                let view = view.clone();
                move |ev: &KeyDownEvent, _, cx| {
                    view.update(cx, |this, cx| {
                        if this.profile_name.key(ev, cx) {
                            this.save_profile(cx);
                        }
                        cx.notify();
                    })
                }
            }))
            .child(button("save-profile", "Save current settings", Variant::Primary, no_monitor, on_view(view, |this, cx| this.save_profile(cx))));

        let mut rows = vec![save.into_any_element()];
        for (i, p) in state.profiles.iter().enumerate() {
            let (apply, export, delete) = (p.name.clone(), p.name.clone(), p.name.clone());
            let worker = state.worker.clone();
            let actions = div()
                .flex()
                .gap(px(6.))
                .child(button(("apply", i), "Apply", Variant::Primary, no_monitor, on_daemon(&self.daemon, move |s, _| {
                    s.worker.send(WorkerCommand::ApplyProfile { index: s.active, name: apply.clone() })
                })))
                .child(button(("export", i), "Export…", Variant::Normal, false, move |_, cx| {
                    let file = format!("{}.json", asusdisplay::profiles::sanitize_profile_name(&export));
                    let dir = std::env::home_dir().unwrap_or_default();
                    let rx = cx.prompt_for_new_path(&dir, Some(&file));
                    let (worker, name) = (worker.clone(), export.clone());
                    cx.spawn(async move |_| {
                        if let Ok(Ok(Some(path))) = rx.await {
                            worker.send(WorkerCommand::ExportProfile { name, path });
                        }
                    })
                    .detach();
                }))
                .child(button(("delete", i), "Delete", Variant::Danger, false, on_daemon(&self.daemon, move |s, _| {
                    s.worker.send(WorkerCommand::DeleteProfile { name: delete.clone() })
                })));
            rows.push(row(p.name.clone(), hint(format!("Saved from {}", p.model)), actions).into_any_element());
        }
        let worker = state.worker.clone();
        rows.push(
            row(
                "Import a profile",
                hint("Load a .json profile exported from AsusDisplay"),
                button("import", "Import…", Variant::Normal, false, move |_, cx| {
                    let rx = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: None });
                    let worker = worker.clone();
                    cx.spawn(async move |_| {
                        if let Ok(Ok(Some(paths))) = rx.await
                            && let Some(path) = paths.into_iter().next()
                        {
                            worker.send(WorkerCommand::ImportProfile { path });
                        }
                    })
                    .detach();
                }),
            )
            .into_any_element(),
        );

        let mut cards = vec![card("Saved Profiles", rows).into_any_element()];
        if let Some(mode) = state.feature("mode") {
            cards.push(card("Automatic Picture Mode", self.appsync_rows(state, mode, view, window)).into_any_element());
        }
        cards
    }

    fn appsync_rows(&self, state: &DaemonState, mode: &FeatureState, view: &Entity<Self>, window: &Window) -> Vec<AnyElement> {
        let cfg = &state.appsync;
        let modes: Vec<(String, String)> = mode.choices.iter().map(|c| (c.raw.to_string(), c.label.clone())).collect();
        let mode_label = |raw: u16| modes.iter().find(|(v, _)| *v == raw.to_string()).map_or("Unknown mode", |(_, l)| l.as_str()).to_string();
        let menu = |id: &'static str| -> (bool, Callback<bool>) {
            let view = view.clone();
            let open = self.open_menu.as_deref() == Some(id);
            (
                open,
                Rc::new(move |open, _, cx| {
                    view.update(cx, |this, cx| {
                        this.open_menu = open.then(|| id.into());
                        cx.notify();
                    })
                }),
            )
        };

        let enabled = cfg.enabled;
        let mut rows = vec![row(
            "Switch picture mode for apps",
            hint(match &state.appsync_match {
                Some(rule) => format!("Now using the mode for {rule}"),
                None => "When an app below is running, the monitor switches to its picture mode.".into(),
            }),
            switch("appsync-on", enabled, false, on_daemon(&self.daemon, move |s, cx| s.save_appsync(|c| c.enabled = !enabled, cx))),
        )
        .into_any_element()];

        let mut fallback = vec![("none".to_string(), "Leave it".to_string())];
        fallback.extend(modes.iter().cloned());
        let (open, toggle) = menu("appsync-fallback");
        let d = self.daemon.clone();
        rows.push(
            row(
                "When none of them is running",
                None,
                dropdown(
                    "appsync-fallback",
                    &fallback,
                    Some(cfg.default_mode.map_or_else(|| "none".to_string(), |m| m.to_string()).as_str()),
                    open,
                    false,
                    toggle,
                    Rc::new(move |v: String, _, cx| d.update(cx, |s, cx| s.save_appsync(|c| c.default_mode = v.parse().ok(), cx))),
                ),
            )
            .into_any_element(),
        );

        for (i, rule) in cfg.rules.iter().enumerate() {
            let target = rule.target_profile.clone().map_or_else(|| rule.target_mode.map(mode_label).unwrap_or_default(), |p| format!("Profile {p}"));
            let rule_on = rule.enabled;
            let controls = div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(switch(("rule-on", i), rule_on, false, on_daemon(&self.daemon, move |s, cx| {
                    s.save_appsync(|c| c.rules.get_mut(i).into_iter().for_each(|r| r.enabled = !rule_on), cx)
                })))
                .child(button(("rule-del", i), "Remove", Variant::Danger, false, on_daemon(&self.daemon, move |s, cx| {
                    s.save_appsync(|c| { if i < c.rules.len() { c.rules.remove(i); } }, cx)
                })));
            rows.push(row(rule.name.clone(), hint(format!("{} → {target}", rule.process_match)), controls).into_any_element());
        }

        let (open, toggle) = menu("appsync-new-mode");
        let pick_mode: Callback<String> = {
            let view = view.clone();
            Rc::new(move |v, _, cx| view.update(cx, |this, cx| {
                this.rule_mode = Some(v);
                cx.notify();
            }))
        };
        rows.push(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(16.))
                .py(px(12.))
                .child(self.rule_process.render("rule-process", window, {
                    let view = view.clone();
                    move |ev: &KeyDownEvent, _, cx| {
                        view.update(cx, |this, cx| {
                            if this.rule_process.key(ev, cx) {
                                this.add_rule(cx);
                            }
                            cx.notify();
                        })
                    }
                }))
                .child(dropdown("appsync-new-mode", &modes, self.rule_mode.as_deref(), open, false, toggle, pick_mode))
                .child(button("rule-add", "Add", Variant::Primary, false, on_view(view, |this, cx| this.add_rule(cx))))
                .into_any_element(),
        );
        rows
    }

    fn settings_page(&self, state: &DaemonState) -> Vec<AnyElement> {
        let autostart = state.autostart;
        let mut general = vec![row(
            "Launch at login",
            hint("Starts AsusDisplay in the system tray when you sign in."),
            switch("autostart", autostart, false, on_daemon(&self.daemon, move |s, cx| s.set_autostart(!autostart, cx))),
        )
        .into_any_element()];

        #[cfg(target_os = "linux")]
        {
            let on = state.auto_rotate;
            general.push(
                row(
                    "Auto-rotate",
                    hint("Rotates the desktop when you pivot the monitor (needs xrandr, kscreen-doctor or hyprctl)."),
                    switch("auto-rotate", on, false, on_daemon(&self.daemon, move |s, cx| {
                        s.auto_rotate = !on;
                        s.worker.send(WorkerCommand::SetAutoRotate { enabled: !on });
                        cx.notify();
                    })),
                )
                .into_any_element(),
            );
        }

        let worker = state.worker.clone();
        general.push(
            row(
                "Quit AsusDisplay",
                hint("Closing the window keeps it running in the tray. This stops it."),
                button("quit", "Quit", Variant::Normal, false, move |_, cx| {
                    worker.send(WorkerCommand::Shutdown);
                    crate::ipc::cleanup_ipc_file();
                    cx.quit();
                }),
            )
            .into_any_element(),
        );

        let help = vec![
            row(
                "Support report",
                hint("Copies this monitor's details so you can paste them into a bug report."),
                button("report", "Copy report", Variant::Normal, state.monitors.is_empty(), on_daemon(&self.daemon, |s, _| {
                    s.worker.send(WorkerCommand::GenerateReport { index: s.active })
                })),
            )
            .into_any_element(),
            row(
                format!("AsusDisplay {}", env!("CARGO_PKG_VERSION")),
                hint("Free and open source. A star on GitHub helps other people find it."),
                button("github", "Open GitHub", Variant::Primary, false, |_, cx| cx.open_url(REPO_URL)),
            )
            .into_any_element(),
        ];
        vec![card("General", general).into_any_element(), card("Help", help).into_any_element()]
    }

    /// Shown instead of a settings page while there is nothing to show yet.
    fn placeholder(&self, state: &DaemonState) -> Option<AnyElement> {
        let rescan = || button("placeholder-rescan", "Rescan", Variant::Primary, false, on_daemon(&self.daemon, |s, cx| s.discover(cx)));
        let message = |title: String, body: &str| {
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .p(px(24.))
                .rounded(px(10.))
                .bg(theme::surface())
                .border_1()
                .border_color(theme::border())
                .child(div().text_size(px(15.)).child(title))
                .child(div().text_size(px(13.)).text_color(theme::subtle()).child(body.to_string()))
        };
        if state.monitors.is_empty() {
            let el = if state.scanning {
                message("Looking for monitors…".into(), "This takes a few seconds.")
            } else {
                message(
                    "No monitors found".into(),
                    "Make sure DDC/CI is turned on in your monitor's menu (usually under System Setup), then rescan.",
                )
                .child(div().pt(px(8.)).child(rescan()))
            };
            return Some(el.into_any_element());
        }
        if state.failed.contains(&state.active) {
            let model = &state.monitors[state.active].model_name;
            let el = message(
                format!("Can't reach {model}"),
                "It stopped answering, which can happen after a graphics driver restart or a cable change. Check that DDC/CI is on in the monitor's menu, then rescan.",
            )
            .child(div().pt(px(8.)).child(rescan()));
            return Some(el.into_any_element());
        }
        if !state.features.contains_key(&state.active) {
            let model = &state.monitors[state.active].model_name;
            return Some(message(format!("Reading settings from {model}…"), "This takes a few seconds the first time.").into_any_element());
        }
        None
    }

    fn toast(state: &DaemonState) -> Option<impl IntoElement> {
        let (message, is_error) = state.status.clone()?;
        Some(
            div()
                .absolute()
                .bottom(px(16.))
                .right(px(16.))
                .max_w(px(440.))
                .px(px(14.))
                .py(px(10.))
                .rounded(px(8.))
                .bg(theme::overlay())
                .border_1()
                .border_color(if is_error { theme::love() } else { theme::border() })
                .shadow_lg()
                .flex()
                .items_center()
                .gap(px(10.))
                .text_size(px(13.))
                .child(div().size(px(8.)).flex_shrink_0().rounded_full().bg(if is_error { theme::love() } else { theme::foam() }))
                .child(message),
        )
    }

    fn confirm_dialog(&self, view: &Entity<Self>) -> Option<impl IntoElement> {
        let c = self.confirm.as_ref()?;
        let (key, input) = (c.key, c.input.clone());
        Some(
            div()
                .id("confirm")
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .occlude()
                .bg(theme::backdrop())
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .w(px(420.))
                        .p(px(20.))
                        .rounded(px(12.))
                        .bg(theme::surface())
                        .border_1()
                        .border_color(theme::border())
                        .shadow_lg()
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .child(div().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).child(c.title.clone()))
                        .child(div().text_size(px(13.)).text_color(theme::subtle()).child(c.body.clone()))
                        .child(
                            div()
                                .pt(px(8.))
                                .flex()
                                .justify_end()
                                .gap(px(8.))
                                .child(button("confirm-cancel", "Cancel", Variant::Normal, false, on_view(view, |this, _| this.confirm = None)))
                                .child(button("confirm-ok", "Continue", Variant::Danger, false, on_view(view, move |this, cx| {
                                    this.confirm = None;
                                    let input = input.clone();
                                    this.daemon.update(cx, |s, cx| s.write(key, input, true, cx));
                                }))),
                        ),
                ),
        )
    }
}

impl Render for MainView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let daemon = self.daemon.clone();
        let state = daemon.read(cx);

        let pages: Vec<Page> = Page::ALL
            .into_iter()
            .filter(|&p| match p {
                Page::Picture | Page::Profiles | Page::Settings => true,
                Page::LightBar => state.lightbar.is_some(),
                _ => state.active_features().iter().any(|f| page_of(f) == p && !HIDDEN.contains(&f.key)),
            })
            .collect();
        if !pages.contains(&self.page) {
            self.page = Page::Picture;
        }

        let body = match self.page {
            Page::LightBar => self.lightbar_page(state),
            Page::Profiles => self.profiles_page(state, &view, window),
            Page::Settings => self.settings_page(state),
            page => match self.placeholder(state) {
                Some(el) => vec![el],
                None => self.feature_page(state, page, &view),
            },
        };

        let content = div().id("content").flex_1().h_full().overflow_y_scroll().child(
            div()
                .w_full()
                .max_w(px(820.))
                .mx_auto()
                .px(px(28.))
                .py(px(24.))
                .flex()
                .flex_col()
                .gap(px(24.))
                .child(div().text_size(px(22.)).font_weight(FontWeight::SEMIBOLD).child(self.page.label()))
                .children(body),
        );

        let client_decorated = matches!(window.window_decorations(), Decorations::Client { .. });
        div()
            .id("root")
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::base())
            .text_color(theme::text())
            .text_size(px(14.))
            .when(cfg!(target_os = "linux") && client_decorated, |d| d.border_1().border_color(theme::border()))
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                if ev.keystroke.key == "escape" {
                    this.open_menu = None;
                    this.confirm = None;
                    cx.notify();
                }
            }))
            .child(self.titlebar(daemon.read(cx), &view, window))
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .flex()
                    .child(self.sidebar(&pages, &view))
                    .child(content),
            )
            .children(Self::toast(daemon.read(cx)))
            .children(self.confirm_dialog(&view))
            .children(resize_handles(window))
    }
}

/// Makes `el` move the window: the OS handles it on Windows, we start the move on Linux.
fn drag_area(el: Stateful<Div>, view: &Entity<MainView>) -> Stateful<Div> {
    #[cfg(target_os = "linux")]
    {
        let (down, up, moved) = (view.clone(), view.clone(), view.clone());
        el.on_mouse_down(MouseButton::Left, move |ev, w, cx| {
            if ev.click_count >= 2 {
                w.zoom_window();
            } else {
                down.update(cx, |this, _| this.move_pending = true);
            }
        })
        .on_mouse_up(MouseButton::Left, move |_, _, cx| up.update(cx, |this, _| this.move_pending = false))
        .on_mouse_move(move |_, w, cx| {
            if moved.read(cx).move_pending {
                moved.update(cx, |this, _| this.move_pending = false);
                w.start_window_move();
            }
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (view, MouseButton::Left);
        el.window_control_area(gpui::WindowControlArea::Drag)
    }
}

/// Edge strips that resize a client-decorated Linux window. Windows keeps its native frame.
fn resize_handles(window: &Window) -> Vec<AnyElement> {
    #[cfg(target_os = "linux")]
    {
        use gpui::{CursorStyle as C, ResizeEdge as E};
        if !matches!(window.window_decorations(), Decorations::Client { .. }) || window.is_maximized() {
            return Vec::new();
        }
        let s = px(6.);
        let edge = |id: &'static str, edge: E, cursor: C, place: fn(Div, gpui::Pixels) -> Div| {
            place(div().absolute(), s)
                .id(id)
                .cursor(cursor)
                .on_mouse_down(MouseButton::Left, move |_, w, _| w.start_window_resize(edge))
                .into_any_element()
        };
        vec![
            edge("rz-t", E::Top, C::ResizeUpDown, |d, s| d.top_0().left(s).right(s).h(s)),
            edge("rz-b", E::Bottom, C::ResizeUpDown, |d, s| d.bottom_0().left(s).right(s).h(s)),
            edge("rz-l", E::Left, C::ResizeLeftRight, |d, s| d.left_0().top(s).bottom(s).w(s)),
            edge("rz-r", E::Right, C::ResizeLeftRight, |d, s| d.right_0().top(s).bottom(s).w(s)),
            edge("rz-tl", E::TopLeft, C::ResizeUpLeftDownRight, |d, s| d.top_0().left_0().size(s)),
            edge("rz-tr", E::TopRight, C::ResizeUpRightDownLeft, |d, s| d.top_0().right_0().size(s)),
            edge("rz-bl", E::BottomLeft, C::ResizeUpRightDownLeft, |d, s| d.bottom_0().left_0().size(s)),
            edge("rz-br", E::BottomRight, C::ResizeUpLeftDownRight, |d, s| d.bottom_0().right_0().size(s)),
        ]
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = window;
        Vec::new()
    }
}

/// Adapts a `(window, cx)` handler to the `(event, window, cx)` shape `on_click` wants.
trait PipeClick {
    fn pipe_click(self) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static;
}

impl<F: Fn(&mut Window, &mut App) + 'static> PipeClick for F {
    fn pipe_click(self) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static {
        move |_, w, cx| self(w, cx)
    }
}
