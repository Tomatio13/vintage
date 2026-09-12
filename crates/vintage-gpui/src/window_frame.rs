//! Client-side resize handles; requests native compositor resizing.
use gpui::{div, prelude::*, px, CursorStyle, MouseButton, ResizeEdge};
pub fn handles() -> Vec<gpui::AnyElement> {
    use ResizeEdge::*;
    [
        Top,
        Bottom,
        Left,
        Right,
        TopLeft,
        TopRight,
        BottomLeft,
        BottomRight,
    ]
    .into_iter()
    .enumerate()
    .map(|(i, edge)| {
        let cursor = match edge {
            Top | Bottom => CursorStyle::ResizeUpDown,
            Left | Right => CursorStyle::ResizeLeftRight,
            TopLeft | BottomRight => CursorStyle::ResizeUpLeftDownRight,
            _ => CursorStyle::ResizeUpRightDownLeft,
        };
        let mut el = div()
            .id(("window-resize", i))
            .absolute()
            .cursor(cursor)
            .occlude();
        el = match edge {
            Top => el.top_0().left(px(10.)).right(px(10.)).h(px(5.)),
            Bottom => el.bottom_0().left(px(10.)).right(px(10.)).h(px(5.)),
            Left => el.left_0().top(px(10.)).bottom(px(10.)).w(px(5.)),
            Right => el.right_0().top(px(10.)).bottom(px(10.)).w(px(5.)),
            TopLeft => el.top_0().left_0().size(px(10.)),
            TopRight => el.top_0().right_0().size(px(10.)),
            BottomLeft => el.bottom_0().left_0().size(px(10.)),
            BottomRight => el.bottom_0().right_0().size(px(10.)),
        };
        el.on_mouse_down(MouseButton::Left, move |_, window, cx| {
            cx.stop_propagation();
            window.start_window_resize(edge);
        })
        .into_any_element()
    })
    .collect()
}
