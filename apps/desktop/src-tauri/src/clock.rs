//! Hard 900 s session clock (PRD §6). Elapsed time is the MAX of the monotonic clock and the wall
//! clock, so neither a backwards clock change nor a sleep that pauses the monotonic clock
//! (macOS) can ever extend a session.

use std::time::Instant;

pub const LIMIT_MS: i64 = 900_000;
/// Do not start a new model response with less than this left.
pub const MIN_RESPONSE_MS: i64 = 6_000;

#[derive(Debug, PartialEq)]
pub enum Tick {
    Running,
    Expired,
    /// Wall clock jumped well beyond the monotonic clock: the machine slept.
    Suspended,
    /// Wall clock went backwards: clock data no longer trustworthy.
    ClockChanged,
}

pub struct Clock {
    start_wall_ms: i64,
    mono_origin: Instant,
    mono_offset_ms: i64,
    last_wall_ms: i64,
    last_mono_ms: i64,
}

impl Clock {
    pub fn start(now_wall_ms: i64) -> Clock {
        Clock { start_wall_ms: now_wall_ms, mono_origin: Instant::now(), mono_offset_ms: 0, last_wall_ms: now_wall_ms, last_mono_ms: 0 }
    }

    /// Resume a persisted session after a crash/restart. None when it may not continue.
    pub fn recover(start_wall_ms: i64, last_heartbeat_ms: i64, now_wall_ms: i64) -> Option<Clock> {
        if now_wall_ms < last_heartbeat_ms || now_wall_ms < start_wall_ms {
            return None; // clock went backwards: unreliable
        }
        let elapsed = now_wall_ms - start_wall_ms;
        if elapsed >= LIMIT_MS {
            return None;
        }
        Some(Clock { start_wall_ms, mono_origin: Instant::now(), mono_offset_ms: elapsed, last_wall_ms: now_wall_ms, last_mono_ms: elapsed })
    }

    pub fn deadline_wall_ms(&self) -> i64 {
        self.start_wall_ms + LIMIT_MS
    }

    fn mono_ms(&self) -> i64 {
        self.mono_offset_ms + self.mono_origin.elapsed().as_millis() as i64
    }

    pub fn elapsed_ms(&self, now_wall_ms: i64) -> i64 {
        self.mono_ms().max(now_wall_ms - self.start_wall_ms)
    }

    pub fn remaining_ms(&self, now_wall_ms: i64) -> i64 {
        (LIMIT_MS - self.elapsed_ms(now_wall_ms)).max(0)
    }

    pub fn expired(&self, now_wall_ms: i64) -> bool {
        self.elapsed_ms(now_wall_ms) >= LIMIT_MS
    }

    /// Called by the watchdog (every ~50 ms).
    pub fn tick(&mut self, now_wall_ms: i64) -> Tick {
        self.tick_with_mono(now_wall_ms, self.mono_ms())
    }

    fn tick_with_mono(&mut self, now_wall_ms: i64, mono: i64) -> Tick {
        let wall_delta = now_wall_ms - self.last_wall_ms;
        let mono_delta = mono - self.last_mono_ms;
        self.last_wall_ms = now_wall_ms;
        self.last_mono_ms = mono;
        if mono.max(now_wall_ms - self.start_wall_ms) >= LIMIT_MS {
            Tick::Expired
        } else if wall_delta < -1_000 {
            Tick::ClockChanged
        } else if wall_delta - mono_delta > 3_000 || mono_delta > 3_000 {
            Tick::Suspended
        } else {
            Tick::Running
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wall_clock_forward_counts_monotonic_never_rewinds() {
        let c = Clock::start(1_000_000);
        assert!(!c.expired(1_000_000));
        assert!(c.expired(1_000_000 + LIMIT_MS)); // slept 15 min: wall time wins
        assert!(!c.expired(0)); // clock moved back: monotonic still holds, not reset
        assert!(c.remaining_ms(0) <= LIMIT_MS);
    }

    #[test]
    fn tick_detects_suspend_clock_change_and_expiry() {
        let mut c = Clock::start(0);
        assert_eq!(c.tick_with_mono(50, 50), Tick::Running);
        assert_eq!(c.tick_with_mono(60_050, 100), Tick::Suspended); // wall jumped, mono did not
        assert_eq!(c.tick_with_mono(50_000, 150), Tick::ClockChanged);
        assert_eq!(c.tick_with_mono(50_050, LIMIT_MS), Tick::Expired);
    }

    #[test]
    fn recovery_never_extends() {
        assert!(Clock::recover(0, 10_000, 5_000).is_none(), "wall clock went back");
        assert!(Clock::recover(0, 10_000, LIMIT_MS).is_none(), "deadline passed");
        let c = Clock::recover(0, 10_000, 20_000).unwrap();
        assert!(c.remaining_ms(20_000) <= LIMIT_MS - 20_000);
        assert_eq!(c.deadline_wall_ms(), LIMIT_MS);
    }

    /// AC-TIME style sweep: across many start/stop offsets nothing is "live" at or past 900 s.
    #[test]
    fn hundred_runs_never_live_after_deadline() {
        for i in 0..100i64 {
            let start = i * 7_919;
            let c = Clock::recover(start, start, start + (i * 8_999) % LIMIT_MS).unwrap();
            for t in [LIMIT_MS, LIMIT_MS + 1, LIMIT_MS + 60_000] {
                assert!(c.expired(start + t));
                assert_eq!(c.remaining_ms(start + t), 0);
            }
        }
    }
}
