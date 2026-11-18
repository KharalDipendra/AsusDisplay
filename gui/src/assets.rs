//! Monochrome SVG icons (filled paths; GPUI tints them with `text_color`) and the tray icon bitmap.

use gpui::{Rgba, px, svg};

macro_rules! icon {
    ($name:ident, $body:literal) => {
        pub const $name: &[u8] = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="#FFFFFF">"##,
            $body,
            "</svg>"
        )
        .as_bytes();
    };
}

/// One-colour version of the app icon: a monitor with an A mark (24x24).
pub const ICON_LOGO: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#FFFFFF"><path fill-rule="evenodd" d="M3 4.5A2.5 2.5 0 0 1 5.5 2h13A2.5 2.5 0 0 1 21 4.5v11a2.5 2.5 0 0 1-2.5 2.5h-4.75v2h2.75a1 1 0 1 1 0 2h-9a1 1 0 1 1 0-2h2.75v-2H5.5A2.5 2.5 0 0 1 3 15.5v-11ZM5.5 4a.5.5 0 0 0-.5.5v11a.5.5 0 0 0 .5.5h13a.5.5 0 0 0 .5-.5v-11a.5.5 0 0 0-.5-.5h-13ZM11 6h2l3.2 8h-2.1l-.8-2h-2.6l-.8 2H7.8L11 6Zm1 2.75-.62 1.55h1.24L12 8.75Z"/></svg>"##;

