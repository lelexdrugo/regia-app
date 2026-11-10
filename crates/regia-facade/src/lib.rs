//! The facade between the English contract and the Python prototype (ADR 0017 §4, ADR 0001 §11). The
//! CLI's passthrough and the conformance harness both go through [`Table::python_argv`] and
//! [`Table::translate_output`]: that is what "the same table" means.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use regia_exec::{DossierPath, ExecError, Launch, Limits, Pin, Program, Search};
use serde::Deserialize;
use serde_json::{Map, Value};

/// Every file of `table/`, by name, embedded by the build script.
const FILES: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/tables.rs"));

/// The prototype's entry point, relative to the dossier.
pub const SCRIPT: &str = "tools/regia.py";

/// A command of the prototype reads the tracker and may write: generous, but bounded.
pub const PROTOTYPE_LIMITS: Limits = Limits { timeout: Duration::from_secs(600), stdout: 64 << 20, stderr: 4 << 20 };

#[derive(Debug, thiserror::Error)]
pub enum FacadeError {
    #[error("the facade table does not know command `{0}`")]
    UnknownCommand(String),
    #[error("the facade table does not know argument `{argument}` of command `{command}`")]
    UnknownArgument { command: String, argument: String },
    #[error("argument `{argument}` of command `{command}` needs a value")]
    MissingValue { command: String, argument: String },
    #[error("command `{command}` takes no further argument, and got `{argument}`")]
    UnexpectedArgument { command: String, argument: String },
    #[error("command `{0}` has no JSON output in the facade table")]
    NoOutputShape(String),
    #[error("key `{key}` at `{path}` is not in the facade table")]
    UnknownKey { path: String, key: String },
    #[error("two keys at `{path}` translate to `{key}`")]
    KeyCollision { path: String, key: String },
    #[error("value `{value}` at `{path}` is not in the enum `{name}` of the facade table")]
    UnknownEnumValue { path: String, name: String, value: String },
    #[error("expected {expected} at `{path}`")]
    WrongType { path: String, expected: &'static str },
    #[error("`{0}` is not ported, and there is no prototype to run it")]
    NotPorted(String),
    #[error("`{0}` was sent to the prototype, and there is no prototype here")]
    NoPrototype(String),
    #[error("the prototype could not run: {0}")]
    Exec(#[from] ExecError),
    #[error("the prototype ended without an exit code ({0})")]
    NoExitCode(String),
    #[error("the prototype printed invalid JSON: {0}")]
    BadJson(serde_json::Error),
}

/// A table file that does not load, or a table whose files do not fit together.
#[derive(Debug, thiserror::Error)]
#[error("facade table, {file}: {problem}")]
pub struct TableError {
    pub file: String,
    pub problem: String,
}

fn problem<T>(file: &str, problem: String) -> Result<T, TableError> {
    Err(TableError { file: file.to_owned(), problem })
}

/// Where a command runs: in this binary, or on the prototype through the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    Native,
    Prototype,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TableFile {
    schema: u32,
    #[serde(default)]
    groups: BTreeMap<String, Group>,
    #[serde(default)]
    commands: BTreeMap<String, CommandEntry>,
    #[serde(default)]
    enums: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default)]
    shapes: BTreeMap<String, Shape>,
}

/// A switch of `groups.toml`: where a ported command of the group runs while the prototype exists.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Group {
    route: Engine,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandEntry {
    python: Vec<String>,
    group: String,
    output: Option<String>,
    #[serde(default)]
    args: BTreeMap<String, Arg>,
    #[serde(default)]
    positionals: Vec<Positional>,
}

/// An option: a new name, with a value that passes, or the whole spec.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Arg {
    Rename(String),
    Spec(ArgSpec),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArgSpec {
    to: String,
    /// It takes no value.
    #[serde(default)]
    flag: bool,
    /// It takes every value that follows, zero or more, up to the next option.
    #[serde(default)]
    many: bool,
    /// Its value translates through this enum, from English to the prototype's.
    #[serde(rename = "enum")]
    enum_name: Option<String>,
}

