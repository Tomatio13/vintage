//! Visual primitives for the native settings surface.
use super::*;
use crate::theme::Palette;

fn action_base(
    id: impl Into<ElementId>,
    label: impl Into<gpui::SharedString>,
    p: Palette,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .min_h(gpui::rems(2.125))
        .px_3()
        .py_1()
        .rounded_md()
        .text_size(gpui::rems(0.8125))
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.color(0xe6e1d8))
        .cursor_pointer()
        .child(label.into())
}

pub fn action(
    id: impl Into<ElementId>,
    label: impl Into<gpui::SharedString>,
    p: Palette,
) -> gpui::Stateful<gpui::Div> {
    action_base(id, label, p).hover(move |s| s.bg(p.color(0x343027)))
}

pub fn primary(
    id: impl Into<ElementId>,
    label: &str,
    p: Palette,
    enabled: bool,
) -> gpui::Stateful<gpui::Div> {
    action_base(id, label.to_owned(), p)
        .bg(p.color(0xc6a66b))
        .text_color(p.color(0x191816))
        .when(!enabled, |s| {
            s.opacity(0.4).cursor(gpui::CursorStyle::Arrow)
        })
        .hover(move |s| {
            s.bg(p.color(0xc6a66b))
                .opacity(if enabled { 0.85 } else { 0.4 })
        })
}

pub fn tag(label: impl Into<gpui::SharedString>, p: Palette) -> gpui::Div {
    div()
        .flex_none()
        .rounded_full()
        .px_2()
        .py_1()
        .text_size(gpui::rems(0.6875))
        .font_weight(FontWeight::MEDIUM)
        .bg(p.color(0x343027))
        .text_color(p.color(0xbab1a1))
        .child(label.into())
}

pub fn hint(text: impl Into<gpui::SharedString>, p: Palette) -> gpui::Div {
    div()
        .text_size(gpui::rems(0.8125))
        .line_height(gpui::rems(1.25))
        .text_color(p.color(0xbab1a1))
        .child(text.into())
}

pub fn heading(title: &str, description: &str, p: Palette) -> gpui::Div {
    div()
        .mb_5()
        .child(
            div()
                .text_size(gpui::rems(1.5))
                .font_weight(FontWeight::SEMIBOLD)
                .child(title.to_owned()),
        )
        .child(hint(description.to_owned(), p).mt_1())
}

pub fn card(p: Palette) -> gpui::Div {
    div()
        .w_full()
        .min_w_0()
        .rounded_lg()
        .border_1()
        .border_color(p.color(0x39352e))
        .bg(p.color(0x201f1c))
        .p_5()
}

pub fn field(label: &str, description: &str, control: gpui::AnyElement, p: Palette) -> gpui::Div {
    div()
        .w_full()
        .min_w_0()
        .flex()
        .items_center()
        .gap_4()
        .when(p.stacked(), |row| row.flex_col().items_start())
        .child(
            div()
                .flex_1()
                .min_w_0()
                .when(p.stacked(), |s| s.flex_none().w_full())
                .child(
                    div()
                        .text_size(gpui::rems(0.875))
                        .font_weight(FontWeight::MEDIUM)
                        .child(label.to_owned()),
                )
                .when(!description.is_empty(), |s| {
                    s.child(hint(description.to_owned(), p).mt_1())
                }),
        )
        .child(
            div()
                .flex_none()
                .min_w_0()
                .max_w_full()
                .when(p.stacked(), |s| s.w_full())
                .child(control),
        )
}

pub fn choice(
    id: impl Into<ElementId>,
    label: impl Into<gpui::SharedString>,
    selected: bool,
    p: Palette,
) -> gpui::Stateful<gpui::Div> {
    action(id, label, p)
        .border_1()
        .border_color(p.color(if selected { 0xc6a66b } else { 0x39352e }))
        .bg(p.color(if selected { 0x343027 } else { 0x191816 }))
}

pub fn select(
    id: impl Into<ElementId>,
    label: impl Into<gpui::SharedString>,
    p: Palette,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .min_w_0()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .min_h(gpui::rems(2.25))
        .px_3()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(p.color(0x544936))
        .bg(p.color(0x191816))
        .text_size(gpui::rems(0.8125))
        .cursor_pointer()
        .hover(move |s| s.border_color(p.color(0xc6a66b)))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .overflow_hidden()
                .child(label.into()),
        )
        .child(div().flex_none().text_color(p.color(0xbab1a1)).child("▾"))
}

pub fn keycap(label: impl Into<gpui::SharedString>, p: Palette) -> gpui::Div {
    div()
        .flex_none()
        .px_2()
        .py_1()
        .min_w(gpui::rems(1.8))
        .rounded_sm()
        .border_1()
        .border_b_2()
        .border_color(p.color(0x544936))
        .bg(p.color(0x191816))
        .text_size(gpui::rems(0.75))
        .text_center()
        .child(label.into())
}

pub fn mini_window(dark: bool) -> gpui::Div {
    let base = if dark { 0x191816 } else { 0xfaf8f4 };
    let surface = if dark { 0x302d27 } else { 0xe6dfd3 };
    let line = if dark { 0x514a3f } else { 0xd0c5b3 };
    div()
        .size_full()
        .rounded_md()
        .overflow_hidden()
        .bg(rgb(base))
        .border_1()
        .border_color(rgb(line))
        .child(
            div()
                .h(px(13.))
                .px_2()
                .flex()
                .items_center()
                .gap(px(3.))
                .bg(rgb(surface))
                .children((0..3).map(|_| div().size(px(3.)).rounded_full().bg(rgb(line)))),
        )
        .child(
            div()
                .flex()
                .h(px(65.))
                .p(px(7.))
                .gap(px(7.))
                .child(div().w(relative(0.22)).rounded_sm().bg(rgb(surface)))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(px(5.))
                        .pt(px(3.))
                        .child(
                            div()
                                .w(relative(0.45))
                                .h(px(5.))
                                .rounded_sm()
                                .bg(rgb(0xc6a66b)),
                        )
                        .child(div().w(relative(0.8)).h(px(4.)).rounded_sm().bg(rgb(line)))
                        .child(div().w(relative(0.6)).h(px(4.)).rounded_sm().bg(rgb(line)))
                        .child(div().flex_1().rounded_sm().bg(rgb(surface))),
                ),
        )
}
