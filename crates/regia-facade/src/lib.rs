//! The facade between the English contract and the Python prototype (ADR 0017 §4). The CLI's
//! passthrough and the conformance harness both go through [`Table::translate_output`]: that is what
//! "the same table" means.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;
use serde_json::{Map, Value};

const TABLE: &str = include_str!("../table/facade.toml");

#[derive(Debug, thiserror::Error)]
pub enum FacadeError {
    #[error("the facade table does not know command `{0}`")]
    UnknownCommand(String),
    #[error("the facade table does not know argument `{argument}` of command `{command}`")]
    UnknownArgument { command: String, argument: String },
    #[error("command `{0}` has no JSON output in the facade table")]
    NoOutputShape(String),
    #[error("key `{key}` at `{path}` is not in the facade table")]
    UnknownKey { path: String, key: String },
    #[error("value `{value}` at `{path}` is not in the enum `{name}` of the facade table")]
    UnknownEnumValue { path: String, name: String, value: String },
    #[error("expected {expected} at `{path}`")]
    WrongType { path: String, expected: &'static str },
    #[error("the prototype could not be started: {0}")]
    Spawn(std::io::Error),
    #[error("the prototype failed with {status}: {stderr}")]
    PythonFailed { status: String, stderr: String },
    #[error("the prototype printed invalid JSON: {0}")]
    BadJson(serde_json::Error),
}

#[derive(Debug, Deserialize)]
struct RawTable {
    schema: u32,
    commands: BTreeMap<String, CommandEntry>,
    enums: BTreeMap<String, BTreeMap<String, String>>,
    shapes: BTreeMap<String, Shape>,
}

#[derive(Debug, Deserialize)]
struct CommandEntry {
    python: Vec<String>,
    output: Option<String>,
    #[serde(default)]
    args: BTreeMap<String, String>,
}

/// An object with known keys, or an object whose keys are an enum, or a list of a shape.
#[derive(Debug, Deserialize)]
struct Shape {
    fields: Option<BTreeMap<String, Field>>,
    key_enum: Option<String>,
    items_of: Option<String>,
}

#[derive(Debug, Deserialize)]
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

pub struct Table {
    raw: RawTable,
}

impl Table {
    /// The table compiled into the binary.
    pub fn embedded() -> Self {
        let raw: RawTable = toml::from_str(TABLE).expect("the embedded facade table is valid");
        assert_eq!(raw.schema, 1, "unsupported facade table schema");
        Self { raw }
    }

    pub fn knows_command(&self, command: &str) -> bool {
        self.raw.commands.contains_key(command)
    }

    /// `regia <command> <args…>` in English → the argv of `regia.py`. Values pass untouched and the
    /// global `--json` goes first, where the prototype's parser wants it.
    pub fn python_argv(&self, command: &str, args: &[String]) -> Result<Vec<String>, FacadeError> {
        let entry = self
            .raw
            .commands
            .get(command)
            .ok_or_else(|| FacadeError::UnknownCommand(command.to_owned()))?;
        let mut global = Vec::new();
        let mut rest = Vec::new();
        for arg in args {
            if arg == "--json" {
                global.push(arg.clone());
            } else if arg.starts_with("--") {
                let translated = entry.args.get(arg).ok_or_else(|| FacadeError::UnknownArgument {
                    command: command.to_owned(),
                    argument: arg.clone(),
                })?;
                rest.push(translated.clone());
            } else {
                rest.push(arg.clone());
            }
        }
        Ok(global.into_iter().chain(entry.python.iter().cloned()).chain(rest).collect())
    }

    /// The prototype's JSON → the English contract.
    pub fn translate_output(&self, command: &str, value: &Value) -> Result<Value, FacadeError> {
        let entry = self
            .raw
            .commands
            .get(command)
            .ok_or_else(|| FacadeError::UnknownCommand(command.to_owned()))?;
        let shape = entry
            .output
            .as_deref()
            .ok_or_else(|| FacadeError::NoOutputShape(command.to_owned()))?;
        self.translate_shape(shape, value, "$")
    }

