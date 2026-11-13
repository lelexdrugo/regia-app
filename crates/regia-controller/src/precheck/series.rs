//! The series of a stall (§6.3 of the dossier's design note, R4-R8, #133): the detector's own row for
//! each project counts the ticks in a row that observed the same facts and found no progress, by the
//! digest of those facts. Only a tick with new activity or a held lock counts; any other idle tick ends
//! the series, and an error leaves it as it was. A postponement of the controller (its `rinvio` cursor,
//! the project's or the global one) is not a progress missed: the tick takes the place of the one put
//! off, or, for the global one, the series starts again.

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use super::cursor::{instant, truthy};
use super::line::Json;
use super::v1::format;
use super::{Fault, Sources};
use regia_store::WallTime;

/// The prototype's `_digest`: the first characters of the SHA-256 of the facts, as `json.dumps` writes
/// them compact and in ASCII. The facts are lists only, so the order of keys plays no part.
pub(crate) fn digest(facts: &Json) -> String {
    let hash = Sha256::digest(facts.to_ascii().as_bytes());
    let hex: String = hash.iter().map(|byte| format!("{byte:02x}")).collect();
    hex[..format().series.digest_length].to_owned()
}

/// `_serie`: the series in the detector's row, none when there is no row or it does not read, and then
/// the count starts again.
fn series_of(row: &Value) -> Option<Map<String, Value>> {
    let words = &format().series;
    let series = row.as_object()?.get(&words.key)?.as_object()?;
    let readable = series.get(&words.digest).is_some_and(Value::is_string)
        && series.get(&words.count).is_some_and(|count| count.is_i64() || count.is_u64())
        && series.get(&words.since).is_some_and(truthy);
    readable.then(|| series.clone())
}

/// A postponement the controller signalled: when, and its id. One that does not read is ignored.
struct Postponement {
    at: WallTime,
    id: String,
}

fn postponement(value: &Value) -> Option<Postponement> {
    let words = &format().series;
    let signal = value.as_object()?;
    let id = signal.get(&words.postponement_id)?.as_str()?.to_owned();
    let at = instant(signal.get(&words.postponement_t).unwrap_or(&Value::Null)).ok()?;
    Some(Postponement { at, id })
}

/// What the accounting of a tick found: how long the series is, from when, and whether a postponement
/// kept it from growing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Accounted {
    pub(crate) count: i64,
    pub(crate) since: Option<String>,
    pub(crate) postponed: bool,
}

