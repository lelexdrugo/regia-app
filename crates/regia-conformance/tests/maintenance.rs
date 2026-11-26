//! The maintenance of the machine's state (#471): the reader of the log of declarations (the prototype's `leggi`),
//! its compaction into one file a month, and the cleaning of the tasks' work files 30 days after they closed. Every
//! folder is a fresh one under `target/`; no file of the machine's own state is read or written. The instants are
//! injected: the policy's `now`, and the files' times set by hand.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use regia_store::{
    CompactionError, CompactionPolicy, LogReadError, WallTime, WorkError, WorkPolicy, clean_work,
    compact_declarations, read_declarations, work_folder,
};

const DAY_S: i64 = 86_400;

fn folder(label: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("maintenance")
        .join(label);
    if dir.exists() {
        fs::remove_dir_all(&dir).unwrap_or_else(|error| panic!("{}: {error}", dir.display()));
    }
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A line as the writer writes it.
fn line(ts: &str, session: &str, project: &str, command: &str) -> String {
    format!(
        r#"{{"v":1,"ts":"{ts}","sessione":"{session}","agente":"claude","claude":"{session}","progetto":"{project}","comando":"{command}"}}"#
    )
}

/// A file of the log, its lines each ended by `\n`, last written `days_ago` days before `now`.
fn file(dir: &Path, name: &str, lines: &[String], now: SystemTime, days_ago: u64) {
    let path = dir.join(name);
    fs::write(
        &path,
        lines
            .iter()
            .map(|line| format!("{line}\n"))
            .collect::<String>(),
    )
    .unwrap();
    let at = now - Duration::from_secs(days_ago * 86_400);
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(at)
        .unwrap();
}

fn wall(at: SystemTime) -> WallTime {
    let micros = at
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_micros();
    WallTime::from_unix_micros(i64::try_from(micros).unwrap()).unwrap()
}

/// What the reader returns, as the prototype's `sync` uses it: instant and line, in order.
fn seen(dir: &Path) -> Vec<(i64, String)> {
    read_declarations(dir)
        .unwrap()
        .into_iter()
        .map(|event| (event.at.unix_micros(), event.line))
        .collect()
}

fn names(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    out.sort();
    out
}

// ------------------------------------------------------------------------------------------------- the reader

/// The prototype's order: time, then the file's place in the sorted names, then the line's place; the lines it skips.
#[test]
fn the_reader_keeps_the_prototype_s_order_and_skips_what_it_skips() {
    let dir = folder("reader-order");
    let now = SystemTime::now();
    let t = "2026-10-10T10:00:00Z";
    file(
        &dir,
        "b.1.jsonl",
        &[
            line(t, "s2", "beta", "dichiara"),
            line("2026-10-10T09:00:00Z", "s2", "beta", "passo inizio"),
        ],
        now,
        0,
    );
    file(
        &dir,
        "a.1.jsonl",
        &[line(t, "s1", "alfa", "dichiara")],
        now,
        0,
    );
    let skipped = [
        "not json".to_owned(),
        "[1, 2]".to_owned(),
        r#"{"v":1,"sessione":"s3","comando":"x"}"#.to_owned(),
        r#"{"v":1,"ts":"never","sessione":"s3","comando":"x"}"#.to_owned(),
        r#"{"v":1,"ts":"2026-10-10T10:00:00Z","sessione":"","comando":"x"}"#.to_owned(),
        String::new(),
    ];
    file(&dir, "c.1.jsonl", &skipped, now, 0);
    fs::write(
        dir.join("d.1.jsonl.tmp"),
        line(t, "s4", "gamma", "dichiara"),
    )
    .unwrap();
    let events = read_declarations(&dir).unwrap();
    let order: Vec<(&str, usize)> = events
        .iter()
        .map(|event| (event.file.as_str(), event.index))
        .collect();
    assert_eq!(
        order,
        [("b.1.jsonl", 1), ("a.1.jsonl", 0), ("b.1.jsonl", 0)]
    );
    assert!(events[1].ids.contains("s1") && events[1].ids.len() == 1);
    assert_eq!(events[0].at.to_iso_seconds(), "2026-10-10T09:00:00Z");
}

/// A file that is not UTF-8 stops the read, as Python's `read_text` stops `leggi`.
#[test]
fn a_file_that_is_not_text_stops_the_reader() {
    let dir = folder("reader-not-text");
    fs::write(dir.join("a.1.jsonl"), b"\xff\xfe{}\n").unwrap();
    assert!(matches!(
        read_declarations(&dir),
        Err(LogReadError::NotText { .. })
    ));
    assert!(
        read_declarations(&dir.join("missing")).unwrap().is_empty(),
        "no folder, no event"
    );
}

// ------------------------------------------------------------------------------------------------- the compaction

/// Quiet files merge into one file a month; the recent, the unclean and those of two months stay; the reader sees
/// the same events in the same order; a second run has nothing left to merge.
#[test]
fn quiet_files_merge_by_month_and_the_reader_sees_the_same() {
    let dir = folder("compaction");
    let now = SystemTime::now();
    file(
        &dir,
        "s1.10.jsonl",
        &[line("2026-10-01T08:00:00Z", "s1", "alfa", "dichiara")],
        now,
        5,
    );
    file(
        &dir,
        "s1.11.jsonl",
        &[
            line("2026-10-01T08:00:00Z", "s1", "beta", "passo inizio"),
            line("2026-10-02T08:00:00Z", "s1", "beta", "passo fine"),
        ],
        now,
        5,
    );
    file(
        &dir,
        "s2.12.jsonl",
        &[line("2026-11-03T08:00:00Z", "s2", "alfa", "dichiara")],
        now,
        5,
    );
    file(
        &dir,
        "s3.13.jsonl",
        &[line("2026-11-04T08:00:00Z", "s3", "alfa", "dichiara")],
        now,
        0,
    );
    file(
        &dir,
        "s4.14.jsonl",
        &[
            line("2026-11-04T08:00:00Z", "s4", "alfa", "dichiara"),
            "broken".to_owned(),
        ],
        now,
        5,
    );
    file(
        &dir,
        "s5.15.jsonl",
        &[
            line("2026-10-31T23:59:59Z", "s5", "alfa", "dichiara"),
            line("2026-11-01T00:00:00Z", "s5", "alfa", "dichiara"),
        ],
        now,
        5,
    );
    let before = seen(&dir);
    let policy = CompactionPolicy::at(wall(now), false);

    let dry = compact_declarations(
        &dir,
        &CompactionPolicy {
            dry_run: true,
            ..policy
        },
    )
    .unwrap();
    assert_eq!(
        (dry.merged, dry.lines, dry.months.len()),
        (3, 4, 2),
        "{dry:?}"
    );
    assert_eq!(names(&dir).len(), 6, "a dry run writes nothing");

    let done = compact_declarations(&dir, &policy).unwrap();
    assert_eq!(
        (
            done.merged,
            done.lines,
            done.kept_recent,
            done.kept_unclean,
            done.kept_spanning,
            done.kept_order
        ),
        (3, 4, 1, 1, 1, 0),
        "{done:?}"
    );
    assert_eq!(
        seen(&dir),
        before,
        "the reader returns the same events in the same order"
    );
    let left = names(&dir);
    assert_eq!(left.len(), 5, "{left:?}");
    assert!(
        left.iter()
            .all(|name| !["s1.10.jsonl", "s1.11.jsonl", "s2.12.jsonl"].contains(&name.as_str())),
        "{left:?}"
    );
    assert!(
        left.iter()
            .all(|name| !name.ends_with(".tmp") && !name.ends_with(".json")),
        "no plan, no half file: {left:?}"
    );

    let again = compact_declarations(&dir, &policy).unwrap();
    assert_eq!((again.merged, again.recovered), (0, false), "{again:?}");
    assert_eq!(seen(&dir), before);
}

/// The order of equal instants comes from the files' names: a month whose merge would put an event before another of
/// the same second that came after it stays as it is.
#[test]
fn a_merge_that_would_change_the_order_of_equal_instants_is_left() {
    let now = SystemTime::now();
    let t = "2026-10-05T08:00:00Z";
    // `0a` merges and sorts first; the recent file that stays sorts before the month file (`a0`), so after the merge
    // its event would come first: refused. One that sorts after the month file (`zz`) keeps the order: merged.
    for (stays, merged) in [("a0.2.jsonl", 0), ("zz.2.jsonl", 1)] {
        let dir = folder(&format!("compaction-ties-{merged}"));
        file(
            &dir,
            "0a.1.jsonl",
            &[line(t, "s1", "alfa", "dichiara")],
            now,
            5,
        );
        file(&dir, stays, &[line(t, "s1", "beta", "dichiara")], now, 0);
        let before = seen(&dir);
        let done = compact_declarations(&dir, &CompactionPolicy::at(wall(now), false)).unwrap();
        assert_eq!(
            (done.merged, done.kept_order),
            (merged, 1 - merged),
            "{stays}: {done:?}"
        );
        assert_eq!(seen(&dir), before, "{stays}: the order is the same");
    }
}

/// A run that stopped after the month file was put in place: the next run removes the merged files the plan names,
/// and merges nothing twice. One that stopped before: the sources stay and merge again.
#[test]
fn a_run_that_stopped_halfway_is_finished_by_the_next() {
    let now = SystemTime::now();
    let policy = CompactionPolicy::at(wall(now), false);
    for replaced in [true, false] {
        let dir = folder(if replaced {
            "recover-replaced"
        } else {
            "recover-not-replaced"
        });
        file(
            &dir,
            "s1.10.jsonl",
            &[line("2026-10-01T08:00:00Z", "s1", "alfa", "dichiara")],
            now,
            5,
        );
        let before = seen(&dir);
        // A first run, done by hand up to the plan: the plan, then (or not) the month file.
        let dry = compact_declarations(
            &dir,
            &CompactionPolicy {
                dry_run: true,
                ..policy
            },
        )
        .unwrap();
        let month = dry.months[0].clone();
        let source = fs::metadata(dir.join("s1.10.jsonl")).unwrap();
        let mtime = i64::try_from(
            source
                .modified()
                .unwrap()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_micros(),
        )
        .unwrap();
        let text = fs::read_to_string(dir.join("s1.10.jsonl")).unwrap();
        let plan = format!(
            r#"{{"v":1,"months":[{{"file":"{month}","before":null,"after":{},"sources":[{{"file":"s1.10.jsonl","len":{},"mtime":{mtime}}}]}}]}}"#,
            text.len(),
            source.len()
        );
        fs::write(dir.join("compattazione.json"), plan).unwrap();
        if replaced {
            fs::write(dir.join(&month), &text).unwrap();
        } else {
            fs::write(dir.join(format!("{month}.tmp")), &text).unwrap();
        }
        let done = compact_declarations(&dir, &policy).unwrap();
        assert!(done.recovered, "{done:?}");
        assert_eq!(seen(&dir), before, "replaced: {replaced}");
        assert_eq!(names(&dir), [month.clone()], "replaced: {replaced}");
    }
}

/// The log's folder as a link is refused; a file of the log that is a link stays.
#[test]
fn the_log_s_folder_is_not_followed() {
    let dir = folder("compaction-link");
    let target = dir.join("target");
    fs::create_dir_all(&target).unwrap();
    let link = dir.join("link");
    if !link_folder(&link, &target) {
        eprintln!("the_log_s_folder_is_not_followed: no link can be made on this machine: skipped");
        return;
    }
    let refused = compact_declarations(&link, &CompactionPolicy::at(WallTime::now(), false));
    assert!(
        matches!(refused, Err(CompactionError::Link { .. })),
        "{refused:?}"
    );
}

// ------------------------------------------------------------------------------------------------- the work files

fn closed(
    entries: &[(u64, WallTime)],
) -> impl Fn() -> Result<BTreeMap<u64, WallTime>, String> + '_ {
    move || Ok(entries.iter().copied().collect())
}