    fn translate_shape(&self, name: &str, value: &Value, path: &str) -> Result<Value, FacadeError> {
        let shape = self.raw.shapes.get(name).expect("shapes in the table refer to shapes in the table");
        if let Some(item) = &shape.items_of {
            return self.translate_items(item, value, path);
        }
        let Value::Object(object) = value else {
            return Err(FacadeError::WrongType { path: path.to_owned(), expected: "an object" });
        };
        let mut output = Map::new();
        if let Some(enum_name) = &shape.key_enum {
            for (key, inner) in object {
                let translated = self.translate_enum(enum_name, key, path)?;
                output.insert(translated, inner.clone());
            }
            return Ok(Value::Object(output));
        }
        let fields = shape.fields.as_ref().expect("a shape has fields, key_enum or items_of");
        for (key, inner) in object {
            let field = fields.get(key).ok_or_else(|| FacadeError::UnknownKey {
                path: path.to_owned(),
                key: key.clone(),
            })?;
            let inner_path = format!("{path}.{key}");
            output.insert(field.to.clone(), self.translate_field(field, inner, &inner_path)?);
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
        self.raw
            .enums
            .get(name)
            .and_then(|values| values.get(value))
            .cloned()
            .ok_or_else(|| FacadeError::UnknownEnumValue {
                path: path.to_owned(),
                name: name.to_owned(),
                value: value.to_owned(),
            })
    }

    /// Runs a command that is not ported yet on the prototype and returns its JSON in English.
    pub fn run_python_json(
        &self,
        prototype: &PythonPrototype,
        command: &str,
        args: &[String],
    ) -> Result<Value, FacadeError> {
        let argv = self.python_argv(command, args)?;
        let output = prototype
            .command(&argv)
            .stdin(Stdio::null())
            .output()
            .map_err(FacadeError::Spawn)?;
        if !output.status.success() {
            return Err(FacadeError::PythonFailed {
                status: output.status.to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }
        let value = serde_json::from_slice(&output.stdout).map_err(FacadeError::BadJson)?;
        self.translate_output(command, &value)
    }
}

/// Where the prototype lives: `tools/regia.py` of the dossier.
pub struct PythonPrototype {
    pub interpreter: String,
    pub script: PathBuf,
}

impl PythonPrototype {
    pub fn in_dossier(dossier: &Path, interpreter: Option<String>) -> Self {
        Self {
            interpreter: interpreter.unwrap_or_else(|| {
                if cfg!(windows) { "python".to_owned() } else { "python3".to_owned() }
            }),
            script: dossier.join("tools").join("regia.py"),
        }
    }

    pub fn command(&self, argv: &[String]) -> Command {
        let mut command = Command::new(&self.interpreter);
        command.arg(&self.script).args(argv);
        command
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
            "controller": {"ultimo_giro": "t", "vecchio": false, "budget": null, "da_aprire": [], "da_riprendere": []},
            "dossier": [{"progetto": "x", "cartella": "c", "problema": "non-pushati", "branch": "b", "avanti": 1}],
            "fuori_dal_tracker": {"x": {"totale": 1, "altre": 0, "voci": [{"controllo": "c", "progetto": "x", "dove": "d", "testo": "t", "perche": "p"}]}},
        });
        let output = Table::embedded().translate_output("brief", &input).unwrap();
        assert_eq!(output["dossier"][0]["problem"], "not_pushed");
        assert_eq!(output["outside_tracker"]["x"]["items"][0]["why"], "p");
        assert_eq!(output["controller"]["stale"], false);
    }

    #[test]
    fn arguments_translate_and_values_do_not() {
        let args: Vec<String> = ["--json", "--project", "--strana"].map(str::to_owned).to_vec();
        let table = Table::embedded();
        assert!(matches!(
            table.python_argv("brief", &args),
            Err(FacadeError::UnknownArgument { .. })
        ));
        let args: Vec<String> = ["--json", "--project", "alpha"].map(str::to_owned).to_vec();
        assert_eq!(table.python_argv("brief", &args).unwrap(), ["--json", "brief", "--progetto", "alpha"]);
    }
}
