//! The v1 format of the state, from `formats/store_v1.toml`, and the SQL built on its names.

use std::sync::OnceLock;

use serde::Deserialize;

const FORMAT: &str = include_str!("../formats/store_v1.toml");

#[derive(Debug, Deserialize)]
pub(crate) struct Format {
    pub table: Table,
    pub kinds: Kinds,
    pub lock: LockKeys,
    pub lease: LeaseKeys,
    pub cursor: CursorKeys,
    pub step: StepKeys,
    pub events: EventsKeys,
    pub declarations: DeclarationKeys,
    pub monotonic: MonotonicKeys,
    pub view: View,
    pub budget: BudgetKeys,
    pub usage: UsageKeys,
    pub usage_window: WindowKeys,
    pub usage_token: TokenKeys,
    pub usage_statuses: UsageStatuses,
    pub token_statuses: TokenStatuses,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Table {
    pub name: String,
    pub kind: String,
    pub key: String,
    pub generation: String,
    pub document: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Kinds {
    pub lock: String,
    pub lease: String,
    pub cursor: String,
    pub cache: String,
    pub probe: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LockKeys {
    pub project: String,
    pub holder: String,
    pub taken_at: String,
    pub ttl_s: String,
    pub expires_at: String,
    pub replaced: String,
    pub stolen_at: String,
    pub renewed_at: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LeaseKeys {
    pub task: String,
    pub project: String,
    pub holder: String,
    pub taken_at: String,
    pub heartbeat_at: String,
    pub ttl_s: String,
    pub expires_at: String,
    pub replaced: String,
    pub stolen_at: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CursorKeys {
    pub value: String,
    pub updated: String,
    pub events: String,
    pub step: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct StepKeys {
    pub version: String,
    pub holder: String,
    pub session: String,
    pub t: String,
    pub comments: String,
    pub generated_holder: String,
    pub commit_word: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct EventsKeys {
    pub version: String,
    pub t: String,
    pub own: String,
    pub comments: String,
    pub reports: String,
    pub report_folder: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DeclarationKeys {
    pub folder: String,
    pub version: String,
    pub ts: String,
    pub session: String,
    pub agent: String,
    pub project: String,
    pub task: String,
    pub command: String,
    pub role: String,
    pub ids: Vec<(String, String)>,
    pub step_role: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MonotonicKeys {
    pub key: String,
    pub written: String,
    pub expires: String,
    pub clock: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct View {
    pub hidden: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct BudgetKeys {
    pub key: String,
    pub providers: String,
    pub valid: String,
    pub valid_at: String,
    pub last: String,
    pub attempted_at: String,
    pub claim_token: String,
    pub claim_expires_at: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UsageKeys {
    pub provider: String,
    pub status: String,
    pub plan: String,
    pub windows: String,
    pub reason: String,
    pub token: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WindowKeys {
    pub status: String,
    pub used_pct: String,
    pub remaining_pct: String,
    pub reset: String,
    pub reset_in_s: String,
    pub open: String,
    pub reason: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TokenKeys {
    pub status: String,
    pub expires_at: String,
    pub expires_in_s: String,
    pub reason: String,
    pub http_status: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UsageStatuses {
    pub ok: String,
    pub unknown: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TokenStatuses {
    pub valid: String,
    pub expiring: String,
    pub expired: String,
    pub unknown: String,
    pub refused: String,
}

/// The statements of the prototype, word for word, on the names of the format.
#[derive(Debug)]
pub(crate) struct Sql {
    pub schema: String,
    pub select: String,
    pub upsert: String,
    pub delete: String,
    pub rows: String,
    pub documents: String,
}

pub(crate) fn format() -> &'static Format {
    static CELL: OnceLock<Format> = OnceLock::new();
    CELL.get_or_init(|| {
        let format: Format = toml::from_str(FORMAT).expect("the embedded v1 state format is valid");
        let table = &format.table;
        for name in [&table.name, &table.kind, &table.key, &table.generation, &table.document] {
            assert!(
                !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_lowercase() || byte == b'_'),
                "{name:?} is not a plain SQL name"
            );
        }
        format
    })
}

pub(crate) fn sql() -> &'static Sql {
    static CELL: OnceLock<Sql> = OnceLock::new();
    CELL.get_or_init(|| {
        let Table { name: t, kind: k, key: c, generation: g, document: d } = &format().table;
        Sql {
            schema: format!(
                "CREATE TABLE IF NOT EXISTS {t} ({k} TEXT NOT NULL, {c} TEXT NOT NULL, {g} INTEGER NOT NULL, \
                 {d} TEXT NOT NULL, PRIMARY KEY ({k}, {c}))"
            ),
            select: format!("SELECT {d} FROM {t} WHERE {k} = ? AND {c} = ?"),
            upsert: format!(
                "INSERT INTO {t} ({k}, {c}, {g}, {d}) VALUES (?, ?, 1, ?) \
                 ON CONFLICT ({k}, {c}) DO UPDATE SET {g} = {g} + 1, {d} = excluded.{d}"
            ),
            delete: format!("DELETE FROM {t} WHERE {k} = ? AND {c} = ?"),
            rows: format!("SELECT {k}, {c}, {g}, {d} FROM {t} ORDER BY {k}, {c}"),
            documents: format!("SELECT {c}, {d} FROM {t} WHERE {k} = ?"),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_statements_run_and_the_generation_counts_writes() {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory database");
        conn.execute(&sql().schema, []).expect("schema");
        conn.execute(&sql().schema, []).expect("the schema statement can run twice");
        for text in ["{}", "{\"a\": 1}"] {
            conn.execute(&sql().upsert, ["k", "x", text]).expect("upsert");
        }
        let rows: Vec<(String, String, i64, String)> = conn
            .prepare(&sql().rows)
            .expect("rows")
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
            .expect("query")
            .collect::<Result<_, _>>()
            .expect("read");
        assert_eq!(rows, [("k".to_owned(), "x".to_owned(), 2, "{\"a\": 1}".to_owned())]);
        let selected: String = conn.query_row(&sql().select, ["k", "x"], |row| row.get(0)).expect("select");
        assert_eq!(selected, "{\"a\": 1}");
        assert_eq!(conn.execute(&sql().delete, ["k", "x"]).expect("delete"), 1);
    }
}
