//! The tasks' aggregates (`moviola.aggregato/v0`, §5.2, §5.6 and §5.7 of the dossier's `docs/moviola/CONTRATTO.md`),
//! as the prototype's `consolida.py` writes them: the raw store joined with the process fields of the reports, one
//! row a task in `.regia/moviola/aggregati.jsonl`, by task, the whole file written again at once. A row of a task
//! whose raw records are gone is kept: the aggregate is permanent. Then the store's retention runs.
//!
//! - `round`: the highest the task's reports declare; with none, the number of the task's dispatches.
//! - Declared subagents: the sum of the reports'; `null` when none declares them.
//! - The checks: the reports', in the reports' order. Defects found: the distinct ids in the checks, `null` when the
//!   task declares none; closed: the distinct `#<task>/D<n>` of the closed defects of any report of the project.
//!   `closes` lists, on the task that fixes them, the references its own reports close.
//! - Tokens: the sum of the records that have them, `null` when none has (not 0); the two cache counts only where a
//!   record has them.
//! - A task with a dispatch still running is deferred, and its raw records are not marked consolidated.
//! - A task with reports and no raw records (a round done in session, §5.6) has a row when a report declares a
//!   process field: `round` as declared or `null`, no models, `null` tokens, a window from the first to the last day
//!   of its reports. With no process field it has no row, and is listed as such.
//! - A session or a dispatch with no task that a report names in its session field brings its model, as an entry
//!   `via: report` that counts the sessions named; its tokens are not summed. Those entries outlive the store.
//!
//! Where it differs from the prototype: a record's task that is not a whole number is an error, where Python groups
//! by the value as it is; a link on the way to the aggregates, or the aggregates' file a link, is refused, as the
//! moviola's reads refuse one (threat model T05); a token count that is a float sums from left to right, where
//! Python 3.12 compensates the rounding. The other errors are the prototype's: what makes Python raise is an error.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use chrono::{DateTime, Utc};
use regex::Regex;

use super::MoviolaError;
use super::files::read_regular;
use super::format::format;
use super::pyjson::{self, Json, Object};
use super::report::{Report, read_reports};
use super::store::{ActivityStore, Pruned, needs_consolidation, write_atomic};
use crate::pystr;

/// What [`consolidate`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Consolidated {
    pub project: String,
    /// The rows the aggregates have now.
    pub aggregates: usize,
    /// The tasks whose row came again from raw records.
    pub recomputed: Vec<i128>,
    /// The tasks with a dispatch still running.
    pub deferred_in_progress: Vec<i128>,
    /// The tasks whose row came from their reports alone.
    pub report_only: Vec<i128>,
    /// The tasks with reports but no process field, and no row.
    pub without_process_fields: Vec<i128>,
    /// The aggregates' file.
    pub file: PathBuf,
    pub pruning: Pruned,
}

/// The aggregates' file of the project's `folder` (`percorso_aggregati`).
pub fn aggregates_file(folder: &Path) -> PathBuf {
    let format = format();
    let mut path = folder.to_path_buf();
    path.extend(&format.folder);
    path.join(&format.aggregates)
}

/// `_RIF`: a reference to a defect, `#<task>/D<n>`, the `#` optional.
fn reference() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"^#?(\d+)/(D(\d+))$").expect("a valid pattern"))
}

/// A closed defect's task and id, as written, and the number of the id.
fn closed_reference(text: &str) -> Option<(i128, String, i128)> {
    let captures = reference().captures(pystr::strip(text))?;
    Some((pyjson::int_of_text(&captures[1])?, captures[2].to_owned(), pyjson::int_of_text(&captures[3])?))
}

/// `_riferimenti_chiusi`: task → the distinct ids the project's reports close.
fn closed_by_task(reports: &[Report]) -> BTreeMap<i128, BTreeSet<String>> {
    let mut out: BTreeMap<i128, BTreeSet<String>> = BTreeMap::new();
    for reference in reports.iter().flat_map(|report| &report.closed_defects) {
        if let Some((task, id, _)) = closed_reference(reference) {
            out.entry(task).or_default().insert(id);
        }
    }
    out
}

/// `_chiude`: the references the task's own reports close, by task and number, each once.
fn closes(reports: &[&Report]) -> Vec<String> {
    let refs: BTreeSet<(i128, i128)> = reports
        .iter()
        .flat_map(|report| &report.closed_defects)
        .filter_map(|reference| closed_reference(reference).map(|(task, _, number)| (task, number)))
        .collect();
    refs.into_iter().map(|(task, number)| format!("#{task}/D{number}")).collect()
}

