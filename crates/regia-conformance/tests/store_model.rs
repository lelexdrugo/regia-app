//! The lock's state machine against a table model (L7 of the dossier's conformance study, and its
//! section 9.3): random sequences of acquire, renew, release and status by three holders on two
//! projects, with a clock moved by hand, jumps of the wall clock, restarts, and documents stamped by
//! another implementation's clock. The model is the prototype's table of outcomes; the store must agree
//! on every step: state, holder, the holder replaced, and the seconds left.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use regia_store::{Holder, LockOutcome, LockState, LockStatus, LockView, ManualClock, ProjectId, Store, Ttl, WallTime};

const HOLDERS: [&str; 3] = ["h1", "h2", "h3"];
const PROJECTS: [&str; 2] = ["p1", "p2"];
const TTLS: [u64; 4] = [5, 30, 60, 600];
const STEPS: usize = 50;

/// A lock as the model keeps it: who holds it and its expiry on both clocks.
#[derive(Debug, Clone)]
struct Row {
    holder: String,
    wall_expires: i64,
    written: f64,
    mono_expires: f64,
    tag: String,
}

#[derive(Debug, Default)]
struct Model {
    rows: HashMap<String, Row>,
}

struct Now {
    wall: i64,
    mono: f64,
    tag: String,
}

impl Model {
    /// The prototype's `_residuo_s`: its own clock if the tag matches, else the wall clock.
    fn remaining(row: &Row, now: &Now) -> f64 {
        if row.tag == now.tag {
            if now.mono < row.written {
                return 0.0;
            }
            return row.mono_expires - now.mono;
        }
        (row.wall_expires - now.wall) as f64 / 1e6
    }

    fn write(&mut self, project: &str, holder: &str, ttl: u64, now: &Now) {
        // Whole seconds on a clock that moves in whole seconds: no rounding to model.
        let wall_expires = now.wall + ttl as i64 * 1_000_000;
        let row = Row {
            holder: holder.to_owned(),
            wall_expires,
            written: now.mono,
            mono_expires: now.mono + (wall_expires - now.wall) as f64 / 1e6,
            tag: now.tag.clone(),
        };
        self.rows.insert(project.to_owned(), row);
    }

    /// The expected state, and the holder a stolen lock replaced.
    fn acquire(&mut self, project: &str, holder: &str, ttl: u64, now: &Now) -> (LockState, Option<String>) {
        let Some(row) = self.rows.get(project).cloned() else {
            self.write(project, holder, ttl, now);
            return (LockState::Taken, None);
        };
        let live = Self::remaining(&row, now) > 0.0;
        match (live, row.holder == holder) {
            (true, false) => (LockState::Busy, None),
            (_, true) => {
                self.write(project, holder, ttl, now);
                (LockState::Renewed, None)
            }
            (false, false) => {
                self.write(project, holder, ttl, now);
                (LockState::Stolen, Some(row.holder))
            }
        }
    }

    fn renew(&mut self, project: &str, holder: &str, ttl: u64, now: &Now) -> LockState {
        match self.rows.get(project) {
            None => LockState::Absent,
            Some(row) if row.holder != holder => LockState::Foreign,
            Some(_) => {
                self.write(project, holder, ttl, now);
                LockState::Renewed
            }
        }
    }

    fn release(&mut self, project: &str, holder: &str) -> LockState {
        match self.rows.get(project) {
            None => LockState::Free,
            Some(row) if row.holder != holder => LockState::Foreign,
            Some(_) => {
                self.rows.remove(project);
                LockState::Released
            }
        }
    }
}

/// xorshift64*: reproducible from the seed printed on failure.
struct Random(u64);

impl Random {
    fn below(&mut self, bound: usize) -> usize {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 33) as usize % bound
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())]
    }
}

fn folder(label: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("store-model").join(label);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("clean folder");
    }
    dir
}

fn left(view: &LockView) -> i64 {
    view.expires_in_s()
}

fn expected_left(row: &Row, now: &Now) -> i64 {
    (Model::remaining(row, now).trunc() as i64).max(0)
}