/// The prototype's `aggiorna_contabilita` for `project` at `now`: the tick counts when `countable`, with
/// the digest of what it observed. Writes the detector's row, and reads the controller's signals.
pub(crate) fn account(sources: &dyn Sources, project: &str, countable: bool, observed: Option<&str>, now: WallTime) -> Result<Accounted, Fault> {
    let words = &format().series;
    let version = &format().detector.version;
    let row = |series: Value| Value::Object(Map::from_iter([(version.clone(), Value::from(1)), (words.key.clone(), series)]));
    let series = series_of(&sources.detector(project).map_err(Fault::runtime)?);
    if !countable {
        if series.is_some() {
            sources.write_detector(project, row(Value::Null)).map_err(Fault::runtime)?;
        }
        return Ok(Accounted::default());
    }
    let signal = |owner: &str| sources.signal(owner, &words.postponement).map(|value| postponement(&value)).map_err(Fault::runtime);
    let (own, global) = (signal(project)?, signal(&format().detector.global_project)?);
    let mut seen = series.as_ref().and_then(|series| series.get(&words.seen)).and_then(Value::as_object).cloned().unwrap_or_default();
    let fresh = |signal: &Option<Postponement>, key: &str| -> Result<bool, Fault> {
        let (Some(signal), Some(series)) = (signal, series.as_ref()) else { return Ok(false) };
        let last = series.get(&words.last).ok_or_else(|| Fault::internal(format!("the series has no `{}`", words.last)))?;
        Ok(signal.at >= instant(last)? && seen.get(key).and_then(Value::as_str) != Some(signal.id.as_str()))
    };
    let (waited, postponed) = (fresh(&global, &words.seen_global)?, fresh(&own, &words.seen_project)?);
    for (key, signal) in [(&words.seen_project, &own), (&words.seen_global, &global)] {
        if let Some(signal) = signal {
            seen.insert(key.clone(), Value::from(signal.id.as_str()));
        }
    }
    let series = if waited { None } else { series };
    let now_text = now.to_iso_seconds();
    let observed = observed.unwrap_or_default();
    let same = series.as_ref().filter(|series| series.get(&words.digest).and_then(Value::as_str) == Some(observed));
    let (series, postponed) = match same {
        Some(series) => {
            let mut series = series.clone();
            let count = series.get(&words.count).and_then(Value::as_i64).unwrap_or(0);
            series.insert(words.count.clone(), Value::from(if postponed { count } else { count + 1 }));
            series.insert(words.last.clone(), Value::from(now_text.as_str()));
            series.insert(words.seen.clone(), Value::Object(seen));
            (series, postponed)
        }
        None => {
            let mut series = Map::new();
            series.insert(words.digest.clone(), Value::from(observed));
            series.insert(words.count.clone(), Value::from(1));
            series.insert(words.since.clone(), Value::from(now_text.as_str()));
            series.insert(words.last.clone(), Value::from(now_text.as_str()));
            series.insert(words.seen.clone(), Value::Object(seen));
            (series, false)
        }
    };
    let count = series.get(&words.count).and_then(Value::as_i64).unwrap_or(1);
    let since = series.get(&words.since).cloned();
    sources.write_detector(project, row(Value::Object(series))).map_err(Fault::runtime)?;
    let since = since.map(|since| since.as_str().map_or_else(|| since.to_string(), str::to_owned));
    Ok(Accounted { count, since, postponed })
}

/// `in_stallo`: at least `ticks` ticks, and `seconds` from the series' start to `now`.
pub(crate) fn stalled(accounted: &Accounted, ticks: i64, seconds: f64, now: WallTime) -> Result<bool, Fault> {
    if accounted.count < ticks {
        return Ok(false);
    }
    let since = instant(&Value::from(accounted.since.clone().unwrap_or_default()))?;
    Ok(since.seconds_until(now) >= seconds)
}

/// `stallo_segnalato`: the Objective is in decision, a fact of the tracker, or the alerting step wrote
/// the stall of this digest less than `realert_s` seconds before `now`.
pub(crate) fn reported(sources: &dyn Sources, project: &str, objective_in_decision: bool, observed: &str, realert_s: f64, now: WallTime) -> Result<bool, Fault> {
    if objective_in_decision {
        return Ok(true);
    }
    let words = &format().series;
    let reports = sources.signal(project, &words.reports).map_err(Fault::runtime)?;
    let key = words.reported.replace("{digest}", observed);
    match reports.as_object().and_then(|reports| reports.get(&key)).filter(|when| !when.is_null()) {
        Some(when) => Ok(instant(when)?.seconds_until(now) < realert_s),
        None => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Python's `hashlib.sha256(json.dumps(x, sort_keys=True, separators=(",", ":")).encode()).hexdigest()[:12]`,
    /// recorded from 3.12 on the facts of a held lock and of a ready task.
    #[test]
    fn the_digest_is_the_prototype_s() {
        let words = &format().series;
        let lock = Json::List(vec![Json::Str(words.lock.clone()), Json::Str(words.contended.clone()), Json::Null, Json::Null, Json::Null, Json::Null]);
        assert_eq!(digest(&lock), "362ddf8a6d92");
        let empty = || Json::List(Vec::new());
        let ready = Json::List(vec![Json::Str(words.events.clone()), Json::List(vec![Json::Int(2)]), empty(), empty(), empty(), empty()]);
        assert_eq!(digest(&ready), "d336feef640d");
        let abc = Sha256::digest(b"abc");
        assert_eq!(format!("{:02x}{:02x}{:02x}", abc[0], abc[1], abc[2]), "ba7816", "the standard's vector");
    }
}
