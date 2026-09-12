//! GUI-independent contracts for the experimental native terminal.

pub mod composition;
pub mod document;
pub mod focus;
pub mod mouse;
pub mod workspace;

pub const MAX_INPUT_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalSize {
    pub columns: u16,
    pub rows: u16,
}

impl TerminalSize {
    pub fn new(columns: u16, rows: u16) -> Result<Self, &'static str> {
        if columns < 2 || rows < 2 || columns > 500 || rows > 500 {
            return Err("Terminal size must be within 2..500 columns and 2..500 rows");
        }
        Ok(Self { columns, rows })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionId {
    pub terminal: String,
    pub pane: String,
    pub generation: u64,
}

impl SessionId {
    pub fn new(terminal: &str, pane: &str, generation: u64) -> Result<Self, &'static str> {
        fn valid(value: &str) -> bool {
            !value.is_empty()
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
        }
        if !valid(terminal) || !valid(pane) || generation == 0 {
            return Err("Invalid terminal, pane or generation");
        }
        Ok(Self {
            terminal: terminal.into(),
            pane: pane.into(),
            generation,
        })
    }
}

#[derive(Clone, Copy, Default)]
pub struct Modifiers {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
}

/// Encode non-text keys. Text/IME commits are delivered separately.
pub fn encode_key(key: &str, modifiers: Modifiers, application_cursor: bool) -> Option<Vec<u8>> {
    let modifier = 1
        + u8::from(modifiers.shift)
        + 2 * u8::from(modifiers.alt)
        + 4 * u8::from(modifiers.control);
    let cursor = match key {
        "up" => Some('A'),
        "down" => Some('B'),
        "right" => Some('C'),
        "left" => Some('D'),
        "home" => Some('H'),
        "end" => Some('F'),
        _ => None,
    };
    if let Some(suffix) = cursor {
        return Some(
            if modifier != 1 {
                format!("\x1b[1;{modifier}{suffix}")
            } else if application_cursor {
                format!("\x1bO{suffix}")
            } else {
                format!("\x1b[{suffix}")
            }
            .into_bytes(),
        );
    }
    let numbered = match key {
        "insert" => Some(2),
        "delete" => Some(3),
        "pageup" => Some(5),
        "pagedown" => Some(6),
        "f5" => Some(15),
        "f6" => Some(17),
        "f7" => Some(18),
        "f8" => Some(19),
        "f9" => Some(20),
        "f10" => Some(21),
        "f11" => Some(23),
        "f12" => Some(24),
        _ => None,
    };
    if let Some(number) = numbered {
        return Some(
            if modifier == 1 {
                format!("\x1b[{number}~")
            } else {
                format!("\x1b[{number};{modifier}~")
            }
            .into_bytes(),
        );
    }
    if let Some(suffix) = match key {
        "f1" => Some('P'),
        "f2" => Some('Q'),
        "f3" => Some('R'),
        "f4" => Some('S'),
        _ => None,
    } {
        return Some(
            if modifier == 1 {
                format!("\x1bO{suffix}")
            } else {
                format!("\x1b[1;{modifier}{suffix}")
            }
            .into_bytes(),
        );
    }
    let mut bytes = match key {
        "enter" => vec![b'\r'],
        "tab" if modifiers.shift => b"\x1b[Z".to_vec(),
        "tab" => vec![b'\t'],
        "backspace" => vec![0x7f],
        "escape" => vec![0x1b],
        "space" if modifiers.control => vec![0],
        _ if modifiers.control && key.len() == 1 => {
            let c = key.as_bytes()[0].to_ascii_uppercase();
            if (b'@'..=b'_').contains(&c) {
                vec![c & 0x1f]
            } else if c == b'?' {
                vec![0x7f]
            } else {
                return None;
            }
        }
        _ if modifiers.alt && !modifiers.control && key.chars().count() == 1 => {
            key.as_bytes().to_vec()
        }
        _ => return None,
    };
    if modifiers.alt {
        bytes.insert(0, 0x1b);
    }
    Some(bytes)
}

pub fn encode_paste(text: &str, bracketed: bool) -> Result<Vec<u8>, &'static str> {
    if text.len() > MAX_INPUT_BYTES - 12 {
        return Err("Paste exceeds the input size limit");
    }
    // ESC stripping prevents clipboard text from terminating bracketed paste early.
    let text = text
        .replace('\x1b', "")
        .replace("\r\n", "\n")
        .replace('\n', "\r");
    Ok(if bracketed {
        format!("\x1b[200~{text}\x1b[201~")
    } else {
        text
    }
    .into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_invalid_session_and_dimensions() {
        assert!(SessionId::new("../bad", "p", 1).is_err());
        assert!(SessionId::new("t", "p", 0).is_err());
        assert!(TerminalSize::new(0, 24).is_err());
        assert!(TerminalSize::new(501, 24).is_err());
        assert!(TerminalSize::new(80, 24).is_ok());
    }
    #[test]
    fn text_is_reserved_for_ime_and_control_keys_are_encoded() {
        assert_eq!(encode_key("a", Modifiers::default(), false), None);
        assert_eq!(
            encode_key(
                "c",
                Modifiers {
                    control: true,
                    ..Default::default()
                },
                false
            ),
            Some(vec![3])
        );
        assert_eq!(
            encode_key("up", Modifiers::default(), true),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(
            encode_key(
                "left",
                Modifiers {
                    control: true,
                    ..Default::default()
                },
                false
            ),
            Some(b"\x1b[1;5D".to_vec())
        );
    }
    #[test]
    fn paste_cannot_inject_end_marker() {
        assert_eq!(
            encode_paste("a\r\nb\x1b[201~", true).unwrap(),
            b"\x1b[200~a\rb[201~\x1b[201~"
        );
        assert!(encode_paste(&"x".repeat(MAX_INPUT_BYTES), false).is_err());
    }
}
