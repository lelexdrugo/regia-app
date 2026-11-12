//! The two clocks an expiry depends on (ADR 0001, section 7; ADR 0008 of the dossier).
//!
//! A document carries its expiry twice: on the wall clock, which every reader understands, and on the
//! writer's monotonic clock, with a tag naming that clock. A reader with the same tag decides on the
//! monotonic clock, so a jump of the wall clock neither steals nor extends a lock; any other reader
//! falls back to the wall clock. The Rust tag is never one of the prototype's, so the two
//! implementations always read each other by the wall clock.

use std::borrow::Cow;
use std::fmt;
use std::sync::{Arc, Mutex};

use crate::time::WallTime;

/// Where a store reads the time. [`SystemClock`] in production; a test or a shadow run injects another.
pub trait Clock: Send + Sync + fmt::Debug {
    /// The UTC wall clock.
    fn now(&self) -> WallTime;
    /// Seconds on this machine's monotonic clock, since an origin shared by every process that writes
    /// the same [`Clock::tag`].
    fn monotonic(&self) -> f64;
    /// Names the monotonic clock in the documents this clock stamps.
    fn tag(&self) -> Cow<'static, str>;
    /// Seconds east of UTC of the local time at `at`, which the budget's readings are written in, as
    /// the prototype's `astimezone()` writes them.
    fn local_offset_s(&self, at: WallTime) -> i32 {
        system_offset_s(at)
    }
}

/// The system's time zone at `at`.
fn system_offset_s(at: WallTime) -> i32 {
    use chrono::{Offset, TimeZone};
    chrono::DateTime::from_timestamp_micros(at.unix_micros())
        .map_or(0, |utc| chrono::Local.offset_from_utc_datetime(&utc.naive_utc()).fix().local_minus_utc())
}

/// The system's clocks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SystemClock;

impl SystemClock {
    /// The tag of this build's monotonic clock: `rust/` and the OS call.
    pub const TAG: &'static str = os::TAG;
}

impl Clock for SystemClock {
    fn now(&self) -> WallTime {
        WallTime::now()
    }

    fn monotonic(&self) -> f64 {
        os::monotonic_seconds()
    }

    fn tag(&self) -> Cow<'static, str> {
        Cow::Borrowed(os::TAG)
    }
}

/// A clock that moves only when told to: wall and monotonic time, the tag and the local offset, set by
/// hand. Its local time is UTC until told otherwise, so no reading depends on the machine.
///
/// Clones share the same time, so a test keeps one and gives the other to the store.
#[derive(Debug, Clone)]
pub struct ManualClock {
    state: Arc<Mutex<ManualState>>,
}

#[derive(Debug)]
struct ManualState {
    wall: WallTime,
    monotonic: f64,
    tag: String,
    offset_s: i32,
}

impl ManualClock {
    /// The tag of [`ManualClock::fixed`]: neither the system's nor the prototype's.
    pub const FIXED_TAG: &'static str = "rust/fixed";

    #[must_use]
    pub fn new(wall: WallTime, monotonic: f64, tag: impl Into<String>) -> Self {
        let state = ManualState { wall, monotonic, tag: tag.into(), offset_s: 0 };
        Self { state: Arc::new(Mutex::new(state)) }
    }

    /// The clock of a run at a fixed instant, the product's `REGIA_NOW`: the wall clock at `at` and a
    /// monotonic clock that stands still, so what is taken now expires exactly its ttl later and reads
    /// so, as the prototype's fixtures run. Its tag is its own, so a reader on the system's clock never
    /// compares its stamps with theirs and reads the wall clock instead.
    #[must_use]
    pub fn fixed(at: WallTime) -> Self {
        Self::new(at, 1000.0, Self::FIXED_TAG)
    }

    fn with<T>(&self, f: impl FnOnce(&mut ManualState) -> T) -> T {
        f(&mut self.state.lock().expect("manual clock poisoned"))
    }

    /// Time passes: both clocks move by `secs`.
    ///
    /// # Panics
    ///
    /// If the wall clock would leave the range of a `datetime`.
    pub fn advance(&self, secs: f64) {
        self.with(|state| {
            state.wall = state.wall.checked_add_secs(secs).unwrap_or_else(|| panic!("wall clock out of range after {secs} s"));
            state.monotonic += secs;
        });
    }

