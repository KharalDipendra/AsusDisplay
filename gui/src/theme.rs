use gpui::{Rgba, rgb, rgba};

/// Zed / shadcn Zinc neutral dark hardware utility palette.
pub struct Theme;

impl Theme {
    // Core surfaces
    pub fn bg_window() -> Rgba {
        rgb(0x09090b) // Zinc 950
    }

    pub fn bg_sidebar() -> Rgba {
        rgb(0x121215)
    }

    pub fn bg_header() -> Rgba {
        rgb(0x121215)
    }

    pub fn bg_surface() -> Rgba {
        rgb(0x18181b) // Zinc 900
    }

    pub fn bg_elevated() -> Rgba {
        rgb(0x27272a) // Zinc 800
    }

    pub fn bg_hover() -> Rgba {
        rgb(0x3f3f46) // Zinc 700
    }

    pub fn bg_input() -> Rgba {
        rgb(0x0f0f12)
    }

    // Borders
    pub fn border() -> Rgba {
        rgb(0x27272a)
    }

    pub fn border_active() -> Rgba {
        rgb(0x52525b)
    }

    // Text
    pub fn text_primary() -> Rgba {
        rgb(0xf4f4f5) // Zinc 100
    }

    pub fn text_secondary() -> Rgba {
        rgb(0xa1a1aa) // Zinc 400
    }

    pub fn text_muted() -> Rgba {
        rgb(0x71717a) // Zinc 500
    }

    // Primary studio blue accent
    pub fn accent() -> Rgba {
        rgb(0x3b82f6)
    }

    pub fn accent_hover() -> Rgba {
        rgb(0x2563eb)
    }

    pub fn accent_subtle() -> Rgba {
        rgba(0x3b82f624)
    }

    pub fn accent_border() -> Rgba {
        rgba(0x3b82f666)
    }

    // Semantic status colors
    pub fn emerald() -> Rgba {
        rgb(0x10b981)
    }

    pub fn emerald_subtle() -> Rgba {
        rgba(0x10b98122)
    }

    pub fn emerald_border() -> Rgba {
        rgba(0x10b98155)
    }

    pub fn amber() -> Rgba {
        rgb(0xf59e0b)
    }

    pub fn amber_subtle() -> Rgba {
        rgba(0xf59e0b22)
    }

    pub fn amber_border() -> Rgba {
        rgba(0xf59e0b55)
    }

    pub fn red() -> Rgba {
        rgb(0xef4444)
    }

    pub fn red_hover() -> Rgba {
        rgb(0xdc2626)
    }

    pub fn red_subtle() -> Rgba {
        rgba(0xef444422)
    }

    pub fn red_border() -> Rgba {
        rgba(0xef444455)
    }

    // Modal overlay
    pub fn modal_backdrop() -> Rgba {
        rgba(0x000000aa)
    }
}
