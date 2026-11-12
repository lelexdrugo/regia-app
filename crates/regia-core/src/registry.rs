//! The registry of a project: the block `<!-- <marker> … -->` in the body of its Objective (R1–R3,
//! R5, R6 of the dossier's conformance study). It is open, one `key: value` a line, and the prototype
//! reads it with Python's rules: the first block, each line split at its first colon, both halves
//! stripped, a line with no colon ignored, and for a key written twice the last value in the place of the
//! first. Its words are data of format v1, in `formats/tracker_v1.toml`.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use regex::Regex;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::model::{Status, Task};
use crate::pystr;

#[derive(Deserialize)]
struct File {
    registry: RegistryFile,
    status_labels: BTreeMap<String, Status>,
}

#[derive(Deserialize)]
struct RegistryFile {
    marker: String,
    no_products: Vec<String>,
    keys: BTreeMap<String, String>,
}

struct Format {
    block: Regex,
    repo: Regex,
    no_products: Vec<String>,
    /// The prototype's key → the contract's.
    keys: BTreeMap<String, String>,
    status_labels: BTreeMap<String, Status>,
}

impl Format {
    /// The prototype's key for a field the contract names: `folder` → its word for it.
    fn key(&self, contract: &str) -> &str {
        self.keys
            .iter()
            .find_map(|(key, name)| (name == contract).then_some(key.as_str()))
            .unwrap_or_else(|| panic!("the v1 format names no key for `{contract}`"))
    }
}

fn format() -> &'static Format {
    static FORMAT: OnceLock<Format> = OnceLock::new();
    FORMAT.get_or_init(|| {
        let file: File = toml::from_str(crate::v1::VOCABULARY).expect("the embedded v1 vocabulary is valid");
        let space = pystr::SPACE;
        let marker = regex::escape(&file.registry.marker);
        Format {
            block: Regex::new(&format!(r"(?s)<!--{space}*{marker}{space}*(.*?)-->")).expect("a valid pattern"),
            repo: Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._-]*/[A-Za-z0-9][A-Za-z0-9._-]*$").expect("a valid pattern"),
            no_products: file.registry.no_products,
            keys: file.registry.keys,
            status_labels: file.status_labels,
        }
    })
}

/// The keys of a registry with their values, each where it was first written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Registry {
    entries: Vec<(String, String)>,
}

impl Registry {
    /// The registry in `body`, empty if it has no block (`registro`).
    pub fn parse(body: &str) -> Registry {
        let mut registry = Registry::default();
        let Some(block) = format().block.captures(body).and_then(|captures| captures.get(1)) else { return registry };
        for line in pystr::split_lines(block.as_str()) {
            if let Some((key, value)) = line.split_once(':') {
                registry.set(pystr::strip(key), pystr::strip(value));
            }
        }
        registry
    }

    fn set(&mut self, key: &str, value: &str) {
        match self.entries.iter_mut().find(|(known, _)| known == key) {
            Some(entry) => value.clone_into(&mut entry.1),
            None => self.entries.push((key.to_owned(), value.to_owned())),
        }
    }

    pub fn entries(&self) -> &[(String, String)] {
        &self.entries
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.iter().find(|(known, _)| known == key).map(|(_, value)| value.as_str())
    }

    /// The project's folder, if the registry names one that is not empty.
    pub fn folder(&self) -> Option<&str> {
        self.get(format().key("folder")).filter(|folder| !folder.is_empty())
    }

    /// The project's wip as written, if the registry gives it: the precheck reads it as a number, where a
    /// value that is not one fails the tick.
    pub fn wip(&self) -> Option<&str> {
        self.get(format().key("wip"))
    }

    /// The project's priority as written, if the registry gives it, read as the wip is.
    pub fn priority(&self) -> Option<&str> {
        self.get(format().key("priority"))
    }

    /// The dossier's repo, `<owner>/<name>`, or `None` when it is missing or not a repo's name: a registry
    /// written by hand must not break `projects` (`repo_del_registro`).
    pub fn repo(&self) -> Option<String> {
        let value = pystr::strip(self.get(format().key("repo")).unwrap_or_default());
        format().repo.is_match(value).then(|| value.to_owned())
    }

    /// The products, each `<owner>/<name>`: none for a word that means none, and none at all when one of
    /// them is not a repo's name (`prodotti_del_registro`). A bare name belongs to `owner`.
    pub fn products(&self, owner: &str) -> Vec<String> {
        let value = pystr::strip(self.get(format().key("products")).unwrap_or_default());
        if format().no_products.contains(&value.to_lowercase()) {
            return Vec::new();
        }
        let items: Option<Vec<String>> =
            value.split(',').filter(|item| !pystr::strip(item).is_empty()).map(|item| full_repo(item, owner)).collect();
        items.unwrap_or_default()
    }
}

/// `<name>` or `<owner>/<name>` as `<owner>/<name>`, or `None` if it is not a repo's name (`repo_pieno`).
pub fn full_repo(value: &str, owner: &str) -> Option<String> {
    let value = pystr::strip(value);
    let full = if value.contains('/') { value.to_owned() } else { format!("{owner}/{value}") };
    (format().repo.is_match(&full) && !full.ends_with(".git")).then_some(full)
}