/// Runs one sequence and returns the states it saw.
fn run_sequence(seed: u64) -> HashSet<String> {
    let mut random = Random(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let start = WallTime::from_unix_micros(1_794_000_000_000_000).expect("a date");
    let clock = ManualClock::new(start, 1000.0, "model-clock-a");
    let store = Store::at(folder(&format!("seed-{seed}"))).expect("store").with_clock(clock.clone());
    let mut model = Model::default();
    let (mut wall, mut mono, mut tag) = (start.unix_micros(), 1000.0_f64, "model-clock-a".to_owned());
    let mut history: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for step in 0..STEPS {
        let project = random.pick(&PROJECTS);
        let holder = random.pick(&HOLDERS);
        let ttl = random.pick(&TTLS);
        let op = random.below(10);
        let now = Now { wall, mono, tag: tag.clone() };
        let id = ProjectId::new(project).expect("id");
        let who = Holder::new(holder).expect("holder");
        let context = |history: &[String]| format!("seed {seed}, step {step}: {}", history.join(" | "));
        match op {
            0..=2 => {
                history.push(format!("acquire {project} {holder} {ttl}"));
                let (want, replaced) = model.acquire(project, holder, ttl, &now);
                let got = store.acquire_lock(&id, &who, Ttl::from_secs(ttl).expect("ttl")).expect("acquire");
                assert_eq!(got.state(), want, "{}", context(&history));
                seen.insert(format!("{want:?}"));
                if let Some(view) = got.lock() {
                    let row = model.rows.get(project).expect("a row in the model");
                    assert_eq!(view.holder(), Some(row.holder.as_str()), "{}", context(&history));
                    assert_eq!(left(view), expected_left(row, &now), "{}", context(&history));
                }
                if let LockOutcome::Stolen(view) = &got {
                    assert_eq!(view.replaced().and_then(|value| value.as_str()), replaced.as_deref(), "{}", context(&history));
                }
            }
            3 | 4 => {
                history.push(format!("renew {project} {holder} {ttl}"));
                let want = model.renew(project, holder, ttl, &now);
                let got = store.renew_lock(&id, &who, Ttl::from_secs(ttl).expect("ttl")).expect("renew");
                assert_eq!(got.state(), want, "{}", context(&history));
                seen.insert(format!("{want:?}"));
            }
            5 | 6 => {
                history.push(format!("release {project} {holder}"));
                let want = model.release(project, holder);
                let got = store.release_lock(&id, &who).expect("release");
                assert_eq!(got.state(), want, "{}", context(&history));
                seen.insert(format!("{want:?}"));
            }
            7 => {
                history.push(format!("status {project}"));
                let got = store.lock_status(&id).expect("status");
                match model.rows.get(project) {
                    None => assert_eq!(got, LockStatus::Free, "{}", context(&history)),
                    Some(row) => {
                        let live = Model::remaining(row, &now) > 0.0;
                        let view = got.lock().unwrap_or_else(|| panic!("no lock: {got:?}, {}", context(&history)));
                        assert_eq!(matches!(got, LockStatus::Held(_)), live, "{}", context(&history));
                        assert_eq!(view.holder(), Some(row.holder.as_str()), "{}", context(&history));
                        assert_eq!(left(view), expected_left(row, &now), "{}", context(&history));
                    }
                }
            }
            8 => {
                // Time passes on both clocks.
                let secs = random.pick(&[0_i64, 1, 4, 5, 29, 31, 59, 61, 700]);
                history.push(format!("advance {secs}"));
                clock.advance(secs as f64);
                wall += secs * 1_000_000;
                mono += secs as f64;
            }
            _ => match random.below(3) {
                0 => {
                    let secs = random.pick(&[-3000_i64, -700, -100, 100, 700, 3000]);
                    history.push(format!("wall jump {secs}"));
                    clock.jump_wall(secs as f64);
                    wall += secs * 1_000_000;
                }
                1 => {
                    history.push("restart".to_owned());
                    mono = random.pick(&[1.0, 10.0, 100.0]);
                    clock.set_monotonic(mono);
                }
                _ => {
                    tag = if tag == "model-clock-a" { "model-clock-b".to_owned() } else { "model-clock-a".to_owned() };
                    history.push(format!("clock {tag}"));
                    clock.set_tag(tag.clone());
                }
            },
        }
    }
    seen
}

/// Every outcome of the table shows up, so a sequence generator gone blind cannot pass.
fn check(seeds: std::ops::Range<u64>) {
    let seen: HashSet<String> = seeds.flat_map(run_sequence).collect();
    for state in ["Taken", "Renewed", "Stolen", "Released", "Free", "Busy", "Foreign", "Absent"] {
        assert!(seen.contains(state), "no sequence reached {state}: {seen:?}");
    }
}

#[test]
fn the_store_follows_the_table_on_random_sequences_0() {
    check(0..10);
}

#[test]
fn the_store_follows_the_table_on_random_sequences_1() {
    check(10..20);
}

#[test]
fn the_store_follows_the_table_on_random_sequences_2() {
    check(20..30);
}

#[test]
fn the_store_follows_the_table_on_random_sequences_3() {
    check(30..40);
}