fn invalid(place: &str, problem: impl Into<String>) -> MoviolaError {
    MoviolaError::Invalid { place: place.to_owned(), problem: problem.into() }
}

/// What Python's `for item in value or []` walks: a list's items, a string's characters; nothing for what is false;
/// an error for anything else, which Python cannot walk.
fn walk(value: &Json, place: &str, key: &str) -> Result<Vec<Json>, MoviolaError> {
    match value {
        Json::List(items) => Ok(items.clone()),
        Json::Str(text) => Ok(text.chars().map(|c| Json::Str(c.to_string())).collect()),
        other if !other.truthy() => Ok(Vec::new()),
        _ => Err(invalid(place, format!("`{key}` is neither a list nor a string"))),
    }
}

/// A sum as Python's `sum` makes it of integers, booleans and floats, from 0.
#[derive(Clone, Copy)]
enum Sum {
    Int(i128),
    Float(f64),
}

impl Sum {
    fn add(self, value: &Json) -> Option<Sum> {
        let value = match value {
            Json::Bool(value) => Sum::Int(i128::from(*value)),
            Json::Int(value) => Sum::Int(*value),
            Json::Float(value) => Sum::Float(*value),
            _ => return None,
        };
        Some(match (self, value) {
            (Sum::Int(a), Sum::Int(b)) => Sum::Int(a.checked_add(b)?),
            (Sum::Int(a), Sum::Float(b)) => Sum::Float(a as f64 + b),
            (Sum::Float(a), Sum::Int(b)) => Sum::Float(a + b as f64),
            (Sum::Float(a), Sum::Float(b)) => Sum::Float(a + b),
        })
    }

    fn json(self) -> Json {
        match self {
            Sum::Int(value) => Json::Int(value),
            Sum::Float(value) => Json::Float(value),
        }
    }
}

/// `_somma_token`: the sum of the records' token counts, `None` when there are none.
fn sum_tokens(counts: &[&Json], place: &str) -> Result<Json, MoviolaError> {
    if counts.is_empty() {
        return Ok(Json::Null);
    }
    let keys = &format().tokens;
    let tokens = &format().record.tokens;
    let objects: Vec<&Object> =
        counts.iter().map(|count| count.as_object().ok_or_else(|| invalid(place, format!("`{tokens}` is not an object")))).collect::<Result<_, _>>()?;
    let not_a_number = |key: &str| invalid(place, format!("`{tokens}.{key}` is not a number"));
    let mut out = Object::new();
    for key in [&keys.input, &keys.output] {
        let mut sum = Sum::Int(0);
        for object in &objects {
            let value = object.value(key);
            if value.truthy() {
                sum = sum.add(value).ok_or_else(|| not_a_number(key))?;
            }
        }
        out.insert(key.clone(), sum.json());
    }
    for key in [&keys.cache_read, &keys.cache_write] {
        let values: Vec<&Json> = objects.iter().map(|object| object.value(key)).filter(|value| !value.is_null()).collect();
        if values.is_empty() {
            continue;
        }
        let mut sum = Sum::Int(0);
        for value in values {
            sum = sum.add(value).ok_or_else(|| not_a_number(key))?;
        }
        out.insert(key.clone(), sum.json());
    }
    Ok(Json::Object(out))
}

/// A model and what it counted, grouped as a Python `dict` groups its keys: equal values are one key, the first one
/// written kept.
struct Groups<T> {
    entries: Vec<([Json; 3], T)>,
}

impl<T> Groups<T> {
    fn new() -> Groups<T> {
        Groups { entries: Vec::new() }
    }

    fn entry(&mut self, key: [Json; 3], empty: impl FnOnce() -> T) -> &mut T {
        let at = match self.entries.iter().position(|(known, _)| known.iter().zip(&key).all(|(a, b)| a.py_eq(b))) {
            Some(at) => at,
            None => {
                self.entries.push((key, empty()));
                self.entries.len() - 1
            }
        };
        &mut self.entries[at].1
    }

    /// The groups by the `str()` of their key's values, the first written first among equals.
    fn sorted(mut self) -> Vec<([Json; 3], T)> {
        self.entries.sort_by_cached_key(|(key, _)| key.iter().map(Json::py_str).collect::<Vec<_>>());
        self.entries
    }
}

