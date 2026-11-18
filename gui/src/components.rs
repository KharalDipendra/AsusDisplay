use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, DispatchPhase, Div, ElementId, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent,
    MouseButton, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, SharedString, StatefulInteractiveElement, Styled,
    Window, anchored, canvas, deferred, div, prelude::FluentBuilder, px, relative,
};

use crate::assets::{ICON_CHEVRON, icon};
use crate::theme;

pub type Callback<T> = Rc<dyn Fn(T, &mut Window, &mut App)>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Normal,
    Primary,
    Danger,
}

pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    variant: Variant,
    disabled: bool,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let (bg, fg, border) = match variant {
        Variant::Normal => (theme::overlay(), theme::text(), theme::border()),
        Variant::Primary => (theme::accent_soft(), theme::accent(), theme::accent()),
        Variant::Danger => (theme::love_soft(), theme::love(), theme::love()),
    };
    div()
        .id(id)
        .flex_shrink_0()
        .px(px(14.))
        .py(px(6.))
        .rounded(px(6.))
        .bg(bg)
        .border_1()
        .border_color(border)
        .text_color(fg)
        .text_size(px(13.))
        .child(label.into())
        .when(disabled, |d| d.opacity(0.4))
        .when(!disabled, |d| {
            d.cursor_pointer().hover(|s| s.bg(theme::hover())).on_click(move |_, w, cx| on_click(w, cx))
        })
}

