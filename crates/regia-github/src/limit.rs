//! A cap on the calls to GitHub that run at once (#572).
//!
//! The list is read with calls in parallel (`read`), and each call is a process of `gh`. Six is enough for
//! the most the read asks at once (the three full pages of a tracker of 300 issues, and the rest of the few
//! issues whose sub-issues or blockers the list cuts), and few enough to stay far from what GitHub
//! tolerates of simultaneous requests from one user (its secondary limit is 100) and to start no crowd of
//! processes on a small machine. A call that finds the six taken waits for one to end; it is never refused.

use std::sync::{Condvar, Mutex, PoisonError};

/// How many calls run at once.
pub(crate) const MAX_CALLS: usize = 6;

pub(crate) struct Calls {
    running: Mutex<usize>,
    ended: Condvar,
    max: usize,
}

/// A place among the running calls, given back when it is dropped.
pub(crate) struct Place<'a>(&'a Calls);

impl Calls {
    pub(crate) fn new(max: usize) -> Self {
        Self {
            running: Mutex::new(0),
            ended: Condvar::new(),
            max,
        }
    }

    /// Waits until fewer than the maximum are running, and takes a place.
    pub(crate) fn start(&self) -> Place<'_> {
        let mut running = self.running.lock().unwrap_or_else(PoisonError::into_inner);
        while *running >= self.max {
            running = self
                .ended
                .wait(running)
                .unwrap_or_else(PoisonError::into_inner);
        }
        *running += 1;
        Place(self)
    }
}

impl Drop for Place<'_> {
    fn drop(&mut self) {
        *self
            .0
            .running
            .lock()
            .unwrap_or_else(PoisonError::into_inner) -= 1;
        self.0.ended.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    use std::time::{Duration, Instant};

    use super::*;

    /// Twenty calls ask at once for three places: never more than three run, and all twenty do.
    #[test]
    fn no_more_than_the_maximum_run_at_once() {
        let calls = Calls::new(3);
        let running = AtomicUsize::new(0);
        let highest = AtomicUsize::new(0);
        let done = AtomicUsize::new(0);
        thread::scope(|scope| {
            for _ in 0..20 {
                scope.spawn(|| {
                    let _place = calls.start();
                    let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                    highest.fetch_max(now, Ordering::SeqCst);
                    // The first ones stay until three are in, so that a fourth that got in would show; a machine too slow to
                    // start three threads in two seconds gives up waiting, and the cap is still checked.
                    let waited = Instant::now();
                    while running.load(Ordering::SeqCst) < 3
                        && done.load(Ordering::SeqCst) == 0
                        && waited.elapsed() < Duration::from_secs(2)
                    {
                        thread::yield_now();
                    }
                    thread::sleep(Duration::from_millis(5));
                    running.fetch_sub(1, Ordering::SeqCst);
                    done.fetch_add(1, Ordering::SeqCst);
                });
            }
        });
        assert_eq!(done.load(Ordering::SeqCst), 20);
        assert_eq!(
            highest.load(Ordering::SeqCst),
            3,
            "the three places were all used, and never a fourth"
        );
    }
}