/// An entry of `modelli`.
fn model_entry(key: [Json; 3], sessions: usize, via: bool) -> Json {
    let keys = &format().model;
    let [agent, model, effort] = key;
    let mut entry = Object::new();
    entry.insert(keys.agent.clone(), agent);
    entry.insert(keys.model.clone(), model);
    entry.insert(keys.effort.clone(), effort);
    entry.insert(keys.sessions.clone(), Json::Int(sessions as i128));
    if via {
        entry.insert(keys.via.clone(), Json::Str(format().via_report.clone()));
    }
    Json::Object(entry)
}

fn model_key(record: &Object) -> [Json; 3] {
    let keys = &format().record;
    [record.value(&keys.agent).clone(), record.value(&keys.model).clone(), record.value(&keys.effort).clone()]
}

/// `sessioni_citate`: the records of the whole store with no task whose session or dispatch a report of the task
/// names in its session field.
fn cited<'a>(reports: &[&Report], all: &[&'a Object]) -> Vec<&'a Object> {
    let named: BTreeSet<String> = reports.iter().filter(|report| report.session.truthy()).map(|report| pystr::strip(&report.session.py_str()).to_owned()).collect();
    if named.is_empty() {
        return Vec::new();
    }
    let keys = &format().record;
    let named_here = |value: &Json| value.as_str().is_some_and(|text| named.contains(text));
    all.iter()
        .filter(|record| record.value(&keys.task).is_null() && (named_here(record.value(&keys.session)) || named_here(record.value(&keys.dispatch))))
        .copied()
        .collect()
}

/// The `via: report` entries of the cited sessions: their models, each with the number of distinct sessions.
fn cited_entries(cited: &[&Object]) -> Vec<Json> {
    let keys = &format().record;
    let mut groups: Groups<Vec<Json>> = Groups::new();
    for record in cited {
        let session = record.value(&keys.session);
        let id = if session.truthy() { session.clone() } else { record.value(&keys.dispatch).clone() };
        let ids = groups.entry(model_key(record), Vec::new);
        if !ids.iter().any(|known| known.py_eq(&id)) {
            ids.push(id);
        }
    }
    groups.sorted().into_iter().map(|(key, ids)| model_entry(key, ids.len(), true)).collect()
}

/// A record's instant at `key`, which the window compares as text.
fn instant<'a>(record: &'a Object, key: &str, place: &str) -> Result<&'a str, MoviolaError> {
    match record.get(key) {
        Some(Json::Str(text)) => Ok(text),
        Some(_) => Err(invalid(place, format!("`{key}` is not a string"))),
        None => Err(invalid(place, format!("`{key}` is missing"))),
    }
}

