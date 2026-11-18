#[cfg(target_os = "linux")]
mod platform {
    use std::fs;
    use std::io::ErrorKind;
    use std::path::PathBuf;

    fn autostart_file() -> Result<PathBuf, String> {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .ok_or_else(|| "HOME is not set".to_string())?;
        Ok(config.join("autostart").join("asusdisplay.desktop"))
    }

    pub fn is_autostart_enabled() -> bool {
        autostart_file().is_ok_and(|p| p.exists())
    }

    pub fn set_autostart_enabled(enabled: bool) -> Result<(), String> {
        let file = autostart_file()?;
        if !enabled {
            return match fs::remove_file(&file) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
                Err(e) => Err(format!("failed to remove autostart entry: {e}")),
            };
        }

        let exe = std::env::var_os("APPIMAGE")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .map(Ok)
            .unwrap_or_else(std::env::current_exe)
            .map_err(|e| format!("failed to locate executable: {e}"))?;

        let exe_str = exe.display().to_string();
        let exec_line = if exe_str.contains(' ') {
            format!("\"{exe_str}\" --tray")
        } else {
            format!("{exe_str} --tray")
        };

        let entry = format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=AsusDisplay\n\
             Comment=ASUS Monitor DDC/CI Control Daemon\n\
             Exec={exec_line}\n\
             Icon=video-display\n\
             Terminal=false\n\
             Categories=Utility;HardwareSettings;Settings;\n\
             StartupNotify=false\n"
        );

        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create autostart directory: {e}"))?;
        }
        fs::write(&file, entry).map_err(|e| format!("failed to write autostart entry: {e}"))
    }
}

#[cfg(windows)]
mod platform {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE_NAME: &str = "AsusDisplay";
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub fn is_autostart_enabled() -> bool {
        Command::new("reg.exe")
            .args(["query", RUN_KEY, "/v", VALUE_NAME])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .is_ok_and(|o| o.status.success())
    }

    pub fn set_autostart_enabled(enabled: bool) -> Result<(), String> {
        if enabled {
            let exe = std::env::current_exe()
                .map_err(|e| format!("failed to locate executable: {e}"))?;
            let value = format!("\"{}\" --tray", exe.display());
            let out = Command::new("reg.exe")
                .args([
                    "add", RUN_KEY, "/v", VALUE_NAME, "/t", "REG_SZ", "/d", &value, "/f",
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .map_err(|e| format!("failed to run reg.exe: {e}"))?;
            if !out.status.success() {
                let err = String::from_utf8_lossy(&out.stderr);
                return Err(format!("reg.exe add failed: {}", err.trim()));
            }
            Ok(())
        } else {
            if !is_autostart_enabled() {
                return Ok(());
            }
            let out = Command::new("reg.exe")
                .args(["delete", RUN_KEY, "/v", VALUE_NAME, "/f"])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .map_err(|e| format!("failed to run reg.exe: {e}"))?;
            if !out.status.success() {
                let err = String::from_utf8_lossy(&out.stderr);
                return Err(format!("reg.exe delete failed: {}", err.trim()));
            }
            Ok(())
        }
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod platform {
    pub fn is_autostart_enabled() -> bool {
        false
    }

    pub fn set_autostart_enabled(_enabled: bool) -> Result<(), String> {
        Err("autostart is only supported on Linux and Windows".into())
    }
}

pub fn is_autostart_enabled() -> bool {
    platform::is_autostart_enabled()
}

pub fn set_autostart_enabled(enabled: bool) -> Result<(), String> {
    platform::set_autostart_enabled(enabled)
}
