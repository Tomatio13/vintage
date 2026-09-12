//! Focus reporting requested by terminal applications with DECSET 1004.

#[derive(Default)]
pub struct FocusReporter {
    reported: Option<bool>,
}

impl FocusReporter {
    /// Synchronize on enable and suppress duplicate notifications. A failed send
    /// is not acknowledged, so the caller can retry the latest focus state.
    pub fn pending(&mut self, enabled: bool, focused: bool) -> Option<&'static [u8]> {
        if !enabled {
            self.reported = None;
            return None;
        }
        if self.reported == Some(focused) {
            None
        } else if focused {
            Some(b"\x1b[I")
        } else {
            Some(b"\x1b[O")
        }
    }

    /// Call only after the input queue accepts the corresponding notification.
    pub fn acknowledge(&mut self, focused: bool) {
        self.reported = Some(focused);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_only_when_requested_and_focus_changes() {
        let mut reporter = FocusReporter::default();
        assert_eq!(reporter.pending(false, true), None);
        assert_eq!(reporter.pending(true, true), Some(b"\x1b[I".as_slice()));
        reporter.acknowledge(true);
        assert_eq!(reporter.pending(true, true), None);
        assert_eq!(reporter.pending(true, false), Some(b"\x1b[O".as_slice()));
        reporter.acknowledge(false);
        assert_eq!(reporter.pending(true, false), None);
        assert_eq!(reporter.pending(true, true), Some(b"\x1b[I".as_slice()));
    }

    #[test]
    fn reenable_synchronizes_even_when_focus_did_not_change() {
        let mut reporter = FocusReporter::default();
        reporter.acknowledge(false);
        assert_eq!(reporter.pending(false, false), None);
        assert_eq!(reporter.pending(true, false), Some(b"\x1b[O".as_slice()));
    }

    #[test]
    fn failed_send_retries_current_state_without_stale_events() {
        let mut reporter = FocusReporter::default();
        assert_eq!(reporter.pending(true, false), Some(b"\x1b[O".as_slice()));
        assert_eq!(reporter.pending(true, false), Some(b"\x1b[O".as_slice()));
        assert_eq!(reporter.pending(true, true), Some(b"\x1b[I".as_slice()));
        reporter.acknowledge(true);
        // An unsent blur that was superseded by focus requires no notification.
        assert_eq!(reporter.pending(true, false), Some(b"\x1b[O".as_slice()));
        assert_eq!(reporter.pending(true, true), None);
    }
}
