//! Retain shaped text for the current visible cells only. Backgrounds, selection
//! and cursor positions are painted separately and do not invalidate text.
use vintage_terminal::Cell;

struct Entry<T> {
    text: String,
    foreground: u32,
    bold: bool,
    italic: bool,
    underline: bool,
    strikeout: bool,
    layout: T,
}

impl<T> Entry<T> {
    fn matches(&self, cell: &Cell) -> bool {
        self.text == cell.text
            && self.foreground == cell.foreground
            && self.bold == cell.bold
            && self.italic == cell.italic
            && self.underline == cell.underline
            && self.strikeout == cell.strikeout
    }
}

pub struct TextCache<T> {
    entries: Vec<Entry<T>>,
    next: usize,
    metrics: Option<(u32, u32)>,
}

impl<T> Default for TextCache<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            next: 0,
            metrics: None,
        }
    }
}

impl<T: Clone> TextCache<T> {
    pub fn begin_frame(&mut self, font_size: f32, scale_factor: f32) {
        let metrics = (font_size.to_bits(), scale_factor.to_bits());
        if self.metrics != Some(metrics) {
            self.entries.clear();
            self.metrics = Some(metrics);
        }
        self.next = 0;
    }

    pub fn layout(&mut self, cell: &Cell, shape: impl FnOnce() -> T) -> T {
        let index = self.next;
        self.next += 1;
        if let Some(entry) = self.entries.get(index) {
            if entry.matches(cell) {
                return entry.layout.clone();
            }
        }
        let layout = shape();
        let entry = Entry {
            text: cell.text.clone(),
            foreground: cell.foreground,
            bold: cell.bold,
            italic: cell.italic,
            underline: cell.underline,
            strikeout: cell.strikeout,
            layout: layout.clone(),
        };
        if index == self.entries.len() {
            self.entries.push(entry);
        } else {
            self.entries[index] = entry;
        }
        layout
    }

    pub fn end_frame(&mut self) {
        // Do not keep text that has scrolled offscreen, or accumulate history.
        self.entries.truncate(self.next);
        if self.entries.capacity() > self.next.saturating_mul(2).max(64) {
            self.entries.shrink_to_fit();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell as Counter;
    use vintage_core::TerminalSize;
    use vintage_terminal::Terminal;

    fn cell() -> Cell {
        let mut terminal = Terminal::new(TerminalSize::new(80, 24).unwrap());
        terminal.feed(b"A");
        terminal.snapshot().cells.remove(0)
    }

    #[test]
    fn unchanged_screen_avoids_repeated_layout_calls() {
        let cell = cell();
        let calls = Counter::new(0);
        let mut cache = TextCache::default();
        for _ in 0..10 {
            cache.begin_frame(12., 1.);
            for _ in 0..80 * 24 {
                cache.layout(&cell, || calls.set(calls.get() + 1));
            }
            cache.end_frame();
        }
        assert_eq!(calls.get(), 80 * 24); // One screen, not ten screens.
    }

    #[test]
    fn selection_background_and_position_do_not_invalidate_text() {
        let mut cell = cell();
        let mut cache = TextCache::default();
        cache.begin_frame(12., 1.);
        cache.layout(&cell, || 7);
        cache.end_frame();
        cell.selected = true;
        cell.background = 0x123456;
        cell.row = 2;
        cell.column = 3;
        cache.begin_frame(12., 1.);
        assert_eq!(cache.layout(&cell, || panic!("unnecessary layout")), 7);
        cache.end_frame();
    }

    #[test]
    fn text_and_each_style_change_invalidates_layout() {
        let original = cell();
        let mut variants = vec![original.clone(); 6];
        variants[0].text = "日\u{301}".into();
        variants[1].foreground ^= 1;
        variants[2].bold = true;
        variants[3].italic = true;
        variants[4].underline = true;
        variants[5].strikeout = true;
        for changed in variants {
            let mut cache = TextCache::default();
            cache.begin_frame(12., 1.);
            cache.layout(&original, || 1);
            cache.end_frame();
            cache.begin_frame(12., 1.);
            assert_eq!(cache.layout(&changed, || 2), 2);
            cache.end_frame();
        }
    }

    #[test]
    fn font_size_scale_and_removed_cells_release_old_layouts() {
        use std::rc::Rc;
        let cell = cell();
        let mut cache = TextCache::default();
        let layout = Rc::new(());
        for metrics in [(12., 1.), (14., 1.), (14., 2.)] {
            cache.begin_frame(metrics.0, metrics.1);
            assert_eq!(Rc::strong_count(&layout), 1);
            cache.layout(&cell, || layout.clone());
            cache.end_frame();
            assert_eq!(Rc::strong_count(&layout), 2);
        }
        cache.begin_frame(14., 2.);
        cache.end_frame();
        assert_eq!(Rc::strong_count(&layout), 1);
        assert!(cache.entries.is_empty());
    }
}
