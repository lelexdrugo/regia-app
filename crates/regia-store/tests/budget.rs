//! The budget cache against the prototype's cases (`tools/tests/test_runtime.py`, the ones on
//! `sonda_cache`, and `test_sveglia.py`, the window not open in the cache), on a manual clock. Every
//! state folder is a fresh one under `target/`, never the live one.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use regia_store::{
    BudgetError, ManualClock, NOTE_OVERTAKEN, NOTE_RUNNING, PROBE_TTL_S, Precision, ProviderUsage, Reading, Store, StoreError, UsageStatus,
    WallTime, Window,
};

/// 2026-10-23T00:30:00Z, the instant of the prototype's cases.
const T0_MICROS: i64 = 1_792_715_400_000_000;

fn folder(label: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("budget-cache").join(label);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap_or_else(|error| panic!("{}: {error}", dir.display()));
    }
    dir
}

fn store(label: &str) -> (Store, ManualClock) {
    let clock = ManualClock::new(WallTime::from_unix_micros(T0_MICROS).unwrap(), 1000.0, "rust/manual");
    let store = Store::at(folder(label)).unwrap().with_clock(clock.clone());
    (store, clock)
}

/// A provider as the prototype's cases read it: valid, with its 5-hour window, or a 429.
fn reading(provider: &str, ok: bool, used: f64, now: WallTime) -> ProviderUsage {
    if !ok {
        return ProviderUsage::unknown(provider, None, "too many requests (HTTP 429)", None);
    }
    let reset = now.checked_add_secs(7200.0).unwrap().to_local_iso(0, Precision::Minutes);
    ProviderUsage::new(provider, Some("max".to_owned()), vec![("5h".to_owned(), Window::read(used, Some((reset, 7200))))], None, None)
}

/// A probe that answers `claude` from a sequence, the last one again once it runs out, and a valid
/// `codex`; it counts its calls.
struct Probe {
    calls: Arc<AtomicUsize>,
    sequence: Vec<(bool, f64)>,
    clock: ManualClock,
}

impl Probe {
    fn new(clock: &ManualClock, sequence: &[(bool, f64)]) -> Probe {
        Probe { calls: Arc::new(AtomicUsize::new(0)), sequence: sequence.to_vec(), clock: clock.clone() }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn run(&self) -> impl FnOnce() -> Result<Reading, std::io::Error> + '_ {
        move || {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            let (ok, used) = self.sequence[call.min(self.sequence.len() - 1)];
            let now = regia_store::Clock::now(&self.clock);
            Ok(Reading {
                generated: None,
                providers: vec![("claude".to_owned(), reading("claude", ok, used, now)), ("codex".to_owned(), reading("codex", true, 40.0, now))],
                cache: None,
            })
        }
    }
}

fn used(reading: &Reading, provider: &str) -> Option<f64> {
    reading.provider(provider).and_then(|usage| usage.window("5h")).and_then(Window::used_pct)
}

/// `test_f_seconda_chiamata_entro_l_intervallo_non_interroga`.
#[test]
fn a_second_reading_within_the_interval_asks_nothing() {
    let (store, clock) = store("within");
    let probe = Probe::new(&clock, &[(true, 40.0)]);
    let first = store.budget(300, false, probe.run()).unwrap();
    let second = store.budget(300, false, probe.run()).unwrap();
    assert_eq!(probe.calls(), 1);
    assert!(!first.cache.as_ref().unwrap().from_cache);
    assert!(second.cache.as_ref().unwrap().from_cache);
    assert_eq!(used(&second, "claude"), Some(40.0));
    let served = second.provider("claude").unwrap().served.as_ref().expect("from the cache");
    assert_eq!(served.age_s, Some(0));
    let json = serde_json::to_value(&second).unwrap();
    assert_eq!(json["provider"]["claude"]["age_s"], 0);
    assert_eq!(json["cache"]["min_interval_s"], 300);
}

/// `test_oltre_l_intervallo_interroga_di_nuovo`.
#[test]
fn past_the_interval_it_asks_again() {
    let (store, clock) = store("past");
    let probe = Probe::new(&clock, &[(true, 10.0), (true, 20.0)]);
    store.budget(0, false, probe.run()).unwrap();
    let second = store.budget(0, false, probe.run()).unwrap();
    assert_eq!(probe.calls(), 2);
    assert_eq!(used(&second, "claude"), Some(20.0));
}