    /// Someone sets the wall clock: only the wall clock moves.
    ///
    /// # Panics
    ///
    /// If the wall clock would leave the range of a `datetime`.
    pub fn jump_wall(&self, secs: f64) {
        self.with(|state| {
            state.wall = state.wall.checked_add_secs(secs).unwrap_or_else(|| panic!("wall clock out of range after {secs} s"));
        });
    }

    /// Sets the monotonic clock, for instance back to near zero after a reboot.
    pub fn set_monotonic(&self, secs: f64) {
        self.with(|state| state.monotonic = secs);
    }

    /// Stamps the next documents as another implementation of the monotonic clock would.
    pub fn set_tag(&self, tag: impl Into<String>) {
        let tag = tag.into();
        self.with(|state| state.tag = tag);
    }

    /// The local time becomes `offset_s` seconds east of UTC.
    pub fn set_local_offset_s(&self, offset_s: i32) {
        self.with(|state| state.offset_s = offset_s);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> WallTime {
        self.with(|state| state.wall)
    }

    fn monotonic(&self) -> f64 {
        self.with(|state| state.monotonic)
    }

    fn tag(&self) -> Cow<'static, str> {
        Cow::Owned(self.with(|state| state.tag.clone()))
    }

    fn local_offset_s(&self, _at: WallTime) -> i32 {
        self.with(|state| state.offset_s)
    }
}

/// Windows: milliseconds since the system started, sleep and hibernation included. Python 3.12 reads
/// the same call with another tag; the values are not shared on purpose (ADR 0001, section 7).
#[cfg(windows)]
mod os {
    pub const TAG: &str = "rust/GetTickCount64";

    #[link(name = "kernel32")]
    unsafe extern "system" {
        safe fn GetTickCount64() -> u64;
    }

    pub fn monotonic_seconds() -> f64 {
        GetTickCount64() as f64 / 1000.0
    }
}

/// Linux: time since boot, suspend included, unlike the `CLOCK_MONOTONIC` the prototype reads.
#[cfg(any(target_os = "linux", target_os = "android"))]
mod os {
    pub const TAG: &str = "rust/CLOCK_BOOTTIME";

    pub fn monotonic_seconds() -> f64 {
        super::clock_gettime(libc::CLOCK_BOOTTIME)
    }
}

/// Other Unix systems, macOS among them, where `CLOCK_MONOTONIC` keeps counting during sleep.
#[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
mod os {
    pub const TAG: &str = "rust/CLOCK_MONOTONIC";

    pub fn monotonic_seconds() -> f64 {
        super::clock_gettime(libc::CLOCK_MONOTONIC)
    }
}

#[cfg(unix)]
fn clock_gettime(clock: libc::clockid_t) -> f64 {
    let mut value = std::mem::MaybeUninit::<libc::timespec>::uninit();
    // SAFETY: `value` is valid for a write of one `timespec`, and `clock` is a clock id this OS defines.
    let status = unsafe { libc::clock_gettime(clock, value.as_mut_ptr()) };
    assert_eq!(status, 0, "clock_gettime({clock}) failed: {}", std::io::Error::last_os_error());
    // SAFETY: `clock_gettime` returned 0, so it wrote the whole struct.
    let value = unsafe { value.assume_init() };
    value.tv_sec as f64 + value.tv_nsec as f64 / 1e9
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_clock_moves_forward_and_its_tag_is_its_own() {
        let clock = SystemClock;
        let first = clock.monotonic();
        std::thread::sleep(std::time::Duration::from_millis(40));
        assert!(clock.monotonic() > first, "monotonic clock did not move");
        assert!(clock.tag().starts_with("rust/"), "{}", clock.tag());
    }

    #[test]
    fn a_manual_clock_moves_only_when_told() {
        let clock = ManualClock::new(WallTime::from_unix_micros(0).expect("epoch"), 100.0, "fake");
        let shared = clock.clone();
        clock.advance(1.5);
        assert_eq!((shared.now().unix_micros(), shared.monotonic()), (1_500_000, 101.5));
        clock.jump_wall(-10.0);
        assert_eq!((shared.now().unix_micros(), shared.monotonic()), (-8_500_000, 101.5));
        clock.set_tag("other");
        assert_eq!(shared.tag(), "other");
        assert_eq!(shared.local_offset_s(shared.now()), 0, "UTC until told otherwise");
        clock.set_local_offset_s(3600);
        assert_eq!(shared.local_offset_s(shared.now()), 3600);
        let offset = SystemClock.local_offset_s(WallTime::now());
        assert!(offset.abs() <= 14 * 3600, "{offset}");
    }
}
