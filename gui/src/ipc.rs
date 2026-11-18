use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use crate::tray::TrayAction;

fn daemon_addr_path() -> PathBuf {
    asusdisplay::config_dir().join("daemon.addr")
}

/// Attempts to contact a running instance via `daemon.addr` and ask it to show its window.
/// Returns `true` if an existing instance answered `OK`.
pub fn try_activate_existing() -> bool {
    let path = daemon_addr_path();
    let Ok(contents) = fs::read_to_string(&path) else {
        return false;
    };
    let Ok(addr) = contents.trim().parse::<SocketAddr>() else {
        return false;
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(150)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(200)));

    if stream.write_all(b"SHOW\n").is_err() {
        return false;
    }
    let _ = stream.flush();

    let mut buf = [0u8; 16];
    let Ok(n) = stream.read(&mut buf) else {
        return false;
    };
    buf[..n].starts_with(b"OK")
}

/// Binds a loopback IPC listener, records its port in `daemon.addr`, and forwards
/// `SHOW` activation requests onto `action_tx`.
pub fn spawn_ipc_listener(action_tx: Sender<TrayAction>) {
    let config_dir = asusdisplay::config_dir();
    let _ = fs::create_dir_all(&config_dir);

    let Ok(listener) = TcpListener::bind("127.0.0.1:0") else {
        return;
    };
    let Ok(addr) = listener.local_addr() else {
        return;
    };
    if fs::write(daemon_addr_path(), addr.to_string()).is_err() {
        return;
    }

    let _ = thread::Builder::new()
        .name("asusdisplay-ipc".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let _ = stream.set_read_timeout(Some(Duration::from_millis(250)));
                let _ = stream.set_write_timeout(Some(Duration::from_millis(250)));

                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                if reader.read_line(&mut line).is_ok() && line.trim() == "SHOW" {
                    let mut stream = reader.into_inner();
                    let _ = stream.write_all(b"OK\n");
                    let _ = stream.flush();
                    if action_tx.send(TrayAction::ShowWindow).is_err() {
                        break;
                    }
                }
            }
        });
}

/// Removes the `daemon.addr` file on clean shutdown.
pub fn cleanup_ipc_file() {
    let _ = fs::remove_file(daemon_addr_path());
}
