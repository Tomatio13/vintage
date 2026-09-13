//! Alacritty parser and screen state. No windowing or PTY creation lives here.
use alacritty_terminal::{
    event::{Event, EventListener, WindowSize},
    grid::{Dimensions, Scroll},
    index::{Column, Line, Point, Side},
    selection::{Selection, SelectionType},
    term::{
        cell::Flags,
        color::Colors,
        {Config, Osc52, TermMode},
    },
    vte::{
        ansi::Processor,
        ansi::{Color, CursorShape, Rgb},
    },
    Term,
};
use std::sync::{Arc, Mutex};
use vintage_core::{
    mouse::{Encoding, MouseMode, Tracking},
    TerminalSize,
};

struct Size(TerminalSize);
impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.screen_lines()
    }
    fn screen_lines(&self) -> usize {
        self.0.rows as usize
    }
    fn columns(&self) -> usize {
        self.0.columns as usize
    }
}

/// Events collected while parsing, split from the PTY replies that must be
/// written back in order. Titles and clipboard contents stay in flight only:
/// they are never logged or persisted.
#[derive(Clone, Default)]
struct Events(Arc<Mutex<Collected>>);

#[derive(Default)]
struct Collected {
    replies: Vec<Event>,
    /// `Some(Some(title))` sets the title, `Some(None)` resets it.
    title: Option<Option<String>>,
    clipboard: Option<String>,
    bells: u32,
}

impl EventListener for Events {
    fn send_event(&self, event: Event) {
        match event {
            // Never retain application titles or clipboard contents from terminal output.
            Event::PtyWrite(_) | Event::ColorRequest(..) | Event::TextAreaSizeRequest(_) => {
                self.0
                    .lock()
                    .expect("event mutex poisoned")
                    .replies
                    .push(event);
            }
            // Paste requests stay rejected by the Osc52::OnlyCopy policy.
            Event::ClipboardStore(_, text) => {
                self.0.lock().expect("event mutex poisoned").clipboard = Some(text);
            }
            Event::Bell => {
                self.0.lock().expect("event mutex poisoned").bells += 1;
            }
            Event::Title(title) => {
                self.0.lock().expect("event mutex poisoned").title =
                    Some(Some(truncate_title(title)));
            }
            Event::ResetTitle => {
                self.0.lock().expect("event mutex poisoned").title = Some(None);
            }
            _ => {}
        }
    }
}

fn truncate_title(title: String) -> String {
    title.chars().take(200).collect()
}

/// Cursor style requested by the application via DECSCUSR.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorStyle {
    Block,
    Underline,
    Beam,
    HollowBlock,
}

/// Underline decoration requested via SGR 4/21.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnderlineKind {
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

fn underline_kind(flags: &Flags) -> Option<UnderlineKind> {
    if flags.contains(Flags::UNDERLINE) {
        Some(UnderlineKind::Single)
    } else if flags.contains(Flags::DOUBLE_UNDERLINE) {
        Some(UnderlineKind::Double)
    } else if flags.contains(Flags::UNDERCURL) {
        Some(UnderlineKind::Curly)
    } else if flags.contains(Flags::DOTTED_UNDERLINE) {
        Some(UnderlineKind::Dotted)
    } else if flags.contains(Flags::DASHED_UNDERLINE) {
        Some(UnderlineKind::Dashed)
    } else {
        None
    }
}

#[derive(Clone, Debug)]
pub struct Cell {
    pub text: String,
    pub column: usize,
    pub row: usize,
    pub width: usize,
    pub foreground: u32,
    pub background: u32,
    pub bold: bool,
    pub italic: bool,
    pub underline: Option<UnderlineKind>,
    pub strikeout: bool,
    pub selected: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub column: usize,
    pub row: usize,
    pub style: CursorStyle,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub size: TerminalSize,
    pub cells: Vec<Cell>,
    pub cursor: Option<Cursor>,
    pub application_cursor: bool,
    pub bracketed_paste: bool,
    pub focus_reporting: bool,
    pub mouse: MouseMode,
    pub display_offset: usize,
    /// Application-provided title (OSC 0/2), already length-capped.
    pub title: Option<String>,
}

