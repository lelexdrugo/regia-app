//! The compaction of the sessions' log of declarations (#471; R15 of the dossier's inventory, #269/V11): the files
//! of a session and a process, about one per command, merge into one file a month. Nothing is deleted from the log:
//! the window in which the moviola's `sync` reads a declaration again has no end (the report of #471), so a
//! declaration never expires; only the number of files goes down.
//!
//! The rule that keeps it safe: **the reader returns the same events, in the same order, before and after.** The
//! prototype's `sync` reads only what its `leggi` returns, so it attributes as before.
//! - A file merges only when it is a plain file, not a link; it has not been written for the quiet time (the writer
//!   appends to it again only if a later process of the same session gets the same pid); every line of it is an
//!   event the reader keeps, each ended by `\n` alone; and its lines fall in one month. Any other file stays as it is.
//! - The month's file holds its lines in the reader's order. The order of equal instants across files depends on
//!   the files' names, so each month's merge is first done in memory and read back with the reader: a month whose
//!   merge would change the order is left for another time.
//! - The month's file is written whole and then put in place, and only after that the merged files go: a reader in
//!   between sees a line twice, never none. A plan written before and removed after lets the next run finish a run
//!   that stopped halfway, before it merges anything else.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use crate::declaration_reader::{self, LogReadError, LoggedDeclaration};
use crate::time::WallTime;
use crate::v1::format;

/// The log's folder under the state folder `state`.
#[must_use]
pub fn log_folder(state: &Path) -> PathBuf {
    state.join(&format().declarations.folder)
}

/// How a compaction runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactionPolicy {
    pub now: WallTime,
    /// A file written less than this long ago stays as it is.
    pub quiet_secs: i64,
    /// Count what would merge, and write nothing.
    pub dry_run: bool,
}

impl CompactionPolicy {
    /// The format's quiet time, at `now`.
    #[must_use]
    pub fn at(now: WallTime, dry_run: bool) -> CompactionPolicy {
        CompactionPolicy {
            now,
            quiet_secs: format().compaction.quiet_hours * 3600,
            dry_run,
        }
    }
}

/// What a compaction did, or would do on a dry run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Compacted {
    pub dry_run: bool,
    /// The files merged into the month files.
    pub merged: usize,
    /// Their lines.
    pub lines: usize,
    /// The month files written, by name.
    pub months: Vec<String>,
    /// Files written within the quiet time.
    pub kept_recent: usize,
    /// Files that are links, do not read, or hold a line the reader skips or an end other than `\n`.
    pub kept_unclean: usize,
    /// Files whose lines fall in more than one month.
    pub kept_spanning: usize,
    /// Files of a month whose merge would change the reader's order.
    pub kept_order: usize,
    /// Files written to while they were merged: they stay, and their lines are in the month's file too.
    pub kept_changed: usize,
    /// A compaction that stopped halfway was finished first.
    pub recovered: bool,
}

