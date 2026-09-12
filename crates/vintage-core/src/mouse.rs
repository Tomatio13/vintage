//! Xterm cell-coordinate mouse protocols, independent of windowing.
//! Reference: https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h2-Mouse-Tracking
use crate::Modifiers;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tracking {
    #[default]
    Off,
    Click,
    Drag,
    Motion,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Encoding {
    #[default]
    Legacy,
    Utf8,
    Sgr,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MouseMode {
    pub tracking: Tracking,
    pub encoding: Encoding,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Middle,
    Right,
}
impl Button {
    pub fn index(self) -> usize {
        match self {
            Self::Left => 0,
            Self::Middle => 1,
            Self::Right => 2,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum MouseAction {
    Press(Button),
    Release(Button),
    Move(Option<Button>),
    Wheel { up: bool },
}

impl MouseMode {
    pub fn enabled(self) -> bool {
        self.tracking != Tracking::Off
    }

    /// Zero-based cells. Unrepresentable coordinates are dropped, never wrapped.
    /// The caller owns Shift-selection overrides and duplicate motion filtering.
    pub fn encode(
        self,
        action: MouseAction,
        (column, row): (usize, usize),
        modifiers: Modifiers,
    ) -> Option<Vec<u8>> {
        if !self.enabled() {
            return None;
        }
        let released = matches!(action, MouseAction::Release(_));
        let button = match action {
            MouseAction::Press(button) | MouseAction::Release(button) => button.index() as u8,
            MouseAction::Move(button) => {
                if self.tracking != Tracking::Motion
                    && !(self.tracking == Tracking::Drag && button.is_some())
                {
                    return None;
                }
                32 + button.map_or(3, |button| button.index() as u8)
            }
            MouseAction::Wheel { up } => {
                if up {
                    64
                } else {
                    65
                }
            }
        };
        let modifiers = 4 * u8::from(modifiers.shift)
            + 8 * u8::from(modifiers.alt)
            + 16 * u8::from(modifiers.control);
        let code = if released && self.encoding != Encoding::Sgr {
            3
        } else {
            button
        } + modifiers;
        match self.encoding {
            Encoding::Sgr => Some(
                format!(
                    "\x1b[<{code};{};{}{}",
                    column.checked_add(1)?,
                    row.checked_add(1)?,
                    if released { 'm' } else { 'M' }
                )
                .into_bytes(),
            ),
            Encoding::Legacy if column < 223 && row < 223 => Some(vec![
                27,
                b'[',
                b'M',
                32 + code,
                33 + column as u8,
                33 + row as u8,
            ]),
            Encoding::Utf8 if column < 2015 && row < 2015 => {
                let mut text = String::from("\x1b[M");
                for value in [32 + code as u32, 33 + column as u32, 33 + row as u32] {
                    text.push(char::from_u32(value)?);
                }
                Some(text.into_bytes())
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn mode(tracking: Tracking, encoding: Encoding) -> MouseMode {
        MouseMode { tracking, encoding }
    }
    #[test]
    fn sgr_buttons_release_and_modifiers() {
        let mode = mode(Tracking::Click, Encoding::Sgr);
        for (button, code) in [(Button::Left, 0), (Button::Middle, 1), (Button::Right, 2)] {
            for (action, suffix) in [
                (MouseAction::Press(button), 'M'),
                (MouseAction::Release(button), 'm'),
            ] {
                assert_eq!(
                    mode.encode(
                        action,
                        (499, 299),
                        Modifiers {
                            control: true,
                            alt: true,
                            shift: false
                        }
                    ),
                    Some(format!("\x1b[<{};500;300{suffix}", code + 24).into_bytes())
                );
            }
        }
    }
    #[test]
    fn motion_requires_requested_tracking() {
        for tracking in [
            Tracking::Off,
            Tracking::Click,
            Tracking::Drag,
            Tracking::Motion,
        ] {
            let mode = mode(tracking, Encoding::Sgr);
            assert_eq!(
                mode.encode(
                    MouseAction::Move(Some(Button::Left)),
                    (1, 2),
                    Modifiers::default()
                ),
                matches!(tracking, Tracking::Drag | Tracking::Motion)
                    .then(|| b"\x1b[<32;2;3M".to_vec())
            );
            assert_eq!(
                mode.encode(MouseAction::Move(None), (1, 2), Modifiers::default()),
                (tracking == Tracking::Motion).then(|| b"\x1b[<35;2;3M".to_vec())
            );
        }
        assert!(MouseMode::default()
            .encode(
                MouseAction::Press(Button::Left),
                (0, 0),
                Modifiers::default()
            )
            .is_none());
    }
    #[test]
    fn legacy_release_keeps_modifiers_and_coordinates_do_not_wrap() {
        let mode = mode(Tracking::Click, Encoding::Legacy);
        assert_eq!(
            mode.encode(
                MouseAction::Release(Button::Right),
                (222, 222),
                Modifiers {
                    control: true,
                    ..Default::default()
                }
            ),
            Some(vec![27, b'[', b'M', 51, 255, 255])
        );
        assert!(mode
            .encode(
                MouseAction::Press(Button::Left),
                (223, 0),
                Modifiers::default()
            )
            .is_none());
        assert!(mode
            .encode(
                MouseAction::Press(Button::Left),
                (0, usize::MAX),
                Modifiers::default()
            )
            .is_none());
    }
    #[test]
    fn utf8_coordinates_and_wheel() {
        let mode = mode(Tracking::Click, Encoding::Utf8);
        assert_eq!(
            mode.encode(
                MouseAction::Wheel { up: true },
                (300, 0),
                Modifiers::default()
            ),
            Some("\x1b[M`ō!".as_bytes().to_vec())
        );
        assert_eq!(
            mode.encode(
                MouseAction::Wheel { up: false },
                (0, 0),
                Modifiers::default()
            ),
            Some(b"\x1b[Ma!!".to_vec())
        );
        assert!(mode
            .encode(
                MouseAction::Press(Button::Left),
                (2014, 2014),
                Modifiers::default()
            )
            .is_some());
        assert!(mode
            .encode(
                MouseAction::Press(Button::Left),
                (0, 2015),
                Modifiers::default()
            )
            .is_none());
        assert!(MouseMode {
            tracking: Tracking::Click,
            encoding: Encoding::Sgr
        }
        .encode(
            MouseAction::Press(Button::Left),
            (usize::MAX, 0),
            Modifiers::default()
        )
        .is_none());
    }
}