/// Borderless icon + label button for the title bar.
pub fn toolbar_button(
    id: impl Into<ElementId>,
    data: &'static [u8],
    label: impl Into<SharedString>,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .h(px(28.))
        .px(px(10.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .gap(px(6.))
        .text_size(px(13.))
        .text_color(theme::subtle())
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
        .on_click(move |_, w, cx| on_click(w, cx))
        .child(icon(data, 14., theme::subtle()))
        .child(label.into())
}

pub fn switch(
    id: impl Into<ElementId>,
    on: bool,
    disabled: bool,
    on_toggle: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .flex_shrink_0()
        .w(px(40.))
        .h(px(22.))
        .p(px(3.))
        .rounded_full()
        .flex()
        .items_center()
        .when(on, |d| d.justify_end().bg(theme::accent()))
        .when(!on, |d| d.bg(theme::border()))
        .child(div().size(px(16.)).rounded_full().bg(if on { theme::base() } else { theme::subtle() }))
        .when(disabled, |d| d.opacity(0.4))
        .when(!disabled, |d| d.cursor_pointer().on_click(move |_, w, cx| on_toggle(w, cx)))
}

thread_local! {
    /// The slider being dragged; the mouse can leave its track while the button is held.
    static DRAGGING: RefCell<Option<SharedString>> = const { RefCell::new(None) };
}

const THUMB: f32 = 18.;

pub fn slider(id: impl Into<SharedString>, value: u16, max: u16, disabled: bool, on_change: Callback<u16>) -> impl IntoElement {
    let id: SharedString = id.into();
    let max = max.max(1);
    let value = value.min(max);
    // A locked control reports a placeholder (often 254); show it empty instead.
    let ratio = if disabled { 0. } else { f32::from(value) / f32::from(max) };
    let readout = if disabled { "–".to_string() } else { value.to_string() };
    let bounds: Rc<Cell<Option<Bounds<Pixels>>>> = Rc::new(Cell::new(None));

    let pick = move |x: Pixels, b: Bounds<Pixels>| -> u16 {
        let width = f32::from(b.size.width) - THUMB;
        let r = ((f32::from(x - b.origin.x) - THUMB / 2.) / width.max(1.)).clamp(0., 1.);
        (r * f32::from(max)).round() as u16
    };

    // Window-wide listeners, registered each paint, keep the drag going outside the track.
    let watch = {
        let (bounds, id, on_change) = (bounds.clone(), id.clone(), on_change.clone());
        canvas(
            move |b, _, _| bounds.set(Some(b)),
            move |b, _, window, _| {
                if disabled {
                    return;
                }
                let (move_id, up_id, on_change) = (id.clone(), id.clone(), on_change.clone());
                window.on_mouse_event(move |ev: &MouseMoveEvent, phase, window, cx| {
                    let active = DRAGGING.with(|d| d.borrow().as_ref() == Some(&move_id));
                    if phase == DispatchPhase::Bubble && active && ev.dragging() {
                        let next = pick(ev.position.x, b);
                        if next != value {
                            on_change(next, window, cx);
                        }
                    }
                });
                window.on_mouse_event(move |_: &MouseUpEvent, phase, _, _| {
                    if phase == DispatchPhase::Bubble {
                        DRAGGING.with(|d| {
                            let mut d = d.borrow_mut();
                            if d.as_ref() == Some(&up_id) {
                                *d = None;
                            }
                        });
                    }
                });
            },
        )
        .absolute()
        .size_full()
    };

    let half = px(THUMB / 2.);
    let track = div()
        .id(id.clone())
        .relative()
        .h(px(THUMB))
        .flex_1()
        .min_w(px(180.))
        .child(watch)
        .child(
            div()
                .absolute()
                .top(px(THUMB / 2. - 2.))
                .left(half)
                .right(half)
                .h(px(4.))
                .rounded_full()
                .bg(theme::border())
                .child(div().h_full().w(relative(ratio)).rounded_full().bg(theme::accent())),
        )
        .child(
            div().absolute().top_0().bottom_0().left_0().right(px(THUMB)).child(
                div()
                    .absolute()
                    .left(relative(ratio))
                    .size(px(THUMB))
                    .rounded_full()
                    .bg(theme::text())
                    .border_2()
                    .border_color(theme::accent()),
            ),
        )
        .when(disabled, |d| d.opacity(0.4))
        .when(!disabled, |d| {
            d.cursor_pointer().on_mouse_down(MouseButton::Left, move |ev, window, cx| {
                DRAGGING.with(|d| *d.borrow_mut() = Some(id.clone()));
                if let Some(b) = bounds.get() {
                    let next = pick(ev.position.x, b);
                    if next != value {
                        on_change(next, window, cx);
                    }
                }
            })
        });

    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .w(px(300.))
        .child(track)
        .child(div().w(px(32.)).flex().justify_end().text_size(px(13.)).text_color(theme::subtle()).child(readout))
}

/// A row of options with one selected. `options` are `(value, label)`.
pub fn segmented(
    id: &str,
    options: &[(String, String)],
    selected: Option<&str>,
    disabled: bool,
    on_select: Callback<String>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_shrink_0()
        .gap(px(2.))
        .p(px(2.))
        .rounded(px(8.))
        .bg(theme::base())
        .border_1()
        .border_color(theme::border())
        .when(disabled, |d| d.opacity(0.4))
        .children(options.iter().map(|(value, label)| {
            let active = selected == Some(value.as_str());
            let (value, on_select) = (value.clone(), on_select.clone());
            div()
                .id(SharedString::from(format!("{id}-{value}")))
                .px(px(12.))
                .py(px(4.))
                .rounded(px(6.))
                .text_size(px(13.))
                .text_color(if active { theme::text() } else { theme::subtle() })
                .when(active, |d| d.bg(theme::overlay()))
                .child(label.clone())
                .when(!disabled, |d| {
                    d.cursor_pointer()
                        .hover(|s| s.text_color(theme::text()))
                        .on_click(move |_, w, cx| on_select(value.clone(), w, cx))
                })
        }))
}

/// A button showing the current option that opens a list. The caller owns the open state.
#[allow(clippy::too_many_arguments)]
pub fn dropdown(
    id: &str,
    options: &[(String, String)],
    selected: Option<&str>,
    open: bool,
    disabled: bool,
    on_open: Callback<bool>,
    on_select: Callback<String>,
) -> impl IntoElement {
    let current = selected
        .and_then(|s| options.iter().find(|(v, _)| v == s))
        .map_or_else(|| "Custom".to_string(), |(_, l)| l.clone());
    let toggle = on_open.clone();

    let trigger = div()
        .id(SharedString::from(format!("{id}-trigger")))
        .min_w(px(200.))
        .px(px(12.))
        .py(px(6.))
        .rounded(px(6.))
        .bg(theme::overlay())
        .border_1()
        .border_color(if open { theme::accent() } else { theme::border() })
        .flex()
        .items_center()
        .justify_between()
        .gap(px(8.))
        .text_size(px(13.))
        .child(current)
        .child(icon(ICON_CHEVRON, 14., theme::subtle()))
        .when(disabled, |d| d.opacity(0.4))
        // `open` is the value at render time, so a click while open closes even if the menu's
        // mouse-down-outside already did.
        .when(!disabled, |d| d.cursor_pointer().hover(|s| s.bg(theme::hover())).on_click(move |_, w, cx| toggle(!open, w, cx)));

    let menu = open.then(|| {
        let close = on_open.clone();
        deferred(
            anchored().snap_to_window_with_margin(px(8.)).child(
                div()
                    .id(SharedString::from(format!("{id}-menu")))
                    .occlude()
                    .mt(px(4.))
                    .min_w(px(200.))
                    .max_h(px(320.))
                    .overflow_y_scroll()
                    .p(px(4.))
                    .rounded(px(8.))
                    .bg(theme::surface())
                    .border_1()
                    .border_color(theme::border())
                    .shadow_lg()
                    .on_mouse_down_out(move |_, w, cx| close(false, w, cx))
                    .children(options.iter().map(|(value, label)| {
                        let active = selected == Some(value.as_str());
                        let (value, on_select, on_open) = (value.clone(), on_select.clone(), on_open.clone());
                        div()
                            .id(SharedString::from(format!("{id}-opt-{value}")))
                            .px(px(10.))
                            .py(px(6.))
                            .rounded(px(5.))
                            .text_size(px(13.))
                            .text_color(if active { theme::accent() } else { theme::text() })
                            .when(active, |d| d.bg(theme::accent_soft()))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::hover()))
                            .on_click(move |_, w, cx| {
                                on_open(false, w, cx);
                                on_select(value.clone(), w, cx);
                            })
                            .child(label.clone())
                    })),
            ),
        )
        .priority(1)
    });

    // `anchored` is absolutely positioned at its parent's origin, so give it a parent below the trigger.
    div().flex_shrink_0().flex().flex_col().child(trigger).child(div().children(menu))
}