/// `aggrega_task`: one row. `records` are the task's raw records, `cited` the sessions its reports name.
fn aggregate_task(
    project: &str,
    task: i128,
    records: &[&Object],
    reports: &[&Report],
    closed: &BTreeMap<i128, BTreeSet<String>>,
    cited: &[&Object],
    place: &str,
) -> Result<Object, MoviolaError> {
    let keys = &format().record;
    let aggregate = &format().aggregate;
    let mut window = Json::Null;
    if !records.is_empty() {
        let mut starts = Vec::new();
        let mut ends = Vec::new();
        for record in records {
            let start = instant(record, &keys.start, place)?;
            if !record.contains_key(&keys.end) {
                return Err(invalid(place, format!("`{}` is missing", keys.end)));
            }
            let end = if record.value(&keys.end).truthy() { instant(record, &keys.end, place)? } else { start };
            starts.push(start);
            ends.push(end);
        }
        let first = starts.iter().min().copied().unwrap_or_default();
        let last = ends.iter().max().copied().unwrap_or_default();
        window = Json::List(vec![Json::Str(first.to_owned()), Json::Str(last.to_owned())]);
    }
    let dispatches: Vec<&&Object> = records.iter().filter(|record| record.value(&keys.dispatch).truthy()).collect();
    let declared_round = reports.iter().filter_map(|report| report.round).max();
    let round = match declared_round {
        Some(round) => Json::Int(round),
        None if dispatches.is_empty() => Json::Null,
        None => Json::Int(dispatches.len() as i128),
    };
    let subagents: Vec<i128> = reports.iter().filter_map(|report| report.subagents).collect();
    let subagents = if subagents.is_empty() { Json::Null } else { Json::Int(subagents.iter().sum()) };

    let mut groups: Groups<usize> = Groups::new();
    for record in &dispatches {
        *groups.entry(model_key(record), || 0) += 1;
    }
    let mut models: Vec<Json> = groups.sorted().into_iter().map(|(key, count)| model_entry(key, count, false)).collect();
    models.extend(cited_entries(cited));

    let check = &format().check;
    let mut checks = Vec::new();
    let mut found: BTreeSet<String> = BTreeSet::new();
    for report in reports {
        for item in &report.checks {
            let mut entry = Object::new();
            entry.insert(check.kind.clone(), item.value(&check.kind).clone());
            entry.insert(check.outcome.clone(), item.value(&check.outcome).clone());
            if !item.value(&check.model).is_null() {
                entry.insert(check.model.clone(), item.value(&check.model).clone());
            }
            let defects: Vec<String> = walk(item.value(&check.defects), &report.file, &check.defects)?.iter().map(Json::py_str).collect();
            found.extend(defects.iter().cloned());
            entry.insert(check.defects.clone(), Json::List(defects.into_iter().map(Json::Str).collect()));
            checks.push(Json::Object(entry));
        }
    }
    let defect = &format().defects;
    let has_checks = reports.iter().any(|report| report.has_checks);
    let closed_here = closed.get(&task).map_or(0, BTreeSet::len);
    let mut defects = Object::new();
    if has_checks || closed_here > 0 {
        defects.insert(defect.found.clone(), Json::Int(found.len() as i128));
        defects.insert(defect.closed.clone(), Json::Int(closed_here as i128));
    } else {
        defects.insert(defect.found.clone(), Json::Null);
        defects.insert(defect.closed.clone(), Json::Null);
    }
    let closing = closes(reports);
    if !closing.is_empty() {
        defects.insert(defect.closes.clone(), Json::List(closing.into_iter().map(Json::Str).collect()));
    }

    let counts: Vec<&Json> = records.iter().map(|record| record.value(&keys.tokens)).filter(|tokens| tokens.truthy()).collect();
    let tokens = sum_tokens(&counts, place)?;

    let mut row = Object::new();
    row.insert(aggregate.schema.clone(), Json::Str(format().aggregate_schema.clone()));
    row.insert(aggregate.project.clone(), Json::Str(project.to_owned()));
    row.insert(aggregate.task.clone(), Json::Int(task));
    row.insert(aggregate.window.clone(), window);
    row.insert(aggregate.round.clone(), round);
    row.insert(aggregate.declared_subagents.clone(), subagents);
    row.insert(aggregate.models.clone(), Json::List(models));
    row.insert(aggregate.checks.clone(), Json::List(checks));
    row.insert(aggregate.defects.clone(), Json::Object(defects));
    row.insert(aggregate.tokens.clone(), tokens);
    Ok(row)
}

/// `aggrega_solo_report`: the row of a task with no raw records, from its reports.
fn aggregate_reports_only(
    project: &str,
    task: i128,
    reports: &[&Report],
    closed: &BTreeMap<i128, BTreeSet<String>>,
    cited: &[&Object],
    place: &str,
) -> Result<Object, MoviolaError> {
    let aggregate = &format().aggregate;
    let mut dates: Vec<&str> = reports.iter().filter_map(|report| report.date.as_deref()).filter(|date| !date.is_empty()).collect();
    dates.sort_unstable();
    let mut row = aggregate_task(project, task, &[], reports, closed, cited, place)?;
    let window = match (dates.first(), dates.last()) {
        (Some(first), Some(last)) => Json::List(vec![Json::Str(format!("{first}T00:00:00Z")), Json::Str(format!("{last}T23:59:59Z"))]),
        _ => Json::Null,
    };
    row.insert(aggregate.window.clone(), window);
    row.insert(aggregate.round.clone(), reports.iter().filter_map(|report| report.round).max().map_or(Json::Null, Json::Int));
    let subagents: Vec<i128> = reports.iter().filter_map(|report| report.subagents).collect();
    row.insert(aggregate.declared_subagents.clone(), if subagents.is_empty() { Json::Null } else { Json::Int(subagents.iter().sum()) });
    Ok(row)
}

/// The models of a row, as Python's `row.get(models) or []` walks them: each must be an object.
fn models_of<'a>(row: &'a Object, place: &str) -> Result<Vec<&'a Object>, MoviolaError> {
    let key = &format().aggregate.models;
    let not_objects = || invalid(place, format!("`{key}` holds a value that is not an object"));
    match row.value(key) {
        Json::List(items) => items.iter().map(|item| item.as_object().ok_or_else(not_objects)).collect(),
        other if !other.truthy() => Ok(Vec::new()),
        _ => Err(not_objects()),
    }
}

