//! The clock behind "Played 12 h": one running session at a time, counted from the moment the game's
//! window showed. Seconds are handed out in parts while it runs ([`Sessions::take`], once per
//! [`CHECKPOINT`]) and the rest at its end, so a crash or a closed launcher keeps what was seen.
//! Times are explicit `Instant`s, so the rules are tested without waiting.

use std::time::{Duration, Instant};

use anyps5_core::paths::GameId;

/// How often a running session hands over what it has counted.
pub const CHECKPOINT: Duration = Duration::from_secs(60);

/// Seconds counted for a game.
pub type Counted = (GameId, u64);

#[derive(Debug)]
struct Session {
    game: GameId,
    /// Everything before this has been handed out.
    counted_until: Instant,
}

/// The session of the running game, if any.
#[derive(Debug, Default)]
pub struct Sessions {
    current: Option<Session>,
}

impl Sessions {
    /// The game's window showed. A session still open (it should not be) is closed first and its
    /// seconds returned.
    pub fn begin(&mut self, game: GameId, at: Instant) -> Option<Counted> {
        let left = self.take(at);
        self.current = Some(Session {
            game,
            counted_until: at,
        });
        left
    }

    /// The whole seconds since the last call. The fraction stays, so nothing is lost in the long run.
    pub fn take(&mut self, at: Instant) -> Option<Counted> {
        let session = self.current.as_mut()?;
        let seconds = at
            .saturating_duration_since(session.counted_until)
            .as_secs();
        session.counted_until += Duration::from_secs(seconds);
        Some((session.game.clone(), seconds))
    }

    /// A session has gone [`CHECKPOINT`] without handing anything over.
    pub fn checkpoint_due(&self, at: Instant) -> bool {
        self.current
            .as_ref()
            .is_some_and(|s| at.saturating_duration_since(s.counted_until) >= CHECKPOINT)
    }

    /// The game exited: what it still has, and the session is over. Another game's exit changes
    /// nothing.
    pub fn end(&mut self, game: &GameId, at: Instant) -> Option<Counted> {
        if self.current.as_ref()?.game != *game {
            return None;
        }
        let left = self.take(at);
        self.current = None;
        left
    }

    pub fn running(&self) -> Option<&GameId> {
        self.current.as_ref().map(|s| &s.game)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(name: &str) -> GameId {
        GameId::from_key(name)
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn a_session_counts_from_its_window_to_its_exit() {
        let t0 = Instant::now();
        let mut sessions = Sessions::default();
        assert_eq!(sessions.begin(id("a"), t0), None);
        assert_eq!(sessions.running(), Some(&id("a")));
        assert_eq!(sessions.end(&id("a"), t0 + secs(754)), Some((id("a"), 754)));
        assert_eq!(sessions.running(), None);
        // Nothing runs, nothing is counted.
        assert_eq!(sessions.take(t0 + secs(900)), None);
    }

    #[test]
    fn checkpoints_hand_over_in_parts_and_the_end_adds_the_rest() {
        let t0 = Instant::now();
        let mut sessions = Sessions::default();
        sessions.begin(id("a"), t0);
        assert!(!sessions.checkpoint_due(t0 + secs(59)));
        assert!(sessions.checkpoint_due(t0 + secs(60)));
        assert_eq!(sessions.take(t0 + secs(61)), Some((id("a"), 61)));
        assert!(!sessions.checkpoint_due(t0 + secs(100)));
        assert!(sessions.checkpoint_due(t0 + secs(121)));
        assert_eq!(sessions.take(t0 + secs(125)), Some((id("a"), 64)));
        assert_eq!(sessions.end(&id("a"), t0 + secs(130)), Some((id("a"), 5)));
    }

    #[test]
    fn fractions_of_a_second_are_not_lost_between_calls() {
        let t0 = Instant::now();
        let mut sessions = Sessions::default();
        sessions.begin(id("a"), t0);
        let mut total = 0;
        for step in 1..=10u32 {
            // 10 x 0.4 s = 4 s.
            let (_, n) = sessions
                .take(t0 + Duration::from_millis(400) * step)
                .unwrap();
            total += n;
        }
        assert_eq!(total, 4);
    }

    #[test]
    fn another_games_exit_leaves_the_session_alone() {
        let t0 = Instant::now();
        let mut sessions = Sessions::default();
        sessions.begin(id("a"), t0);
        assert_eq!(sessions.end(&id("b"), t0 + secs(30)), None);
        assert_eq!(sessions.running(), Some(&id("a")));
    }

    #[test]
    fn a_new_window_closes_a_session_left_open() {
        let t0 = Instant::now();
        let mut sessions = Sessions::default();
        sessions.begin(id("a"), t0);
        assert_eq!(sessions.begin(id("b"), t0 + secs(20)), Some((id("a"), 20)));
        assert_eq!(sessions.running(), Some(&id("b")));
    }

    #[test]
    fn a_clock_that_goes_back_counts_nothing() {
        let t0 = Instant::now() + secs(100);
        let mut sessions = Sessions::default();
        sessions.begin(id("a"), t0);
        assert_eq!(sessions.take(t0 - secs(50)), Some((id("a"), 0)));
    }
}
