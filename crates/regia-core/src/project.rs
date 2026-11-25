//! What `project new` and `project set` decide before they write (the prototype's commands of a new project and
//! of a project's levers, its `scaffold`, `leva_valida` and `lista_prodotti`): the levers of a project, the list of
//! its products, the body of its Objective and the scaffold of its dossier. Pure: the CLI reads the tracker and
//! writes the files. The words other programs read are data of format v1, in `formats/project_v1.toml` and
//! `formats/tracker_v1.toml`.

use std::fmt;
use std::sync::OnceLock;

use serde::Deserialize;

use crate::pystr;
use crate::registry::{full_repo, registry_key, registry_marker};

const FORMAT: &str = include_str!("../formats/project_v1.toml");

#[derive(Deserialize)]
struct File {
    registry: RegistryFile,
    defaults: Defaults,
    scaffold: ScaffoldFormat,
}

#[derive(Deserialize)]
struct RegistryFile {
    no_products: String,
}

#[derive(Deserialize)]
struct Defaults {
    playbook: String,
}

#[derive(Deserialize)]
struct ScaffoldFormat {
    folder: String,
    folders: Vec<String>,
    placeholders: Placeholders,
    added: Added,
    files: Vec<Model>,
}

#[derive(Deserialize)]
struct Placeholders {
    id: String,
    title: String,
    objective: String,
    repo: String,
    date: String,
}

#[derive(Deserialize)]
struct Added {
    folder: String,
    file: String,
    text: String,
}

#[derive(Deserialize)]
struct Model {
    name: String,
    text: String,
}

fn format() -> &'static File {
    static FILE: OnceLock<File> = OnceLock::new();
    FILE.get_or_init(|| toml::from_str(FORMAT).expect("the embedded project format is valid"))
}

/// The word the registry gets for no products.
#[must_use]
pub fn no_products_word() -> &'static str {
    &format().registry.no_products
}

/// The playbook of a project created without `--playbook`.
#[must_use]
pub fn default_playbook() -> &'static str {
    &format().defaults.playbook
}

/// A lever of a project, in its registry: how many tasks run at once, and its priority among the projects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectLever {
    Wip,
    Priority,
}

impl ProjectLever {
    /// In the order the prototype reads, writes and reports them.
    pub const ALL: [ProjectLever; 2] = [ProjectLever::Wip, ProjectLever::Priority];

    /// The key of the lever in the registry.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            ProjectLever::Wip => registry_key("wip"),
            ProjectLever::Priority => registry_key("priority"),
        }
    }

    /// The option that sets it, as the errors name it.
    #[must_use]
    pub fn option(self) -> &'static str {
        match self {
            ProjectLever::Wip => "--wip",
            ProjectLever::Priority => "--priority",
        }
    }

    /// The values the CLI accepts (`LEVE_LIMITI`): a value the precheck cannot read stops it on every project,
    /// so the CLI never writes one.
    #[must_use]
    pub fn limits(self) -> (u64, Option<u64>) {
        match self {
            ProjectLever::Wip => (1, Some(10)),
            ProjectLever::Priority => (1, None),
        }
    }

    /// `value` as the lever's integer, as `leva_valida` reads it: blanks around it, then ASCII digits only,
    /// within the limits.
    pub fn parse(self, value: &str) -> Result<u64, LeverError> {
        let digits = pystr::strip(value);
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(LeverError::NotAnInteger { option: self.option(), value: value.to_owned() });
        }
        let significant = digits.trim_start_matches('0');
        let shown = if significant.is_empty() { "0" } else { significant };
        let (minimum, maximum) = self.limits();
        let out_of_range = || LeverError::OutOfRange { option: self.option(), value: shown.to_owned(), minimum, maximum };
        // More digits than a u64 holds: past any maximum; with none, a priority Python would take, refused here.
        let number: u64 = shown.parse().map_err(|_| out_of_range())?;
        if number < minimum || maximum.is_some_and(|maximum| number > maximum) {
            return Err(out_of_range());
        }
        Ok(number)
    }
}

/// A value a lever does not take, with the option and the value as given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeverError {
    NotAnInteger { option: &'static str, value: String },
    OutOfRange { option: &'static str, value: String, minimum: u64, maximum: Option<u64> },
}

impl fmt::Display for LeverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LeverError::NotAnInteger { option, value } => write!(f, "{option} '{value}': expected an integer"),
            LeverError::OutOfRange { option, value, minimum, maximum: Some(maximum) } => {
                write!(f, "{option} {value}: expected {minimum} to {maximum}")
            }
            LeverError::OutOfRange { option, value, minimum, maximum: None } => write!(f, "{option} {value}: expected at least {minimum}"),
        }
    }
}

impl std::error::Error for LeverError {}

