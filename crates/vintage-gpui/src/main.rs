//! Native workspace Preview, deliberately separate from production data.
mod files;
mod settings;
mod settings_ui;
mod text_cache;
mod theme;
mod window_frame;
mod workspace;

use gpui::{
    actions, div, fill, font, point, prelude::*, px, relative, rgb, size, svg, App, AssetSource,
    Bounds, ClipboardItem, Context, ElementId, ElementInputHandler, Entity, EntityInputHandler,
    FocusHandle, FontStyle, FontWeight, GlobalElementId, InspectorElementId, KeyDownEvent,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    ScrollDelta, ScrollWheelEvent, ShapedLine, Style, TextAlign, TextRun, UTF16Selection,
    UnderlineStyle, Window, WindowBounds, WindowOptions,
};
use std::{
    borrow::Cow,
    ops::Range,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use text_cache::TextCache;
use vintage_core::composition::Composition;
use vintage_core::focus::FocusReporter;
use vintage_core::mouse::{Button, MouseAction};
use vintage_core::{encode_key, encode_paste, Modifiers, TerminalSize};
use vintage_runtime::{
    native_sessions::{NativeSessions, Owner},
    Session,
};
use vintage_terminal::Snapshot;

actions!(preview, [Quit]);

struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        match path {
            "favicon.svg" => Ok(Some(Cow::Borrowed(include_bytes!("../assets/favicon.svg")))),
            _ => Ok(None),
        }
    }

    fn list(&self, _path: &str) -> anyhow::Result<Vec<gpui::SharedString>> {
        Ok(Vec::new())
    }
}

struct TerminalView {
    owner: Arc<Mutex<Owner>>,
    focus: FocusHandle,
    focused: bool,
    focus_reporter: FocusReporter,
    _focus_subscriptions: Vec<gpui::Subscription>,
    _refresh_task: gpui::Task<()>,
    snapshot: Option<Snapshot>,
    revision: u64,
    requested_size: TerminalSize,
    font_size: f32,
    font_family: String,
    configured_scrollback: Option<usize>,
    shell_label: String,
    cell_width: Pixels,
    line_height: Pixels,
    bounds: Option<Bounds<Pixels>>,
    composition: Composition,
    preedit_layout: Option<ShapedLine>,
    text_cache: TextCache<ShapedLine>,
    selecting: Option<(usize, usize)>,
    scroll_remainder: f32,
    reported_buttons: [bool; 3],
    last_mouse_cell: Option<(usize, usize)>,
    error: Option<String>,
}

impl TerminalView {
    fn new(owner: Arc<Mutex<Owner>>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let focused = window.is_window_active() && focus.is_focused(window);
        let subscriptions = vec![
            cx.observe_window_activation(window, Self::update_focus),
            cx.on_focus(&focus, window, Self::update_focus),
            cx.on_blur(&focus, window, Self::update_focus),
        ];
        let executor = cx.background_executor().clone();
        let refresh_task = cx.spawn(async move |entity, cx| {
            // Only startup polls a JoinHandle. Once the service exists, the task
            // sleeps on its coalescing notification instead of an idle timer.
            let mut updates = loop {
                let state = entity.update(cx, |view: &mut Self, cx| {
                    view.refresh(cx);
                    let mut owner = view.owner.lock().expect("session owner mutex poisoned");
                    if let Some(session) = &mut owner.session {
                        Some(session.take_updates())
                    } else if owner.startup.is_none() {
                        Some(None)
                    } else {
                        None
                    }
                });
                match state {
                    Ok(Some(Some(updates))) => break updates,
                    Ok(None) => executor.timer(Duration::from_millis(16)).await,
                    _ => return,
                }
            };
            let mut retry = true;
            loop {
                if !retry && !updates.next().await {
                    break;
                }
                // Bound repaint requests during sustained output, not during idle.
                executor.timer(Duration::from_millis(16)).await;
                match entity.update(cx, |view, cx| view.refresh(cx)) {
                    Ok(needs_retry) => retry = needs_retry,
                    Err(_) => break,
                }
            }
        });
        let preferred = cx.global::<theme::Preferences>().0.font_family().to_owned();
        let font_family = if cx.text_system().all_font_names().contains(&preferred) {
            preferred
        } else {
            vintage_runtime::settings::Settings::default()
                .font_family()
                .to_owned()
        };
        Self {
            owner,
            focus,
            focused,
            focus_reporter: FocusReporter::default(),
            _focus_subscriptions: subscriptions,
            _refresh_task: refresh_task,
            snapshot: None,
            revision: 0,
            requested_size: TerminalSize::new(80, 24).unwrap(),
            font_size: cx.global::<theme::Preferences>().0.font_size as f32,
            font_family,
            configured_scrollback: None,
            shell_label: String::new(),
            cell_width: px(8.),
            line_height: px(18.),
            bounds: None,
            composition: Composition::default(),
            preedit_layout: None,
            text_cache: TextCache::default(),
            selecting: None,
            scroll_remainder: 0.,
            reported_buttons: [false; 3],
            last_mouse_cell: None,
            error: None,
        }
    }

