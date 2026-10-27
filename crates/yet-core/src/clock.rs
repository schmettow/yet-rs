// SPDX-License-Identifier: GPL-3.0-only

//! Clock abstraction for a testable, non-blocking engine.

use core::time::Duration;

use crate::time::SessionTime;

/// A source of monotonic session time.
pub trait Clock {
    /// Current session time.
    fn now(&self) -> SessionTime;
}

/// A manually advanced clock for tests and headless runs.
#[derive(Clone, Debug, Default)]
pub struct TestClock {
    now: SessionTime,
}

impl TestClock {
    /// Create a clock at session time zero.
    pub const fn new() -> Self {
        Self {
            now: SessionTime::ZERO,
        }
    }

    /// Advance the clock and return the new time.
    pub fn advance(&mut self, by: Duration) -> SessionTime {
        self.now = self.now.saturating_add(by);
        self.now
    }

    /// Set the clock to an absolute session time.
    pub fn set(&mut self, now: SessionTime) {
        self.now = now;
    }
}

impl Clock for TestClock {
    fn now(&self) -> SessionTime {
        self.now
    }
}