impl Arg {
    fn to(&self) -> &str {
        match self {
            Arg::Rename(to) => to,
            Arg::Spec(spec) => &spec.to,
        }
    }

    fn flag(&self) -> bool {
        matches!(self, Arg::Spec(ArgSpec { flag: true, .. }))
    }

    fn many(&self) -> bool {
        matches!(self, Arg::Spec(ArgSpec { many: true, .. }))
    }

    fn enum_name(&self) -> Option<&str> {
        match self {
            Arg::Rename(_) => None,
            Arg::Spec(spec) => spec.enum_name.as_deref(),
        }
    }
}

/// A positional argument: its value passes, or translates through an enum.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Positional {
    #[serde(rename = "enum")]
    enum_name: Option<String>,
}

/// An object with known keys, or an object whose keys are an enum, or a list of a shape.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Shape {
    fields: Option<BTreeMap<String, Field>>,
    key_enum: Option<String>,
    items_of: Option<String>,
    /// Keys not in `fields` pass as data instead of failing.
    #[serde(default)]
    open: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Field {
    to: String,
    #[serde(rename = "enum")]
    enum_name: Option<String>,
    /// The value is an object of this shape.
    shape: Option<String>,
    /// The value is a list of objects of this shape.
    items: Option<String>,
    /// The value is an object with data as keys and this shape as values.
    values: Option<String>,
}

/// What the prototype printed on stdout, in English where the table says how.
pub enum Printed {
    Json(Value),
    /// Text passes as it is: the agents read `--json` and the exit code (ADR 0001 §11).
    Text(Vec<u8>),
}

/// A run of the prototype through the table.
pub struct Run {
    pub exit: u8,
    pub stdout: Printed,
    /// Its warnings and errors, as it wrote them.
    pub stderr: Vec<u8>,
}

#[derive(Debug)]
pub struct Table {
    groups: BTreeMap<String, Group>,
    commands: BTreeMap<String, CommandEntry>,
    enums: BTreeMap<String, BTreeMap<String, String>>,
    shapes: BTreeMap<String, Shape>,
    /// Each enum read backwards, from English to the prototype's, for argument values.
    reverse: BTreeMap<String, BTreeMap<String, String>>,
}

fn merge<V>(into: &mut BTreeMap<String, V>, from: BTreeMap<String, V>, what: &str, file: &str) -> Result<(), TableError> {
    for (name, value) in from {
        if into.contains_key(&name) {
            return problem(file, format!("{what} `{name}` is defined in another file too"));
        }
        into.insert(name, value);
    }
    Ok(())
}

impl Table {
    /// The table compiled into the binary.
    pub fn embedded() -> Self {
        Self::from_files(FILES.iter().copied()).unwrap_or_else(|error| panic!("{error}"))
    }