/// `test_sconosciuto_non_cancella_il_valore_buono`.
#[test]
fn an_unknown_reading_does_not_erase_the_valid_one() {
    let (store, clock) = store("unknown-keeps");
    let probe = Probe::new(&clock, &[(true, 33.0), (false, 0.0)]);
    store.budget(0, false, probe.run()).unwrap();
    clock.advance(1.1);
    let reading = store.budget(0, false, probe.run()).unwrap();
    let claude = reading.provider("claude").unwrap();
    assert_eq!(probe.calls(), 2);
    assert_eq!(claude.status, UsageStatus::Ok);
    assert_eq!(used(&reading, "claude"), Some(33.0));
    let served = claude.served.as_ref().unwrap();
    assert!(served.age_s.is_some_and(|age| age >= 1), "{served:?}");
    assert!(served.update_failed.as_deref().is_some_and(|reason| reason.contains("429")), "{served:?}");
    store.budget(300, false, probe.run()).unwrap();
    assert_eq!(probe.calls(), 2, "the failed attempt counts: it is not pressed");
}

/// `test_mai_valido_resta_sconosciuto`.
#[test]
fn a_provider_never_valid_stays_unknown() {
    let (store, clock) = store("never-valid");
    let probe = Probe::new(&clock, &[(false, 0.0)]);
    let reading = store.budget(300, false, probe.run()).unwrap();
    assert_eq!(reading.provider("claude").unwrap().status, UsageStatus::Unknown);
    assert_eq!(reading.provider("codex").unwrap().status, UsageStatus::Ok);
    let json = serde_json::to_value(&reading).unwrap();
    assert_eq!(json["provider"]["claude"]["age_s"], serde_json::Value::Null);
    assert_eq!(json["provider"]["claude"]["reason"], "too many requests (HTTP 429)");
}

/// The case of `TestStorage` on what the folder holds afterwards, for the cache: no journal, no lock
/// file.
#[test]
fn only_the_database_is_left_in_the_folder() {
    let (store, _clock) = store("only-the-database");
    store.budget(300, false, || Ok::<_, std::io::Error>(Reading { generated: None, providers: Vec::new(), cache: None })).unwrap();
    let names: Vec<String> =
        std::fs::read_dir(store.folder()).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, [regia_store::database_name()]);
}

/// `test_sonda_rilevata_non_viene_sovrascritta_dalla_sonda_ferma`: probe A stalls past the claim's
/// expiry, B takes its place; when A resumes, its reading stays out of the cache.
#[test]
fn a_stalled_probe_overtaken_does_not_overwrite_the_one_that_replaced_it() {
    let (store, clock) = store("fencing");
    let (inside_a, a_is_inside) = mpsc::channel();
    let (release_a, a_released) = mpsc::channel::<()>();
    let a_store = store.clone();
    let a_clock = clock.clone();
    let a = thread::spawn(move || {
        a_store.budget(300, false, move || {
            inside_a.send(()).unwrap();
            a_released.recv_timeout(Duration::from_secs(10)).unwrap();
            let now = regia_store::Clock::now(&a_clock);
            Ok::<_, std::io::Error>(Reading { generated: None, providers: vec![("p".to_owned(), reading("p", true, 1.0, now))], cache: None })
        })
    });
    a_is_inside.recv_timeout(Duration::from_secs(5)).expect("A probes");
    // Only the monotonic clock moves: the claim of A is abandoned, as for a process that stalled.
    clock.set_monotonic(1000.0 + PROBE_TTL_S + 10.0);
    let b = store
        .budget(300, false, || {
            let now = regia_store::Clock::now(&clock);
            Ok::<_, std::io::Error>(Reading { generated: None, providers: vec![("p".to_owned(), reading("p", true, 2.0, now))], cache: None })
        })
        .unwrap();
    release_a.send(()).unwrap();
    let a = a.join().unwrap().unwrap();
    assert!(!b.cache.as_ref().unwrap().from_cache);
    assert_eq!(a.cache.as_ref().unwrap().note.as_deref(), Some(NOTE_OVERTAKEN));
    let read = store.budget(300, false, || -> Result<Reading, std::io::Error> { panic!("no probe within the interval") }).unwrap();
    assert_eq!(used(&read, "p"), Some(2.0), "B's reading stays");
    assert!(read.cache.unwrap().from_cache);
}

/// `test_sonda_che_fallisce_libera_il_claim`.
#[test]
fn a_probe_that_fails_releases_its_claim() {
    let (store, _clock) = store("released");
    let failed = store.budget(300, false, || Err::<Reading, _>(std::io::Error::other("network down")));
    assert!(matches!(failed, Err(BudgetError::Probe(_))));
    let calls = AtomicUsize::new(0);
    let fast = store.clone().with_probe_wait(Duration::ZERO);
    let reading = fast
        .budget(300, false, || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::io::Error>(Reading { generated: None, providers: Vec::new(), cache: None })
        })
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1, "no claim was left: the probe runs at once");
    assert_eq!(reading.cache.unwrap().note, None);
}