icon!(ICON_MONITOR, r#"<path fill-rule="evenodd" d="M2.5 2A1.5 1.5 0 0 0 1 3.5v7A1.5 1.5 0 0 0 2.5 12h4.75v1.5H5a.75.75 0 0 0 0 1.5h6a.75.75 0 0 0 0-1.5H8.75V12h4.75A1.5 1.5 0 0 0 15 10.5v-7A1.5 1.5 0 0 0 13.5 2h-11ZM2.5 3.5h11v7h-11v-7Z"/>"#);
icon!(ICON_SUN, r#"<path d="M8 1a.75.75 0 0 1 .75.75v1.5a.75.75 0 0 1-1.5 0v-1.5A.75.75 0 0 1 8 1Zm0 10.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7Zm0-1.5a2 2 0 1 1 0-4 2 2 0 0 1 0 4Zm0 2.75a.75.75 0 0 1 .75.75v1.5a.75.75 0 0 1-1.5 0v-1.5a.75.75 0 0 1 .75-.75ZM3.25 8a.75.75 0 0 1-.75.75H1a.75.75 0 0 1 0-1.5h1.5a.75.75 0 0 1 .75.75Zm12 0a.75.75 0 0 1-.75.75H13a.75.75 0 0 1 0-1.5h1.5a.75.75 0 0 1 .75.75ZM3.05 3.05a.75.75 0 0 1 1.06 0l1.06 1.06a.75.75 0 0 1-1.06 1.06L3.05 4.11a.75.75 0 0 1 0-1.06Zm7.78 7.78a.75.75 0 0 1 1.06 0l1.06 1.06a.75.75 0 1 1-1.06 1.06l-1.06-1.06a.75.75 0 0 1 0-1.06Zm1.06-7.78a.75.75 0 0 1 0 1.06l-1.06 1.06a.75.75 0 0 1-1.06-1.06l1.06-1.06a.75.75 0 0 1 1.06 0ZM5.17 10.83a.75.75 0 0 1 0 1.06l-1.06 1.06a.75.75 0 0 1-1.06-1.06l1.06-1.06a.75.75 0 0 1 1.06 0Z"/>"#);
icon!(ICON_DROP, r#"<path fill-rule="evenodd" d="M8 1.5a.73.73 0 0 0-.55.27C6.3 3.1 3.5 6.6 3.5 9.5a4.5 4.5 0 0 0 9 0c0-2.9-2.8-6.4-3.95-7.73A.73.73 0 0 0 8 1.5Zm0 1.95c1.25 1.53 3 4.1 3 6.05a3 3 0 0 1-6 0c0-1.95 1.75-4.52 3-6.05Z"/>"#);
icon!(ICON_CROSSHAIR, r#"<path fill-rule="evenodd" d="M8 1.75a6.25 6.25 0 1 0 0 12.5 6.25 6.25 0 0 0 0-12.5Zm0 1.5a4.75 4.75 0 1 1 0 9.5 4.75 4.75 0 0 1 0-9.5Z"/><path d="M8 6.5a1.5 1.5 0 1 0 0 3 1.5 1.5 0 0 0 0-3ZM7.25 0h1.5v3.5h-1.5zM7.25 12.5h1.5V16h-1.5zM0 7.25h3.5v1.5H0zM12.5 7.25H16v1.5h-3.5z"/>"#);
icon!(ICON_SHIELD, r#"<path fill-rule="evenodd" d="M8 1.25a.75.75 0 0 1 .3.06l5.25 2.25A.75.75 0 0 1 14 4.25v3.9c0 3.55-2.34 5.88-5.73 6.82a.75.75 0 0 1-.54 0C4.34 14.03 2 11.7 2 8.15v-3.9a.75.75 0 0 1 .45-.69L7.7 1.31a.75.75 0 0 1 .3-.06ZM3.5 4.75v3.4c0 2.75 1.74 4.58 4.5 5.42 2.76-.84 4.5-2.67 4.5-5.42v-3.4L8 2.82 3.5 4.75Z"/>"#);
icon!(ICON_PIP, r#"<path fill-rule="evenodd" d="M2.5 2.5A1.5 1.5 0 0 0 1 4v8a1.5 1.5 0 0 0 1.5 1.5h11A1.5 1.5 0 0 0 15 12V4a1.5 1.5 0 0 0-1.5-1.5h-11ZM2.5 4h11v8h-11V4Zm5.75 3.5A.75.75 0 0 0 7.5 8.25v2.5c0 .41.34.75.75.75h4a.75.75 0 0 0 .75-.75v-2.5a.75.75 0 0 0-.75-.75h-4Z"/>"#);
icon!(ICON_LIGHTBAR, r#"<path d="M2.5 2.5A1.5 1.5 0 0 1 4 1h8a1.5 1.5 0 0 1 1.5 1.5v2A1.5 1.5 0 0 1 12 6H4a1.5 1.5 0 0 1-1.5-1.5v-2ZM4 2.5v2h8v-2H4Zm4 5.75a.75.75 0 0 1 .75.75v4.5a.75.75 0 0 1-1.5 0V9a.75.75 0 0 1 .75-.75Zm-3.18.42a.75.75 0 0 1 .43.97l-1.5 4a.75.75 0 1 1-1.4-.54l1.5-4a.75.75 0 0 1 .97-.43Zm6.36 0a.75.75 0 0 1 .97.43l1.5 4a.75.75 0 1 1-1.4.54l-1.5-4a.75.75 0 0 1 .43-.97Z"/>"#);
icon!(ICON_LAYERS, r#"<path d="M7.66 1.33a.75.75 0 0 1 .68 0l6 3a.75.75 0 0 1 0 1.34l-6 3a.75.75 0 0 1-.68 0l-6-3a.75.75 0 0 1 0-1.34l6-3ZM3.42 5 8 7.29 12.58 5 8 2.71 3.42 5Zm-1.76 3.16a.75.75 0 0 1 1.01-.33L8 10.49l5.33-2.66a.75.75 0 1 1 .68 1.34l-5.67 2.83a.75.75 0 0 1-.68 0L2 9.17a.75.75 0 0 1-.34-1.01Zm0 3a.75.75 0 0 1 1.01-.33L8 13.49l5.33-2.66a.75.75 0 1 1 .68 1.34l-5.67 2.83a.75.75 0 0 1-.68 0L2 12.17a.75.75 0 0 1-.34-1.01Z"/>"#);
icon!(ICON_SLIDERS, r#"<path d="M1.75 3.25a.75.75 0 0 0 0 1.5h2.6a2 2 0 0 0 3.9 0h6a.75.75 0 0 0 0-1.5h-6a2 2 0 0 0-3.9 0h-2.6Zm8.6 4a2 2 0 0 0-1.95 1.25H1.75a.75.75 0 0 0 0 1.5H8.4a2 2 0 0 0 3.9 0h1.95a.75.75 0 0 0 0-1.5H12.3a2 2 0 0 0-1.95-1.25Zm-4.6 4.5a2 2 0 0 0-1.95 1.25H1.75a.75.75 0 0 0 0 1.5H3.8a2 2 0 0 0 3.9 0h6.55a.75.75 0 0 0 0-1.5H7.7a2 2 0 0 0-1.95-1.25Z"/>"#);
icon!(ICON_REFRESH, r#"<path d="M8 2.5a5.5 5.5 0 1 0 5.16 7.42.75.75 0 0 1 1.4.53A7 7 0 1 1 12.2 2.4V1.75a.75.75 0 0 1 1.5 0v2.75a.75.75 0 0 1-.75.75H10.2a.75.75 0 0 1 0-1.5h1.02A5.47 5.47 0 0 0 8 2.5Z"/>"#);
icon!(ICON_CHEVRON, r#"<path d="M3.47 5.97a.75.75 0 0 1 1.06 0L8 9.44l3.47-3.47a.75.75 0 1 1 1.06 1.06l-4 4a.75.75 0 0 1-1.06 0l-4-4a.75.75 0 0 1 0-1.06Z"/>"#);
icon!(ICON_WIN_MIN, r#"<path d="M3 7.5h10v1H3z"/>"#);
icon!(ICON_WIN_MAX, r#"<path fill-rule="evenodd" d="M3 3h10v10H3V3Zm1 1v8h8V4H4Z"/>"#);
icon!(ICON_WIN_RESTORE, r#"<path fill-rule="evenodd" d="M5 3h8v8h-2v-1h1V4H6v1H5V3ZM3 5h8v8H3V5Zm1 1v6h6V6H4Z"/>"#);
icon!(ICON_WIN_CLOSE, r#"<path d="M3.35 2.65 8 7.29l4.65-4.64.7.7L8.71 8l4.64 4.65-.7.7L8 8.71l-4.65 4.64-.7-.7L7.29 8 2.65 3.35l.7-.7Z"/>"#);

pub fn icon(data: &'static [u8], size: f32, color: Rgba) -> gpui::Svg {
    use gpui::Styled;
    svg().data(data).size(px(size)).flex_shrink_0().text_color(color)
}

/// The app icon (also the README logo), scaled to `size` x `size`.
pub fn app_icon(size: u32) -> image::RgbaImage {
    let icon = image::load_from_memory(include_bytes!("../assets/icon.png")).expect("bundled icon is a PNG");
    image::imageops::resize(&icon.into_rgba8(), size, size, image::imageops::FilterType::Lanczos3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_icon_decodes() {
        let icon = app_icon(32);
        assert_eq!(icon.dimensions(), (32, 32));
        assert_eq!(icon.get_pixel(0, 0)[3], 0, "corner is transparent");
        assert_eq!(icon.get_pixel(16, 16)[3], 255, "center is opaque");
    }
}
