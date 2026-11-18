#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod app;
mod autostart;
mod components;
mod ipc;
mod theme;
mod tray;
mod worker;

use std::sync::mpsc;
use std::time::Duration;

use gpui::{
    App, AppContext, Bounds, Entity, QuitMode, SharedString, TitlebarOptions, WindowBounds,
    WindowOptions, px, size,
};

use crate::app::{DaemonState, MainView};
use crate::tray::{SystemTrayHandle, TrayAction};
use crate::worker::WorkerCommand;

fn open_main_window(cx: &mut App, daemon: Entity<DaemonState>) {
    let bounds = Bounds::centered(None, size(px(1160.0), px(780.0)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some(SharedString::from("AsusDisplay")),
            appears_transparent: false,
            traffic_light_position: None,
        }),
        window_min_size: Some(size(px(900.0), px(620.0))),
        ..Default::default()
    };
    let _ = cx.open_window(options, |window, cx| {
        cx.new(|cx| MainView::new(daemon, window, cx))
    });
    cx.activate(true);
}

fn main() {
    let start_in_tray = std::env::args()
        .any(|arg| matches!(arg.as_str(), "--tray" | "--minimized" | "--background"));

    if !start_in_tray && ipc::try_activate_existing() {
        return;
    }

    let (worker_handle, worker_rx) = worker::spawn_worker();
    let (tray_tx, tray_rx) = mpsc::channel::<TrayAction>();
    let tray_handle = SystemTrayHandle::spawn(tray_tx.clone());
    ipc::spawn_ipc_listener(tray_tx);

    gpui_platform::application()
        .with_quit_mode(QuitMode::Explicit)
        .run(move |cx: &mut App| {
            let daemon = cx.new(|cx| {
                DaemonState::new(worker_handle.clone(), tray_handle, !start_in_tray, cx)
            });

            if !start_in_tray {
                open_main_window(cx, daemon.clone());
            }

            let poll_daemon = daemon.clone();
            let shutdown_worker = worker_handle.clone();
            cx.spawn(async move |cx: &mut gpui::AsyncApp| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(32))
                        .await;

                    let mut worker_events = Vec::new();
                    while let Ok(ev) = worker_rx.try_recv() {
                        worker_events.push(ev);
                    }

                    let mut tray_actions = Vec::new();
                    while let Ok(act) = tray_rx.try_recv() {
                        tray_actions.push(act);
                    }

                    let should_quit = cx.update(|cx: &mut App| {
                        let win_open = !cx.windows().is_empty();
                        poll_daemon.update(cx, |state, _| {
                            state.sync_window_visible(win_open);
                        });

                        if !worker_events.is_empty() {
                            poll_daemon.update(cx, |state, cx| {
                                for ev in worker_events {
                                    state.handle_worker_event(ev, cx);
                                }
                            });
                        }

                        for action in tray_actions {
                            match action {
                                TrayAction::ToggleWindow => {
                                    if let Some(win) = cx.windows().first().copied() {
                                        let _ = win.update(cx, |_, window, _| {
                                            window.remove_window();
                                        });
                                        poll_daemon.update(cx, |state, _| {
                                            state.sync_window_visible(false);
                                        });
                                    } else {
                                        open_main_window(cx, poll_daemon.clone());
                                        poll_daemon.update(cx, |state, _| {
                                            state.sync_window_visible(true);
                                        });
                                    }
                                }
                                TrayAction::ShowWindow => {
                                    if let Some(win) = cx.windows().first().copied() {
                                        let _ = win.update(cx, |_, window, cx| {
                                            window.activate();
                                            cx.activate(true);
                                        });
                                    } else {
                                        open_main_window(cx, poll_daemon.clone());
                                    }
                                    poll_daemon.update(cx, |state, _| {
                                        state.sync_window_visible(true);
                                    });
                                }
                                TrayAction::Quit => {
                                    shutdown_worker.send(WorkerCommand::Shutdown);
                                    ipc::cleanup_ipc_file();
                                    cx.quit();
                                    return true;
                                }
                                other => {
                                    poll_daemon.update(cx, |state, cx| {
                                        state.handle_tray_action(other, cx);
                                    });
                                }
                            }
                        }

                        false
                    });

                    if should_quit {
                        break;
                    }
                }
            })
            .detach();
        });
}