fn days_before(now: WallTime, days: i64) -> WallTime {
    WallTime::from_unix_micros(now.unix_micros() - days * DAY_S * 1_000_000).unwrap()
}

fn task_folder(root: &Path, project: &str, task: u64) -> PathBuf {
    let dir = root.join(project).join(task.to_string());
    fs::create_dir_all(dir.join("inner")).unwrap();
    fs::write(dir.join("inner").join("draft.md"), "x").unwrap();
    dir
}

/// Acceptance 3: a task closed 29 days ago keeps its folder, one closed 31 days ago loses it; an open task and one
/// the tracker does not know keep theirs; a dry run removes nothing.
#[test]
fn a_task_closed_31_days_ago_is_cleaned_and_one_closed_29_days_ago_stays() {
    let cache = folder("work");
    let root = work_folder(&cache);
    let now = WallTime::parse_iso("2026-12-01T09:00:00Z").unwrap();
    let recent = task_folder(&root, "alfa", 29);
    let old = task_folder(&root, "alfa", 31);
    let open = task_folder(&root, "beta", 7);
    fs::write(
        root.join("alfa").join("notes.txt"),
        "a file, not a task's folder",
    )
    .unwrap();
    fs::create_dir_all(root.join("not_an_id").join("40")).unwrap();
    let tasks = [
        (29, days_before(now, 29)),
        (31, days_before(now, 31)),
        (40, days_before(now, 90)),
    ];

    let dry = clean_work(&root, &closed(&tasks), &WorkPolicy::at(now, true)).unwrap();
    assert_eq!(dry.removed, ["alfa/31"]);
    assert!(old.exists(), "a dry run removes nothing");

    let done = clean_work(&root, &closed(&tasks), &WorkPolicy::at(now, false)).unwrap();
    assert_eq!(done.removed, ["alfa/31"], "{done:?}");
    assert_eq!((done.kept_recent, done.kept_open), (1, 1), "{done:?}");
    assert!(!old.exists() && recent.exists() && open.exists());
    assert!(
        root.join("alfa").join("notes.txt").exists() && root.join("not_an_id").join("40").exists(),
        "only a project's id is looked at"
    );
}