fn is_via_report(entry: &Object) -> bool {
    entry.value(&format().model.via).as_str() == Some(format().via_report.as_str())
}

/// `_con_citate_precedenti`: the `via: report` entries of the row already written that the new one has not, kept.
fn with_earlier_citations(mut row: Object, existing: Option<&Object>, place: &str) -> Result<Object, MoviolaError> {
    let Some(existing) = existing.filter(|existing| !existing.is_empty()) else { return Ok(row) };
    let keys = &format().model;
    let triple = |entry: &Object| [entry.value(&keys.agent).clone(), entry.value(&keys.model).clone(), entry.value(&keys.effort).clone()];
    let new: Vec<[Json; 3]> = models_of(&row, place)?.into_iter().filter(|entry| is_via_report(entry)).map(triple).collect();
    let kept: Vec<Json> = models_of(existing, place)?
        .into_iter()
        .filter(|entry| is_via_report(entry) && !new.iter().any(|known| known.iter().zip(&triple(entry)).all(|(a, b)| a.py_eq(b))))
        .map(|entry| Json::Object(entry.clone()))
        .collect();
    if !kept.is_empty() {
        let key = format().aggregate.models.clone();
        let mut models = row.value(&key).as_list().map(<[Json]>::to_vec).unwrap_or_default();
        models.extend(kept);
        row.insert(key, Json::List(models));
    }
    Ok(row)
}

/// `_leggi_righe`: the rows already written, by task. A line that is not JSON, or has no task Python's `int()` reads,
/// is dropped, as the prototype drops it.
fn read_rows(folder: &Path, shown: &str) -> Result<BTreeMap<i128, Object>, MoviolaError> {
    let mut out = BTreeMap::new();
    let Some(dir) = super::files::moviola_dir(folder)? else { return Ok(out) };
    let Some(text) = read_regular(&dir.join(&format().aggregates), shown)? else { return Ok(out) };
    for line in pystr::split_lines(&text) {
        if pystr::strip(line).is_empty() {
            continue;
        }
        let Ok(Json::Object(row)) = pyjson::loads(line) else { continue };
        match row.get(&format().aggregate.task).map(pyjson::int_of) {
            Some(pyjson::IntOf::Value(task)) => {
                out.insert(task, row);
            }
            Some(pyjson::IntOf::Overflow) => return Err(invalid(shown, format!("`{}` is an infinite number", format().aggregate.task))),
            Some(pyjson::IntOf::Not) | None => {}
        }
    }
    Ok(out)
}

/// The moviola's folder of the dossier, created where it is missing, never through a link. A dossier's folder that
/// is not there is made too, as the prototype's `mkdir(parents=True)` makes it.
fn moviola_dir_for_writing(folder: &Path) -> Result<PathBuf, MoviolaError> {
    if fs::symlink_metadata(folder).is_err_and(|error| error.kind() == ErrorKind::NotFound) {
        fs::create_dir_all(folder).map_err(|source| MoviolaError::Write { file: folder.display().to_string(), source })?;
    }
    let mut dir = folder.to_path_buf();
    let mut walked: Vec<&str> = Vec::new();
    for segment in &format().folder {
        dir.push(segment);
        walked.push(segment);
        match fs::symlink_metadata(&dir) {
            Err(error) if error.kind() == ErrorKind::NotFound => {
                fs::create_dir(&dir).map_err(|source| MoviolaError::Write { file: walked.join("/"), source })?;
            }
            Err(source) => return Err(MoviolaError::Read { file: walked.join("/"), source }),
            Ok(metadata) if metadata.file_type().is_symlink() => return Err(MoviolaError::Link(walked.join("/"))),
            Ok(metadata) if !metadata.is_dir() => return Err(MoviolaError::NotAFile(walked.join("/"))),
            Ok(_) => {}
        }
    }
    Ok(dir)
}

