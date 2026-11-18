#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod app;
mod assets;
mod autostart;
mod components;
mod ipc;
mod theme;
mod tray;
mod worker;

use std::sync::{Arc, mpsc};

use gpui::{
    App, AppContext, Bounds, Entity, QuitMode, SharedString, TitlebarOptions, WindowBounds, WindowDecorations,
    WindowOptions, px, size,
};

use crate::app::{DaemonState, MainView};
use crate::tray::{SystemTrayHandle, TrayAction};
use crate::worker::WorkerCommand;

fn open_main_window(cx: &mut App, daemon: Entity<DaemonState>) {
    let bounds = Bounds::centered(None, size(px(1000.0), px(720.0)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        // We draw our own title bar (app::MainView::render_titlebar).
        titlebar: Some(TitlebarOptions {
            title: Some(SharedString::from("AsusDisplay")),
            appears_transparent: true,
            traffic_light_position: None,
        }),
        window_decorations: Some(WindowDecorations::Client),
        app_id: Some("asusdisplay".into()),
        // Linux only; Windows takes the icon embedded by build.rs.
        icon: Some(Arc::new(assets::app_icon(128))),
        window_min_size: Some(size(px(760.0), px(520.0))),
        ..Default::default()
    };
    let daemon_for_sync = daemon.clone();
    if let Ok(win) = cx.open_window(options, |window, cx| {
        cx.new(|cx| MainView::new(daemon, window, cx))
    }) {
        daemon_for_sync.update(cx, |state, _| {
            state.sync_window_visible(true);
        });
        let daemon_on_close = daemon_for_sync.clone();
        let _ = win.update(cx, |_, window, cx| {
            window.on_window_should_close(cx, move |_, cx| {
                daemon_on_close.update(cx, |state, _| {
                    state.sync_window_visible(false);
                });
                true
            });
        });
    }
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

    // Bridge blocking mpsc tray/IPC events onto an async_channel for zero-polling GPUI wakeup.
    let (async_tray_tx, async_tray_rx) = async_channel::unbounded::<TrayAction>();
    let _ = std::thread::Builder::new()
        .name("asusdisplay-tray-bridge".into())
        .spawn(move || {
            while let Ok(act) = tray_rx.recv() {
                if async_tray_tx.try_send(act).is_err() {
                    break;
                }
            }
        });

    gpui_platform::application()
        .with_quit_mode(QuitMode::Explicit)
        .run(move |cx: &mut App| {
            let daemon = cx.new(|cx| {
                DaemonState::new(worker_handle.clone(), tray_handle, !start_in_tray, cx)
            });

            if !start_in_tray {
                open_main_window(cx, daemon.clone());
            }

            // Event-driven worker listener (0ms wakeup latency, zero idle polling).
            let worker_daemon = daemon.clone();
            cx.spawn(async move |cx: &mut gpui::AsyncApp| {
                while let Ok(first_ev) = worker_rx.recv().await {
                    let mut batch = vec![first_ev];
                    while let Ok(ev) = worker_rx.try_recv() {
                        batch.push(ev);
                    }
                    cx.update(|cx: &mut App| {
                        worker_daemon.update(cx, |state, cx| {
                            for ev in batch {
                                state.handle_worker_event(ev, cx);
                            }
                        });
                    });
                }
            })
            .detach();

            // Event-driven tray / single-instance IPC listener.
            let tray_daemon = daemon.clone();
            let shutdown_worker = worker_handle.clone();
            cx.spawn(async move |cx: &mut gpui::AsyncApp| {
                while let Ok(first_act) = async_tray_rx.recv().await {
                    let mut batch = vec![first_act];
                    while let Ok(act) = async_tray_rx.try_recv() {
                        batch.push(act);
                    }
                    let should_quit = cx.update(|cx: &mut App| {
                        for action in batch {
                            match action {
                                TrayAction::ToggleWindow => {
                                    if let Some(win) = cx.windows().first().copied() {
                                        let _ = win.update(cx, |_, window, _| {
                                            window.remove_window();
                                        });
                                        tray_daemon.update(cx, |state, _| {
                                            state.sync_window_visible(false);
                                        });
                                    } else {
                                        open_main_window(cx, tray_daemon.clone());
                                    }
                                }
                                TrayAction::ShowWindow => {
                                    if let Some(win) = cx.windows().first().copied() {
                                        let _ = win.update(cx, |_, window, cx| {
                                            window.activate();
                                            cx.activate(true);
                                        });
                                        tray_daemon.update(cx, |state, _| {
                                            state.sync_window_visible(true);
                                        });
                                    } else {
                                        open_main_window(cx, tray_daemon.clone());
                                    }
                                }
                                TrayAction::Quit => {
                                    shutdown_worker.send(WorkerCommand::Shutdown);
                                    ipc::cleanup_ipc_file();
                                    cx.quit();
                                    return true;
                                }
                                other => {
                                    tray_daemon.update(cx, |state, cx| {
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