pub struct Terminal {
    term: Term<Events>,
    parser: Processor,
    events: Events,
    size: TerminalSize,
    title: Option<String>,
    clipboard: Option<String>,
    bells: u32,
}

impl Terminal {
    pub fn new(size: TerminalSize) -> Self {
        let events = Events::default();
        let config = Config {
            scrolling_history: 1000,
            // Copy requests only: paste requests would let remote output read
            // the system clipboard.
            osc52: Osc52::OnlyCopy,
            ..Config::default()
        };
        Self {
            term: Term::new(config, &Size(size), events.clone()),
            parser: Processor::new(),
            events,
            size,
            title: None,
            clipboard: None,
            bells: 0,
        }
    }

    pub fn set_scrollback(&mut self, lines: usize) {
        self.term.set_options(Config {
            scrolling_history: lines.min(10000),
            osc52: Osc52::OnlyCopy,
            ..Config::default()
        });
    }

    /// Feed ordered raw bytes, including incomplete UTF-8 or escape sequences.
    /// Returned replies must be written back to the same PTY, in order.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        self.parser.advance(&mut self.term, bytes);
        let mut collected =
            std::mem::take(&mut *self.events.0.lock().expect("event mutex poisoned"));
        if let Some(title) = collected.title.take() {
            self.title = title;
        }
        if collected.bells > 0 {
            self.bells = self.bells.saturating_add(collected.bells);
        }
        if let Some(text) = collected.clipboard.take() {
            self.clipboard = Some(text);
        }
        collected
            .replies
            .into_iter()
            .filter_map(|event| match event {
                Event::PtyWrite(text) => Some(text.into_bytes()),
                Event::ColorRequest(index, format) => {
                    let value = palette(index, self.term.colors());
                    Some(
                        format(Rgb {
                            r: (value >> 16) as u8,
                            g: (value >> 8) as u8,
                            b: value as u8,
                        })
                        .into_bytes(),
                    )
                }
                Event::TextAreaSizeRequest(format) => Some(
                    format(WindowSize {
                        num_lines: self.size.rows,
                        num_cols: self.size.columns,
                        cell_width: 0,
                        cell_height: 0,
                    })
                    .into_bytes(),
                ),
                _ => None,
            })
            .collect()
    }

    /// Application-provided title (OSC 0/2), if any.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Consume the newest pending OSC 52 clipboard copy, if any.
    pub fn take_clipboard(&mut self) -> Option<String> {
        self.clipboard.take()
    }

    /// Consume pending bell requests as a single flag.
    pub fn take_bell(&mut self) -> bool {
        std::mem::take(&mut self.bells) > 0
    }

    pub fn resize(&mut self, size: TerminalSize) {
        self.term.resize(Size(size));
        self.size = size;
    }

    pub fn scroll(&mut self, lines: i32) {
        self.term.scroll_display(Scroll::Delta(lines));
    }
    pub fn scroll_to_bottom(&mut self) {
        self.term.scroll_display(Scroll::Bottom);
    }

    pub fn select(&mut self, start: (usize, usize), end: (usize, usize)) {
        let offset = self.term.grid().display_offset() as i32;
        let convert = |(column, row): (usize, usize)| {
            Point::new(
                Line(row.min(self.size.rows as usize - 1) as i32 - offset),
                Column(column.min(self.size.columns as usize - 1)),
            )
        };
        let mut selection = Selection::new(SelectionType::Simple, convert(start), Side::Left);
        selection.update(convert(end), Side::Right);
        self.term.selection = Some(selection);
    }
    pub fn clear_selection(&mut self) {
        self.term.selection = None;
    }
    pub fn selection_text(&self) -> Option<String> {
        self.term.selection_to_string()
    }

    /// Live bottom screen, independent of the viewport's scroll position.
    pub fn bottom_text(&self) -> String {
        let mut lines = Vec::new();
        for row in 0..self.size.rows as i32 {
            let mut text = String::new();
            for column in 0..self.size.columns as usize {
                let cell = &self.term.grid()[Point::new(Line(row), Column(column))];
                if cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }
                text.push(cell.c);
                if let Some(chars) = cell.zerowidth() {
                    text.extend(chars);
                }
            }
            lines.push(text.trim_end().to_string());
        }
        lines.join("\n")
    }

    pub fn snapshot(&self) -> Snapshot {
        let content = self.term.renderable_content();
        let mut cells = Vec::with_capacity(self.size.rows as usize * self.size.columns as usize);
        for indexed in content.display_iter {
            let cell = indexed.cell;
            let row = indexed.point.line.0 + content.display_offset as i32;
            if row < 0 || row >= self.size.rows as i32 {
                continue;
            }
            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            let mut text = cell.c.to_string();
            if let Some(chars) = cell.zerowidth() {
                text.extend(chars);
            }
            let mut foreground = resolve_color(cell.fg, content.colors);
            let mut background = resolve_color(cell.bg, content.colors);
            if cell.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut foreground, &mut background);
            }
            if cell.flags.contains(Flags::DIM) {
                foreground = (foreground & 0xfefefe) >> 1;
            }
            if cell.flags.contains(Flags::HIDDEN) {
                text.clear();
            }
            cells.push(Cell {
                text,
                column: indexed.point.column.0,
                row: row as usize,
                width: if cell.flags.contains(Flags::WIDE_CHAR) {
                    2
                } else {
                    1
                },
                foreground,
                background,
                bold: cell.flags.contains(Flags::BOLD),
                italic: cell.flags.contains(Flags::ITALIC),
                underline: underline_kind(&cell.flags),
                strikeout: cell.flags.contains(Flags::STRIKEOUT),
                selected: content
                    .selection
                    .is_some_and(|selection| selection.contains(indexed.point)),
            });
        }
        let cursor_row = content.cursor.point.line.0 + content.display_offset as i32;
        let cursor = (content.cursor.shape != CursorShape::Hidden
            && cursor_row >= 0
            && cursor_row < self.size.rows as i32)
            .then_some(Cursor {
                column: content.cursor.point.column.0,
                row: cursor_row as usize,
                style: match content.cursor.shape {
                    CursorShape::Underline => CursorStyle::Underline,
                    CursorShape::Beam => CursorStyle::Beam,
                    CursorShape::HollowBlock => CursorStyle::HollowBlock,
                    _ => CursorStyle::Block,
                },
            });
        Snapshot {
            size: self.size,
            cells,
            cursor,
            application_cursor: content.mode.contains(TermMode::APP_CURSOR),
            bracketed_paste: content.mode.contains(TermMode::BRACKETED_PASTE),
            focus_reporting: content.mode.contains(TermMode::FOCUS_IN_OUT),
            mouse: MouseMode {
                tracking: if content.mode.contains(TermMode::MOUSE_MOTION) {
                    Tracking::Motion
                } else if content.mode.contains(TermMode::MOUSE_DRAG) {
                    Tracking::Drag
                } else if content.mode.contains(TermMode::MOUSE_REPORT_CLICK) {
                    Tracking::Click
                } else {
                    Tracking::Off
                },
                encoding: if content.mode.contains(TermMode::SGR_MOUSE) {
                    Encoding::Sgr
                } else if content.mode.contains(TermMode::UTF8_MOUSE) {
                    Encoding::Utf8
                } else {
                    Encoding::Legacy
                },
            },
            display_offset: content.display_offset,
            title: self.title.clone(),
        }
    }
}