/// The products of `value`, each `<owner>/<name>`, as `lista_prodotti` reads them: a word that means none gives
/// none, otherwise the items between commas that are not blank, each a repo's name, a bare one `owner`'s. The first
/// item that is not a repo's name is the error.
pub fn products_list(value: &str, owner: &str) -> Result<Vec<String>, String> {
    let stripped = pystr::strip(value);
    if crate::registry::no_products().iter().any(|none| *none == stripped.to_lowercase()) {
        return Ok(Vec::new());
    }
    stripped
        .split(',')
        .filter(|item| !pystr::strip(item).is_empty())
        .map(|item| full_repo(item, owner).ok_or_else(|| repo_error(item)))
        .collect()
}

/// `<name>` or `<owner>/<name>` as `<owner>/<name>`, or the error `repo_pieno` gives.
pub fn repo_argument(value: &str, owner: &str) -> Result<String, String> {
    full_repo(value, owner).ok_or_else(|| repo_error(value))
}

fn repo_error(value: &str) -> String {
    format!("repo '{value}': expected <name> or <owner>/<name>")
}

/// How many lines of the registry in `body` name `key`, as the prototype counts them before it rewrites a lever:
/// the block's lines as `splitlines` gives them, each with a colon and `key` before it. A key written twice is
/// rewritten where it is first and read where it is last, so a lever twice is refused.
#[must_use]
pub fn registry_key_count(body: &str, key: &str) -> usize {
    let Some(block) = crate::registry::block(body) else { return 0 };
    pystr::split_lines(block).into_iter().filter(|line| line.contains(':') && pystr::strip(line.split(':').next().unwrap_or_default()) == key).count()
}

/// The headings of an Objective's sections and the mandate it gets when none is given, in the user's language.
#[derive(Debug, Clone, Copy)]
pub struct ObjectiveWords<'a> {
    pub objective: &'a str,
    pub definition_of_done: &'a str,
    pub mandate: &'a str,
    pub mandate_to_come: &'a str,
}

/// The body of a new Objective (R2, R7): the registry, one `key: value` a line, then its three sections.
#[must_use]
pub fn objective_body(registry: &[(&str, String)], words: &ObjectiveWords<'_>, objective: &str, definition_of_done: &str, mandate: &str) -> String {
    let lines: Vec<String> = registry.iter().map(|(key, value)| format!("{key}: {value}")).collect();
    let mandate = if mandate.is_empty() { words.mandate_to_come } else { mandate };
    format!(
        "<!-- {}\n{}\n-->\n\n## {}\n\n{objective}\n\n## {}\n\n{definition_of_done}\n\n## {}\n\n{mandate}\n",
        registry_marker(),
        lines.join("\n"),
        words.objective,
        words.definition_of_done,
        words.mandate,
    )
}

/// The values the models of the scaffold are filled with.
#[derive(Debug, Clone, Copy)]
pub struct ScaffoldValues<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub objective: u64,
    /// The tracker's repo, `<owner>/<name>`.
    pub repo: &'a str,
    /// The local day, `YYYY-MM-DD`.
    pub date: &'a str,
}

/// A file of the scaffold: its path under the scaffold's folder, one segment each, and its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScaffoldFile {
    pub segments: Vec<String>,
    pub text: String,
    /// Not in the prototype: an addition of #452.
    pub added: bool,
}

/// The folder of the scaffold in the project's folder.
#[must_use]
pub fn scaffold_folder() -> &'static str {
    &format().scaffold.folder
}

/// The files of the scaffold, filled, in the order they are written and listed: the models, then the rule of the
/// moviola's folder.
#[must_use]
pub fn scaffold_files(values: &ScaffoldValues<'_>) -> Vec<ScaffoldFile> {
    let scaffold = &format().scaffold;
    let mut files: Vec<ScaffoldFile> = scaffold
        .files
        .iter()
        .map(|model| ScaffoldFile { segments: vec![model.name.clone()], text: fill(&lf(&model.text), values), added: false })
        .collect();
    files.push(ScaffoldFile {
        segments: vec![scaffold.added.folder.clone(), scaffold.added.file.clone()],
        text: lf(&scaffold.added.text),
        added: true,
    });
    files
}

/// The folders of the scaffold, made after its models, empty: the prototype's, then the moviola's.
#[must_use]
pub fn scaffold_folders() -> Vec<String> {
    let scaffold = &format().scaffold;
    scaffold.folders.iter().cloned().chain(std::iter::once(scaffold.added.folder.clone())).collect()
}

/// A model's placeholders replaced one after the other, in the prototype's order.
fn fill(text: &str, values: &ScaffoldValues<'_>) -> String {
    let placeholders = &format().scaffold.placeholders;
    let objective = format!("#{}", values.objective);
    [
        (&placeholders.id, values.id),
        (&placeholders.title, values.title),
        (&placeholders.objective, objective.as_str()),
        (&placeholders.repo, values.repo),
        (&placeholders.date, values.date),
    ]
    .into_iter()
    .fold(text.to_owned(), |text, (placeholder, value)| text.replace(placeholder.as_str(), value))
}