    fn refresh(&mut self, cx: &mut Context<Self>) -> bool {
        let mut retry = false;
        let capacity = cx.global::<theme::Preferences>().0.scrollback;
        if self.configured_scrollback != Some(capacity) {
            let owner = self.owner.lock().expect("session owner mutex poisoned");
            if let Some(session) = &owner.session {
                if session.set_scrollback(capacity).is_ok() {
                    self.configured_scrollback = Some(capacity);
                } else {
                    retry = true;
                }
            }
        }
        let mut owner = self.owner.lock().expect("session owner mutex poisoned");
        let was_starting = owner.startup.is_some();
        owner.poll();
        if was_starting && owner.startup.is_none() {
            cx.notify();
        }
        if let Some(session) = &owner.session {
            let revision = session.revision();
            if revision != self.revision {
                if let Some(snapshot) = session.snapshot() {
                    if self.snapshot.as_ref().map(|old| old.mouse) != Some(snapshot.mouse) {
                        self.reported_buttons = [false; 3];
                        self.last_mouse_cell = None;
                    }
                    self.snapshot = Some(snapshot);
                    self.revision = revision;
                    self.error = session.error();
                    cx.notify();
                } else {
                    retry = true;
                }
            }
        }
        drop(owner);
        self.report_focus(cx) || retry
    }