fn resolve_color(color: Color, colors: &Colors) -> u32 {
    match color {
        Color::Spec(rgb) => ((rgb.r as u32) << 16) | ((rgb.g as u32) << 8) | rgb.b as u32,
        Color::Indexed(index) => palette(index as usize, colors),
        Color::Named(index) => palette(index as usize, colors),
    }
}

fn palette(index: usize, colors: &Colors) -> u32 {
    if index < 269 {
        if let Some(rgb) = colors[index] {
            return ((rgb.r as u32) << 16) | ((rgb.g as u32) << 8) | rgb.b as u32;
        }
    }
    // Match the production DARK_TERMINAL_THEME in TerminalSurface.tsx.
    const ANSI: [u32; 16] = [
        0x24211d, 0xff7e68, 0x5ee6a8, 0xe6c56f, 0x56a8ff, 0xf778ff, 0x42d9ff, 0xe6e1d8, 0x928879,
        0xff9b89, 0x8ff0c4, 0xf2d98f, 0x8fc7ff, 0xffadff, 0x8be9ff, 0xfffaf0,
    ];
    match index {
        0..=15 => ANSI[index],
        16..=231 => {
            let n = index - 16;
            let component = |n| if n == 0 { 0 } else { 55 + n * 40 };
            ((component(n / 36) << 16) | (component(n / 6 % 6) << 8) | component(n % 6)) as u32
        }
        232..=255 => {
            let n = (8 + (index - 232) * 10) as u32;
            (n << 16) | (n << 8) | n
        }
        257 => 0x191816,
        258 => 0xc6a66b,
        259..=266 => (ANSI[index - 259] & 0xfefefe) >> 1,
        _ => 0xe6e1d8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn terminal() -> Terminal {
        Terminal::new(TerminalSize::new(20, 4).unwrap())
    }
    #[test]
    fn focus_reporting_tracks_enable_disable_and_reset() {
        let mut t = terminal();
        assert!(!t.snapshot().focus_reporting);
        t.feed(b"\x1b[?1004h");
        assert!(t.snapshot().focus_reporting);
        t.feed(b"\x1b[?1004l");
        assert!(!t.snapshot().focus_reporting);
        t.feed(b"\x1b[?1004h\x1bc");
        assert!(!t.snapshot().focus_reporting);
    }
    #[test]
    fn mouse_modes_follow_application_requests_and_reset() {
        let mut t = terminal();
        assert_eq!(t.snapshot().mouse, MouseMode::default());
        for (sequence, tracking) in [
            (1000, Tracking::Click),
            (1002, Tracking::Drag),
            (1003, Tracking::Motion),
        ] {
            t.feed(format!("\x1b[?{sequence}h").as_bytes());
            assert_eq!(t.snapshot().mouse.tracking, tracking);
            t.feed(format!("\x1b[?{sequence}l").as_bytes());
            assert!(!t.snapshot().mouse.enabled());
        }
        t.feed(b"\x1b[?1002h\x1b[?1005h");
        assert_eq!(t.snapshot().mouse.encoding, Encoding::Utf8);
        t.feed(b"\x1b[?1006h");
        assert_eq!(t.snapshot().mouse.encoding, Encoding::Sgr);
        t.feed(b"\x1b[?1006l");
        assert_eq!(t.snapshot().mouse.encoding, Encoding::Legacy);
        t.feed(b"\x1bc");
        assert_eq!(t.snapshot().mouse, MouseMode::default());
    }
    #[test]
    fn fragmented_unicode_color_and_cursor_queries() {
        let mut t = terminal();
        let bytes = "\x1b[31m日本e\u{301}🙂".as_bytes();
        for byte in bytes {
            t.feed(&[*byte]);
        }
        let screen = t.snapshot();
        assert_eq!(screen.cells[0].text, "日");
        assert_eq!(screen.cells[0].width, 2);
        assert_eq!(screen.cells[0].foreground, 0xff7e68);
        assert!(screen.cells.iter().any(|cell| cell.text == "e\u{301}"));
        assert_eq!(t.feed(b"\x1b[6n"), vec![b"\x1b[1;8R".to_vec()]);
    }
    #[test]
    fn changing_scrollback_trims_history_and_preserves_visible_text() {
        let mut terminal = Terminal::new(TerminalSize::new(80, 4).unwrap());
        terminal.set_scrollback(2500);
        for i in 0..1500 {
            terminal.feed(format!("line-{i}\r\n").as_bytes());
        }
        let before = terminal.bottom_text();
        terminal.scroll(10000);
        assert!(terminal.snapshot().display_offset > 1000);
        terminal.set_scrollback(1000);
        assert!(terminal.snapshot().display_offset <= 1000);
        assert_eq!(terminal.bottom_text(), before);
    }
    #[test]
    fn alternate_screen_restores_content_and_paste_mode() {
        let mut t = terminal();
        t.feed(b"original\x1b[?1049h\x1b[2Jtemporary\x1b[?2004h");
        assert!(t.bottom_text().contains("temporary"));
        assert!(t.snapshot().bracketed_paste);
        t.feed(b"\x1b[?1049l");
        assert!(t.bottom_text().contains("original"));
    }
    #[test]
    fn bottom_snapshot_ignores_user_scrollback() {
        let mut t = terminal();
        for n in 0..30 {
            t.feed(format!("line {n}\r\n").as_bytes());
        }
        let bottom = t.bottom_text();
        t.scroll(15);
        assert!(t.snapshot().display_offset > 0);
        assert_eq!(t.bottom_text(), bottom);
        t.resize(TerminalSize::new(10, 6).unwrap());
        assert_eq!(t.snapshot().size.rows, 6);
    }
    #[test]
    fn selection_respects_wide_cells() {
        let mut t = terminal();
        t.feed("a日本b".as_bytes());
        t.select((1, 0), (4, 0));
        assert_eq!(t.selection_text().as_deref(), Some("日本"));
    }
    #[test]
    fn split_escape_sequences_and_wrapping_are_lossless() {
        let mut t = Terminal::new(TerminalSize::new(3, 2).unwrap());
        t.feed(b"\x1b[");
        t.feed(b"32mabcdefg");
        assert_eq!(t.bottom_text(), "def\ng");
        assert_eq!(t.snapshot().cells[0].foreground, 0x5ee6a8);
    }
    #[test]
    fn osc52_copy_reaches_the_clipboard_and_paste_stays_rejected() {
        let mut t = terminal();
        t.feed(b"\x1b]52;c;aGVsbG8=\x07");
        assert_eq!(t.take_clipboard().as_deref(), Some("hello"));
        assert_eq!(t.take_clipboard(), None);
    }
    #[test]
    fn cursor_style_follows_decscusr() {
        let mut t = terminal();
        t.feed(b"\x1b[6 q");
        assert_eq!(
            t.snapshot().cursor.map(|cursor| cursor.style),
            Some(CursorStyle::Beam)
        );
        t.feed(b"\x1b[4 q");
        assert_eq!(
            t.snapshot().cursor.map(|cursor| cursor.style),
            Some(CursorStyle::Underline)
        );
        t.feed(b"\x1b[2 q");
        assert_eq!(
            t.snapshot().cursor.map(|cursor| cursor.style),
            Some(CursorStyle::Block)
        );
    }
    #[test]
    fn bells_coalesce_until_consumed() {
        let mut t = terminal();
        t.feed(b"\x07\x07");
        assert!(t.take_bell());
        assert!(!t.take_bell());
    }
    #[test]
    fn titles_set_reset_and_cap_length() {
        let mut t = terminal();
        t.feed(b"\x1b[22;0t"); // Push the default (no title).
        t.feed(b"\x1b]2;vintage\x07");
        assert_eq!(t.title(), Some("vintage"));
        assert_eq!(t.snapshot().title.as_deref(), Some("vintage"));
        t.feed(b"\x1b[23;0t"); // Pop restores the default.
        assert_eq!(t.title(), None);
        assert_eq!(t.snapshot().title, None);
        let long = "x".repeat(500);
        t.feed(format!("\x1b]2;{long}\x07").as_bytes());
        assert_eq!(t.title().map(str::len), Some(200));
    }
    #[test]
    fn underline_styles_map_to_kinds_and_reset() {
        let mut t = terminal();
        t.feed(b"a\x1b[4mb\x1b[4:2mc\x1b[4:3md\x1b[4:4me\x1b[4:5mf\x1b[24mg");
        let cells = t.snapshot().cells;
        let underline_at = |column: usize| {
            cells
                .iter()
                .find(|cell| cell.column == column && cell.row == 0)
                .and_then(|cell| cell.underline)
        };
        assert_eq!(underline_at(0), None);
        assert_eq!(underline_at(1), Some(UnderlineKind::Single));
        assert_eq!(underline_at(2), Some(UnderlineKind::Double));
        assert_eq!(underline_at(3), Some(UnderlineKind::Curly));
        assert_eq!(underline_at(4), Some(UnderlineKind::Dotted));
        assert_eq!(underline_at(5), Some(UnderlineKind::Dashed));
        assert_eq!(underline_at(6), None);
    }
}
