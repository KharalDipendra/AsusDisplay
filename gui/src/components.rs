use gpui::{
    AnyElement, App, Div, ElementId, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};

use crate::theme::Theme;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    Primary,
    Secondary,
    Ghost,
    Danger,
}

/// Compact pill badge with optional 1px border.
pub fn badge(
    text: impl Into<SharedString>,
    text_color: gpui::Rgba,
    bg_color: gpui::Rgba,
    border_color: gpui::Rgba,
) -> Div {
    div()
        .px(px(7.0))
        .py(px(2.0))
        .rounded(px(4.0))
        .bg(bg_color)
        .border_1()
        .border_color(border_color)
        .text_size(px(11.0))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(text_color)
        .child(text.into())
}

/// Monospace VCP hex badge e.g. `0x10`.
pub fn code_badge(code: u8) -> Div {
    div()
        .px(px(6.0))
        .py(px(2.0))
        .rounded(px(4.0))
        .bg(Theme::bg_elevated())
        .border_1()
        .border_color(Theme::border())
        .text_size(px(10.5))
        .text_color(Theme::text_muted())
        .child(format!("0x{code:02X}"))
}

/// Standard interactive button with hover & disabled states.
pub fn action_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    variant: ButtonVariant,
    disabled: bool,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let (bg, hover_bg, border, text_col) = match variant {
        ButtonVariant::Primary => (
            Theme::accent(),
            Theme::accent_hover(),
            Theme::accent(),
            Theme::text_primary(),
        ),
        ButtonVariant::Secondary => (
            Theme::bg_elevated(),
            Theme::bg_hover(),
            Theme::border(),
            Theme::text_primary(),
        ),
        ButtonVariant::Ghost => (
            Theme::bg_surface(),
            Theme::bg_elevated(),
            Theme::border(),
            Theme::text_secondary(),
        ),
        ButtonVariant::Danger => (
            Theme::red_subtle(),
            Theme::red_hover(),
            Theme::red_border(),
            Theme::text_primary(),
        ),
    };

    let mut btn = div()
        .id(id)
        .px(px(10.0))
        .py(px(5.0))
        .rounded(px(6.0))
        .border_1()
        .border_color(border)
        .bg(bg)
        .text_size(px(12.0))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(if disabled { Theme::text_muted() } else { text_col })
        .flex()
        .items_center()
        .justify_center()
        .child(label.into());

    if disabled {
        btn = btn.opacity(0.45);
    } else {
        btn = btn
            .cursor_pointer()
            .hover(move |s| s.bg(hover_bg).border_color(Theme::border_active()))
            .on_click(move |_, window, cx| on_click(window, cx));
    }
    btn
}

/// Compact step button used alongside precision sliders (`-5`, `-1`, `+1`, `+5`).
pub fn step_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    disabled: bool,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let mut el = div()
        .id(id)
        .w(px(28.0))
        .h(px(24.0))
        .rounded(px(5.0))
        .bg(Theme::bg_elevated())
        .border_1()
        .border_color(Theme::border())
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(11.0))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(if disabled {
            Theme::text_muted()
        } else {
            Theme::text_secondary()
        })
        .child(label.into());

    if disabled {
        el = el.opacity(0.45);
    } else {
        el = el
            .cursor_pointer()
            .hover(|s| {
                s.bg(Theme::bg_hover())
                    .border_color(Theme::border_active())
                    .text_color(Theme::text_primary())
            })
            .on_click(move |_, window, cx| on_click(window, cx));
    }
    el
}

/// Hardware-style pill toggle switch (38x20px).
pub fn toggle_switch(
    id: impl Into<ElementId>,
    on: bool,
    disabled: bool,
    on_toggle: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let track_bg = if on {
        Theme::accent()
    } else {
        Theme::bg_elevated()
    };
    let track_border = if on {
        Theme::accent()
    } else {
        Theme::border_active()
    };

    let knob = div()
        .w(px(14.0))
        .h(px(14.0))
        .rounded(px(99.0))
        .bg(if on {
            Theme::text_primary()
        } else {
            Theme::text_secondary()
        });

    let mut track = div()
        .id(id)
        .w(px(38.0))
        .h(px(20.0))
        .px(px(2.0))
        .rounded(px(99.0))
        .bg(track_bg)
        .border_1()
        .border_color(track_border)
        .flex()
        .items_center()
        .justify_between();

    if on {
        track = track.child(div().w(px(1.0))).child(knob);
    } else {
        track = track.child(knob).child(div().w(px(1.0)));
    }

    if disabled {
        track = track.opacity(0.45);
    } else {
        track = track
            .cursor_pointer()
            .hover(|s| s.border_color(Theme::text_secondary()))
            .on_click(move |_, window, cx| on_toggle(window, cx));
    }

    track
}

