//! Workspace chrome owns views; native resources remain in the runtime service.
use super::*;
use std::collections::BTreeMap;
use vintage_core::{
    composition::Composition,
    workspace::{Axis, Id, Layout, Workspaces},
};

#[derive(Clone)]
struct DraggedFilesPanel;

impl Render for DraggedFilesPanel {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

pub struct WorkspaceView {
    settings_view: Entity<crate::settings::SettingsView>,
    settings_visible: bool,
    _settings_subscriptions: Vec<gpui::Subscription>,
    _appearance_subscription: gpui::Subscription,
    model: Workspaces,
    terminals: BTreeMap<Id, Entity<TerminalView>>,
    sessions: Arc<NativeSessions>,
    shell: String,
    focus: FocusHandle,
    error: Option<String>,
    picker_pending: bool,
    picker: Option<gpui::Task<()>>,
    sidebar_visible: bool,
    files_visible: bool,
    files_panel_width: Pixels,
    files_resize_start: Option<(Pixels, Pixels)>,
    window_drag: bool,
    files_panel: Option<(Id, Entity<crate::files::FilesPanel>)>,
    rename: Option<(Id, Composition)>,
    rename_focus: FocusHandle,
    hook_activity: BTreeMap<Id, String>,
    block_notice: Option<Id>,
    _hook_task: gpui::Task<()>,
}

pub(crate) fn button(
    id: impl Into<ElementId>,
    label: impl Into<gpui::SharedString>,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded_md()
        .cursor_pointer()
        .text_size(gpui::rems(0.75))
        .hover(|style| style.bg(gpui::rgba(0xc6a66b25)))
        .child(label.into())
}

impl WorkspaceView {
    pub fn new(
        root: PathBuf,
        shell: String,
        sessions: Arc<NativeSessions>,
        hook_events: Arc<Mutex<std::sync::mpsc::Receiver<vintage_runtime::hook_ipc::HookActivity>>>,
        setup: crate::settings::Setup,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let settings_view = cx.new(|cx| crate::settings::SettingsView::new(setup, cx));
        let subscriptions = vec![
            cx.subscribe_in(
                &settings_view,
                window,
                |view, _, event: &crate::settings::Saved, window, cx| {
                    cx.set_global(crate::theme::Preferences(event.0.clone()));
                    if !event.0.hook_notifications {
                        view.block_notice = None;
                    } else if view.block_notice.is_none() {
                        view.block_notice = view
                            .hook_activity
                            .iter()
                            .find(|(_, state)| state.as_str() == "blocked")
                            .map(|(pane, _)| *pane);
                    }
                    window.set_rem_size(px(16. * event.0.ui_percent as f32 / 100.));
                    view.shell = event.0.shell.clone();
                    let installed = cx.text_system().all_font_names();
                    let family = if installed.iter().any(|f| f == event.0.font_family()) {
                        event.0.font_family().to_owned()
                    } else {
                        vintage_runtime::settings::Settings::default()
                            .font_family()
                            .to_owned()
                    };
                    for terminal in view.terminals.values() {
                        terminal.update(cx, |v, cx| {
                            v.font_size = event.0.font_size as f32;
                            v.font_family = family.clone();
                            v.text_cache = TextCache::default();
                            v.configured_scrollback = None;
                            v.refresh(cx);
                            cx.notify();
                        });
                    }
                    window.refresh();
                    cx.notify();
                },
            ),
            cx.subscribe_in(
                &settings_view,
                window,
                |view, _, _: &crate::settings::Closed, window, cx| {
                    view.settings_visible = false;
                    view.focus_active(window, cx);
                    cx.notify();
                },
            ),
        ];
        let appearance_subscription =
            window.observe_window_appearance(|window, _| window.refresh());
        window.set_rem_size(px(16.
            * cx.global::<crate::theme::Preferences>().0.ui_percent as f32
            / 100.));
        let executor = cx.background_executor().clone();
        let hook_task = cx.spawn(async move |entity, cx| loop {
            let events = hook_events.clone();
            let event = executor
                .spawn(async move {
                    events
                        .lock()
                        .expect("hook event receiver mutex poisoned")
                        .recv()
                })
                .await;
            let Ok(event) = event else {
                return;
            };
            if entity
                .update(cx, |view, cx| {
                    if view.terminals.contains_key(&event.pane) {
                        let was_blocked = view.hook_activity.get(&event.pane).map(String::as_str)
                            == Some("blocked");
                        match event.state.as_deref() {
                            Some("released") | None => {
                                view.hook_activity.remove(&event.pane);
                                if view.block_notice == Some(event.pane) {
                                    view.block_notice = None;
                                }
                            }
                            Some(state) => {
                                view.hook_activity.insert(event.pane, state.to_string());
                                if state == "blocked"
                                    && cx
                                        .global::<crate::theme::Preferences>()
                                        .0
                                        .hook_notifications
                                    && !was_blocked
                                {
                                    view.block_notice = Some(event.pane);
                                }
                                if state != "blocked" && view.block_notice == Some(event.pane) {
                                    view.block_notice = None;
                                }
                            }
                        }
                        cx.notify();
                    }
                })
                .is_err()
            {
                return;
            }
        });
        let mut view = Self {
            settings_view,
            settings_visible: false,
            _settings_subscriptions: subscriptions,
            _appearance_subscription: appearance_subscription,
            model: Workspaces::default(),
            terminals: BTreeMap::new(),
            sessions,
            shell,
            focus: cx.focus_handle(),
            error: None,
            picker_pending: false,
            picker: None,
            sidebar_visible: true,
            files_visible: false,
            files_panel_width: px(460.),
            files_resize_start: None,
            window_drag: false,
            files_panel: None,
            rename: None,
            rename_focus: cx.focus_handle(),
            hook_activity: BTreeMap::new(),
            block_notice: None,
            _hook_task: hook_task,
        };
        view.model
            .add_workspace(root)
            .expect("initial workspace fits limits");
        view.synchronize(window, cx);
        view
    }
    fn synchronize(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let panes: BTreeMap<Id, PathBuf> = self
            .model
            .items
            .iter()
            .flat_map(|w| {
                w.tabs
                    .iter()
                    .flat_map(|t| t.layout.panes())
                    .map(|id| (id, w.root.clone()))
            })
            .collect();
        let removed: Vec<_> = self
            .terminals
            .keys()
            .filter(|id| !panes.contains_key(id))
            .copied()
            .collect();
        for id in removed {
            self.terminals.remove(&id);
            self.hook_activity.remove(&id);
            if self.block_notice == Some(id) {
                self.block_notice = None;
            }
            self.sessions.close(id);
        }
        for (id, root) in panes {
            if let std::collections::btree_map::Entry::Vacant(vacant) = self.terminals.entry(id) {
                let owner = self.sessions.start_with_scrollback(
                    id,
                    root,
                    self.shell.clone(),
                    cx.global::<crate::theme::Preferences>().0.scrollback,
                );
                let view = cx.new(|cx| {
                    let mut view = TerminalView::new(owner, window, cx);
                    view.shell_label = PathBuf::from(&self.shell)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    view
                });
                // Titles and bell markers live in this view, so repaint the
                // workspace chrome when it changes.
                cx.observe(&view, |_, _, cx| cx.notify()).detach();
                vacant.insert(view);
            }
        }
        self.synchronize_files(cx);
        self.focus_active(window, cx);
        cx.notify();
    }
    fn synchronize_files(&mut self, cx: &mut Context<Self>) {
        let target = self
            .files_visible
            .then(|| self.model.current())
            .flatten()
            .map(|w| (w.id, w.root.clone()));
        if self.files_panel.as_ref().map(|(id, _)| *id) == target.as_ref().map(|(id, _)| *id) {
            return;
        }
        if let Some((_, panel)) = self.files_panel.take() {
            panel.update(cx, |panel, cx| panel.close(cx));
        }
        if let Some((id, root)) = target {
            self.files_panel = Some((id, cx.new(|cx| crate::files::FilesPanel::new(id, root, cx))));
        }
    }
    fn toggle_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_visible = !self.sidebar_visible;
        self.focus_active(window, cx);
        cx.notify();
    }
    fn toggle_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.files_visible = !self.files_visible;
        self.synchronize_files(cx);
        if !self.files_visible {
            self.focus_active(window, cx);
        }
        cx.notify();
    }
    fn start_files_resize(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        self.files_resize_start = Some((event.position.x, self.files_panel_width));
        cx.stop_propagation();
    }
    fn resize_files(&mut self, position: Pixels, cx: &mut Context<Self>) {
        let Some((start_x, start_width)) = self.files_resize_start else {
            return;
        };
        let width = (start_width + start_x - position).clamp(px(240.), px(840.));
        if width != self.files_panel_width {
            self.files_panel_width = width;
            cx.notify();
        }
    }
    fn focus_active(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(terminal) = self
            .model
            .tab()
            .and_then(|t| self.terminals.get(&t.active_pane))
        {
            terminal.read(cx).focus.clone().focus(window, cx);
        } else {
            self.focus.focus(window, cx);
        }
    }
    fn start_rename(&mut self, id: Id, window: &mut Window, cx: &mut Context<Self>) {
        let Some(title) = self.model.current().and_then(|workspace| {
            workspace
                .tabs
                .iter()
                .find(|tab| tab.id == id)
                .map(|tab| tab.title.clone())
        }) else {
            return;
        };
        let mut composition = Composition::default();
        let length = title.encode_utf16().count();
        let _ = composition.replace(None, &title, Some(0..length));
        self.rename = Some((id, composition));
        self.rename_focus.focus(window, cx);
        cx.notify();
    }
    fn commit_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, composition)) = self.rename.take() else {
            return;
        };
        self.error = self
            .model
            .rename_tab(id, composition.text().to_owned())
            .err()
            .map(str::to_owned);
        self.focus_active(window, cx);
        cx.notify();
    }
    fn cancel_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.rename = None;
        self.focus_active(window, cx);
        cx.notify();
    }
    fn add_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.error = self.model.add_tab().err().map(str::to_owned);
        self.synchronize(window, cx);
    }
    fn split(&mut self, axis: Axis, window: &mut Window, cx: &mut Context<Self>) {
        self.error = self.model.split(axis).err().map(str::to_owned);
        self.synchronize(window, cx);
    }
    fn activate(
        &mut self,
        workspace: Id,
        tab: Option<Id>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.model.activate(workspace, tab);
        self.error = None;
        self.synchronize_files(cx);
        self.focus_active(window, cx);
        cx.notify();
    }
    fn close_pane(&mut self, id: Id, window: &mut Window, cx: &mut Context<Self>) {
        self.model.close_pane(id);
        self.error = None;
        self.synchronize(window, cx);
    }
    fn close_tab(&mut self, id: Id, window: &mut Window, cx: &mut Context<Self>) {
        self.model.close_tab(id);
        self.error = None;
        self.synchronize(window, cx);
    }
    fn close_workspace(&mut self, id: Id, window: &mut Window, cx: &mut Context<Self>) {
        self.model.close_workspace(id);
        self.error = None;
        self.synchronize(window, cx);
    }
    fn show_blocked_pane(&mut self, pane: Id, window: &mut Window, cx: &mut Context<Self>) {
        self.block_notice = None;
        if let Some((workspace, tab)) = self.model.pane_location(pane) {
            self.model.activate(workspace, Some(tab));
            self.model.focus(pane);
            self.synchronize_files(cx);
            self.focus_active(window, cx);
        }
        cx.notify();
    }
    fn dismiss_block_notice(&mut self, cx: &mut Context<Self>) {
        self.block_notice = None;
        cx.notify();
    }
    fn open_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.picker_pending {
            return;
        }
        self.picker_pending = true;
        let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open workspace".into()),
        });
        let executor = cx.background_executor().clone();
        self.picker = Some(cx.spawn_in(window, async move |entity, cx| {
            let result = match paths.await {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        executor
                            .spawn(async move {
                                vintage_runtime::native_sessions::workspace_root(&path).map(Some)
                            })
                            .await
                    } else {
                        Ok(None)
                    }
                }
                Ok(Ok(None)) => Ok(None),
                _ => Err(anyhow::anyhow!("Cannot open the folder picker")),
            };
            let _ = entity.update_in(cx, |view, window, cx| {
                view.picker_pending = false;
                match result {
                    Ok(Some(path)) => {
                        view.error = view.model.add_workspace(path).err().map(str::to_owned);
                        view.synchronize(window, cx);
                    }
                    Ok(None) => view.focus_active(window, cx),
                    Err(error) => view.error = Some(error.to_string()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn toggle_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_visible = !self.settings_visible;
        if self.settings_visible {
            self.settings_view
                .update(cx, |view, cx| view.focus(window, cx));
        } else {
            self.focus_active(window, cx);
        }
        cx.notify();
    }
    fn navigate(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.model.navigate(index);
        self.synchronize_files(cx);
        self.focus_active(window, cx);
        cx.notify();
    }
    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let modifiers = event.keystroke.modifiers;
        if self.settings_visible {
            return;
        }
        if self.rename.is_some() {
            match event.keystroke.key.to_ascii_lowercase().as_str() {
                "enter" | "return" => self.commit_rename(window, cx),
                "escape" => self.cancel_rename(window, cx),
                _ => return,
            }
            cx.stop_propagation();
            return;
        }
        if modifiers.control && !modifiers.alt && !modifiers.platform && event.keystroke.key == ","
        {
            self.toggle_settings(window, cx);
            cx.stop_propagation();
            return;
        }
        let chord = vintage_runtime::settings::Binding {
            key: event.keystroke.key.to_ascii_lowercase(),
            ctrl: modifiers.control,
            alt: modifiers.alt,
            shift: modifiers.shift,
        };
        if !modifiers.platform {
            if let Some(index) = cx
                .global::<crate::theme::Preferences>()
                .0
                .bindings
                .iter()
                .position(|b| b == &chord)
            {
                match index {
                    0..=5 => self.navigate(index, window, cx),
                    6 => self.add_tab(window, cx),
                    7 => self.split(Axis::Horizontal, window, cx),
                    8 => self.split(Axis::Vertical, window, cx),
                    9 => {
                        if let Some(pane) = self.model.tab().map(|tab| tab.active_pane) {
                            if let Some(terminal) = self.terminals.get(&pane) {
                                terminal.update(cx, |view, cx| view.toggle_search(window, cx));
                            }
                        }
                    }
                    10 => self.toggle_sidebar(window, cx),
                    11 => {
                        if let Some(pane) = self.model.tab().map(|tab| tab.active_pane) {
                            self.close_pane(pane, window, cx);
                        }
                    }
                    _ => unreachable!("settings validation limits shortcut actions"),
                }
                cx.stop_propagation();
                return;
            }
        }
        if !modifiers.control || !modifiers.shift || modifiers.alt {
            return;
        }
        match event.keystroke.key.to_ascii_lowercase().as_str() {
            "f" => self.toggle_files(window, cx),

            "o" => self.open_workspace(window, cx),
            "tab" => {
                if let Some(w) = self.model.current() {
                    if let Some(i) = w.tabs.iter().position(|t| Some(t.id) == w.active_tab) {
                        let id = w.id;
                        let next = w.tabs[(i + 1) % w.tabs.len()].id;
                        self.activate(id, Some(next), window, cx);
                    }
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }
    fn activity_for_panes(&self, panes: impl IntoIterator<Item = Id>) -> Option<&str> {
        panes
            .into_iter()
            .filter_map(|pane| self.hook_activity.get(&pane).map(String::as_str))
            .max_by_key(|state| match *state {
                "blocked" => 3,
                "working" => 2,
                "idle" => 1,
                _ => 0,
            })
    }
    fn activity_mark(state: Option<&str>) -> &'static str {
        match state {
            Some("working") => "●",
            Some("blocked") => "!",
            Some("idle") => "○",
            _ => "○",
        }
    }
    fn activity_color(state: Option<&str>) -> u32 {
        match state {
            Some("working") => 0xc6a66b,
            Some("blocked") => 0xe8aa82,
            Some("idle") => 0x9b958a,
            _ => 0x544936,
        }
    }
    fn pane_layout(
        &self,
        layout: &Layout,
        palette: crate::theme::Palette,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let rgb = |value| palette.color(value);
        let notifications_enabled = cx
            .global::<crate::theme::Preferences>()
            .0
            .hook_notifications;
        match layout {
            Layout::Pane(id) => {
                let id = *id;
                let active = self.model.tab().is_some_and(|t| t.active_pane == id);
                let terminal = self.terminals.get(&id).expect("pane has a view").clone();
                let activity = self.hook_activity.get(&id).map(String::as_str);
                let activity = if !notifications_enabled && activity == Some("blocked") {
                    None
                } else {
                    activity
                };
                let blocked = activity == Some("blocked");
                div()
                    .id(("pane", id))
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .size_full()
                    .flex()
                    .flex_col()
                    .border_1()
                    .rounded_sm()
                    .overflow_hidden()
                    .border_color(rgb(if blocked {
                        0xe8aa82
                    } else if active {
                        0x8c754e
                    } else {
                        0x302d27
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |view, _, window, cx| {
                            view.model.focus(id);
                            view.focus_active(window, cx);
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .h(gpui::rems(1.875))
                            .flex_none()
                            .px_2()
                            .flex()
                            .items_center()
                            .justify_between()
                            .bg(rgb(if blocked { 0x3b2922 } else { 0x201f1c }))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_size(gpui::rems(0.75))
                                    .text_color(rgb(if active { 0xe6e1d8 } else { 0xbab1a1 }))
                                    .child(format!(
                                        "Terminal · {}",
                                        terminal.read(cx).header_title()
                                    )),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .mr_2()
                                    .text_size(gpui::rems(0.75))
                                    .text_color(rgb(if terminal.read(cx).needs_attention {
                                        0xe8aa82
                                    } else {
                                        Self::activity_color(activity)
                                    }))
                                    .child(if terminal.read(cx).needs_attention {
                                        "!"
                                    } else {
                                        Self::activity_mark(activity)
                                    }),
                            )
                            .child(
                                button(("close-pane", id), "×")
                                    .flex_none()
                                    .ml_2()
                                    .border_1()
                                    .border_color(rgb(0x39352e))
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation()
                                    })
                                    .on_click(cx.listener(move |view, _, window, cx| {
                                        cx.stop_propagation();
                                        view.close_pane(id, window, cx);
                                    })),
                            ),
                    )
                    .child(div().flex_1().min_h_0().min_w_0().child(terminal))
                    .into_any_element()
            }
            Layout::Split {
                axis,
                first,
                second,
            } => {
                let mut container = div().size_full().flex().gap(px(5.)).min_w_0().min_h_0();
                if *axis == Axis::Vertical {
                    container = container.flex_col();
                }
                container
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .overflow_hidden()
                            .child(self.pane_layout(first, palette, cx)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .overflow_hidden()
                            .child(self.pane_layout(second, palette, cx)),
                    )
                    .into_any_element()
            }
        }
    }
}
impl Render for WorkspaceView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = crate::theme::Palette::new(window, cx);
        let rgb = |value| palette.color(value);
        let notifications_enabled = cx
            .global::<crate::theme::Preferences>()
            .0
            .hook_notifications;
        let blocked_panes: Vec<_> = self
            .hook_activity
            .iter()
            .filter(|(_, state)| notifications_enabled && state.as_str() == "blocked")
            .filter_map(|(pane, _)| {
                self.model
                    .pane_location(*pane)
                    .and_then(|(workspace, tab)| {
                        self.model
                            .items
                            .iter()
                            .find(|item| item.id == workspace)
                            .and_then(|item| item.tabs.iter().find(|item| item.id == tab))
                            .map(|tab| (*pane, tab.title.clone()))
                    })
            })
            .collect();
        let mut sidebar = div()
            .id("workspaces-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_3()
            .py_2();
        if !blocked_panes.is_empty() {
            let mut notices = div()
                .mb_3()
                .p_2()
                .rounded_md()
                .bg(rgb(0x2b201c))
                .border_1()
                .border_color(rgb(0xe8aa82))
                .child(
                    div()
                        .mb_2()
                        .text_size(gpui::rems(0.6875))
                        .text_color(rgb(0xf2c3a8))
                        .child("NEEDS YOUR INPUT"),
                );
            for (pane, title) in &blocked_panes {
                let pane = *pane;
                notices = notices.child(
                    div()
                        .id(("blocked-pane", pane))
                        .mb_1()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .cursor_pointer()
                        .bg(rgb(0x3b2922))
                        .hover(|style| style.bg(rgb(0x5a3829)))
                        .on_click(cx.listener(move |view, _, window, cx| {
                            view.show_blocked_pane(pane, window, cx)
                        }))
                        .child(
                            div()
                                .text_size(gpui::rems(0.75))
                                .text_color(rgb(0xf2c3a8))
                                .child(format!("!  {title}")),
                        )
                        .child(
                            div()
                                .text_size(gpui::rems(0.6875))
                                .text_color(rgb(0xc6a66b))
                                .child("Open terminal →"),
                        ),
                );
            }
            sidebar = sidebar.child(notices);
        }
        for workspace in &self.model.items {
            let id = workspace.id;
            let active = self.model.active == Some(id);
            let name = workspace
                .root
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| workspace.root.display().to_string());
            let mut group = div().mb_2().child(
                div()
                    .id(("workspace", id))
                    .flex()
                    .items_center()
                    .justify_between()
                    .rounded_md()
                    .px_2()
                    .py_2()
                    .bg(rgb(if active { 0x343027 } else { 0x201f1c }))
                    .cursor_pointer()
                    .on_click(
                        cx.listener(move |view, _, window, cx| view.activate(id, None, window, cx)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_size(gpui::rems(0.875))
                            .child(name),
                    )
                    .child(button(("remove-workspace", id), "×").on_click(cx.listener(
                        move |view, _, window, cx| {
                            cx.stop_propagation();
                            view.close_workspace(id, window, cx);
                        },
                    ))),
            );
            for tab in &workspace.tabs {
                let tab_id = tab.id;
                let activity = self.activity_for_panes(tab.layout.panes());
                let activity = if !notifications_enabled && activity == Some("blocked") {
                    None
                } else {
                    activity
                };
                let bell = tab.layout.panes().into_iter().any(|pane| {
                    self.terminals
                        .get(&pane)
                        .is_some_and(|terminal| terminal.read(cx).needs_attention)
                });
                group = group.child(
                    div()
                        .id(("sidebar-tab", tab_id))
                        .pl_5()
                        .py_1()
                        .text_size(gpui::rems(0.8125))
                        .cursor_pointer()
                        .text_color(rgb(if active && workspace.active_tab == Some(tab_id) {
                            0xe6e1d8
                        } else {
                            0x9b958a
                        }))
                        .hover(|s| s.bg(rgb(0x2b2923)))
                        .on_click(cx.listener(move |view, _, window, cx| {
                            view.activate(id, Some(tab_id), window, cx)
                        }))
                        .child(format!(
                            "{}  {}  ·  {}",
                            if bell {
                                "!"
                            } else {
                                Self::activity_mark(activity)
                            },
                            tab.title,
                            tab.layout.panes().len()
                        )),
                );
            }
            sidebar = sidebar.child(group);
        }
        let mut tabs = div()
            .id("tabs")
            .flex()
            .items_center()
            .flex_none()
            .min_w_0()
            .max_w(relative(0.65))
            .overflow_x_scroll()
            .gap_1();
        if let Some(workspace) = self.model.current() {
            let workspace_id = workspace.id;
            for tab in &workspace.tabs {
                let id = tab.id;
                let activity = self.activity_for_panes(tab.layout.panes());
                let activity = if !notifications_enabled && activity == Some("blocked") {
                    None
                } else {
                    activity
                };
                let bell = tab.layout.panes().into_iter().any(|pane| {
                    self.terminals
                        .get(&pane)
                        .is_some_and(|terminal| terminal.read(cx).needs_attention)
                });
                tabs = tabs.child(
                    div()
                        .id(("tab", id))
                        .flex()
                        .items_center()
                        .flex_none()
                        .h_full()
                        .px_3()
                        .gap_2()
                        .border_b_2()
                        .border_color(rgb(if workspace.active_tab == Some(id) {
                            0xc6a66b
                        } else {
                            0x201f1c
                        }))
                        .bg(rgb(if workspace.active_tab == Some(id) {
                            0x2b2923
                        } else {
                            0x201f1c
                        }))
                        .text_color(rgb(if workspace.active_tab == Some(id) {
                            0xe6e1d8
                        } else {
                            0x9b958a
                        }))
                        .cursor_pointer()
                        .on_click(
                            cx.listener(move |view, event: &gpui::ClickEvent, window, cx| {
                                if event.click_count() == 2 {
                                    view.start_rename(id, window, cx);
                                } else {
                                    view.activate(workspace_id, Some(id), window, cx);
                                }
                            }),
                        )
                        .child(div().size(px(6.)).rounded_full().bg(rgb(if bell {
                            0xe8aa82
                        } else if activity.is_some() {
                            Self::activity_color(activity)
                        } else if workspace.active_tab == Some(id) {
                            0xc6a66b
                        } else {
                            0x544936
                        })))
                        .child(
                            div()
                                .relative()
                                .min_w(px(96.))
                                .text_size(gpui::rems(0.8125))
                                .when(
                                    self.rename
                                        .as_ref()
                                        .is_some_and(|(renaming, _)| *renaming == id),
                                    |input| {
                                        let (_, composition) = self.rename.as_ref().unwrap();
                                        let entity = cx.entity().clone();
                                        let focus = self.rename_focus.clone();
                                        input
                                            // Join the key dispatch tree so
                                            // Enter/Escape reach the workspace
                                            // key handler during rename.
                                            .track_focus(&focus)
                                            .px_1()
                                            .bg(rgb(0x191816))
                                            .border_1()
                                            .border_color(rgb(0xc6a66b))
                                            .child(composition.text().to_owned())
                                            .child(
                                                gpui::canvas(
                                                    |_, _, _| (),
                                                    move |bounds, _, window, cx| {
                                                        window.handle_input(
                                                            &focus,
                                                            ElementInputHandler::new(
                                                                bounds, entity,
                                                            ),
                                                            cx,
                                                        );
                                                    },
                                                )
                                                .absolute()
                                                .size_full(),
                                            )
                                    },
                                )
                                .when(
                                    self.rename
                                        .as_ref()
                                        .is_none_or(|(renaming, _)| *renaming != id),
                                    |label| label.child(tab.title.clone()),
                                ),
                        )
                        .when(
                            self.rename
                                .as_ref()
                                .is_some_and(|(renaming, _)| *renaming == id),
                            |tab| {
                                tab.child(button(("save-tab-name", id), "✓").on_click(cx.listener(
                                    move |view, _, window, cx| {
                                        cx.stop_propagation();
                                        view.commit_rename(window, cx);
                                    },
                                )))
                                .child(
                                    button(("cancel-tab-name", id), "×").on_click(cx.listener(
                                        move |view, _, window, cx| {
                                            cx.stop_propagation();
                                            view.cancel_rename(window, cx);
                                        },
                                    )),
                                )
                            },
                        )
                        .child(button(("close-tab", id), "×").on_click(cx.listener(
                            move |view, _, window, cx| {
                                cx.stop_propagation();
                                view.close_tab(id, window, cx);
                            },
                        ))),
                );
            }
        }
        tabs = tabs.child(
            button("new-tab", "+")
                .on_click(cx.listener(|view, _, window, cx| view.add_tab(window, cx))),
        );
        let block_notice = self
            .block_notice
            .filter(|_| notifications_enabled)
            .and_then(|pane| {
                self.model.pane_location(pane).and_then(|(workspace, tab)| {
                    self.model
                        .items
                        .iter()
                        .find(|item| item.id == workspace)
                        .and_then(|item| item.tabs.iter().find(|item| item.id == tab))
                        .map(|tab| (pane, tab.title.clone()))
                })
            });
        let content = if let Some(tab) = self.model.tab() {
            self.pane_layout(&tab.layout, palette, cx)
        } else {
            div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .child(
                    div()
                        .text_size(gpui::rems(1.375))
                        .child("Your workspace, ready when you are"),
                )
                .child(
                    div()
                        .text_color(rgb(0x9b958a))
                        .child("Open a folder, then add a terminal tab."),
                )
                .child(
                    button("empty-open", "Open workspace").on_click(
                        cx.listener(|view, _, window, cx| view.open_workspace(window, cx)),
                    ),
                )
                .when(self.model.current().is_some(), |s| {
                    s.child(
                        button("empty-tab", "New terminal")
                            .on_click(cx.listener(|view, _, window, cx| view.add_tab(window, cx))),
                    )
                })
                .into_any_element()
        };
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x191816))
            .text_color(rgb(0xe6e1d8))
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(Self::key_down))
            .on_drag_move(cx.listener(
                |view, event: &gpui::DragMoveEvent<DraggedFilesPanel>, _, cx| {
                    view.resize_files(event.event.position.x, cx)
                },
            ))
            .child(
                div()
                    .h(gpui::rems(2.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .bg(rgb(0x24231f))
                    .border_b_1()
                    .border_color(rgb(0x39352e))
                    .child(
                        div()
                            .id("toggle-sidebar")
                            .size(px(28.))
                            .ml_2()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_md()
                            .cursor_pointer()
                            .border_1()
                            .border_color(rgb(0x4b4740))
                            .hover(|style| style.bg(rgb(0x343027)))
                            .on_click(
                                cx.listener(|view, _, window, cx| view.toggle_sidebar(window, cx)),
                            )
                            .child(
                                svg()
                                    .path("favicon.svg")
                                    .size(px(16.))
                                    .text_color(rgb(0xc6a66b)),
                            ),
                    )
                    .child(tabs)
                    .child(
                        div()
                            .id("window-drag-region")
                            .h_full()
                            .flex_1()
                            .px_4()
                            .flex()
                            .items_center()
                            .justify_between()
                            .window_control_area(gpui::WindowControlArea::Drag)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|view, _, _, _| view.window_drag = true),
                            )
                            .on_mouse_down_out(
                                cx.listener(|view, _, _, _| view.window_drag = false),
                            )
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|view, _, _, _| view.window_drag = false),
                            )
                            .on_mouse_up_out(
                                MouseButton::Left,
                                cx.listener(|view, _, _, _| view.window_drag = false),
                            )
                            .on_mouse_move(cx.listener(
                                |view, event: &MouseMoveEvent, window, _| {
                                    if view.window_drag {
                                        view.window_drag = false;
                                        #[cfg(not(windows))]
                                        if event.pressed_button == Some(MouseButton::Left) {
                                            window.start_window_move();
                                        }
                                        #[cfg(windows)]
                                        let _ = (event, window);
                                    }
                                },
                            ))
                            .on_click(|event, window, _| {
                                #[cfg(not(windows))]
                                if event.click_count() == 2 {
                                    window.zoom_window();
                                }
                                #[cfg(windows)]
                                let _ = (event, window);
                            }),
                    )
                    .child(button("toolbar-split-right", "◫").on_click(
                        cx.listener(|view, _, window, cx| view.split(Axis::Horizontal, window, cx)),
                    ))
                    .child(button("toolbar-split-down", "⬒").on_click(
                        cx.listener(|view, _, window, cx| view.split(Axis::Vertical, window, cx)),
                    ))
                    .child(
                        button("toggle-files", "▤")
                            .bg(rgb(if self.files_visible {
                                0x393328
                            } else {
                                0x24231f
                            }))
                            .on_click(
                                cx.listener(|view, _, window, cx| view.toggle_files(window, cx)),
                            ),
                    )
                    .child(
                        button("minimize-window", "−")
                            .on_click(|_, window, _| window.minimize_window()),
                    )
                    .child(
                        button("maximize-window", "□")
                            .on_click(|_, window, _| window.zoom_window()),
                    )
                    .child(
                        button("close-window", "×").on_click(|_, window, _| window.remove_window()),
                    ),
            )
            .when(self.settings_visible, |root| {
                root.child(div().flex_1().min_h_0().child(self.settings_view.clone()))
            })
            .when(!self.settings_visible, |root| {
                root.child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .when(self.sidebar_visible, |row| {
                            row.child(
                                div()
                                    .w(px(280.))
                                    .flex_none()
                                    .flex()
                                    .flex_col()
                                    .bg(rgb(0x201f1c))
                                    .border_r_1()
                                    .border_color(rgb(0x39352e))
                                    .child(
                                        div()
                                            .h(gpui::rems(2.75))
                                            .px_3()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .text_size(gpui::rems(0.6875))
                                                    .text_color(rgb(0x9b958a))
                                                    .child("WORKSPACE"),
                                            )
                                            .child(
                                                button(
                                                    "open-workspace",
                                                    if self.picker_pending { "…" } else { "+" },
                                                )
                                                .on_click(cx.listener(|view, _, window, cx| {
                                                    view.open_workspace(window, cx)
                                                })),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .px_3()
                                            .py_2()
                                            .flex()
                                            .flex_col()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .id("sidebar-new-terminal")
                                                    .px_2()
                                                    .py_2()
                                                    .rounded_md()
                                                    .flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .cursor_pointer()
                                                    .bg(rgb(0x343027))
                                                    .hover(|style| style.bg(rgb(0x393328)))
                                                    .on_click(cx.listener(|view, _, window, cx| {
                                                        view.add_tab(window, cx)
                                                    }))
                                                    .child(div().child("＋  New terminal"))
                                                    .child(
                                                        div()
                                                            .text_size(gpui::rems(0.625))
                                                            .text_color(rgb(0x9b958a))
                                                            .child("⌃⇧T"),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .id("sidebar-open-folder")
                                                    .px_2()
                                                    .py_2()
                                                    .rounded_md()
                                                    .flex()
                                                    .items_center()
                                                    .cursor_pointer()
                                                    .text_color(rgb(0xbab1a1))
                                                    .hover(|style| style.bg(rgb(0x2b2923)))
                                                    .on_click(cx.listener(|view, _, window, cx| {
                                                        view.open_workspace(window, cx)
                                                    }))
                                                    .child("▣  Open folder"),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .px_3()
                                            .pt_4()
                                            .text_size(gpui::rems(0.6875))
                                            .text_color(rgb(0x9b958a))
                                            .child("PROJECTS"),
                                    )
                                    .child(sidebar)
                                    .child(
                                        div()
                                            .border_t_1()
                                            .border_color(rgb(0x39352e))
                                            .px_3()
                                            .py_3()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap_2()
                                                    .text_color(rgb(0xbab1a1))
                                                    .child(
                                                        svg()
                                                            .path("favicon.svg")
                                                            .size(px(18.))
                                                            .text_color(rgb(0xc6a66b)),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(gpui::rems(0.8125))
                                                            .child("VINTAGE"),
                                                    ),
                                            )
                                            .child(button("open-settings", "⚙").on_click(
                                                cx.listener(|v, _, window, cx| {
                                                    v.toggle_settings(window, cx)
                                                }),
                                            )),
                                    ),
                            )
                        })
                        .child(
                            div().flex_1().min_w_0().flex().flex_col().child(
                                div()
                                    .flex_1()
                                    .min_h_0()
                                    .flex()
                                    .child(
                                        div().flex_1().min_w_0().min_h_0().p(px(6.)).child(content),
                                    )
                                    .when_some(self.files_panel.as_ref(), |row, (_, panel)| {
                                        row.child(
                                            div()
                                                .id("files-resize-handle")
                                                .w(px(6.))
                                                .flex_none()
                                                .cursor_col_resize()
                                                .bg(rgb(0x39352e))
                                                .hover(|style| style.bg(rgb(0xc6a66b)))
                                                .on_drag(DraggedFilesPanel, |dragged, _, _, cx| {
                                                    cx.stop_propagation();
                                                    cx.new(|_| dragged.clone())
                                                })
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|view, event, _, cx| {
                                                        view.start_files_resize(event, cx)
                                                    }),
                                                )
                                                .on_mouse_up(MouseButton::Left, |_, _, cx| {
                                                    cx.stop_propagation();
                                                })
                                                .occlude(),
                                        )
                                        .child(
                                            div()
                                                .w(self.files_panel_width)
                                                .min_w(px(240.))
                                                .max_w(relative(0.55))
                                                .flex_none()
                                                .min_w_0()
                                                .child(panel.clone()),
                                        )
                                    }),
                            ),
                        ),
                )
            })
            .when_some(block_notice, |root, (pane, title)| {
                root.child(
                    div()
                        .id(("blocked-notice", pane))
                        .absolute()
                        .right(px(24.))
                        .bottom(px(24.))
                        .w(px(336.))
                        .p_3()
                        .rounded_md()
                        .border_1()
                        .border_color(rgb(0xe8aa82))
                        .bg(rgb(0x2b201c))
                        .cursor_pointer()
                        .on_click(cx.listener(move |view, _, window, cx| {
                            view.show_blocked_pane(pane, window, cx)
                        }))
                        .child(
                            div()
                                .flex()
                                .items_start()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .child(
                                            div()
                                                .text_size(gpui::rems(0.8125))
                                                .text_color(rgb(0xf2c3a8))
                                                .child("Input required"),
                                        )
                                        .child(
                                            div()
                                                .text_size(gpui::rems(0.75))
                                                .text_color(rgb(0xd3c5b8))
                                                .child(format!(
                                                    "{title} is waiting for your response."
                                                )),
                                        )
                                        .child(
                                            div()
                                                .mt_1()
                                                .text_size(gpui::rems(0.75))
                                                .text_color(rgb(0xc6a66b))
                                                .child("Show terminal →"),
                                        ),
                                )
                                .child(
                                    button(("dismiss-blocked-notice", pane), "×")
                                        .flex_none()
                                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation()
                                        })
                                        .on_click(cx.listener(|view, _, _, cx| {
                                            cx.stop_propagation();
                                            view.dismiss_block_notice(cx);
                                        })),
                                ),
                        ),
                )
            })
            .when(!window.is_maximized() && !window.is_fullscreen(), |root| {
                root.children(crate::window_frame::handles())
            })
    }
}

