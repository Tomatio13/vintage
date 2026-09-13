//! Retain shaped text runs for the current visible frame only. Runs cover
//! whole stretches of same-style cells, so rows shape once instead of once
//! per cell. Backgrounds, selection and positions are painted separately and
//! do not invalidate text.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StyleKey {
    pub foreground: u32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
}

struct Entry<T> {
    text: String,
    style: StyleKey,
    layout: T,
}

impl<T> Entry<T> {
    fn matches(&self, text: &str, style: &StyleKey) -> bool {
        self.text == text && self.style == *style
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

    pub fn layout(&mut self, text: &str, style: StyleKey, shape: impl FnOnce() -> T) -> T {
        let index = self.next;
        self.next += 1;
        if let Some(entry) = self.entries.get(index) {
            if entry.matches(text, &style) {
                return entry.layout.clone();
            }
        }
        let layout = shape();
        let entry = Entry {
            text: text.to_owned(),
            style,
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

    fn style() -> StyleKey {
        StyleKey {
            foreground: 0xe6e1d8,
            bold: false,
            italic: false,
            underline: false,
            strikeout: false,
        }
    }

    #[test]
    fn unchanged_screen_avoids_repeated_layout_calls() {
        let calls = Counter::new(0);
        let mut cache = TextCache::default();
        for _ in 0..10 {
            cache.begin_frame(12., 1.);
            for _ in 0..80 {
                cache.layout("row", style(), || calls.set(calls.get() + 1));
            }
            cache.end_frame();
        }
        assert_eq!(calls.get(), 80); // One screen, not ten screens.
    }

    #[test]
    fn positions_and_unrelated_cells_do_not_invalidate_layout() {
        let mut cache = TextCache::default();
        cache.begin_frame(12., 1.);
        cache.layout("first", style(), || 7);
        cache.layout("second", style(), || 8);
        cache.end_frame();
        cache.begin_frame(12., 1.);
        // Layout order stays positional: the same text in the same slot hits.
        assert_eq!(
            cache.layout("first", style(), || panic!("unnecessary layout")),
            7
        );
        assert_eq!(
            cache.layout("second", style(), || panic!("unnecessary layout")),
            8
        );
        cache.end_frame();
    }

    #[test]
    fn text_and_each_style_change_invalidates_layout() {
        let mut variants = vec![style(); 5];
        variants[0].foreground ^= 1;
        variants[1].bold = true;
        variants[2].italic = true;
        variants[3].underline = true;
        variants[4].strikeout = true;
        for changed in variants {
            let mut cache = TextCache::default();
            cache.begin_frame(12., 1.);
            cache.layout("text", style(), || 1);
            cache.end_frame();
            cache.begin_frame(12., 1.);
            assert_eq!(cache.layout("text", changed, || 2), 2);
            cache.end_frame();
        }
        let mut cache = TextCache::default();
        cache.begin_frame(12., 1.);
        cache.layout("text", style(), || 1);
        cache.end_frame();
        cache.begin_frame(12., 1.);
        assert_eq!(cache.layout("other", style(), || 3), 3);
        cache.end_frame();
    }

    #[test]
    fn font_size_scale_and_removed_runs_release_old_layouts() {
        use std::rc::Rc;
        let mut cache = TextCache::default();
        let layout = Rc::new(());
        for metrics in [(12., 1.), (14., 1.), (14., 2.)] {
            cache.begin_frame(metrics.0, metrics.1);
            assert_eq!(Rc::strong_count(&layout), 1);
            cache.layout("text", style(), || layout.clone());
            cache.end_frame();
            assert_eq!(Rc::strong_count(&layout), 2);
        }
        cache.begin_frame(14., 2.);
        cache.end_frame();
        assert_eq!(Rc::strong_count(&layout), 1);
        assert!(cache.entries.is_empty());
    }
}