/// A model as the prototype reads it, in text mode: `\r\n` and `\r` as `\n`.
fn lf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Registry;

    fn values() -> ScaffoldValues<'static> {
        ScaffoldValues { id: "gamma", title: "A title", objective: 9, repo: "example/tracker", date: "2026-01-10" }
    }

    /// The levers' limits are inclusive, the value is read from its ASCII digits only, blanks around it allowed,
    /// and the error names the option and the value as given, or the number as read.
    #[test]
    fn a_lever_takes_ascii_digits_within_its_limits() {
        assert_eq!(ProjectLever::Wip.parse("1"), Ok(1));
        assert_eq!(ProjectLever::Wip.parse(" 10 "), Ok(10));
        assert_eq!(ProjectLever::Wip.parse("007"), Ok(7));
        assert_eq!(ProjectLever::Priority.parse("99"), Ok(99));
        for value in ["x", "-1", "2.5", "", "+3", "²", "٣", "1e3", "1_0"] {
            assert!(matches!(ProjectLever::Wip.parse(value), Err(LeverError::NotAnInteger { .. })), "{value}");
        }
        assert_eq!(ProjectLever::Wip.parse("11").unwrap_err().to_string(), "--wip 11: expected 1 to 10");
        assert_eq!(ProjectLever::Wip.parse("0011").unwrap_err().to_string(), "--wip 11: expected 1 to 10");
        assert_eq!(ProjectLever::Priority.parse("0").unwrap_err().to_string(), "--priority 0: expected at least 1");
        assert_eq!(ProjectLever::Wip.parse("x").unwrap_err().to_string(), "--wip 'x': expected an integer");
        assert!(ProjectLever::Wip.parse("99999999999999999999999").is_err());
    }

    #[test]
    fn products_read_as_lista_prodotti() {
        assert_eq!(products_list(" o/a ,p/b,", "me"), Ok(vec!["o/a".to_owned(), "p/b".to_owned()]));
        assert_eq!(products_list("c", "me"), Ok(vec!["me/c".to_owned()]));
        for none in ["", "—", "-", "Nessuno", " nessuna "] {
            assert_eq!(products_list(none, "me"), Ok(Vec::new()), "{none}");
        }
        assert!(products_list("a/b, pasticcio brutto", "me").is_err());
        assert!(products_list("a/b, c.git", "me").is_err());
        for bad in ["", "a/b/c", "a b", "o/n\nrepo: x/y", "o/n.git", "-x/y"] {
            assert!(repo_argument(bad, "me").is_err(), "{bad:?}");
        }
        assert_eq!(repo_argument("proj-x", "me").as_deref(), Ok("me/proj-x"));
        assert_eq!(repo_argument("o/n.x_y-z", "me").as_deref(), Ok("o/n.x_y-z"));
    }

    /// A key is counted on the registry's lines only, as `splitlines` splits them.
    #[test]
    fn a_key_is_counted_in_the_registry_alone() {
        let wip = ProjectLever::Wip.key();
        let body = format!("<!-- {}\nid: a\n{wip}: 2\n{wip} : 5\n-->\n{wip}: 9\n", registry_marker());
        assert_eq!(registry_key_count(&body, wip), 2);
        assert_eq!(registry_key_count(&body, "id"), 1);
        assert_eq!(registry_key_count("no block", wip), 0);
        let crlf = format!("<!-- {}\r\n{wip}: 2\r\n-->\r\n", registry_marker());
        assert_eq!(registry_key_count(&crlf, wip), 1);
    }

    #[test]
    fn the_objective_s_body_reads_back_as_its_registry() {
        let words = ObjectiveWords { objective: "O", definition_of_done: "D", mandate: "M", mandate_to_come: "later" };
        let body = objective_body(&[("id", "gamma".to_owned()), (ProjectLever::Wip.key(), "2".to_owned())], &words, "obj", "done", "");
        assert!(body.ends_with("## O\n\nobj\n\n## D\n\ndone\n\n## M\n\nlater\n"), "{body}");
        let registry = Registry::parse(&body);
        assert_eq!((registry.get("id"), registry.wip()), (Some("gamma"), Some("2")));
    }

    /// The models are filled one placeholder after the other, so a title that holds a later placeholder has it
    /// replaced; the rule of the moviola comes last, unfilled.
    #[test]
    fn the_scaffold_fills_its_models_in_the_prototype_s_order() {
        let files = scaffold_files(&values());
        assert_eq!(files.len(), 5);
        assert!(files.iter().take(4).all(|file| !file.added && file.segments.len() == 1));
        let added = files.last().unwrap();
        assert!(added.added && added.segments.len() == 2 && added.text.ends_with(".svg\n"));
        for file in &files {
            assert!(!file.text.contains("{{") && !file.text.contains('\r'), "{:?}", file.segments);
        }
        let placeholders = &format().scaffold.placeholders;
        let tricky = ScaffoldValues { title: &format!("t {}", placeholders.date), ..values() };
        let with_date = scaffold_files(&tricky);
        assert!(with_date.iter().any(|file| file.text.contains("t 2026-01-10")), "the title's placeholder is replaced too");
        assert_eq!(scaffold_folders().last().map(String::as_str), Some(added.segments[0].as_str()));
        assert_eq!(scaffold_folders().len(), 4);
    }
}