impl EntityInputHandler for WorkspaceView {
    fn text_for_range(
        &mut self,
        range: std::ops::Range<usize>,
        actual: &mut Option<std::ops::Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let (_, composition) = self.rename.as_ref()?;
        let text = composition.text_for_range(range.clone())?;
        *actual = Some(range);
        Some(text)
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let (_, composition) = self.rename.as_ref()?;
        Some(UTF16Selection {
            range: composition.selection(),
            reversed: false,
        })
    }
    fn marked_text_range(
        &self,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<std::ops::Range<usize>> {
        self.rename
            .as_ref()
            .and_then(|(_, composition)| composition.marked_range())
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        cx.notify();
    }
    fn replace_text_in_range(
        &mut self,
        range: Option<std::ops::Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((_, composition)) = self.rename.as_mut() {
            let range = range.or_else(|| Some(composition.selection()));
            let _ = composition.replace(range, text, None);
        }
        cx.notify();
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<std::ops::Range<usize>>,
        text: &str,
        selected: Option<std::ops::Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((_, composition)) = self.rename.as_mut() {
            let range = range.or_else(|| Some(composition.selection()));
            let _ = composition.replace(range, text, selected);
        }
        cx.notify();
    }
    fn bounds_for_range(
        &mut self,
        _: std::ops::Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(bounds)
    }
    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        self.rename
            .as_ref()
            .map(|(_, composition)| composition.text().encode_utf16().count())
    }
}
