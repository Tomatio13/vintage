//! IME composition is local text; only an explicit commit reaches the PTY.
use crate::MAX_INPUT_BYTES;
use std::ops::Range;

#[derive(Default)]
pub struct Composition {
    text: String,
    selection: Range<usize>,
}

impl Composition {
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn selection(&self) -> Range<usize> {
        self.selection.clone()
    }
    pub fn marked_range(&self) -> Option<Range<usize>> {
        (!self.text.is_empty()).then(|| 0..self.text.encode_utf16().count())
    }
    pub fn clear(&mut self) {
        self.text.clear();
        self.selection = 0..0;
    }

    /// Return None for an out-of-range offset or the middle of a surrogate pair.
    pub fn byte_offset(&self, utf16: usize) -> Option<usize> {
        let mut offset = 0;
        for (byte, character) in self.text.char_indices() {
            if offset == utf16 {
                return Some(byte);
            }
            offset += character.len_utf16();
        }
        (offset == utf16).then_some(self.text.len())
    }
    pub fn text_for_range(&self, range: Range<usize>) -> Option<String> {
        if range.start > range.end {
            return None;
        }
        Some(self.text[self.byte_offset(range.start)?..self.byte_offset(range.end)?].to_string())
    }

    pub fn replace(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
    ) -> Result<(), &'static str> {
        let range = range.unwrap_or(0..self.text.encode_utf16().count());
        if range.start > range.end {
            return Err("Invalid IME replacement range");
        }
        let start = self
            .byte_offset(range.start)
            .ok_or("Invalid IME replacement start")?;
        let end = self
            .byte_offset(range.end)
            .ok_or("Invalid IME replacement end")?;
        let length = self.text.len() - (end - start);
        if text.len() > MAX_INPUT_BYTES.saturating_sub(length) {
            return Err("IME text exceeds the input size limit");
        }
        let count = text.encode_utf16().count();
        let selected = selected.unwrap_or(count..count);
        if selected.start > selected.end || selected.end > count {
            return Err("Invalid IME selection");
        }
        // Also reject selections inside a UTF-16 surrogate pair.
        let replacement = Self {
            text: text.into(),
            selection: 0..0,
        };
        if replacement.byte_offset(selected.start).is_none()
            || replacement.byte_offset(selected.end).is_none()
        {
            return Err("Invalid IME selection boundary");
        }
        self.text.replace_range(start..end, text);
        self.selection = range.start + selected.start..range.start + selected.end;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preedit_replacement_and_utf16_selection() {
        let mut composition = Composition::default();
        composition.replace(None, "に🙂ご", Some(1..3)).unwrap();
        assert_eq!(composition.selection(), 1..3);
        assert_eq!(composition.text_for_range(1..3).as_deref(), Some("🙂"));
        composition.replace(Some(1..3), "ほん", Some(2..2)).unwrap();
        assert_eq!(composition.text(), "にほんご");
        assert_eq!(composition.selection(), 3..3);
        composition.replace(None, "日本語", None).unwrap();
        assert_eq!(composition.text().as_bytes(), "日本語".as_bytes());
        composition.clear();
        assert_eq!(composition.marked_range(), None);
        assert_eq!(composition.selection(), 0..0);
    }
    #[test]
    fn invalid_or_oversized_composition_does_not_mutate_text() {
        let mut composition = Composition::default();
        composition.replace(None, "🙂", None).unwrap();
        assert!(composition.replace(Some(1..2), "x", None).is_err());
        assert!(composition.replace(None, "x", Some(0..usize::MAX)).is_err());
        assert!(composition
            .replace(None, &"x".repeat(MAX_INPUT_BYTES + 1), None)
            .is_err());
        assert_eq!(composition.text(), "🙂");
    }
}