/// Writes the aggregates of `project`, whose dossier is `folder`, from `store` and the reports, then prunes the
/// store at `now` (`consolida`). `now` is also the instant written beside each task the store records as
/// consolidated, which the prototype takes from its clock.
pub fn consolidate(project: &str, folder: &Path, store: &ActivityStore, now: DateTime<Utc>) -> Result<Consolidated, MoviolaError> {
    let keys = &format().record;
    let read = store.read()?;
    let all: Vec<&Object> = read.values().collect();
    let store_place = store.folder().display().to_string();

    let mut per_task: Vec<(i128, Vec<&Object>)> = Vec::new();
    let mut index: HashMap<i128, usize> = HashMap::new();
    for record in &all {
        if record.value(&keys.project).as_str() != Some(project) || !needs_consolidation(record) {
            continue;
        }
        let task = match record.value(&keys.task) {
            Json::Int(task) => *task,
            _ => return Err(invalid(&store_place, format!("`{}` of a record is not a whole number", keys.task))),
        };
        let at = *index.entry(task).or_insert_with(|| {
            per_task.push((task, Vec::new()));
            per_task.len() - 1
        });
        per_task[at].1.push(record);
    }
    let running = |record: &&Object| record.value(&keys.outcome).as_str() == Some(keys.running.as_str());
    let mut deferred: Vec<i128> = per_task.iter().filter(|(_, records)| records.iter().any(running)).map(|(task, _)| *task).collect();
    deferred.sort_unstable();
    per_task.retain(|(task, _)| !deferred.contains(task));

    let reports = read_reports(folder)?;
    let closed = closed_by_task(&reports);
    let of_task = |task: i128| reports.iter().filter(|report| report.task == task).collect::<Vec<&Report>>();

    let file = aggregates_file(folder);
    let mut shown: Vec<&str> = format().folder.iter().map(String::as_str).collect();
    shown.push(&format().aggregates);
    let shown = shown.join("/");
    let mut rows = read_rows(folder, &shown)?;

    for (task, records) in &per_task {
        let reports = of_task(*task);
        let row = aggregate_task(project, *task, records, &reports, &closed, &cited(&reports, &all), &store_place)?;
        let row = with_earlier_citations(row, rows.get(task), &shown)?;
        rows.insert(*task, row);
    }
    let with_records: BTreeSet<i128> = per_task.iter().map(|(task, _)| *task).collect();
    let reported: BTreeSet<i128> = reports.iter().map(|report| report.task).collect();
    let mut report_only = Vec::new();
    let mut without_process_fields = Vec::new();
    for task in reported.into_iter().filter(|task| !with_records.contains(task) && !deferred.contains(task)) {
        let reports = of_task(task);
        let existing = rows.get(&task);
        if !reports.iter().any(|report| report.has_process) {
            if existing.is_none() {
                without_process_fields.push(task);
            }
            continue;
        }
        if let Some(existing) = existing
            && models_of(existing, &shown)?.into_iter().any(|entry| !is_via_report(entry))
        {
            // A row born of raw records now expired: it is kept as it is.
            continue;
        }
        let row = aggregate_reports_only(project, task, &reports, &closed, &cited(&reports, &all), &store_place)?;
        let row = with_earlier_citations(row, existing, &shown)?;
        rows.insert(task, row);
        report_only.push(task);
    }

    let text: String = rows.values().map(|row| format!("{}\n", pyjson::dumps_object(row))).collect();
    let dir = moviola_dir_for_writing(folder)?;
    let path = dir.join(&format().aggregates);
    if read_regular(&path, &shown)?.as_deref() != Some(text.as_str()) {
        write_atomic(&path, &text).map_err(|source| MoviolaError::Write { file: shown.clone(), source })?;
    }

    // Only now: the raw records of these tasks are safe in the aggregates.
    for (task, records) in &per_task {
        store.mark_consolidated(project, *task, records, now)?;
    }
    let pruning = store.prune(now)?;
    Ok(Consolidated {
        project: project.to_owned(),
        aggregates: rows.len(),
        recomputed: with_records.into_iter().collect(),
        deferred_in_progress: deferred,
        report_only,
        without_process_fields,
        file,
        pruning,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reference_reads_with_or_without_its_hash_and_is_normalized_where_it_closes() {
        assert_eq!(closed_reference(" #12/D01 "), Some((12, "D01".to_owned(), 1)));
        assert_eq!(closed_reference("12/D3"), Some((12, "D3".to_owned(), 3)));
        assert_eq!(closed_reference("D3"), None);
        assert_eq!(closed_reference("#12/D"), None);
    }

    #[test]
    fn sums_are_python_s() {
        let sum = |values: &[Json]| values.iter().try_fold(Sum::Int(0), |sum, value| sum.add(value)).map(Sum::json);
        assert_eq!(sum(&[Json::Int(1), Json::Bool(true)]).map(|value| pyjson::dumps(&value)), Some("2".to_owned()));
        assert_eq!(sum(&[Json::Int(1), Json::Float(0.5)]).map(|value| pyjson::dumps(&value)), Some("1.5".to_owned()));
        assert!(sum(&[Json::Str("1".to_owned())]).is_none());
    }
}
