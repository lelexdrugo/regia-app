//! The v1 format of the state, from `formats/store_v1.toml`, and the SQL built on its names.

use std::sync::OnceLock;

use serde::Deserialize;

const FORMAT: &str = include_str!("../formats/store_v1.toml");

#[derive(Debug, Deserialize)]
pub(crate) struct Format {
    pub table: Table,
    pub kinds: Kinds,
    pub lock: LockKeys,
    pub monotonic: MonotonicKeys,
    pub view: View,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Table {
    pub name: String,
    pub kind: String,
    pub key: String,
    pub generation: String,
    pub document: String,
}

/// Only the kinds ported so far; the file lists them all.
#[derive(Debug, Deserialize)]
pub(crate) struct Kinds {
    pub lock: String,
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

/// The statements of the prototype, word for word, on the names of the format.
#[derive(Debug)]
pub(crate) struct Sql {
    pub schema: String,
    pub select: String,
    pub upsert: String,
    pub delete: String,
    pub rows: String,
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
