use std::fmt::Write;

use crate::{Kind, Monitor};

pub fn generate_device_report(mon: &mut Monitor) -> String {
    let mut out = String::new();

    let model = mon.model.clone();
    let connector = mon.connector.clone().unwrap_or_else(|| "-".to_string());
    let id = mon.id.clone();
    let panel_type = format!(
        "{:?}{}",
        mon.product_line(),
        if mon.is_oled() { " (OLED)" } else { "" }
    );
    let is_asus = mon.is_asus();
    let is_asus_vcp = mon.is_asus_vcp();
    let table_kind = if mon.new_table() { "new (v2)" } else { "legacy" };

    let fw = match mon.get_vcp(0xFF) {
        Ok(v) => format!("{} (panel 0x{:02X})", v.max, v.current & 0xFF),
        Err(_) => match mon.get_vcp(0xFA) {
            Ok(v) => format!("0xFA={} (max {})", v.current, v.max),
            Err(_) => "unavailable".to_string(),
        },
    };

    let (caps_count, caps_raw) = match mon.capabilities() {
        Ok(caps) => (caps.vcp.len().to_string(), Ok(caps.raw.clone())),
        Err(e) => ("unavailable".to_string(), Err(e)),
    };

    let _ = writeln!(out, "## Monitor Identity\n");
    let _ = writeln!(out, "| Field | Value |");
    let _ = writeln!(out, "|---|---|");
    let _ = writeln!(out, "| Model | {} |", md_escape(&model));
    let _ = writeln!(out, "| Connector | {} |", md_escape(&connector));
    let _ = writeln!(out, "| Bus / ID | `{}` |", md_escape(&id));
    let _ = writeln!(out, "| Panel Type | {} |", md_escape(&panel_type));
    let _ = writeln!(
        out,
        "| ASUS Detected | {} (0xEF vendor: {}) |",
        is_asus, is_asus_vcp
    );
    let _ = writeln!(out, "| VCP Table | {} |", table_kind);
    let _ = writeln!(out, "| Firmware | {} |", md_escape(&fw));
    let _ = writeln!(out, "| Capabilities VCP Count | {} |", caps_count);

    let _ = writeln!(out, "\n## Raw Capabilities\n");
    match caps_raw {
        Ok(raw) => {
            let _ = writeln!(out, "```text\n{raw}\n```");
        }
        Err(err) => {
            let _ = writeln!(out, "```text\n<unavailable: {err}>\n```");
        }
    }

    let _ = writeln!(out, "\n## Supported Features\n");
    let _ = writeln!(out, "| Code | Feature | CLI Name | Value | Max | Raw (SH/SL) |");
    let _ = writeln!(out, "|---|---|---|---|---|---|");

    let features = mon.features(false);
    for f in features.into_iter().filter(|f| f.kind != Kind::Action) {
        match mon.read(&f) {
            Ok(r) => {
                let sh = r.current >> 8;
                let sl = r.current & 0xFF;
                let _ = writeln!(
                    out,
                    "| `0x{:02X}` | {} | `{}` | {} | {} | `0x{:04X}` (`0x{:02X}` / `0x{:02X}`) |",
                    f.code,
                    md_escape(f.label),
                    f.key,
                    md_escape(&r.text),
                    r.max,
                    r.current,
                    sh,
                    sl
                );
            }
            Err(e) => {
                let _ = writeln!(
                    out,
                    "| `0x{:02X}` | {} | `{}` | <{}> | - | - |",
                    f.code,
                    md_escape(f.label),
                    f.key,
                    md_escape(&e)
                );
            }
        }
    }

    let _ = writeln!(out, "\n## Vendor VCP Scan (`0xE0`–`0xFE`)\n");
    let _ = writeln!(out, "| Code | Current | Max | Raw (SH/SL) |");
    let _ = writeln!(out, "|---|---|---|---|");
    let mut found_vendor = false;
    for code in 0xE0u8..=0xFE {
        if let Ok(v) = mon.get_vcp(code) {
            found_vendor = true;
            let sh = v.current >> 8;
            let sl = v.current & 0xFF;
            let _ = writeln!(
                out,
                "| `0x{code:02X}` | {} (`0x{:04X}`) | {} (`0x{:04X}`) | `0x{sh:02X}` / `0x{sl:02X}` |",
                v.current, v.current, v.max, v.max
            );
        }
    }
    if !found_vendor {
        let _ = writeln!(out, "| - | No responding vendor VCP codes | - | - |");
    }

    out
}

fn md_escape(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::{Ddc, Edid, Result, Vcp};

    struct FakeDdc(HashMap<u8, Vcp>);

    impl Ddc for FakeDdc {
        fn get(&mut self, code: u8) -> Result<Vcp> {
            self.0
                .get(&code)
                .copied()
                .ok_or_else(|| format!("0x{code:02X} unsupported"))
        }

        fn set(&mut self, _code: u8, _value: u16) -> Result<()> {
            Ok(())
        }

        fn capabilities(&mut self) -> Result<String> {
            Ok("(prot(monitor)type(LCD)model(XG27ACDMS)vcp(10 12 14(05 0B) DC(04 0A) EF FD(7879) FF))".into())
        }
    }

    #[test]
    fn report_contains_expected_markdown_sections() {
        let mut map = HashMap::new();
        map.insert(0x10, Vcp { current: 70, max: 100 });
        map.insert(0x12, Vcp { current: 80, max: 100 });
        map.insert(0x14, Vcp { current: 0x0B, max: 0x0B });
        map.insert(0xDC, Vcp { current: 4, max: 10 });
        map.insert(0xEF, Vcp { current: 0xB1, max: 0x0217 });
        map.insert(0xFD, Vcp { current: 0x7869, max: 0x7879 });
        map.insert(0xFF, Vcp { current: 0x30, max: 102 });

        let edid = Edid {
            manufacturer: "AUS".into(),
            name: Some("XG27ACDMS".into()),
            serial: None,
        };
        let mut mon = Monitor::new(
            Box::new(FakeDdc(map)),
            "/dev/i2c-4".into(),
            Some("DP-1".into()),
            Some(edid),
            None,
        );

        let report = generate_device_report(&mut mon);
        assert!(report.contains("## Monitor Identity"));
        assert!(report.contains("| Model | XG27ACDMS |"));
        assert!(report.contains("| Connector | DP-1 |"));
        assert!(report.contains("| Firmware | 102 (panel 0x30) |"));
        assert!(report.contains("## Raw Capabilities"));
        assert!(report.contains("## Supported Features"));
        assert!(report.contains("| `0x10` | Brightness | `brightness` |"));
        assert!(report.contains("## Vendor VCP Scan (`0xE0`–`0xFE`)"));
        assert!(report.contains("| `0xEF` |"));
        assert!(report.contains("| `0xFD` |"));
    }
}