/// Interactive segmented precision slider with `-5` / `-1` / `+1` / `+5` step buttons
/// and a 21-segment clickable track (`0%` to `100%` in 5% increments).
pub fn precision_slider<F>(
    id_prefix: impl Into<SharedString>,
    current: u16,
    max: u16,
    disabled: bool,
    accent_color: gpui::Rgba,
    readout_override: Option<String>,
    on_change: F,
) -> impl IntoElement
where
    F: Fn(u16, &mut Window, &mut App) + Clone + 'static,
{
    let prefix = id_prefix.into();
    let effective_max = max.max(1);
    let clamped = current.min(effective_max);
    let filled_segments = ((u32::from(clamped) * 20 + u32::from(effective_max) / 2)
        / u32::from(effective_max)) as usize;

    let dec5 = on_change.clone();
    let dec1 = on_change.clone();
    let inc1 = on_change.clone();
    let inc5 = on_change.clone();

    let mut track = div()
        .flex()
        .items_center()
        .gap(px(2.0))
        .h(px(22.0))
        .px(px(4.0))
        .rounded(px(6.0))
        .bg(Theme::bg_input())
        .border_1()
        .border_color(Theme::border())
        .flex_1()
        .min_w(px(140.0));

    for seg in 0..=20usize {
        let seg_val = ((seg as u32 * u32::from(effective_max) + 10) / 20) as u16;
        let active = !disabled && seg <= filled_segments && (clamped > 0 || seg == 0);
        let seg_cb = on_change.clone();
        let seg_id = SharedString::from(format!("{prefix}-seg-{seg}"));

        let mut bar = div()
            .id(seg_id)
            .flex_1()
            .h(px(10.0))
            .rounded(px(2.0))
            .bg(if active {
                accent_color
            } else {
                Theme::bg_elevated()
            });

        if !disabled {
            bar = bar
                .cursor_pointer()
                .hover(move |s| s.bg(Theme::text_secondary()))
                .on_click(move |_, window, cx| {
                    seg_cb(seg_val, window, cx);
                });
        }
        track = track.child(bar);
    }

    let readout = readout_override.unwrap_or_else(|| {
        if disabled {
            "n/a".to_string()
        } else {
            format!("{clamped} / {effective_max}")
        }
    });

    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(step_button(
            SharedString::from(format!("{prefix}-dec5")),
            "-5",
            disabled,
            move |w, cx| dec5(clamped.saturating_sub(5), w, cx),
        ))
        .child(step_button(
            SharedString::from(format!("{prefix}-dec1")),
            "-1",
            disabled,
            move |w, cx| dec1(clamped.saturating_sub(1), w, cx),
        ))
        .child(track)
        .child(step_button(
            SharedString::from(format!("{prefix}-inc1")),
            "+1",
            disabled,
            move |w, cx| inc1(clamped.saturating_add(1).min(effective_max), w, cx),
        ))
        .child(step_button(
            SharedString::from(format!("{prefix}-inc5")),
            "+5",
            disabled,
            move |w, cx| inc5(clamped.saturating_add(5).min(effective_max), w, cx),
        ))
        .child(
            div()
                .min_w(px(68.0))
                .px(px(8.0))
                .py(px(3.0))
                .rounded(px(5.0))
                .bg(Theme::bg_elevated())
                .border_1()
                .border_color(Theme::border())
                .text_size(px(11.5))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(if disabled {
                    Theme::text_muted()
                } else {
                    Theme::text_primary()
                })
                .flex()
                .justify_center()
                .child(readout),
        )
}

/// Horizontal wrap group of selectable pills for enum/preset values.
pub fn pill_selector<F>(
    id_prefix: impl Into<SharedString>,
    options: &[(u16, String)],
    current: u16,
    disabled: bool,
    on_select: F,
) -> Div
where
    F: Fn(u16, &mut Window, &mut App) + Clone + 'static,
{
    let prefix = id_prefix.into();
    let mut wrap = div().flex().flex_wrap().gap(px(5.0));

    for (val, label) in options {
        let val = *val;
        let active = !disabled && val == current;
        let cb = on_select.clone();
        let pill_id = SharedString::from(format!("{prefix}-opt-{val}"));

        let mut pill = div()
            .id(pill_id)
            .px(px(9.0))
            .py(px(4.0))
            .rounded(px(5.0))
            .border_1()
            .border_color(if active {
                Theme::accent_border()
            } else {
                Theme::border()
            })
            .bg(if active {
                Theme::accent_subtle()
            } else {
                Theme::bg_elevated()
            })
            .text_size(px(11.5))
            .font_weight(if active {
                gpui::FontWeight::SEMIBOLD
            } else {
                gpui::FontWeight::NORMAL
            })
            .text_color(if disabled {
                Theme::text_muted()
            } else if active {
                Theme::text_primary()
            } else {
                Theme::text_secondary()
            })
            .child(label.clone());

        if disabled {
            pill = pill.opacity(0.45);
        } else {
            pill = pill
                .cursor_pointer()
                .hover(move |s| {
                    if active {
                        s.bg(Theme::accent_subtle())
                            .border_color(Theme::accent())
                    } else {
                        s.bg(Theme::bg_hover())
                            .border_color(Theme::border_active())
                            .text_color(Theme::text_primary())
                    }
                })
                .on_click(move |_, window, cx| cb(val, window, cx));
        }

        wrap = wrap.child(pill);
    }

    wrap
}

