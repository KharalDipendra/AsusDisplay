# Advanced

Command line, troubleshooting and how AsusDisplay talks to your monitor. For everyday use, the
[README](../README.md) is all you need.

## How it works

DisplayWidget Center doesn't use a special driver. It sends standard **DDC/CI** commands (VESA
MCCS) over the display cable, plus a set of ASUS-specific VCP codes (`0xDC`, `0xE0`–`0xFF`).
AsusDisplay sends the same commands:

- **Linux:** straight over `/dev/i2c-*` (just needs the `i2c-dev` kernel module; no ddcutil), with a
  native D-Bus `StatusNotifierItem` tray icon (`ksni`) that works on Wayland and X11 without GTK.
- **Windows:** through the built-in monitor API (`dxva2.dll`), with a native Win32 tray icon.

The app has no model list. It reads each monitor's capability report and the ASUS table version,
then shows the settings that monitor exposes. The GUI is pure Rust, rendered on the GPU with
[GPUI](https://gpui.rs/): no WebView, no Node, no .NET runtime. Closing the window frees the GPU
surface and leaves only the tray icon running.

The full code map, and how each setting was worked out, is in [PROTOCOL.md](PROTOCOL.md).

## Command line

Building the workspace also gives you `asusdisplay`, a CLI for scripts and keyboard shortcuts.

```sh
asusdisplay list                       # monitors
asusdisplay status                     # every supported setting and its value
asusdisplay get brightness
asusdisplay set brightness 40
asusdisplay set brightness +10         # relative
asusdisplay set mode fps               # GameVisual / Splendid mode
asusdisplay set uniform-brightness off # toggles take on / off / toggle
asusdisplay set input hdmi1
asusdisplay -d VG27 set contrast 75    # pick a monitor by index, model, serial or connector
asusdisplay -d all set brightness -10  # every monitor at once
asusdisplay profile save gaming        # save current settings to a named profile
asusdisplay profile apply gaming       # restore a saved profile
asusdisplay profile list               # list saved profiles
asusdisplay report                     # generate a markdown device report for GitHub issues
asusdisplay debug                      # list settings this monitor doesn't expose, and why
asusdisplay features                   # names, VCP codes and accepted values
```

Brightness and the other sliders apply in about 0.2 s, so they work well bound to keyboard
shortcuts. Disruptive settings and values outside what the monitor advertises need `--force`.
`asusdisplay raw 0xE9` reads any VCP code and `asusdisplay raw 0xE9 1` writes it, without checks.

If you have an ASUS monitor light bar, `asusdisplay lightbar white 60 5000` and
`asusdisplay lightbar rgb static 255 80 0` control it (Linux, hidraw).

`asusdisplay-gui --tray` starts straight into the tray. `asusdisplay-gui --monitor 1` opens on the
second monitor's tab.

## Troubleshooting

- **Nothing found:** turn on DDC/CI in the monitor's on-screen menu. Don't run DisplayWidget Center
  at the same time; DDC/CI doesn't like two programs talking to the monitor together.
- **Linux, nothing found with the NVIDIA proprietary driver:** add
  `options nvidia NVreg_RegistryDwords=RMUseSwI2c=0x01;RMI2cSpeed=100` to
  `/etc/modprobe.d/nvidia-i2c.conf` and reboot.
- **Linux, permission denied:** the udev rule in the README grants the logged-in user access to GPU
  I2C buses only. If you already run `ddcutil`, its rule does the same.
- **Reads fail now and then:** some monitors need more time between commands. Try `--sleep-mult 2`.

## Adding your device

1. Click **Copy report** in the app's Settings page, or run `asusdisplay report > report.md`, and
   attach it to a GitHub issue.
2. For a setting that reads wrong or `unknown`: run `asusdisplay raw 0xNN`, change it in the
   monitor's OSD, run it again, and note which value moved.
3. Add or fix entries in `src/features.rs`, run `cargo test`, and open a PR. Mention which settings
   you checked on the real monitor.
