// SPDX-License-Identifier: GPL-3.0-only

//! Monotonic session time.
//!
//! The engine must be testable without a real clock. [`SessionTime`] is a
//! serializable, deterministic alternative to [`std::time::Instant`] for
//! experiment-relative timestamps. The application maps a monotonic `Instant`
//! to `SessionTime` once, at the session boundary.

use std::ops::Add;
use std::time::Duration;

/// Monotonic time since the start of an experiment session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionTime(Duration);

impl SessionTime {
    /// The instant at which a session starts.
    pub const ZERO: Self = Self(Duration::ZERO);

    /// Wrap a duration since session start.
    pub const fn from_duration(duration: Duration) -> Self {
        Self(duration)
    }

    /// Return the duration since session start.
    pub const fn as_duration(self) -> Duration {
        self.0
    }

    /// Add a duration, saturating at [`Duration::MAX`].
    pub fn saturating_add(self, duration: Duration) -> Self {
        Self(self.0.saturating_add(duration))
    }

    /// Add a duration, returning `None` on overflow.
    pub fn checked_add(self, duration: Duration) -> Option<Self> {
        self.0.checked_add(duration).map(Self)
    }

    /// Time elapsed from `earlier` to `self`, saturating at zero.
    pub fn duration_since(self, earlier: Self) -> Duration {
        self.0.saturating_sub(earlier.0)
    }
}

impl Add<Duration> for SessionTime {
    type Output = Self;

    fn add(self, rhs: Duration) -> Self::Output {
        Self(self.0 + rhs)
    }
}