/// After `test_monotono_di_un_altro_orologio_ricade_sulla_parete`, for the cache's stamp: a cache
/// stamped by another clock, or before a restart, ages by the wall clock.
#[test]
fn a_cache_stamped_by_another_clock_ages_by_the_wall_clock() {
    let (store, clock) = store("other-clock");
    clock.set_tag("OtherPythonClock()");
    clock.set_monotonic(1e6);
    let probe = Probe::new(&clock, &[(true, 5.0), (true, 6.0)]);
    store.budget(300, false, probe.run()).unwrap();
    clock.set_tag("rust/manual");
    clock.set_monotonic(10.0);
    clock.jump_wall(10.0);
    let read = store.budget(300, false, probe.run()).unwrap();
    assert_eq!(probe.calls(), 1, "ten seconds on the wall clock: within the interval");
    assert_eq!(read.cache.unwrap().attempt_age_s, Some(10));
    // The same clock, gone back: the machine restarted, and the wall clock decides again.
    store.budget(0, false, probe.run()).unwrap();
    clock.set_monotonic(1.0);
    clock.jump_wall(400.0);
    store.budget(300, false, probe.run()).unwrap();
    assert_eq!(probe.calls(), 3, "400 s on the wall clock: past the interval");
}

/// A probe another process is running is waited for, then the cache answers and says so.
#[test]
fn a_probe_running_elsewhere_is_waited_for_then_the_cache_answers() {
    let (store, clock) = store("running-elsewhere");
    let (inside, is_inside) = mpsc::channel();
    let (release, released) = mpsc::channel::<()>();
    let other = store.clone();
    let other_clock = clock.clone();
    let running = thread::spawn(move || {
        other.budget(0, false, move || {
            inside.send(()).unwrap();
            released.recv_timeout(Duration::from_secs(10)).unwrap();
            let now = regia_store::Clock::now(&other_clock);
            Ok::<_, std::io::Error>(Reading { generated: None, providers: vec![("p".to_owned(), reading("p", true, 3.0, now))], cache: None })
        })
    });
    is_inside.recv_timeout(Duration::from_secs(5)).unwrap();
    let waited = store
        .clone()
        .with_probe_wait(Duration::from_millis(200))
        .budget(0, false, || -> Result<Reading, std::io::Error> { panic!("the claim is someone else's") })
        .unwrap();
    release.send(()).unwrap();
    running.join().unwrap().unwrap();
    let cache = waited.cache.unwrap();
    assert!(cache.from_cache);
    assert_eq!(cache.note.as_deref(), Some(NOTE_RUNNING));
    assert_eq!(cache.attempt_age_s, None, "nothing was read before");
}

/// A reading of a provider the user refused to probe never reaches the cache, which the prototype
/// reads too.
#[test]
fn a_reading_the_user_refused_is_never_stored() {
    let (store, _clock) = store("refused-not-stored");
    let reading = store
        .budget(300, false, || {
            Ok::<_, std::io::Error>(Reading { generated: None, providers: vec![("claude".to_owned(), ProviderUsage::probe_off("claude", "off"))], cache: None })
        })
        .unwrap();
    assert!(reading.providers.is_empty(), "{reading:?}");
    let rows = store.rows().unwrap();
    let cache = rows.iter().find(|row| row.key == "budget").expect("the cache row");
    assert!(!cache.document.contains("claude"), "{}", cache.document);
}

/// `TestFinestraNonAperta.test_la_cache_compone_una_finestra_senza_reset`: the window not open yet
/// stays so from the cache, with no reset to count down, and its reading ages.
#[test]
fn the_cache_composes_a_window_not_open() {
    let (store, clock) = store("not-open");
    store
        .budget(300, false, || {
            let windows = vec![("5h".to_owned(), Window::read(0.0, None))];
            Ok::<_, std::io::Error>(Reading {
                generated: None,
                providers: vec![("claude".to_owned(), ProviderUsage::new("claude", Some("max".to_owned()), windows, None, None))],
                cache: None,
            })
        })
        .unwrap();
    clock.advance(60.0);
    let read = store.budget(300, false, || -> Result<Reading, std::io::Error> { panic!("within the interval") }).unwrap();
    let json = serde_json::to_value(&read).unwrap();
    let window = &json["provider"]["claude"]["windows"]["5h"];
    assert_eq!((window["open"].clone(), window["reset"].clone(), window["reset_in_s"].clone()), (false.into(), serde_json::Value::Null, serde_json::Value::Null));
    assert_eq!(json["provider"]["claude"]["age_s"], 60);
    assert_eq!(json["generated"], "2026-10-23T00:30:00+00:00", "when it was attempted, in local time: UTC on this clock");
}

#[test]
fn a_busy_store_is_an_error_of_the_store() {
    let error: BudgetError<std::io::Error> = StoreError::InUse.into();
    assert!(error.to_string().contains("in use"));
}