    fn update_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focused = window.is_window_active() && self.focus.is_focused(window);
        if !self.focused {
            self.selecting = None;
            self.reported_buttons = [false; 3];
            self.last_mouse_cell = None;
        }
        self.report_focus(cx);
        cx.notify();
    }

    fn report_focus(&mut self, cx: &mut Context<Self>) -> bool {
        let enabled = self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.focus_reporting);
        let Some(bytes) = self.focus_reporter.pending(enabled, self.focused) else {
            return false;
        };
        let running = self
            .owner
            .lock()
            .expect("session owner mutex poisoned")
            .session
            .as_ref()
            .is_some_and(|session| !session.exited());
        if !running {
            return false;
        }
        if self.send(bytes.to_vec(), cx) {
            self.focus_reporter.acknowledge(self.focused);
            false
        } else {
            true
        }
    }

    fn operate(
        &mut self,
        operation: impl FnOnce(&Session) -> anyhow::Result<()>,
        cx: &mut Context<Self>,
    ) -> bool {
        let result = {
            let owner = self.owner.lock().expect("session owner mutex poisoned");
            owner
                .session
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Terminal is still starting"))
                .and_then(operation)
        };
        if let Err(error) = result {
            self.error = Some(error.to_string());
            cx.notify();
            false
        } else {
            self.error = None;
            true
        }
    }
    fn send(&mut self, bytes: Vec<u8>, cx: &mut Context<Self>) -> bool {
        self.operate(|session| session.write(session.id(), bytes), cx)
    }
    fn copy(&mut self, cx: &mut Context<Self>) {
        let text = self
            .owner
            .lock()
            .expect("session owner mutex poisoned")
            .session
            .as_ref()
            .and_then(Session::selected_text);
        if let Some(text) = text {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }
    fn paste(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            match encode_paste(
                &text,
                self.snapshot.as_ref().is_some_and(|s| s.bracketed_paste),
            ) {
                Ok(bytes) => {
                    self.send(bytes, cx);
                }
                Err(error) => {
                    self.error = Some(error.into());
                    cx.notify();
                }
            }
        }
    }
    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let modifiers = event.keystroke.modifiers;
        if modifiers.control && modifiers.shift && key.eq_ignore_ascii_case("c") {
            self.copy(cx);
            cx.stop_propagation();
            return;
        }
        if modifiers.control && modifiers.shift && key.eq_ignore_ascii_case("v") {
            self.paste(cx);
            cx.stop_propagation();
            return;
        }
        if modifiers.control && matches!(key, "=" | "+" | "-" | "0") {
            self.font_size = match key {
                "0" => cx.global::<theme::Preferences>().0.font_size as f32,
                "-" => (self.font_size - 1.).max(8.),
                _ => (self.font_size + 1.).min(48.),
            };
            cx.notify();
            cx.stop_propagation();
            return;
        }
        // IME owns editing keys while a composition is active.
        if !self.composition.text().is_empty() {
            return;
        }
        if modifiers.shift && matches!(key, "pageup" | "pagedown") {
            let lines = if key == "pageup" { 12 } else { -12 };
            self.operate(|s| s.scroll(lines), cx);
            cx.stop_propagation();
            return;
        }
        if let Some(bytes) = encode_key(
            key,
            Modifiers {
                control: modifiers.control,
                alt: modifiers.alt,
                shift: modifiers.shift,
            },
            self.snapshot.as_ref().is_some_and(|s| s.application_cursor),
        ) {
            self.send(bytes, cx);
            cx.stop_propagation();
        }
    }
    fn cell_at(&self, point: Point<Pixels>) -> Option<(usize, usize)> {
        let bounds = self.bounds?;
        let column = ((point.x - bounds.left()) / self.cell_width)
            .floor()
            .max(0.) as usize;
        let row = ((point.y - bounds.top()) / self.line_height)
            .floor()
            .max(0.) as usize;
        Some((
            column.min(self.requested_size.columns as usize - 1),
            row.min(self.requested_size.rows as usize - 1),
        ))
    }
    fn mouse_button(button: MouseButton) -> Option<Button> {
        match button {
            MouseButton::Left => Some(Button::Left),
            MouseButton::Middle => Some(Button::Middle),
            MouseButton::Right => Some(Button::Right),
            _ => None,
        }
    }
    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
        if !self
            .bounds
            .is_some_and(|bounds| bounds.contains(&event.position))
        {
            return;
        }
        if let (Some(cell), Some(button)) = (
            self.cell_at(event.position),
            Self::mouse_button(event.button),
        ) {
            if self.snapshot.as_ref().is_some_and(|s| s.mouse.enabled()) && !event.modifiers.shift {
                self.reported_buttons[button.index()] =
                    self.mouse_report(cell, MouseAction::Press(button), event.modifiers, cx);
            } else if button == Button::Left {
                self.selecting = Some(cell);
                self.operate(Session::clear_selection, cx);
            }
        }
    }
    fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(start) = self.selecting {
            // A release outside the window may not have reached this view.
            if event.pressed_button != Some(MouseButton::Left) {
                self.selecting = None;
            } else if let Some(end) = self.cell_at(event.position) {
                if start != end {
                    self.operate(|s| s.select(start, end), cx);
                }
                return;
            }
        }
        if event.pressed_button.is_none() {
            self.reported_buttons = [false; 3];
        }
        if event.modifiers.shift
            || !self
                .bounds
                .is_some_and(|bounds| bounds.contains(&event.position))
        {
            self.last_mouse_cell = None;
            return;
        }
        let button = event.pressed_button.and_then(Self::mouse_button);
        if event.pressed_button.is_some()
            && !button.is_some_and(|button| self.reported_buttons[button.index()])
        {
            return;
        }
        if let Some(cell) = self.cell_at(event.position) {
            if self.last_mouse_cell != Some(cell) {
                self.mouse_report(cell, MouseAction::Move(button), event.modifiers, cx);
            }
        }
    }
    fn mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.button == MouseButton::Left && self.selecting.take().is_some() {
            return;
        }
        if let Some(button) = Self::mouse_button(event.button) {
            // Complete an already reported press even if Shift was pressed mid-drag.
            if std::mem::take(&mut self.reported_buttons[button.index()]) {
                if let Some(cell) = self.cell_at(event.position) {
                    self.mouse_report(cell, MouseAction::Release(button), event.modifiers, cx);
                }
            }
        }
    }
    fn mouse_report(
        &mut self,
        cell: (usize, usize),
        action: MouseAction,
        modifiers: gpui::Modifiers,
        cx: &mut Context<Self>,
    ) -> bool {
        let bytes = self.snapshot.as_ref().and_then(|snapshot| {
            snapshot.mouse.encode(
                action,
                cell,
                Modifiers {
                    control: modifiers.control,
                    alt: modifiers.alt,
                    shift: modifiers.shift,
                },
            )
        });
        if let Some(bytes) = bytes {
            if self.send(bytes, cx) {
                self.last_mouse_cell = Some(cell);
                return true;
            }
        }
        false
    }
    fn scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self
            .bounds
            .is_some_and(|bounds| bounds.contains(&event.position))
        {
            return;
        }
        self.scroll_remainder += match event.delta {
            ScrollDelta::Lines(delta) => delta.y,
            ScrollDelta::Pixels(delta) => delta.y / self.line_height,
        };
        let lines = self.scroll_remainder.trunc() as i32;
        self.scroll_remainder -= lines as f32;
        if lines == 0 {
            return;
        }
        if self.snapshot.as_ref().is_some_and(|s| s.mouse.enabled()) && !event.modifiers.shift {
            if let Some(cell) = self.cell_at(event.position) {
                for _ in 0..lines.unsigned_abs().min(32) {
                    self.mouse_report(
                        cell,
                        MouseAction::Wheel { up: lines > 0 },
                        event.modifiers,
                        cx,
                    );
                }
            }
        } else {
            self.operate(|s| s.scroll(lines), cx);
        }
    }
    fn cursor_bounds(&self) -> Option<Bounds<Pixels>> {
        let (column, row) = self.snapshot.as_ref()?.cursor?;
        let bounds = self.bounds?;
        Some(Bounds::new(
            point(
                bounds.left() + self.cell_width * column as f32,
                bounds.top() + self.line_height * row as f32,
            ),
            size(self.cell_width, self.line_height),
        ))
    }
}

impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let text = self.composition.text_for_range(range.clone())?;
        *actual = Some(range);
        Some(text)
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.composition.selection(),
            reversed: false,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.composition.marked_range()
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.composition.clear();
        cx.notify();
    }
    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.composition.replace(range, text, None) {
            Ok(()) => {
                if self.send(self.composition.text().as_bytes().to_vec(), cx) {
                    self.composition.clear();
                }
            }
            Err(error) => self.error = Some(error.into()),
        }
        cx.notify();
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = self.composition.replace(range, text, selected) {
            self.error = Some(error.into());
        }
        cx.notify();
    }
    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let cursor = self.cursor_bounds()?;
        let Some(layout) = &self.preedit_layout else {
            return Some(cursor);
        };
        let left = layout.x_for_index(self.composition.byte_offset(range.start)?);
        let right = layout.x_for_index(self.composition.byte_offset(range.end)?);
        Some(Bounds::new(
            point(cursor.left() + left, cursor.top()),
            size((right - left).max(px(1.)), self.line_height),
        ))
    }
    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let cursor = self.cursor_bounds()?;
        let layout = self.preedit_layout.as_ref()?;
        let index = layout.index_for_x(point.x - cursor.left())?;
        Some(self.composition.text().get(..index)?.encode_utf16().count())
    }
}