    /// The names of the files compiled into the binary.
    pub fn embedded_files() -> Vec<&'static str> {
        FILES.iter().map(|(name, _)| *name).collect()
    }

    /// The files of a table, merged and checked: a name defined twice, a reference to nothing, or an enum
    /// two values of which read the same in English is an error.
    pub fn from_files<'a>(files: impl IntoIterator<Item = (&'a str, &'a str)>) -> Result<Self, TableError> {
        let mut table = Table {
            groups: BTreeMap::new(),
            commands: BTreeMap::new(),
            enums: BTreeMap::new(),
            shapes: BTreeMap::new(),
            reverse: BTreeMap::new(),
        };
        for (name, text) in files {
            let file: TableFile = toml::from_str(text).or_else(|error| problem(name, error.to_string()))?;
            if file.schema != 1 {
                return problem(name, format!("unsupported schema {}", file.schema));
            }
            merge(&mut table.groups, file.groups, "group", name)?;
            merge(&mut table.commands, file.commands, "command", name)?;
            merge(&mut table.enums, file.enums, "enum", name)?;
            merge(&mut table.shapes, file.shapes, "shape", name)?;
        }
        table.check()?;
        Ok(table)
    }

    fn check(&mut self) -> Result<(), TableError> {
        let file = "the merged table";
        let has_enum = |name: &Option<String>| name.as_ref().is_none_or(|name| self.enums.contains_key(name));
        let has_shape = |name: &Option<String>| name.as_ref().is_none_or(|name| self.shapes.contains_key(name));
        for (name, command) in &self.commands {
            let words: Vec<&str> = name.split(' ').collect();
            if words.len() > 2 || words.iter().any(|word| word.is_empty() || !word.chars().all(|c| c.is_ascii_lowercase())) {
                return problem(file, format!("command `{name}` is not one or two lowercase words"));
            }
            if !self.groups.contains_key(&command.group) {
                return problem(file, format!("command `{name}` is in group `{}`, which does not exist", command.group));
            }
            if !has_shape(&command.output) || command.python.is_empty() {
                return problem(file, format!("command `{name}` names an output shape that does not exist, or no prototype command"));
            }
            for (argument, spec) in &command.args {
                if !argument.starts_with("--") || !spec.to().starts_with("--") || !has_enum(&spec.enum_name().map(str::to_owned)) {
                    return problem(file, format!("argument `{argument}` of `{name}` is not an option, or names an unknown enum"));
                }
                if spec.flag() && spec.many() {
                    return problem(file, format!("argument `{argument}` of `{name}` cannot be a flag and take many values"));
                }
            }
            if !command.positionals.iter().all(|positional| has_enum(&positional.enum_name)) {
                return problem(file, format!("a positional argument of `{name}` names an unknown enum"));
            }
        }
        for (name, shape) in &self.shapes {
            let kinds = [shape.fields.is_some(), shape.key_enum.is_some(), shape.items_of.is_some()];
            if kinds.iter().filter(|kind| **kind).count() != 1 || (shape.open && shape.fields.is_none()) {
                return problem(file, format!("shape `{name}` needs exactly one of fields, key_enum, items_of, and only fields can be open"));
            }
            if !has_enum(&shape.key_enum) || !has_shape(&shape.items_of) {
                return problem(file, format!("shape `{name}` refers to something that does not exist"));
            }
            for (key, field) in shape.fields.iter().flatten() {
                let kinds = [field.enum_name.is_some(), field.shape.is_some(), field.items.is_some(), field.values.is_some()];
                let resolved = has_enum(&field.enum_name) && has_shape(&field.shape) && has_shape(&field.items) && has_shape(&field.values);
                if kinds.iter().filter(|kind| **kind).count() > 1 || !resolved {
                    return problem(file, format!("field `{key}` of shape `{name}` refers to something that does not exist, or to two things"));
                }
            }
        }
        for (name, values) in &self.enums {
            let mut reverse = BTreeMap::new();
            for (python, english) in values {
                if reverse.insert(english.clone(), python.clone()).is_some() {
                    return problem(file, format!("enum `{name}` has two values that read `{english}` in English"));
                }
            }
            self.reverse.insert(name.clone(), reverse);
        }
        Ok(())
    }

    fn command(&self, command: &str) -> Result<&CommandEntry, FacadeError> {
        self.commands.get(command).ok_or_else(|| FacadeError::UnknownCommand(command.to_owned()))
    }

    pub fn knows_command(&self, command: &str) -> bool {
        self.commands.contains_key(command)
    }

    /// Whether `command` prints JSON that the table translates, when `--json` is asked.
    pub fn has_output(&self, command: &str) -> bool {
        self.commands.get(command).is_some_and(|entry| entry.output.is_some())
    }

    /// The command named by the first one or two words of `argv`, the longer name first, and the rest.
    pub fn split_command<'a>(&self, argv: &'a [String]) -> Option<(String, &'a [String])> {
        if let [first, second, rest @ ..] = argv {
            let name = format!("{first} {second}");
            if self.knows_command(&name) {
                return Some((name, rest));
            }
        }
        let (first, rest) = argv.split_first()?;
        self.knows_command(first).then(|| (first.clone(), rest))
    }

    /// Where `command` runs. `native` says whether this binary implements it, `prototype` whether this
    /// machine has the prototype, and `forced` comes from `REGIA_ENGINE`. Without a choice forced, the
    /// switch of the command's group decides, and falls back to the other side when its own is not there.
    pub fn engine(&self, command: &str, native: bool, prototype: bool, forced: Option<Engine>) -> Result<Engine, FacadeError> {
        let entry = self.command(command)?;
        let switch = self.groups[&entry.group].route;
        let not_ported = || FacadeError::NotPorted(command.to_owned());
        match (forced, switch) {
            (Some(Engine::Prototype), _) if prototype => Ok(Engine::Prototype),
            (Some(Engine::Prototype), _) => Err(FacadeError::NoPrototype(command.to_owned())),
            (Some(Engine::Native), _) => native.then_some(Engine::Native).ok_or_else(not_ported),
            (None, Engine::Prototype) if prototype => Ok(Engine::Prototype),
            (None, Engine::Native) if native => Ok(Engine::Native),
            (None, _) if native => Ok(Engine::Native),
            (None, _) if prototype => Ok(Engine::Prototype),
            (None, _) => Err(not_ported()),
        }
    }

    /// `regia <command> <args…>` in English → the argv of `regia.py`. Options are renamed, values pass
    /// untouched unless the table names an enum for them, and the global `--json` goes first, where the
    /// prototype's parser wants it. An option that takes `many` values takes those that follow, as the
    /// prototype's `nargs="*"` does: repeating it there would keep only the last list. An option or a
    /// positional argument the table does not know is an error, and so is a value that starts with a
    /// dash.
    pub fn python_argv(&self, command: &str, args: &[String]) -> Result<Vec<String>, FacadeError> {
        let entry = self.command(command)?;
        let error = |argument: &str| (command.to_owned(), argument.to_owned());
        let mut json = false;
        let mut rest = Vec::new();
        let mut positionals = entry.positionals.iter();
        let mut args = args.iter().peekable();
        while let Some(arg) = args.next() {
            if arg == "--json" {
                json = true;
            } else if arg.starts_with("--") {
                let spec = entry.args.get(arg).ok_or_else(|| {
                    let (command, argument) = error(arg);
                    FacadeError::UnknownArgument { command, argument }
                })?;
                rest.push(spec.to().to_owned());
                if spec.many() {
                    while let Some(value) = args.next_if(|value| !value.starts_with('-')) {
                        rest.push(self.argument_value(spec.enum_name(), value, &format!("{command} {arg}"))?);
                    }
                } else if !spec.flag() {
                    let value = args.next().filter(|value| !value.starts_with('-')).ok_or_else(|| {
                        let (command, argument) = error(arg);
                        FacadeError::MissingValue { command, argument }
                    })?;
                    rest.push(self.argument_value(spec.enum_name(), value, &format!("{command} {arg}"))?);
                }
            } else {
                let Some(positional) = positionals.next().filter(|_| !arg.starts_with('-')) else {
                    let (command, argument) = error(arg);
                    return Err(FacadeError::UnexpectedArgument { command, argument });
                };
                rest.push(self.argument_value(positional.enum_name.as_deref(), arg, command)?);
            }
        }
        let json = json.then(|| "--json".to_owned());
        Ok(json.into_iter().chain(entry.python.iter().cloned()).chain(rest).collect())
    }

    fn argument_value(&self, enum_name: Option<&str>, value: &str, path: &str) -> Result<String, FacadeError> {
        let Some(name) = enum_name else { return Ok(value.to_owned()) };
        self.reverse[name].get(value).cloned().ok_or_else(|| FacadeError::UnknownEnumValue {
            path: path.to_owned(),
            name: name.to_owned(),
            value: value.to_owned(),
        })
    }

    /// The prototype's JSON → the English contract.
    pub fn translate_output(&self, command: &str, value: &Value) -> Result<Value, FacadeError> {
        let entry = self.command(command)?;
        let shape = entry.output.as_deref().ok_or_else(|| FacadeError::NoOutputShape(command.to_owned()))?;
        self.translate_shape(shape, value, "$")
    }

    fn translate_shape(&self, name: &str, value: &Value, path: &str) -> Result<Value, FacadeError> {
        let shape = &self.shapes[name];
        if let Some(item) = &shape.items_of {
            return self.translate_items(item, value, path);
        }
        let Value::Object(object) = value else {
            return Err(FacadeError::WrongType { path: path.to_owned(), expected: "an object" });
        };
        let mut output = Map::new();
        for (key, inner) in object {
            let (to, translated) = match (&shape.key_enum, &shape.fields) {
                (Some(enum_name), _) => (self.translate_enum(enum_name, key, path)?, inner.clone()),
                (None, Some(fields)) => match fields.get(key) {
                    Some(field) => (field.to.clone(), self.translate_field(field, inner, &format!("{path}.{key}"))?),
                    None if shape.open => (key.clone(), inner.clone()),
                    None => return Err(FacadeError::UnknownKey { path: path.to_owned(), key: key.clone() }),
                },
                (None, None) => unreachable!("a checked shape has fields, key_enum or items_of"),
            };
            if output.insert(to.clone(), translated).is_some() {
                return Err(FacadeError::KeyCollision { path: path.to_owned(), key: to });
            }
        }
        Ok(Value::Object(output))
    }

    fn translate_field(&self, field: &Field, value: &Value, path: &str) -> Result<Value, FacadeError> {
        if value.is_null() {
            return Ok(Value::Null);
        }
        if let Some(enum_name) = &field.enum_name {
            let Value::String(text) = value else {
                return Err(FacadeError::WrongType { path: path.to_owned(), expected: "a string" });
            };
            return self.translate_enum(enum_name, text, path).map(Value::String);
        }
        if let Some(shape) = &field.shape {
            return self.translate_shape(shape, value, path);
        }
        if let Some(item) = &field.items {
            return self.translate_items(item, value, path);
        }
        if let Some(values) = &field.values {
            let Value::Object(object) = value else {
                return Err(FacadeError::WrongType { path: path.to_owned(), expected: "an object" });
            };
            let mut output = Map::new();
            for (key, inner) in object {
                let inner_path = format!("{path}[{key}]");
                output.insert(key.clone(), self.translate_shape(values, inner, &inner_path)?);
            }
            return Ok(Value::Object(output));
        }
        Ok(value.clone())
    }

    fn translate_items(&self, shape: &str, value: &Value, path: &str) -> Result<Value, FacadeError> {
        let Value::Array(items) = value else {
            return Err(FacadeError::WrongType { path: path.to_owned(), expected: "a list" });
        };
        items
            .iter()
            .enumerate()
            .map(|(i, item)| self.translate_shape(shape, item, &format!("{path}[{i}]")))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array)
    }

    fn translate_enum(&self, name: &str, value: &str, path: &str) -> Result<String, FacadeError> {
        self.enums
            .get(name)
            .and_then(|values| values.get(value))
            .cloned()
            .ok_or_else(|| FacadeError::UnknownEnumValue {
                path: path.to_owned(),
                name: name.to_owned(),
                value: value.to_owned(),
            })
    }

    /// Runs `command` on the prototype and carries back its exit code, its stderr, and its stdout: JSON
    /// translated when `--json` was asked and the command has an output shape, even on a refusal, and
    /// text as it is otherwise.
    pub fn run_prototype(&self, prototype: &PythonPrototype, command: &str, args: &[String]) -> Result<Run, FacadeError> {
        let argv = self.python_argv(command, args)?;
        let json = argv.first().is_some_and(|arg| arg == "--json") && self.command(command)?.output.is_some();
        let output = prototype.launch(&argv)?.run()?;
        let exit = output
            .status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .ok_or_else(|| FacadeError::NoExitCode(output.status.to_string()))?;
        let stdout = if json && !output.stdout.trim_ascii().is_empty() {
            let value = serde_json::from_slice(&output.stdout).map_err(FacadeError::BadJson)?;
            Printed::Json(self.translate_output(command, &value)?)
        } else {
            Printed::Text(output.stdout)
        };
        Ok(Run { exit, stdout, stderr: output.stderr })
    }
}

