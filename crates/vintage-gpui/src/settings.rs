//! Native preferences: local draft, visual previews and explicit background save.
use super::*;
use crate::{
    settings_ui as ui,
    theme::{Palette, Preferences},
};
use vintage_runtime::settings::{
    Appearance, Binding, Settings, SettingsStore, ACTIONS, FONT_PRESETS, SCROLLBACK,
};

pub struct Setup {
    pub store: SettingsStore,
    pub shells: Vec<(String, String)>,
    pub error: Option<String>,
}
pub struct Saved(pub Settings);
pub struct Closed;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Menu {
    Font,
    CustomFont,
    Shell,
}
const SECTIONS: [&str; 5] = [
    "Appearance",
    "Terminal",
    "Shortcuts",
    "Integrations",
    "Updates",
];
pub struct SettingsView {
    draft: Settings,
    store: SettingsStore,
    shells: Vec<(String, String)>,
    fonts: Vec<String>,
    section: usize,
    menu: Option<Menu>,
    recording: Option<usize>,
    focus: FocusHandle,
    error: Option<String>,
    saving: bool,
    recovery_required: bool,
    task: Option<gpui::Task<()>>,
    integrations: Vec<vintage_runtime::IntegrationSummary>,
    integrations_loading: bool,
    integration_task: Option<gpui::Task<()>>,
}
impl gpui::EventEmitter<Saved> for SettingsView {}
impl gpui::EventEmitter<Closed> for SettingsView {}
impl SettingsView {
    pub fn new(setup: Setup, cx: &mut Context<Self>) -> Self {
        let mut fonts = cx.text_system().all_font_names();
        fonts.sort();
        fonts.dedup();
        Self {
            draft: cx.global::<Preferences>().0.clone(),
            store: setup.store,
            shells: setup.shells,
            fonts,
            section: 0,
            menu: None,
            recording: None,
            focus: cx.focus_handle(),
            recovery_required: setup.error.is_some(),
            error: setup.error,
            saving: false,
            task: None,
            integrations: Vec::new(),
            integrations_loading: false,
            integration_task: None,
        }
    }
    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.focus.focus(window, cx);
    }
    fn dirty(&self, cx: &App) -> bool {
        self.recovery_required || self.draft != cx.global::<Preferences>().0
    }
    fn edit(&mut self, change: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        change(&mut self.draft);
        self.error = None;
        cx.notify();
    }
    fn discard(&mut self, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        self.draft = cx.global::<Preferences>().0.clone();
        self.error = None;
        self.menu = None;
        self.recording = None;
        cx.notify();
    }
    fn select_section(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        self.section = index;
        self.menu = None;
        self.recording = None;
        cx.notify();
    }
    fn toggle_menu(&mut self, menu: Menu, cx: &mut Context<Self>) {
        self.menu = if self.menu == Some(menu) {
            None
        } else {
            Some(menu)
        };
        cx.notify();
    }
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving || !self.dirty(cx) {
            return;
        }
        if let Err(e) = self.draft.validate() {
            self.error = Some(e.to_string());
            cx.notify();
            return;
        }
        self.saving = true;
        self.menu = None;
        self.recording = None;
        self.error = None;
        self.focus.focus(window, cx);
        let draft = self.draft.clone();
        let store = self.store.clone();
        let executor = cx.background_executor().clone();
        self.task = Some(cx.spawn(async move |entity, cx| {
            let result = executor
                .spawn(async move { store.save(&draft).map(|_| draft) })
                .await;
            let _ = entity.update(cx, |view, cx| {
                view.saving = false;
                match result {
                    Ok(settings) => {
                        view.recovery_required = false;
                        cx.emit(Saved(settings));
                    }
                    Err(e) => view.error = Some(format!("Could not save changes: {e}")),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            cx.stop_propagation();
            return;
        }
        let key = event.keystroke.key.to_ascii_lowercase();
        let m = event.keystroke.modifiers;
        if let Some(index) = self.recording {
            cx.stop_propagation();
            if key == "escape" {
                self.recording = None;
                self.error = None;
                cx.notify();
                return;
            }
            if matches!(key.as_str(), "shift" | "control" | "alt" | "super" | "cmd") {
                return;
            }
            if m.platform {
                self.error = Some("Use Ctrl or Alt instead of the system key.".into());
            } else {
                match self.draft.rebind(
                    index,
                    Binding {
                        key,
                        ctrl: m.control,
                        alt: m.alt,
                        shift: m.shift,
                    },
                ) {
                    Ok(()) => {
                        self.recording = None;
                        self.error = None;
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            cx.notify();
            return;
        }
        if key == "escape" {
            if self.menu.take().is_some() {
                cx.notify();
            } else {
                cx.emit(Closed);
            }
            cx.stop_propagation();
        } else if m.control && !m.alt && !m.platform {
            match key.as_str() {
                "s" => self.save(window, cx),
                "," => cx.emit(Closed),
                "1" | "2" | "3" | "4" | "5" => {
                    self.select_section(key.parse::<usize>().unwrap() - 1, cx)
                }
                _ => return,
            }
            cx.stop_propagation();
        }
    }
    fn stepper(&self, terminal: bool, p: Palette, cx: &mut Context<Self>) -> gpui::AnyElement {
        let (id, value, min, max, step, suffix) = if terminal {
            ("font-size", self.draft.font_size, 8, 48, 1, " px")
        } else {
            ("ui-scale", self.draft.ui_percent, 85, 200, 5, "%")
        };
        let change = move |view: &mut Self, delta: i32, cx: &mut Context<Self>| {
            view.edit(
                |s| {
                    let next = (value as i32 + delta).clamp(min as i32, max as i32) as u16;
                    if id == "ui-scale" {
                        s.ui_percent = next;
                    } else {
                        s.font_size = next;
                    }
                },
                cx,
            )
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_1()
            .rounded_md()
            .border_1()
            .border_color(p.color(0x544936))
            .bg(p.color(0x191816))
            .child(
                ui::action((id, 0usize), "−", p)
                    .min_w(gpui::rems(2.2))
                    .when(value == min, |s| {
                        s.opacity(0.35).cursor(gpui::CursorStyle::Arrow)
                    })
                    .on_click(cx.listener(move |v, _, _, cx| {
                        if value > min {
                            change(v, -step, cx);
                        }
                    })),
            )
            .child(
                div()
                    .min_w(gpui::rems(4.25))
                    .text_center()
                    .text_size(gpui::rems(0.875))
                    .font_weight(FontWeight::MEDIUM)
                    .child(format!("{value}{suffix}")),
            )
            .child(
                ui::action((id, 1usize), "+", p)
                    .min_w(gpui::rems(2.2))
                    .when(value == max, |s| {
                        s.opacity(0.35).cursor(gpui::CursorStyle::Arrow)
                    })
                    .on_click(cx.listener(move |v, _, _, cx| {
                        if value < max {
                            change(v, step, cx);
                        }
                    })),
            )
            .into_any_element()
    }
    fn appearance(&self, p: Palette, cx: &mut Context<Self>) -> gpui::AnyElement {
        let mut themes = div().flex().gap_3().when(p.stacked(), |s| s.flex_col());
        for (index, (appearance, label, description)) in [
            (Appearance::System, "System", "Match your device"),
            (Appearance::Light, "Light", "A brighter workspace"),
            (Appearance::Dark, "Dark", "A warmer workspace"),
            (
                Appearance::Graphite,
                "Graphite",
                "A neutral charcoal workspace",
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let selected = self.draft.appearance == appearance;
            let miniature = if appearance == Appearance::System {
                div()
                    .flex()
                    .gap(px(4.))
                    .size_full()
                    .child(div().flex_1().min_w_0().child(ui::mini_window(false)))
                    .child(div().flex_1().min_w_0().child(ui::mini_window(true)))
            } else {
                div().size_full().child(ui::mini_window(matches!(
                    appearance,
                    Appearance::Dark | Appearance::Graphite
                )))
            };
            themes = themes.child(
                div()
                    .id(("theme", index))
                    .flex_1()
                    .min_w_0()
                    .when(p.stacked(), |s| s.flex_none().w_full())
                    .p_3()
                    .rounded_lg()
                    .border_2()
                    .border_color(p.color(if selected { 0xc6a66b } else { 0x39352e }))
                    .bg(p.color(0x191816))
                    .cursor_pointer()
                    .hover(move |s| s.border_color(p.color(0x8c754e)))
                    .on_click(
                        cx.listener(move |v, _, _, cx| v.edit(|s| s.appearance = appearance, cx)),
                    )
                    .child(div().h(px(80.)).child(miniature))
                    .child(
                        div()
                            .mt_3()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(gpui::rems(0.875))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(label),
                            )
                            .child(
                                div()
                                    .size(px(16.))
                                    .flex_none()
                                    .rounded_full()
                                    .border_1()
                                    .border_color(p.color(if selected {
                                        0xc6a66b
                                    } else {
                                        0x544936
                                    }))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .when(selected, |s| {
                                        s.child(
                                            div().size(px(8.)).rounded_full().bg(p.color(0xc6a66b)),
                                        )
                                    }),
                            ),
                    )
                    .child(ui::hint(description, p).mt_1()),
            );
        }
        let scale = self.stepper(false, p, cx);
        let mut presets = div().flex().flex_wrap().gap_2();
        for value in [85, 100, 125, 150, 200] {
            presets = presets.child(
                ui::choice(
                    ("ui-preset", value as usize),
                    format!("{value}%"),
                    self.draft.ui_percent == value,
                    p,
                )
                .on_click(cx.listener(move |v, _, _, cx| v.edit(|s| s.ui_percent = value, cx))),
            );
        }
        div()
            .child(ui::heading(
                "Appearance",
                "Choose a color mode and a comfortable interface size.",
                p,
            ))
            .child(
                ui::card(p)
                    .child(
                        div()
                            .mb_4()
                            .text_size(gpui::rems(0.875))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Color mode"),
                    )
                    .child(themes),
            )
            .child(
                ui::card(p)
                    .mt_4()
                    .child(ui::field(
                        "Interface size",
                        "Find a comfortable size for labels, menus and controls.",
                        scale,
                        p,
                    ))
                    .child(presets.mt_4())
                    .child(
                        div()
                            .mt_4()
                            .p_4()
                            .min_w_0()
                            .rounded_md()
                            .border_1()
                            .border_color(p.color(0x39352e))
                            .bg(p.color(0x191816))
                            .child(ui::hint("PREVIEW", p).text_size(gpui::rems(0.625)))
                            .child(
                                div()
                                    .mt_2()
                                    .text_size(px(14. * self.draft.ui_percent as f32 / 100.))
                                    .child("Your workspace"),
                            )
                            .child(
                                div()
                                    .mt_1()
                                    .text_size(px(12. * self.draft.ui_percent as f32 / 100.))
                                    .text_color(p.color(0xbab1a1))
                                    .child("Workspace  /  Terminal  /  Files"),
                            ),
                    ),
            )
            .child(
                ui::hint(
                    "Terminal text has its own size control in the Terminal tab.",
                    p,
                )
                .mt_3(),
            )
            .into_any_element()
    }
    fn font_label(&self) -> String {
        if self.draft.font_preset == 5 {
            if self.draft.custom_font.is_empty() {
                "Choose a font".into()
            } else {
                self.draft.custom_font.clone()
            }
        } else {
            FONT_PRESETS
                .get(self.draft.font_preset)
                .unwrap_or(&"Default")
                .to_string()
        }
    }
    fn font_menu(&self, p: Palette, cx: &mut Context<Self>) -> gpui::AnyElement {
        let mut menu = div()
            .mt_3()
            .rounded_md()
            .border_1()
            .border_color(p.color(0x544936))
            .bg(p.color(0x191816))
            .p_1();
        for (i, label) in FONT_PRESETS.iter().enumerate() {
            menu = menu.child(
                ui::action(
                    ("font-option", i),
                    if i == 5 {
                        "Browse installed fonts…"
                    } else {
                        label
                    },
                    p,
                )
                .w_full()
                .justify_start()
                .when(self.draft.font_preset == i, |s| s.bg(p.color(0x343027)))
                .on_click(cx.listener(move |v, _, _, cx| {
                    if i == 5 {
                        v.menu = Some(Menu::CustomFont);
                        cx.notify();
                    } else {
                        v.edit(|s| s.font_preset = i, cx);
                        v.menu = None;
                    }
                })),
            );
        }
        menu.into_any_element()
    }
    fn custom_fonts(&self, p: Palette, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .mt_3()
            .rounded_md()
            .border_1()
            .border_color(p.color(0x544936))
            .bg(p.color(0x191816))
            .overflow_hidden()
            .child(
                div()
                    .px_3()
                    .py_2()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(ui::hint(format!("{} installed fonts", self.fonts.len()), p))
                    .child(
                        ui::action("back-to-presets", "Back", p).on_click(cx.listener(
                            |v, _, _, cx| {
                                v.menu = Some(Menu::Font);
                                cx.notify();
                            },
                        )),
                    ),
            )
            .child(
                gpui::uniform_list(
                    "installed-fonts",
                    self.fonts.len(),
                    cx.processor(move |view, range: Range<usize>, _, cx| {
                        range
                            .map(|i| {
                                let family = view.fonts[i].clone();
                                ui::action(("installed-font", i), family.clone(), p)
                                    .w_full()
                                    .h(gpui::rems(2.25))
                                    .justify_start()
                                    .overflow_hidden()
                                    .on_click(cx.listener(move |v, _, _, cx| {
                                        v.edit(
                                            |s| {
                                                s.font_preset = 5;
                                                s.custom_font = family.clone();
                                            },
                                            cx,
                                        );
                                        v.menu = None;
                                    }))
                                    .into_any_element()
                            })
                            .collect()
                    }),
                )
                .h(gpui::rems(11.25)),
            )
            .into_any_element()
    }
    fn terminal_preview(&self, p: Palette) -> gpui::AnyElement {
        let family = if self
            .fonts
            .iter()
            .any(|name| name == self.draft.font_family())
        {
            self.draft.font_family()
        } else if cfg!(windows) {
            "Consolas"
        } else {
            "DejaVu Sans Mono"
        };
        div()
            .mt_4()
            .rounded_md()
            .overflow_hidden()
            .border_1()
            .border_color(p.color(0x544936))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .py_2()
                    .bg(p.color(0x191816))
                    .child(ui::hint("Terminal preview", p))
                    .child(ui::tag(format!("{} px", self.draft.font_size), p)),
            )
            .child(
                div()
                    .id("font-preview")
                    .overflow_x_scroll()
                    .p_4()
                    .bg(rgb(0x191816))
                    .font_family(family.to_owned())
                    .text_size(px(self.draft.font_size as f32))
                    .line_height(px(self.draft.font_size as f32 * 1.5))
                    .text_color(rgb(0xe6e1d8))
                    .child(div().text_color(rgb(0xc6a66b)).child("~/workspace"))
                    .child(div().child("$ echo 'Hello, 世界'"))
                    .child(div().text_color(rgb(0xbab1a1)).child("Hello, 世界")),
            )
            .into_any_element()
    }
    fn terminal(&self, p: Palette, cx: &mut Context<Self>) -> gpui::AnyElement {
        let mut typography = ui::card(p).child(ui::field(
            "Font family",
            "Use a preset or choose any installed font.",
            div()
                .w(gpui::rems(17.))
                .max_w_full()
                .child(
                    ui::select("font-menu", self.font_label(), p)
                        .on_click(cx.listener(|v, _, _, cx| v.toggle_menu(Menu::Font, cx))),
                )
                .into_any_element(),
            p,
        ));
        if self.draft.font_preset != 0
            && !self
                .fonts
                .iter()
                .any(|name| name == self.draft.font_family())
        {
            typography = typography.child(
                ui::hint(
                    "This font is not installed. The default monospace font will be used.",
                    p,
                )
                .mt_3(),
            );
        }
        if self.menu == Some(Menu::Font) {
            typography = typography.child(self.font_menu(p, cx));
        }
        if self.menu == Some(Menu::CustomFont) {
            typography = typography.child(self.custom_fonts(p, cx));
        }
        typography = typography
            .child(
                div()
                    .mt_4()
                    .pt_4()
                    .border_t_1()
                    .border_color(p.color(0x39352e))
                    .child(ui::field(
                        "Text size",
                        "Independent of the interface size.",
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .flex_wrap()
                            .child(self.stepper(true, p, cx))
                            .when(self.draft.font_size != 12, |s| {
                                s.child(ui::action("reset-font-size", "Reset", p).on_click(
                                    cx.listener(|v, _, _, cx| v.edit(|s| s.font_size = 12, cx)),
                                ))
                            })
                            .into_any_element(),
                        p,
                    )),
            )
            .child(self.terminal_preview(p));
        let mut history = div().flex().flex_wrap().gap_2();
        for (i, lines) in SCROLLBACK.into_iter().enumerate() {
            history = history.child(
                ui::choice(
                    ("history", i),
                    ["1,000", "2,500", "5,000", "10,000"][i],
                    self.draft.scrollback == lines,
                    p,
                )
                .on_click(cx.listener(move |v, _, _, cx| v.edit(|s| s.scrollback = lines, cx))),
            );
        }
        let shell_label = self
            .shells
            .iter()
            .find(|(id, _)| id == &self.draft.shell)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| self.draft.shell.clone());
        let mut shell = ui::card(p).mt_4().child(ui::field(
            "Default shell",
            "Used for new terminals. Running sessions stay as they are.",
            div()
                .w(gpui::rems(17.))
                .max_w_full()
                .child(
                    ui::select("shell-menu", shell_label, p)
                        .on_click(cx.listener(|v, _, _, cx| v.toggle_menu(Menu::Shell, cx))),
                )
                .into_any_element(),
            p,
        ));
        if self.menu == Some(Menu::Shell) {
            let mut options = div()
                .mt_3()
                .p_1()
                .rounded_md()
                .border_1()
                .border_color(p.color(0x544936))
                .bg(p.color(0x191816));
            for (i, (id, label)) in self.shells.iter().enumerate() {
                let id = id.clone();
                options = options.child(
                    ui::action(("shell-option", i), label.clone(), p)
                        .w_full()
                        .justify_start()
                        .when(self.draft.shell == id, |s| s.bg(p.color(0x343027)))
                        .on_click(cx.listener(move |v, _, _, cx| {
                            v.edit(|s| s.shell = id.clone(), cx);
                            v.menu = None;
                        })),
                );
            }
            shell = shell.child(options);
        }
        div()
            .child(ui::heading(
                "Terminal",
                "Fonts, shells and scrollback, with a live preview.",
                p,
            ))
            .child(typography)
            .child(shell)
            .child(
                ui::card(p)
                    .mt_4()
                    .child(ui::field(
                        "Scrollback",
                        "Lines retained per terminal. Fewer lines use less memory.",
                        history.into_any_element(),
                        p,
                    ))
                    .child(
                        ui::hint(
                            "Reducing this limit removes the oldest retained lines when you save.",
                            p,
                        )
                        .mt_3(),
                    ),
            )
            .into_any_element()
    }
    fn shortcut_button(
        &self,
        index: usize,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let recording = self.recording == Some(index);
        let binding = &self.draft.bindings[index];
        let mut keys = div()
            .id(("record", index))
            .flex()
            .flex_wrap()
            .items_center()
            .gap_1()
            .p_1()
            .rounded_md()
            .cursor_pointer()
            .bg(p.color(if recording { 0x343027 } else { 0x201f1c }))
            .hover(move |s| s.bg(p.color(0x343027)))
            .on_click(cx.listener(move |v, _, window, cx| {
                v.recording = Some(index);
                v.menu = None;
                v.error = None;
                v.focus.focus(window, cx);
                cx.notify();
            }));
        if recording {
            keys = keys.child(
                ui::hint("Press a shortcut…", p)
                    .text_color(p.color(0xc6a66b))
                    .px_2()
                    .py_1(),
            );
        } else {
            if binding.ctrl {
                keys = keys.child(ui::keycap("Ctrl", p));
            }
            if binding.alt {
                keys = keys.child(ui::keycap("Alt", p));
            }
            if binding.shift {
                keys = keys.child(ui::keycap("Shift", p));
            }
            let label = match binding.key.as_str() {
                "left" => "←".into(),
                "right" => "→".into(),
                "up" => "↑".into(),
                "down" => "↓".into(),
                _ => binding.key.to_uppercase(),
            };
            keys = keys.child(ui::keycap(label, p));
        }
        keys.into_any_element()
    }
    fn shortcuts(&self, p: Palette, cx: &mut Context<Self>) -> gpui::AnyElement {
        let mut content = div().child(ui::heading(
            "Keyboard shortcuts",
            "Click a key combination to change it. Escape cancels recording.",
            p,
        ));
        for (title, indices) in [
            ("Tabs", &[0, 1, 6][..]),
            ("Panes", &[2, 3, 7, 8][..]),
            ("Workspaces", &[4, 5][..]),
        ] {
            let mut card = ui::card(p).mb_3().child(
                div()
                    .mb_3()
                    .text_size(gpui::rems(0.75))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(p.color(0xbab1a1))
                    .child(title),
            );
            for &index in indices {
                card = card.child(div().py_1().child(ui::field(
                    ACTIONS[index],
                    "",
                    self.shortcut_button(index, p, cx),
                    p,
                )));
            }
            content = content.child(card);
        }
        content
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .flex_wrap()
                    .child(ui::hint("Ctrl+, opens settings · Ctrl+S saves changes", p))
                    .child(
                        ui::action("reset-keys", "Restore defaults", p)
                            .border_1()
                            .border_color(p.color(0x39352e))
                            .on_click(cx.listener(|v, _, _, cx| {
                                v.edit(
                                    |s| s.bindings = vintage_runtime::settings::default_bindings(),
                                    cx,
                                );
                                v.recording = None;
                            })),
                    ),
            )
            .into_any_element()
    }
    fn refresh_integrations(&mut self, cx: &mut Context<Self>) {
        if self.integrations_loading {
            return;
        }
        self.integrations_loading = true;
        let executor = cx.background_executor().clone();
        self.integration_task = Some(cx.spawn(async move |entity, cx| {
            let statuses = executor
                .spawn(async move { vintage_runtime::integration_statuses() })
                .await;
            let _ = entity.update(cx, |view, cx| {
                view.integrations = statuses;
                view.integrations_loading = false;
                view.integration_task = None;
                cx.notify();
            });
        }));
    }
    fn change_integration(&mut self, agent: &'static str, install: bool, cx: &mut Context<Self>) {
        if self.integrations_loading {
            return;
        }
        self.integrations_loading = true;
        self.error = None;
        let executor = cx.background_executor().clone();
        self.integration_task = Some(cx.spawn(async move |entity, cx| {
            let result = executor
                .spawn(async move {
                    if install {
                        vintage_runtime::install_integration(agent)
                    } else {
                        vintage_runtime::uninstall_integration(agent)
                    }
                })
                .await;
            let _ = entity.update(cx, |view, cx| {
                view.integrations_loading = false;
                view.integration_task = None;
                match result {
                    Ok(status) => {
                        if let Some(current) = view
                            .integrations
                            .iter_mut()
                            .find(|current| current.agent == status.agent)
                        {
                            *current = status;
                        } else {
                            view.integrations.push(status);
                        }
                    }
                    Err(error) => {
                        view.error = Some(format!("Could not update integration: {error}"))
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn integrations(&mut self, p: Palette, cx: &mut Context<Self>) -> gpui::AnyElement {
        if self.integrations.is_empty() && !self.integrations_loading {
            self.refresh_integrations(cx);
        }
        let mut content = div()
            .child(ui::heading(
                "Agent connections",
                "Install managed hook or plugin assets for supported agent CLIs.",
                p,
            ))
            .child(
                ui::hint(
                    "Existing user configuration is preserved. Activity reporting is not available in this Preview yet.",
                    p,
                )
                .mb_4(),
            );
        for (index, (agent, name, initials, description)) in [
            ("codex", "Codex", "Cx", "Managed session-start hook"),
            ("claude", "Claude Code", "Cl", "Managed session-start hook"),
            ("opencode", "OpenCode", "Op", "Managed lifecycle plugin"),
        ]
        .into_iter()
        .enumerate()
        {
            let status = self
                .integrations
                .iter()
                .find(|status| status.agent == agent);
            let state = status
                .map(|status| status.state.clone())
                .unwrap_or_else(|| "Checking".to_string());
            let message = status
                .map(|status| status.message.clone())
                .unwrap_or_else(|| "Checking the current installation…".to_string());
            let installed = state == "Installed";
            let conflict = state == "Conflict";
            let busy = self.integrations_loading;
            let action = if installed { "Remove" } else { "Install" };
            content = content.child(
                ui::card(p).mb_3().child(
                    div()
                        .flex()
                        .gap_3()
                        .items_center()
                        .flex_wrap()
                        .child(
                            div()
                                .size(gpui::rems(2.75))
                                .flex_none()
                                .rounded_lg()
                                .bg(p.color(0x343027))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(gpui::rems(0.875))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(initials),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_size(gpui::rems(0.9375))
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(name),
                                )
                                .child(ui::hint(description, p).mt_1())
                                .child(ui::hint(message.clone(), p).mt_1()),
                        )
                        .child(
                            div()
                                .flex_none()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(ui::tag(
                                    if conflict {
                                        "Conflict".to_string()
                                    } else {
                                        state.clone()
                                    },
                                    p,
                                ))
                                .when(!conflict, |row| {
                                    row.child(
                                        ui::primary(
                                            ("integration-action", index),
                                            action,
                                            p,
                                            !busy,
                                        )
                                        .on_click(
                                            cx.listener(move |view, _, _, cx| {
                                                view.change_integration(agent, !installed, cx)
                                            }),
                                        ),
                                    )
                                }),
                        ),
                ),
            );
        }
        content.into_any_element()
    }
    fn updates(&self, p: Palette) -> gpui::AnyElement {
        div()
            .child(ui::heading(
                "About this edition",
                "Version and update availability for VINTAGE.",
                p,
            ))
            .child(
                ui::card(p)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_4()
                            .flex_wrap()
                            .child(
                                div()
                                    .size(gpui::rems(3.5))
                                    .rounded_lg()
                                    .overflow_hidden()
                                    .flex_none()
                                    .bg(p.color(0x191816))
                                    .border_1()
                                    .border_color(p.color(0x544936))
                                    .p_2()
                                    .child(
                                        svg()
                                            .path("favicon.svg")
                                            .size_full()
                                            .text_color(p.color(0xc6a66b)),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .text_size(gpui::rems(1.125))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child("VINTAGE"),
                                    )
                                    .child(
                                        ui::hint(
                                            format!("Version {}", env!("CARGO_PKG_VERSION")),
                                            p,
                                        )
                                        .mt_1(),
                                    ),
                            )
                            .child(ui::tag("GPUI Preview", p)),
                    )
                    .child(
                        div()
                            .mt_5()
                            .pt_4()
                            .border_t_1()
                            .border_color(p.color(0x39352e))
                            .child(
                                div()
                                    .text_size(gpui::rems(0.875))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("You're using the native Preview"),
                            )
                            .child(
                                ui::hint(
                                    "Automatic updates for this edition are not available yet.",
                                    p,
                                )
                                .mt_2(),
                            ),
                    ),
            )
            .into_any_element()
    }
}
impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::new(window, cx);
        let dirty = self.dirty(cx);
        let mut navigation = div()
            .id("settings-navigation")
            .flex()
            .items_center()
            .gap_1()
            .flex_wrap()
            .py_2();
        for (i, label) in SECTIONS.into_iter().enumerate() {
            let selected = self.section == i;
            navigation = navigation.child(
                ui::action(("settings-section", i), label, p)
                    .px_4()
                    .bg(p.color(if selected { 0x343027 } else { 0x191816 }))
                    .text_color(p.color(if selected { 0xc6a66b } else { 0xbab1a1 }))
                    .on_click(cx.listener(move |v, _, _, cx| v.select_section(i, cx))),
            );
        }
        let content = match self.section {
            0 => self.appearance(p, cx),
            1 => self.terminal(p, cx),
            2 => self.shortcuts(p, cx),
            3 => self.integrations(p, cx),
            _ => self.updates(p),
        };
        let status = if self.saving {
            "Saving your preferences…"
        } else if dirty {
            "Unsaved changes"
        } else {
            "All changes saved"
        };
        let header = div()
            .flex_none()
            .px_5()
            .pt_4()
            .border_b_1()
            .border_color(p.color(0x39352e))
            .child(
                div()
                    .w_full()
                    .max_w(gpui::rems(66.))
                    .mx_auto()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(
                                div()
                                    .text_size(gpui::rems(1.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Settings"),
                            )
                            .child(ui::action("settings-back", "← Workspace", p).on_click(
                                cx.listener(|v, _, _, cx| {
                                    if !v.saving {
                                        cx.emit(Closed);
                                    }
                                }),
                            )),
                    )
                    .child(navigation.mt_2()),
            );
        let body = div().px_5().py_5().child(
            div()
                .w_full()
                .max_w(gpui::rems(60.))
                .mx_auto()
                .child(content),
        );
        let scrolling = div()
            .id(("settings-scroll", self.section))
            .flex_1()
            .min_h_0()
            .overflow_y_scroll();
        let mut root = div()
            .id("settings")
            .relative()
            .size_full()
            .min_h_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .bg(p.color(0x191816))
            .text_color(p.color(0xe6e1d8))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key));
        if p.stacked() {
            root = root.child(scrolling.child(header).child(body));
        } else {
            root = root.child(header).child(scrolling.child(body));
        }
        root.child(
            div()
                .flex_none()
                .border_t_1()
                .border_color(p.color(0x39352e))
                .bg(p.color(0x201f1c))
                .px_5()
                .py_3()
                .child(
                    div()
                        .w_full()
                        .max_w(gpui::rems(66.))
                        .mx_auto()
                        .flex()
                        .flex_col()
                        .when_some(self.error.as_ref(), |s, error| {
                            s.child(
                                ui::hint(error.clone(), p)
                                    .text_color(p.color(0xe8aa82))
                                    .mb_2(),
                            )
                        })
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .flex_wrap()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            div().size(px(6.)).rounded_full().bg(
                                                p.color(if dirty { 0xc6a66b } else { 0x9b958a })
                                            ),
                                        )
                                        .child(ui::hint(status, p)),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .when(dirty, |s| {
                                            s.child(
                                                ui::action("discard-settings", "Discard", p)
                                                    .on_click(
                                                        cx.listener(|v, _, _, cx| v.discard(cx)),
                                                    ),
                                            )
                                        })
                                        .child(
                                            ui::primary(
                                                "save-settings",
                                                "Save changes",
                                                p,
                                                dirty && !self.saving,
                                            )
                                            .on_click(
                                                cx.listener(|v, _, window, cx| v.save(window, cx)),
                                            ),
                                        ),
                                ),
                        ),
                ),
        )
        .when(self.saving, |root| {
            root.child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
            )
        })
    }
}
