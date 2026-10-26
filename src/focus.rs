//! Pomodoro: a focus period followed by a break. Only the end time is stored;
//! the remaining time is computed when something needs to show it.

use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Focus,
    Rest,
}

#[derive(Clone, Copy, Default)]
pub struct Focus {
    current: Option<(Phase, Instant, Duration)>,
}

impl Focus {
    pub fn start(&mut self, phase: Phase, minutes: u32) {
        let length = Duration::from_secs(minutes as u64 * 60);
        self.current = Some((phase, Instant::now() + length, length));
    }

    pub fn stop(&mut self) {
        self.current = None;
    }

    pub fn phase(&self) -> Option<Phase> {
        self.current.map(|(phase, _, _)| phase)
    }

    pub fn remaining(&self) -> Option<Duration> {
        self.current
            .map(|(_, end, _)| end.saturating_duration_since(Instant::now()))
    }

    /// Fraction of the current period already elapsed (0..=1).
    pub fn progress(&self) -> f32 {
        match (self.current, self.remaining()) {
            (Some((_, _, length)), Some(left)) if !length.is_zero() => {
                1.0 - left.as_secs_f32() / length.as_secs_f32()
            }
            _ => 0.0,
        }
    }

    /// Milliseconds until the minute shown in the closed pill changes (or the period ends).
    pub fn next_tick_ms(&self) -> Option<u32> {
        let left = self.remaining()?.as_millis() as u32;
        let to_minute = match left % 60_000 {
            0 => 60_000,
            ms => ms,
        };
        Some(to_minute.min(left).max(1))
    }
}

/// "24:59"
pub fn clock(d: Duration) -> String {
    let s = d.as_secs_f32().ceil() as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}

/// "25 min", rounding up so the pill never shows "0 min" before the end.
pub fn minutes(d: Duration) -> String {
    format!("{} min", (d.as_secs_f32() / 60.0).ceil() as u64)
}