/// A tracker that does not read removes nothing; a folder named otherwise than the work folder is refused.
#[test]
fn the_cleaning_fails_closed() {
    let cache = folder("work-closed");
    let root = work_folder(&cache);
    let old = task_folder(&root, "alfa", 31);
    let now = WallTime::parse_iso("2026-12-01T09:00:00Z").unwrap();
    let broken = || Err::<BTreeMap<u64, WallTime>, String>("no tracker".to_owned());
    assert!(matches!(
        clean_work(&root, &broken, &WorkPolicy::at(now, false)),
        Err(WorkError::Closures(_))
    ));
    assert!(old.exists());
    let other = clean_work(
        &cache,
        &closed(&[(31, days_before(now, 31))]),
        &WorkPolicy::at(now, false),
    );
    assert!(matches!(other, Err(WorkError::NotWork { .. })), "{other:?}");
    assert!(old.exists());
}

/// A link inside a task's folder goes as a link, and what it points to stays; a project's folder that is a link is
/// not followed; the work folder itself as a link is refused.
#[test]
fn the_cleaning_follows_no_link_nor_junction() {
    let cache = folder("work-links");
    let root = work_folder(&cache);
    let now = WallTime::parse_iso("2026-12-01T09:00:00Z").unwrap();
    let outside = cache.join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("keep.txt"), "must survive").unwrap();
    let old = task_folder(&root, "alfa", 31);
    if !link_folder(&old.join("inner").join("to-outside"), &outside) {
        eprintln!(
            "the_cleaning_follows_no_link_nor_junction: no link can be made on this machine: skipped"
        );
        return;
    }
    assert!(link_folder(
        &root.join("gamma"),
        &task_folder(&cache.join("elsewhere"), "gamma", 31)
            .parent()
            .unwrap()
            .to_path_buf()
    ));
    let done = clean_work(
        &root,
        &closed(&[(31, days_before(now, 31))]),
        &WorkPolicy::at(now, false),
    )
    .unwrap();
    assert_eq!(done.removed, ["alfa/31"], "{done:?}");
    assert!(!old.exists(), "the task's folder is gone");
    assert!(
        outside.join("keep.txt").exists(),
        "the link's target is untouched"
    );
    assert!(
        cache
            .join("elsewhere")
            .join("gamma")
            .join("31")
            .join("inner")
            .join("draft.md")
            .exists(),
        "a linked project is not followed"
    );

    let linked_root = cache.join("second").join(root.file_name().unwrap());
    fs::create_dir_all(linked_root.parent().unwrap()).unwrap();
    assert!(link_folder(&linked_root, &root));
    let refused = clean_work(&linked_root, &closed(&[]), &WorkPolicy::at(now, false));
    assert!(
        matches!(refused, Err(WorkError::Link { .. })),
        "{refused:?}"
    );
}

/// A link to a folder: a symbolic link, or on Windows without the privilege a junction (`mklink /J`, started through
/// the harness, the only way a test starts a process).
fn link_folder(link: &Path, target: &Path) -> bool {
    #[cfg(unix)]
    return std::os::unix::fs::symlink(target, link).is_ok();
    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_dir(target, link).is_ok() {
            return true;
        }
        let system = std::env::var_os("SystemRoot")
            .map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
        let args: Vec<String> = [
            "/C",
            "mklink",
            "/J",
            &link.display().to_string(),
            &target.display().to_string(),
        ]
        .map(str::to_owned)
        .to_vec();
        regia_exec::harness::launch(&system.join("System32").join("cmd.exe"), &args, target)
            .run()
            .is_ok_and(|output| output.status.success())
            && link.is_dir()
    }
}