/// Zinc-900 setting group card with hairline border and optional header right slot.
pub fn section_card(
    title: impl Into<SharedString>,
    subtitle: Option<SharedString>,
    right_slot: Option<AnyElement>,
    rows: Vec<AnyElement>,
) -> Div {
    let mut header_left = div()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(
            div()
                .text_size(px(13.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(Theme::text_primary())
                .child(title.into()),
        );

    if let Some(sub) = subtitle {
        header_left = header_left.child(
            div()
                .text_size(px(11.5))
                .text_color(Theme::text_secondary())
                .child(sub),
        );
    }

    let mut header = div()
        .px(px(14.0))
        .py(px(10.0))
        .border_b_1()
        .border_color(Theme::border())
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .child(header_left);

    if let Some(slot) = right_slot {
        header = header.child(slot);
    }

    let mut body = div().flex().flex_col();
    let count = rows.len();
    for (idx, row) in rows.into_iter().enumerate() {
        let mut wrapper = div().px(px(14.0)).py(px(10.0)).child(row);
        if idx + 1 < count {
            wrapper = wrapper.border_b_1().border_color(Theme::border());
        }
        body = body.child(wrapper);
    }

    div()
        .rounded(px(8.0))
        .bg(Theme::bg_surface())
        .border_1()
        .border_color(Theme::border())
        .flex()
        .flex_col()
        .child(header)
        .child(body)
}

/// Reusable focusable single-line text input backed by GPUI `FocusHandle` + `on_key_down`.
#[derive(Clone)]
pub struct TextInputState {
    pub value: String,
    pub placeholder: String,
    pub focus_handle: FocusHandle,
}

impl TextInputState {
    pub fn new(placeholder: impl Into<String>, cx: &App) -> Self {
        Self {
            value: String::new(),
            placeholder: placeholder.into(),
            focus_handle: cx.focus_handle(),
        }
    }

    /// Processes a keystroke; returns `true` if `Enter` was pressed.
    pub fn handle_key_down(&mut self, event: &KeyDownEvent) -> bool {
        let ks = &event.keystroke;
        match ks.key.as_str() {
            "enter" => return true,
            "backspace" => {
                self.value.pop();
            }
            "escape" => {
                self.value.clear();
            }
            _ => {
                if !ks.modifiers.control && !ks.modifiers.platform && !ks.modifiers.alt {
                    if let Some(ch) = &ks.key_char {
                        if !ch.chars().all(|c| c.is_control()) {
                            self.value.push_str(ch);
                        }
                    } else if ks.key == "space" {
                        self.value.push(' ');
                    }
                }
            }
        }
        false
    }
}

pub type ClearCallback = Box<dyn Fn(&mut Window, &mut App) + 'static>;

/// Renders a single-line interactive text input box.
pub fn text_input_box(
    id: impl Into<SharedString>,
    state: &TextInputState,
    window: &Window,
    on_key: impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static,
    on_clear: Option<ClearCallback>,
) -> impl IntoElement {
    let id_str = id.into();
    let clear_id = SharedString::from(format!("{id_str}-clear"));
    let focused = state.focus_handle.is_focused(window);
    let focus_handle = state.focus_handle.clone();
    let is_empty = state.value.is_empty();
    let display_text = if is_empty {
        if focused {
            "|".to_string()
        } else {
            state.placeholder.clone()
        }
    } else if focused {
        format!("{}|", state.value)
    } else {
        state.value.clone()
    };

    let mut box_el = div()
        .id(id_str)
        .track_focus(&state.focus_handle)
        .h(px(30.0))
        .px(px(10.0))
        .rounded(px(6.0))
        .bg(Theme::bg_input())
        .border_1()
        .border_color(if focused {
            Theme::accent()
        } else {
            Theme::border()
        })
        .flex()
        .items_center()
        .justify_between()
        .gap(px(6.0))
        .cursor_text()
        .on_click(move |_, window, cx| {
            window.focus(&focus_handle, cx);
        })
        .on_key_down(move |ev, window, cx| {
            on_key(ev, window, cx);
        })
        .child(
            div()
                .flex_1()
                .overflow_hidden()
                .text_size(px(12.0))
                .text_color(if is_empty && !focused {
                    Theme::text_muted()
                } else {
                    Theme::text_primary()
                })
                .child(display_text),
        );

    if !is_empty
        && let Some(clear_cb) = on_clear
    {
        box_el = box_el.child(
            div()
                .id(clear_id)
                .px(px(4.0))
                .rounded(px(4.0))
                .text_size(px(11.0))
                .text_color(Theme::text_muted())
                .cursor_pointer()
                .hover(|s| s.bg(Theme::bg_elevated()).text_color(Theme::text_primary()))
                .on_click(move |_, window, cx| clear_cb(window, cx))
                .child("×"),
        );
    }

    box_el
}