/// Where the prototype lives: `tools/regia.py` of the dossier.
pub struct PythonPrototype {
    pub dossier: PathBuf,
    pub script: PathBuf,
    /// An absolute path, or a name to look up; without it, the search finds `python` or `python3`.
    interpreter: Option<String>,
}

impl PythonPrototype {
    pub fn in_dossier(dossier: &Path, interpreter: Option<String>) -> Self {
        Self { dossier: dossier.to_path_buf(), script: dossier.join("tools").join("regia.py"), interpreter }
    }

    /// The prototype of `dossier`, if there is a dossier and it has the prototype's entry point.
    pub fn find(dossier: Option<&Path>, interpreter: Option<String>) -> Option<Self> {
        let prototype = Self::in_dossier(dossier?, interpreter);
        prototype.script.is_file().then_some(prototype)
    }

    /// The interpreter at its absolute path: the one named at construction, or the search's.
    pub fn interpreter(&self) -> Result<Pin, FacadeError> {
        Ok(match &self.interpreter {
            Some(name) => Pin::named(Program::Python, Path::new(name), &Search::of_this_process())?,
            None => Pin::resolve(Program::Python)?,
        })
    }

    /// The prototype on `argv`, started from the root of the dossier through `regia-exec`: an
    /// absolute interpreter, the environment of [`Program::Python`], and limits.
    pub fn launch(&self, argv: &[String]) -> Result<Launch, FacadeError> {
        let script = DossierPath::parse(SCRIPT).map_err(ExecError::from)?;
        let args = regia_exec::python::prototype(&script, argv).map_err(ExecError::from)?;
        Ok(self.interpreter()?.launch(args, &self.dossier).limits(PROTOTYPE_LIMITS))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn data_used_as_keys_is_never_translated() {
        let input = json!({"per_progetto": {"pronti": {"Approvato": 1, "—": 2}}});
        let output = Table::embedded().translate_output("brief", &input).unwrap();
        assert_eq!(output, json!({"by_project": {"pronti": {"approved": 1, "unset": 2}}}));
    }

    #[test]
    fn it_fails_closed_on_unknown_keys_and_values() {
        let table = Table::embedded();
        let key = table.translate_output("brief", &json!({"chiave_nuova": 1}));
        assert!(matches!(key, Err(FacadeError::UnknownKey { .. })));
        let value = table.translate_output("brief", &json!({"in_corso": [{"stato": "Boh"}]}));
        assert!(matches!(value, Err(FacadeError::UnknownEnumValue { .. })));
        let lock = table.translate_output("lock renew", &json!({"ok": false, "stato": "altrui", "nuova": 1}));
        assert!(matches!(lock, Err(FacadeError::UnknownKey { .. })));
    }

    #[test]
    fn null_fields_stay_null() {
        let input = json!({"pronti": [{"stato": null, "maturita": null, "sblocca": null}]});
        let output = Table::embedded().translate_output("brief", &input).unwrap();
        assert_eq!(output, json!({"ready": [{"status": null, "maturity": null, "unblocks": null}]}));
    }

    #[test]
    fn local_machine_sections_translate_too() {
        let input = json!({
            "controller": {
                "ultimo_giro": "t", "vecchio": false,
                "budget": {"giri": 3, "da": "t", "motivo": "m", "allarme": true},
                "da_aprire": [{"nome": "n", "giorno": "g", "esito": "e", "cartella": "c", "prompt": "p", "motivo": "m", "tentativi": 2}],
                "da_riprendere": [{
                    "progetto": "x", "sessione": "s", "riprende": "t", "come": "c", "comando_orca": "o", "motivo": "m",
                    "cartella": "c", "origine": "o", "adapter": "a", "agente": "g", "sessione_orca": "so", "transcript": "t",
                }],
            },
            "dossier": [{"progetto": "x", "cartella": "c", "problema": "non-pushati", "branch": "b", "avanti": 1}],
            "fuori_dal_tracker": {"x": {"totale": 1, "altre": 0, "voci": [{"controllo": "c", "progetto": "x", "dove": "d", "testo": "t", "perche": "p"}]}},
        });
        let output = Table::embedded().translate_output("brief", &input).unwrap();
        assert_eq!(output["dossier"][0]["problem"], "not_pushed");
        assert_eq!(output["outside_tracker"]["x"]["items"][0]["why"], "p");
        assert_eq!(output["controller"]["stale"], false);
        assert_eq!(output["controller"]["to_open"][0]["attempts"], 2);
        assert_eq!(output["controller"]["to_resume"][0]["orca_session"], "so");
        assert_eq!(output["controller"]["budget"]["alarm"], true);
    }

    #[test]
    fn arguments_translate_and_values_do_not() {
        let table = Table::embedded();
        assert!(matches!(
            table.python_argv("brief", &strings(&["--json", "--strana"])),
            Err(FacadeError::UnknownArgument { .. })
        ));
        assert!(matches!(
            table.python_argv("brief", &strings(&["--project", "--json"])),
            Err(FacadeError::MissingValue { .. })
        ));
        assert_eq!(table.python_argv("brief", &strings(&["--json", "--project", "alpha"])).unwrap(), ["--json", "brief", "--progetto", "alpha"]);
        // `--json` goes first wherever it was written, and a project called like an enum value stays.
        let renew = strings(&["alpha", "--holder", "ready", "--ttl", "60", "--json"]);
        assert_eq!(
            table.python_argv("lock renew", &renew).unwrap(),
            ["--json", "lock", "rinnova", "alpha", "--proprietario", "ready", "--ttl", "60"]
        );
    }

    /// `--reports-processed a b` is one list for the prototype's `nargs="*"`, which would keep only the
    /// last of two repeated options; no value is a list too.
    #[test]
    fn an_option_with_many_values_takes_those_that_follow() {
        let table = Table::embedded();
        let argv = strings(&["--project", "alpha", "--reports-processed", "a.md", "b.md", "--no-push", "--json"]);
        assert_eq!(
            table.python_argv("step finish", &argv).unwrap(),
            ["--json", "passo", "fine", "--progetto", "alpha", "--report-elaborati", "a.md", "b.md", "--no-push"]
        );
        let empty = strings(&["--reports-processed", "--project", "alpha"]);
        assert_eq!(table.python_argv("step finish", &empty).unwrap(), ["passo", "fine", "--report-elaborati", "--progetto", "alpha"]);
        let both = "schema = 1\n[commands.x]\npython = [\"x\"]\ngroup = \"reads\"\n[commands.x.args]\n\"--y\" = { to = \"--y\", flag = true, many = true }\n";
        let files = [FILES.to_vec(), vec![("x.toml", both)]].concat();
        assert!(Table::from_files(files).unwrap_err().problem.contains("many values"));
    }

    #[test]
    fn positional_values_translate_through_their_enum() {
        let table = Table::embedded();
        assert_eq!(table.python_argv("task maturity", &strings(&["3", "ready"])).unwrap(), ["task", "maturita", "3", "pronto"]);
        assert_eq!(table.python_argv("task maturity", &strings(&["3", "draft"])).unwrap(), ["task", "maturita", "3", "abbozzo"]);
        for refused in [&["3", "pronto"][..], &["3", "ready", "4"], &["3", "-x"]] {
            assert!(table.python_argv("task maturity", &strings(refused)).is_err(), "{refused:?}");
        }
    }

    #[test]
    fn the_longest_command_name_wins() {
        let table = Table::embedded();
        let argv = strings(&["lock", "renew", "alpha"]);
        let (command, rest) = table.split_command(&argv).unwrap();
        assert_eq!((command.as_str(), rest), ("lock renew", &argv[2..]));
        let argv = strings(&["brief", "--json"]);
        assert_eq!(table.split_command(&argv).unwrap().0, "brief");
        assert!(table.split_command(&strings(&["lock", "steal"])).is_none());
    }

    /// The routing rules, with the switch of the group at its default, the prototype.
    #[test]
    fn a_command_runs_where_the_switch_and_the_machine_allow() {
        let table = Table::embedded();
        let engine = |native, prototype, forced| table.engine("brief", native, prototype, forced);
        assert_eq!(engine(true, true, None).unwrap(), Engine::Prototype, "the default is the prototype, where there is one");
        assert_eq!(engine(true, false, None).unwrap(), Engine::Native, "a colleague's machine runs what is ported");
        assert_eq!(engine(true, true, Some(Engine::Native)).unwrap(), Engine::Native);
        assert!(matches!(engine(true, false, Some(Engine::Prototype)), Err(FacadeError::NoPrototype(_))));
        let renew = |native, prototype, forced| table.engine("lock renew", native, prototype, forced);
        assert_eq!(renew(false, true, None).unwrap(), Engine::Prototype);
        assert!(matches!(renew(false, false, None), Err(FacadeError::NotPorted(_))));
        assert!(matches!(renew(false, true, Some(Engine::Native)), Err(FacadeError::NotPorted(_))));
    }

    /// A switch flipped to native keeps a ported command in the binary even where the prototype is.
    #[test]
    fn a_flipped_switch_keeps_the_command_native() {
        let mut files: Vec<(&str, &str)> = FILES.iter().copied().filter(|(name, _)| *name != "groups.toml").collect();
        let groups = "schema = 1\n[groups.reads]\nroute = \"prototype\"\n[groups.writes]\nroute = \"prototype\"\n[groups.state]\nroute = \"native\"\n";
        files.push(("groups.toml", groups));
        let table = Table::from_files(files).unwrap();
        assert_eq!(table.engine("brief", true, true, None).unwrap(), Engine::Native);
        assert_eq!(table.engine("lock renew", false, true, None).unwrap(), Engine::Prototype, "not ported: the prototype still runs it");
    }

    /// The registry of a project is a free block: an open shape lets its keys through as data, and still
    /// refuses two keys that would land on one name.
    #[test]
    fn an_open_shape_passes_unknown_keys_as_data() {
        let test = r#"
            schema = 1
            [groups.reads]
            route = "prototype"
            [commands.things]
            python = ["cose"]
            group = "reads"
            output = "things"
            [shapes.things]
            items_of = "thing"
            [shapes.thing]
            open = true
            [shapes.thing.fields]
            id = { to = "id" }
            titolo = { to = "title" }
        "#;
        let table = Table::from_files([("test.toml", test)]).unwrap();
        let input = json!([{"id": "alpha", "titolo": "A", "colore_a_mano": "blu"}]);
        let output = table.translate_output("things", &input).unwrap();
        assert_eq!(output, json!([{"id": "alpha", "title": "A", "colore_a_mano": "blu"}]));
        let collision = table.translate_output("things", &json!([{"titolo": "A", "title": "B"}]));
        assert!(matches!(collision, Err(FacadeError::KeyCollision { .. })));
        let closed = Table::embedded().translate_output("brief", &json!({"colore_a_mano": "blu"}));
        assert!(matches!(closed, Err(FacadeError::UnknownKey { .. })), "a shape that is not open stays closed");
    }

    #[test]
    fn a_table_that_does_not_fit_together_is_refused() {
        let base: Vec<(&str, &str)> = FILES.to_vec();
        let twice = [base.clone(), vec![("again.toml", "schema = 1\n[enums.maturity]\nabbozzo = \"draft\"\n")]].concat();
        assert!(Table::from_files(twice).unwrap_err().problem.contains("another file"));
        let ambiguous = [base.clone(), vec![("x.toml", "schema = 1\n[enums.both]\nuno = \"one\"\nuna = \"one\"\n")]].concat();
        assert!(Table::from_files(ambiguous).unwrap_err().problem.contains("two values"));
        let dangling = [base.clone(), vec![("x.toml", "schema = 1\n[commands.x]\npython = [\"x\"]\ngroup = \"nowhere\"\n")]].concat();
        assert!(Table::from_files(dangling).unwrap_err().problem.contains("does not exist"));
        let typo = [base, vec![("x.toml", "schema = 1\n[commands.x]\npython = [\"x\"]\ngroup = \"reads\"\noutptu = \"brief\"\n")]].concat();
        assert!(Table::from_files(typo).is_err(), "an unknown field of the table is an error");
    }

    /// The build embeds the whole folder: a file added for a group is never left out.
    #[test]
    fn every_file_of_the_folder_is_embedded() {
        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("table");
        let mut on_disk: Vec<String> = std::fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        on_disk.sort();
        assert_eq!(on_disk, Table::embedded_files());
    }
}