/// Why a compaction stopped. Whatever it wrote stays consistent: the next run finishes it.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CompactionError {
    #[error(transparent)]
    Read(#[from] LogReadError),
    #[error("{} is a link: the log's folder is not followed", folder.display())]
    Link { folder: PathBuf },
    #[error("cannot {action} {}", path.display())]
    Io {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the plan {} does not read: {reason}", path.display())]
    Plan { path: PathBuf, reason: String },
    #[error("after the compaction the log in {} no longer reads as before", folder.display())]
    Changed { folder: PathBuf },
}

/// The plan of a compaction, written before the month files and removed after the merged files went.
#[derive(Debug, Serialize, Deserialize)]
struct Plan {
    v: u32,
    months: Vec<PlannedMonth>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PlannedMonth {
    file: String,
    /// The month file's length before, `None` when it was not there.
    before: Option<u64>,
    after: u64,
    sources: Vec<PlannedSource>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PlannedSource {
    file: String,
    len: u64,
    mtime: i64,
}

fn io(action: &'static str, path: &Path) -> impl FnOnce(std::io::Error) -> CompactionError {
    let path = path.to_path_buf();
    move |source| CompactionError::Io {
        action,
        path,
        source,
    }
}

/// A link, a junction, or on Windows any reparse point: never followed, never merged, never removed.
pub(crate) fn is_link(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const REPARSE_POINT: u32 = 0x400;
        meta.file_type().is_symlink() || meta.file_attributes() & REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

/// When a file was last written, in microseconds since the epoch.
pub(crate) fn modified_micros(meta: &fs::Metadata) -> Option<i64> {
    let since = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(since.as_micros()).ok()
}

/// The month of an instant, `AAAA-MM` in UTC.
fn month_of(at: WallTime) -> String {
    at.to_iso_seconds()[..7].to_owned()
}

/// The month file of `month`.
fn month_file(month: &str) -> String {
    format!("{}{month}.jsonl", format().compaction.month_prefix)
}

/// Whether `name` is a month file: the prefix, `AAAA-MM`, `.jsonl`.
fn is_month_file(name: &str) -> bool {
    let Some(month) = name
        .strip_prefix(&format().compaction.month_prefix)
        .and_then(|rest| rest.strip_suffix(".jsonl"))
    else {
        return false;
    };
    month.len() == 7
        && month.bytes().enumerate().all(|(at, byte)| {
            if at == 4 {
                byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        })
}

/// The one month of a clean file's lines, or why it stays.
enum Shape {
    Month(String),
    Unclean,
    Spanning,
}

fn shape(name: &str, text: &str) -> Shape {
    let lines = declaration_reader::split_lines(text);
    if lines.is_empty()
        || lines
            .iter()
            .map(|line| format!("{line}\n"))
            .collect::<String>()
            != text
    {
        return Shape::Unclean;
    }
    let mut months = BTreeSet::new();
    for (index, line) in lines.iter().enumerate() {
        match declaration_reader::event(line, name, index) {
            Some(event) => months.insert(month_of(event.at)),
            None => return Shape::Unclean,
        };
    }
    match months.len() {
        1 => Shape::Month(months.into_iter().next().unwrap_or_default()),
        _ => Shape::Spanning,
    }
}

/// What the reader sees of an event: the order compares these.
fn seen(events: &[LoggedDeclaration]) -> Vec<(i64, &str)> {
    events
        .iter()
        .map(|event| (event.at.unix_micros(), event.line.as_str()))
        .collect()
}

/// The texts of the folder after merging, for each month file, its sources: the sources go, each month file holds
/// its own lines and its sources' in the reader's order.
fn merged_texts(
    texts: &[(String, Option<String>)],
    before: &[LoggedDeclaration],
    merges: &BTreeMap<String, BTreeSet<String>>,
) -> Vec<(String, Option<String>)> {
    let gone: BTreeSet<&String> = merges.values().flatten().collect();
    let mut out: Vec<(String, Option<String>)> = texts
        .iter()
        .filter(|(name, _)| !gone.contains(name) && !merges.contains_key(name))
        .cloned()
        .collect();
    for (month_file, sources) in merges {
        let text: String = before
            .iter()
            .filter(|event| &event.file == month_file || sources.contains(&event.file))
            .map(|event| format!("{}\n", event.line))
            .collect();
        out.push((month_file.clone(), Some(text)));
    }
    out.sort_by_key(|(name, _)| declaration_reader::sort_key(name));
    out
}

/// Writes `text` to `path` whole: a file beside it, flushed, then put in its place.
fn write_whole(path: &Path, text: &[u8]) -> Result<(), CompactionError> {
    let mut name = path
        .file_name()
        .map(std::ffi::OsStr::to_os_string)
        .unwrap_or_default();
    name.push(".tmp");
    let tmp = path.with_file_name(name);
    let mut file = fs::File::create(&tmp).map_err(io("create", &tmp))?;
    file.write_all(text).map_err(io("write", &tmp))?;
    file.sync_all().map_err(io("flush", &tmp))?;
    drop(file);
    // A reader that has the file open keeps it on Windows for a moment: a few tries, then the error.
    let mut tries = 0;
    loop {
        match fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(_) if tries < 5 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(source) => {
                return Err(CompactionError::Io {
                    action: "replace",
                    path: path.to_path_buf(),
                    source,
                });
            }
        }
    }
}

fn tmp_of(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(std::ffi::OsStr::to_os_string)
        .unwrap_or_default();
    name.push(".tmp");
    path.with_file_name(name)
}

/// Finishes the compaction the plan at `plan_path` describes: where the month file was put in place, its sources
/// that did not change since go; where it was not, nothing does. The plan goes last, so a source that cannot be
/// removed keeps it for the next run.
fn recover(folder: &Path, plan_path: &Path) -> Result<(), CompactionError> {
    let text = fs::read_to_string(plan_path).map_err(io("read", plan_path))?;
    let plan: Plan = serde_json::from_str(&text).map_err(|error| CompactionError::Plan {
        path: plan_path.to_path_buf(),
        reason: error.to_string(),
    })?;
    for month in &plan.months {
        let path = folder.join(&month.file);
        let len = fs::metadata(&path).ok().map(|meta| meta.len());
        if len == Some(month.after) {
            for source in &month.sources {
                let source_path = folder.join(&source.file);
                match fs::symlink_metadata(&source_path) {
                    Ok(meta)
                        if !is_link(&meta)
                            && meta.len() == source.len
                            && modified_micros(&meta) == Some(source.mtime) =>
                    {
                        fs::remove_file(&source_path).map_err(io("remove", &source_path))?;
                    }
                    _ => {}
                }
            }
        }
        let _ = fs::remove_file(tmp_of(&path));
    }
    fs::remove_file(plan_path).map_err(io("remove", plan_path))
}

/// Compacts the log in `folder` by `policy`; no folder, nothing to do.
///
/// # Errors
///
/// The folder is a link, a file of the log is not UTF-8, a plan left by a run that stopped does not read, a file
/// cannot be written or removed, or the log reads differently after the merge.
pub fn compact_declarations(
    folder: &Path,
    policy: &CompactionPolicy,
) -> Result<Compacted, CompactionError> {
    let mut out = Compacted {
        dry_run: policy.dry_run,
        ..Compacted::default()
    };
    let meta = match fs::symlink_metadata(folder) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(source) => {
            return Err(CompactionError::Io {
                action: "read",
                path: folder.to_path_buf(),
                source,
            });
        }
    };
    if is_link(&meta) {
        return Err(CompactionError::Link {
            folder: folder.to_path_buf(),
        });
    }
    let plan_path = folder.join(&format().compaction.plan);
    if !policy.dry_run && plan_path.exists() {
        recover(folder, &plan_path)?;
        out.recovered = true;
    }
    let files = declaration_reader::list(folder);
    let texts = declaration_reader::texts(folder, &files)?;
    let before = declaration_reader::events(&texts);

    // The files that may merge, by month; a month file that is not clean is never rewritten.
    let mut by_month: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut unclean_months = BTreeSet::new();
    let mut sources_meta: BTreeMap<String, (u64, i64)> = BTreeMap::new();
    for (name, text) in &texts {
        let path = folder.join(name);
        let Ok(meta) = fs::symlink_metadata(&path) else {
            out.kept_unclean += 1;
            continue;
        };
        let shape = match text {
            Some(text) if !is_link(&meta) && meta.is_file() => shape(name, text),
            _ => Shape::Unclean,
        };
        if is_month_file(name) {
            if !matches!(shape, Shape::Month(ref month) if month_file(month) == *name) {
                unclean_months.insert(name.clone());
            }
            continue;
        }
        let Some(modified) = modified_micros(&meta) else {
            out.kept_unclean += 1;
            continue;
        };
        if policy.now.unix_micros() - modified < policy.quiet_secs.saturating_mul(1_000_000) {
            out.kept_recent += 1;
            continue;
        }
        match shape {
            Shape::Month(month) => {
                sources_meta.insert(name.clone(), (meta.len(), modified));
                by_month
                    .entry(month_file(&month))
                    .or_default()
                    .insert(name.clone());
            }
            Shape::Unclean => out.kept_unclean += 1,
            Shape::Spanning => out.kept_spanning += 1,
        }
    }

    // Each month's merge, read back in memory: the reader must see what it sees now.
    let now_seen = seen(&before);
    let mut accepted: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (month_file, sources) in by_month {
        let alone = BTreeMap::from([(month_file.clone(), sources.clone())]);
        let same = !unclean_months.contains(&month_file)
            && seen(&declaration_reader::events(&merged_texts(
                &texts, &before, &alone,
            ))) == now_seen;
        if same {
            accepted.insert(month_file, sources);
        } else {
            out.kept_order += sources.len();
        }
    }
    let after_texts = merged_texts(&texts, &before, &accepted);
    if !accepted.is_empty() && seen(&declaration_reader::events(&after_texts)) != now_seen {
        out.kept_order += accepted.values().map(BTreeSet::len).sum::<usize>();
        accepted.clear();
    }
    out.merged = accepted.values().map(BTreeSet::len).sum();
    out.lines = before
        .iter()
        .filter(|event| {
            accepted
                .values()
                .any(|sources| sources.contains(&event.file))
        })
        .count();
    out.months = accepted.keys().cloned().collect();
    if policy.dry_run || accepted.is_empty() {
        return Ok(out);
    }

    // The plan, then each month file whole, then the sources, then no plan.
    let new_text = |month_file: &str| {
        after_texts
            .iter()
            .find(|(name, _)| name == month_file)
            .and_then(|(_, text)| text.clone())
            .unwrap_or_default()
    };
    let plan = Plan {
        v: 1,
        months: accepted
            .iter()
            .map(|(month_file, sources)| PlannedMonth {
                file: month_file.clone(),
                before: fs::metadata(folder.join(month_file))
                    .ok()
                    .map(|meta| meta.len()),
                after: new_text(month_file).len() as u64,
                sources: sources
                    .iter()
                    .map(|file| PlannedSource {
                        file: file.clone(),
                        len: sources_meta[file].0,
                        mtime: sources_meta[file].1,
                    })
                    .collect(),
            })
            .collect(),
    };
    let plan_text = serde_json::to_vec(&plan).map_err(|error| CompactionError::Plan {
        path: plan_path.clone(),
        reason: error.to_string(),
    })?;
    write_whole(&plan_path, &plan_text)?;
    for month in &plan.months {
        write_whole(&folder.join(&month.file), new_text(&month.file).as_bytes())?;
    }
    for month in &plan.months {
        for source in &month.sources {
            let path = folder.join(&source.file);
            match fs::symlink_metadata(&path) {
                Ok(meta)
                    if meta.len() == source.len && modified_micros(&meta) == Some(source.mtime) =>
                {
                    fs::remove_file(&path).map_err(io("remove", &path))?;
                }
                Ok(_) => out.kept_changed += 1,
                Err(_) => {}
            }
        }
    }
    fs::remove_file(&plan_path).map_err(io("remove", &plan_path))?;

    // What the reader sees now holds what it saw, in the same order: a line written meanwhile may come between.
    let after = read_seen(folder)?;
    let mut rest = after.iter();
    if !now_seen
        .iter()
        .all(|wanted| rest.any(|got| (got.0, got.1.as_str()) == *wanted))
    {
        return Err(CompactionError::Changed {
            folder: folder.to_path_buf(),
        });
    }
    Ok(out)
}

fn read_seen(folder: &Path) -> Result<Vec<(i64, String)>, CompactionError> {
    Ok(declaration_reader::read_declarations(folder)?
        .into_iter()
        .map(|event| (event.at.unix_micros(), event.line))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_month_file_is_the_prefix_a_month_and_the_extension() {
        let name = month_file("2026-10");
        assert!(is_month_file(&name), "{name}");
        assert!(!is_month_file(&name.replace("2026-10", "2026-1")));
        assert!(!is_month_file("s.1.jsonl"));
        assert_eq!(
            month_of(WallTime::parse_iso("2026-10-31T23:59:59Z").unwrap()),
            "2026-10"
        );
    }
}