/// `body` with the keys of its registry set: the first line of a key is rewritten, a new key goes after
/// the last line that is not blank, and the rest of the body stays as it was (`registro_con`, R3). `None`
/// if the body has no registry. A key written twice is rewritten where it is first, while
/// [`Registry::parse`] reads the last: a caller refuses that case.
pub fn registry_with(body: &str, keys: &[(&str, &str)]) -> Option<String> {
    let block = format().block.captures(body)?.get(1)?;
    let mut lines: Vec<String> = block.as_str().split('\n').map(str::to_owned).collect();
    for (key, value) in keys {
        let line = format!("{key}: {value}");
        let written =
            lines.iter().position(|old| old.contains(':') && pystr::strip(old.split(':').next().unwrap_or_default()) == *key);
        match written {
            Some(at) => lines[at] = line,
            None => {
                let at = lines.iter().rposition(|old| !pystr::strip(old).is_empty()).map_or(0, |last| last + 1);
                lines.insert(at, line);
            }
        }
    }
    Some(format!("{}{}{}", &body[..block.start()], lines.join("\n"), &body[block.end()..]))
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EntryError {
    #[error("the registry of #{number} gives the status `{value}`, which is not one of the tracker's")]
    UnknownStatus { number: u64, value: String },
    #[error("two keys of the registry of #{number} are named `{key}`")]
    Collision { number: u64, key: String },
}

/// The entry of `projects` for an Objective: its project, number, title and status, then every key of its
/// registry, then the repo and the products as they read. A key of the registry with the name of one
/// before it takes its place, as in the prototype, where the block's `id` wins over the label's. The
/// keys the prototype writes get the contract's names and the others keep their own, so two keys that
/// land on one name are an error, as in the facade.
pub fn project_entry(objective: &Task, registry: &Registry, owner: &str) -> Result<Map<String, Value>, EntryError> {
    let format = format();
    let mut row: Vec<(String, Value)> = Vec::new();
    let mut set = |key: &str, value: Value| match row.iter_mut().find(|(known, _)| known == key) {
        Some(entry) => entry.1 = value,
        None => row.push((key.to_owned(), value)),
    };
    set(format.key("id"), json!(objective.project));
    set(format.key("objective"), json!(objective.number));
    set(format.key("title"), json!(objective.title));
    set(format.key("status"), json!(objective.status.map(Status::id)));
    for (key, value) in registry.entries() {
        let value = if key == format.key("status") {
            let status = format.status_labels.get(value);
            json!(status.ok_or_else(|| EntryError::UnknownStatus { number: objective.number, value: value.clone() })?.id())
        } else {
            json!(value)
        };
        set(key, value);
    }
    set(format.key("repo"), json!(registry.repo()));
    set(format.key("products"), json!(registry.products(owner)));
    let mut entry = Map::new();
    for (key, value) in row {
        let name = format.keys.get(&key).cloned().unwrap_or(key);
        if entry.contains_key(&name) {
            return Err(EntryError::Collision { number: objective.number, key: name });
        }
        entry.insert(name, value);
    }
    Ok(entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The registry's words, from the format: the sources hold none of them.
    fn key(contract: &str) -> &'static str {
        format().key(contract)
    }

    fn block(lines: &str) -> String {
        format!("intro\n<!-- {}\n{lines}-->\n\n## after\n", toml_marker())
    }

    fn toml_marker() -> String {
        let file: File = toml::from_str(crate::v1::VOCABULARY).unwrap();
        file.registry.marker
    }

    #[test]
    fn the_last_value_of_a_key_wins_in_the_place_of_the_first() {
        let folder = key("folder");
        let registry = Registry::parse(&block(&format!("id: alpha\n{folder}: one\nno colon here\n note : a: b \n{folder}: two\n")));
        let entries: Vec<(&str, &str)> = registry.entries().iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        assert_eq!(entries, [("id", "alpha"), (folder, "two"), ("note", "a: b")]);
        assert_eq!(registry.folder(), Some("two"));
        let (wip, priority) = (key("wip"), key("priority"));
        let numbers = Registry::parse(&block(&format!("{wip}: 4\n{priority}:\n")));
        assert_eq!((numbers.wip(), numbers.priority()), (Some("4"), Some("")), "as written, an empty one included");
        assert_eq!((registry.wip(), registry.priority()), (None, None));
        assert_eq!(Registry::parse("no block"), Registry::default());
        assert_eq!(Registry::parse(&block(&format!("{folder}:  \n"))).folder(), None, "an empty folder is none");
    }

    #[test]
    fn repo_and_products_read_as_the_prototype_reads_them() {
        let (repo, products) = (key("repo"), key("products"));
        let read = |lines: &str| Registry::parse(&block(lines));
        assert_eq!(read(&format!("{repo}: me/dossier\n")).repo().as_deref(), Some("me/dossier"));
        assert_eq!(read(&format!("{repo}: dossier\n")).repo(), None, "the registry's repo is not completed");
        assert_eq!(read(&format!("{products}: a/b, c\n")).products("me"), ["a/b", "me/c"]);
        assert!(read(&format!("{products}: a/b, c.git\n")).products("me").is_empty(), "one bad item drops them all");
        assert!(read(&format!("{products}: -\n")).products("me").is_empty());
        assert!(read("").products("me").is_empty());
    }

    #[test]
    fn a_key_is_rewritten_where_it_is_or_added_after_the_last_line() {
        let folder = key("folder");
        let body = block(&format!("id: alpha\n{folder}: old\n\n"));
        let written = registry_with(&body, &[(folder, "new"), ("extra", "x")]).unwrap();
        assert_eq!(written, block(&format!("id: alpha\n{folder}: new\nextra: x\n\n")));
        assert_eq!(Registry::parse(&written).get("extra"), Some("x"));
        assert_eq!(registry_with("no block", &[("a", "b")]), None);
    }
}
