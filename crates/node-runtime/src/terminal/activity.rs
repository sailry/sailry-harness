// Adapted from sailry-code 67ae9fa0, sailry-terminal-activity/src/lib.rs.
// See third_party_licenses/sailry-code-terminal.md.
use sailry_protocol::terminal::Activity;
use vte::{Parser, Perform};

#[derive(Default)]
pub(super) struct Tracker {
    parser: Parser,
    activity: Option<Activity>,
}

impl Tracker {
    pub fn update(&mut self, bytes: &[u8]) -> Option<Activity> {
        let before = self.activity;
        for part in bytes.split_inclusive(|byte| matches!(*byte, 0x18 | 0x1a)) {
            if let Some((&last, body)) = part.split_last()
                && matches!(last, 0x18 | 0x1a)
            {
                // vte dispatches unfinished OSC on CAN/SUB. Cancellation must
                // discard that partial report without changing product activity.
                self.parser.advance(&mut Report(&mut self.activity), body);
                self.parser = Parser::default();
            } else {
                self.parser.advance(&mut Report(&mut self.activity), part);
            }
        }
        (before != self.activity).then_some(self.activity).flatten()
    }
}

struct Report<'a>(&'a mut Option<Activity>);

impl Perform for Report<'_> {
    fn osc_dispatch(&mut self, params: &[&[u8]], _: bool) {
        let next = match params {
            [b"9", b"4", state, ..] => match *state {
                b"0" => Activity::Idle,
                b"1" | b"3" => Activity::Working,
                b"2" => Activity::Failed,
                b"4" => Activity::WaitingForInput,
                _ => return,
            },
            // Semantic prompt markers describe command lifetime, not application
            // progress. An interactive command can spend that lifetime awaiting
            // input. Clear stale progress at shell boundaries without inventing work.
            [b"133" | b"633", b"A" | b"B" | b"C" | b"D", ..] => Activity::Idle,
            _ => return,
        };
        *self.0 = Some(next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tracks_fragmented_progress() {
        let mut tracker = Tracker::default();
        assert_eq!(tracker.update(b"prompt text"), None);
        assert_eq!(tracker.update(b"\x1b]9;"), None);
        assert_eq!(tracker.update(b"4;3\x07"), Some(Activity::Working));
        assert_eq!(
            tracker.update(b"\x1b]9;4;4\x1b\\"),
            Some(Activity::WaitingForInput)
        );
        assert_eq!(
            tracker.update(b"\x1b]9;4;1;50\x07"),
            Some(Activity::Working)
        );
        assert_eq!(tracker.update(b"\x1b]9;4;0\x07"), Some(Activity::Idle));
        assert_eq!(tracker.update(b"\x1b]9;4;2\x07"), Some(Activity::Failed));
        assert_eq!(tracker.update(b"\x1b]9;4;0\x07"), Some(Activity::Idle));
    }
    #[test]
    fn excludes_shell_markers() {
        let mut tracker = Tracker::default();
        assert_eq!(tracker.update(b"\x1b]133;C\x07"), Some(Activity::Idle));
        assert_eq!(tracker.update(b"waiting for a task"), None);
        assert_eq!(tracker.update(b"\x1b]133;D;1\x07"), None);
        assert_eq!(tracker.update(b"\x1b]633;C\x1b\\"), None);
        assert_eq!(tracker.update(b"\x1b]633;D;0\x1b\\"), None);
        assert_eq!(tracker.update(b"\x1b]9;4;3\x07"), Some(Activity::Working));
        assert_eq!(tracker.update(b"\x1b]133;A\x07"), Some(Activity::Idle));
        assert_eq!(
            tracker.update(b"\x1b]9;4;4\x07"),
            Some(Activity::WaitingForInput)
        );
        assert_eq!(tracker.update(b"\x1b]633;B\x07"), Some(Activity::Idle));
    }
    #[test]
    fn ignores_unrelated_and_cancelled_sequences() {
        let mut tracker = Tracker::default();
        assert_eq!(
            tracker.update(b"9;4;3\x1b]2;9;4;3\x07\x1b]9;4;3\x18\x07"),
            None
        );
        assert_eq!(tracker.update(b"\x1b]9;4;garbage\x07"), None);
        assert_eq!(tracker.update(b"\x1b]9;4;0\x07"), Some(Activity::Idle));
    }
}