struct TerminalElement {
    view: Entity<TerminalView>,
}
struct PaintState {
    lines: Vec<(ShapedLine, Point<Pixels>)>,
}
impl IntoElement for TerminalElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for TerminalElement {
    type RequestLayoutState = ();
    type PrepaintState = PaintState;
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> PaintState {
        self.view.update(cx, |view, cx| {
            let base_font = font(view.font_family.clone());
            let font_id = window.text_system().resolve_font(&base_font);
            let cell_width = window
                .text_system()
                .advance(font_id, px(view.font_size), 'M')
                .map(|s| s.width)
                .unwrap_or(px(view.font_size * 0.6));
            let line_height = px((view.font_size * 1.5).ceil());
            view.bounds = Some(bounds);
            view.cell_width = cell_width;
            view.line_height = line_height;
            let columns = (bounds.size.width / cell_width).floor().clamp(2., 500.) as u16;
            let rows = (bounds.size.height / line_height).floor().clamp(2., 500.) as u16;
            let dimensions = TerminalSize::new(columns, rows).unwrap();
            let ready = view
                .owner
                .lock()
                .expect("session owner mutex poisoned")
                .session
                .as_ref()
                .is_some_and(|session| !session.exited());
            if dimensions != view.requested_size
                && ready
                && view.operate(|s| s.resize(dimensions), cx)
            {
                view.requested_size = dimensions;
            }
            view.text_cache
                .begin_frame(view.font_size, window.scale_factor());
            let mut lines = Vec::new();
            if let Some(snapshot) = &view.snapshot {
                for cell in &snapshot.cells {
                    if cell.text.trim().is_empty() {
                        continue;
                    }
                    let line = view.text_cache.layout(cell, || {
                        let mut font = base_font.clone();
                        if cell.bold {
                            font.weight = FontWeight::BOLD;
                        }
                        if cell.italic {
                            font.style = FontStyle::Italic;
                        }
                        let run = TextRun {
                            len: cell.text.len(),
                            font,
                            color: rgb(cell.foreground).into(),
                            background_color: None,
                            underline: cell.underline.then_some(UnderlineStyle {
                                color: None,
                                thickness: px(1.),
                                wavy: false,
                            }),
                            strikethrough: cell.strikeout.then_some(gpui::StrikethroughStyle {
                                color: None,
                                thickness: px(1.),
                            }),
                        };
                        window.text_system().shape_line(
                            cell.text.clone().into(),
                            px(view.font_size),
                            &[run],
                            None,
                        )
                    });
                    lines.push((
                        line,
                        point(
                            bounds.left() + cell_width * cell.column as f32,
                            bounds.top() + line_height * cell.row as f32,
                        ),
                    ));
                }
            }
            view.text_cache.end_frame();
            view.preedit_layout = None;
            if !view.composition.text().is_empty() {
                if let Some(cursor) = view.cursor_bounds() {
                    let run = TextRun {
                        len: view.composition.text().len(),
                        font: base_font,
                        color: rgb(0xc6a66b).into(),
                        background_color: Some(rgb(0x201f1c).into()),
                        underline: Some(UnderlineStyle {
                            color: None,
                            thickness: px(1.),
                            wavy: false,
                        }),
                        strikethrough: None,
                    };
                    let line = window.text_system().shape_line(
                        view.composition.text().to_owned().into(),
                        px(view.font_size),
                        &[run],
                        None,
                    );
                    view.preedit_layout = Some(line.clone());
                    lines.push((line, cursor.origin));
                }
            }
            PaintState { lines }
        })
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        state: &mut PaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let view = self.view.read(cx);
        if view.focus.is_focused(window) {
            window.handle_input(
                &view.focus,
                ElementInputHandler::new(bounds, self.view.clone()),
                cx,
            );
        }
        if let Some(snapshot) = &view.snapshot {
            for cell in &snapshot.cells {
                if cell.background != 0x191816 || cell.selected {
                    let area = Bounds::new(
                        point(
                            bounds.left() + view.cell_width * cell.column as f32,
                            bounds.top() + view.line_height * cell.row as f32,
                        ),
                        size(view.cell_width * cell.width as f32, view.line_height),
                    );
                    window.paint_quad(fill(
                        area,
                        rgb(if cell.selected {
                            0x514736
                        } else {
                            cell.background
                        }),
                    ));
                }
            }
            if window.is_window_active()
                && view.focus.is_focused(window)
                && view.composition.text().is_empty()
            {
                if let Some(cursor) = view.cursor_bounds() {
                    window.paint_quad(fill(cursor, gpui::rgba(0xc6a66b66)));
                }
            }
        }
        let line_height = view.line_height;
        for (line, origin) in &state.lines {
            if line
                .paint(*origin, line_height, TextAlign::Left, None, window, cx)
                .is_err()
            {
                // Report a generic failure only; never log terminal content.
                self.view.update(cx, |view, _| {
                    view.error = Some("Terminal text rendering failed".into())
                });
            }
        }
    }
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::Palette::new(window, cx);
        let owner = self.owner.lock().expect("session owner mutex poisoned");
        let status = self.error.clone().or_else(|| owner.error.clone()).unwrap_or_else(|| {
            if owner.startup.is_some() { "Starting shell…".into() }
            else if owner.session.as_ref().is_some_and(Session::exited) { "Shell exited — output remains available".into() }
            else { format!("{} × {}  ·  {} px  ·  Ctrl+Shift+C/V copy/paste  ·  Shift+wheel select scrollback", self.requested_size.columns, self.requested_size.rows, self.font_size) }
        });
        drop(owner);
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(palette.color(0x191816))
            .text_color(palette.color(0xe6e1d8))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .child(
                div()
                    .id("terminal-surface")
                    .flex_1()
                    .min_h_0()
                    .p(px(8.))
                    .overflow_hidden()
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
                    .on_mouse_down(MouseButton::Middle, cx.listener(Self::mouse_down))
                    .on_mouse_down(MouseButton::Right, cx.listener(Self::mouse_down))
                    .on_mouse_move(cx.listener(Self::mouse_move))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
                    .on_mouse_up_out(MouseButton::Left, cx.listener(Self::mouse_up))
                    .on_mouse_up(MouseButton::Middle, cx.listener(Self::mouse_up))
                    .on_mouse_up_out(MouseButton::Middle, cx.listener(Self::mouse_up))
                    .on_mouse_up(MouseButton::Right, cx.listener(Self::mouse_up))
                    .on_mouse_up_out(MouseButton::Right, cx.listener(Self::mouse_up))
                    .on_scroll_wheel(cx.listener(Self::scroll))
                    .child(TerminalElement { view: cx.entity() }),
            )
            .child(
                div()
                    .h(gpui::rems(1.75))
                    .flex_none()
                    .px_3()
                    .flex()
                    .items_center()
                    .bg(palette.color(0x201f1c))
                    .text_size(gpui::rems(0.6875))
                    .text_color(palette.color(0xbab1a1))
                    .child(status),
            )
    }
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let mut root = PathBuf::from(".");
    let mut shell = vintage_runtime::default_shell_id().to_string();
    let mut exit_after = None;
    let mut settings_path = None;
    let mut shell_override = false;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--cwd" => {
                root = PathBuf::from(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("--cwd requires a path"))?,
                )
            }
            "--settings" => {
                settings_path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        anyhow::anyhow!("--settings requires a path")
                    })?));
            }
            "--shell" => {
                shell_override = true;
                shell = args
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--shell requires an ID or executable path"))?
            }
            "--exit-after" => {
                let seconds: u64 = args
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--exit-after requires seconds"))?
                    .parse()?;
                anyhow::ensure!(
                    (1..=60).contains(&seconds),
                    "--exit-after must be within 1..60 seconds"
                );
                exit_after = Some(seconds);
            }
            "--help" => {
                println!("VINTAGE GPUI Preview\nUsage: vintage-gpui [--cwd PATH] [--shell ID_OR_PATH] [--settings PATH] [--exit-after SECONDS]\n--exit-after: bounded startup/shutdown smoke test\nCtrl+Shift+C/V: copy/paste; Ctrl+plus/minus/0: font size; Shift+PageUp/PageDown: scroll\nCtrl+Shift+O: workspace; Ctrl+Shift+N: tab; Ctrl+Shift+D: split right; Ctrl+Shift+T: split down; Ctrl+Shift+W: close pane\nCtrl+comma: settings; Ctrl+Shift+F: files; drag window edges to resize");
                return Ok(());
            }
            "--list-shells" => {
                for (id, label) in vintage_runtime::available_shells() {
                    println!("{id}\t{label}");
                }
                return Ok(());
            }
            _ => anyhow::bail!("Unknown argument: {argument}"),
        }
    }
    let root = vintage_runtime::native_sessions::workspace_root(&root)?;
    let store = if let Some(path) = settings_path {
        vintage_runtime::settings::SettingsStore::new(path)
    } else {
        vintage_runtime::settings::SettingsStore::default_location()?
    };
    let (preferences, settings_error) = match store.load() {
        Ok(s) => (s, None),
        Err(e) => (
            vintage_runtime::settings::Settings::default(),
            Some(e.to_string()),
        ),
    };
    if !shell_override {
        shell = preferences.shell.clone();
    }
    let shells = vintage_runtime::available_shells();
    let application = gpui_platform::application().with_assets(Assets);
    let hook_ipc =
        Arc::new(vintage_runtime::hook_ipc::HookIpc::start().map_err(anyhow::Error::msg)?);
    let hook_events = hook_ipc.events();
    let sessions = Arc::new(NativeSessions::with_hook_ipc(hook_ipc));
    let shutdown_sessions = sessions.clone();
    let final_sessions = sessions.clone();
    let window_failed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let final_window_failed = window_failed.clone();
    application.run(move |cx: &mut App| {
        cx.set_global(theme::Preferences(preferences.clone()));
        if let Some(seconds) = exit_after {
            let executor = cx.background_executor().clone();
            cx.spawn(async move |cx| {
                executor.timer(Duration::from_secs(seconds)).await;
                cx.update(|cx| cx.quit());
            })
            .detach();
        }
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.on_app_quit(move |cx| {
            let sessions = shutdown_sessions.clone();
            cx.background_spawn(async move {
                // Final shutdown below reports any cleanup errors.
                let _ = sessions.shutdown();
            })
        })
        .detach();
        cx.on_action(|_: &Quit, cx| cx.quit());
        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                is_resizable: true,
                window_min_size: Some(size(px(760.), px(520.))),
                app_id: Some("dev.tiebi.vintage.gpui".to_owned()),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("VINTAGE GPUI Preview".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                cx.new(|cx| {
                    workspace::WorkspaceView::new(
                        root.clone(),
                        shell.clone(),
                        sessions.clone(),
                        hook_events.clone(),
                        settings::Setup {
                            store: store.clone(),
                            shells: shells.clone(),
                            error: settings_error.clone(),
                        },
                        window,
                        cx,
                    )
                })
            },
        );
        if opened.is_err() {
            window_failed.store(true, std::sync::atomic::Ordering::Release);
            cx.quit();
        }
        cx.activate(true);
    });
    final_sessions.shutdown()?;
    anyhow::ensure!(
        !final_window_failed.load(std::sync::atomic::Ordering::Acquire),
        "Cannot open the GPUI Preview window"
    );
    Ok(())
}