/// One setting: label and optional hint on the left, the control on the right.
pub fn row(label: impl Into<SharedString>, hint: Option<(String, gpui::Rgba)>, control: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(24.))
        .px(px(16.))
        .py(px(12.))
        .child(
            div()
                .flex_1()
                .min_w(px(120.))
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().text_size(px(14.)).text_color(theme::text()).child(label.into()))
                .children(hint.map(|(text, color)| div().text_size(px(12.5)).text_color(color).child(text))),
        )
        .child(control)
}

/// A group of rows with dividers between them, under an optional title.
pub fn card(title: &str, rows: Vec<AnyElement>) -> Div {
    let count = rows.len();
    let title = (!title.is_empty()).then(|| {
        div().px(px(4.)).text_size(px(13.)).font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme::subtle()).child(title.to_string())
    });
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .children(title)
        .child(
            div()
                .flex()
                .flex_col()
                .rounded(px(10.))
                .bg(theme::surface())
                .border_1()
                .border_color(theme::border())
                .children(rows.into_iter().enumerate().map(|(i, r)| {
                    div().when(i + 1 < count, |d| d.border_b_1().border_color(theme::hover())).child(r)
                })),
        )
}

/// Single-line text field: type, Backspace, Ctrl+V, Enter submits.
pub struct TextInput {
    pub value: String,
    pub placeholder: &'static str,
    pub focus: FocusHandle,
}

impl TextInput {
    pub fn new(placeholder: &'static str, cx: &App) -> Self {
        Self { value: String::new(), placeholder, focus: cx.focus_handle() }
    }

    /// Applies a key press; returns true on Enter.
    pub fn key(&mut self, ev: &KeyDownEvent, cx: &App) -> bool {
        let ks = &ev.keystroke;
        let m = &ks.modifiers;
        match ks.key.as_str() {
            "enter" => return true,
            "backspace" => {
                self.value.pop();
            }
            "escape" => self.value.clear(),
            "v" if m.control || m.platform => {
                if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                    self.value.push_str(text.lines().next().unwrap_or_default());
                }
            }
            _ if !m.control && !m.platform && !m.alt => {
                if let Some(ch) = ks.key_char.as_ref().filter(|c| !c.chars().any(char::is_control)) {
                    self.value.push_str(ch);
                }
            }
            _ => {}
        }
        false
    }

    pub fn render(&self, id: &'static str, window: &Window, on_key: impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static) -> impl IntoElement {
        let focused = self.focus.is_focused(window);
        let focus = self.focus.clone();
        let empty = self.value.is_empty();
        div()
            .id(id)
            .track_focus(&self.focus)
            .flex_1()
            .min_w(px(160.))
            .px(px(10.))
            .py(px(6.))
            .rounded(px(6.))
            .bg(theme::base())
            .border_1()
            .border_color(if focused { theme::accent() } else { theme::border() })
            .text_size(px(13.))
            .cursor_text()
            .on_click(move |_, w, cx| w.focus(&focus, cx))
            .on_key_down(move |ev, w, cx| on_key(ev, w, cx))
            .flex()
            .items_center()
            .child(if empty && !focused {
                div().text_color(theme::muted()).child(self.placeholder)
            } else {
                div().text_color(theme::text()).child(self.value.clone())
            })
            .when(focused, |d| d.child(div().w(px(1.5)).h(px(16.)).bg(theme::accent())))
    }
}
