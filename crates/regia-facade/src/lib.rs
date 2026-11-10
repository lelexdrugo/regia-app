//! The facade between the English contract and the Python prototype (ADR 0017 §4). The CLI's
//! passthrough and the conformance harness both go through [`Table::translate_output`]: that is what
//! "the same table" means.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use regia_exec::{DossierPath, ExecError, Launch, Limits, Pin, Program, Search};
use serde::Deserialize;
use serde_json::{Map, Value};

const TABLE: &str = include_str!("../table/facade.toml");

/// The prototype's entry point, relative to the dossier.
const SCRIPT: &str = "tools/regia.py";

/// A command of the prototype reads the tracker and may write: generous, but bounded.
const PROTOTYPE_LIMITS: Limits = Limits { timeout: Duration::from_secs(600), stdout: 64 << 20, stderr: 4 << 20 };

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
    #[error("the prototype could not run: {0}")]
    Exec(#[from] ExecError),
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
        let output = prototype.launch(&argv)?.run()?;
        if !output.status.success() {
            return Err(FacadeError::PythonFailed {
                status: output.status.to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }
        // Its warnings are the only sign that a section was lost, so they reach the user.
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
        let value = serde_json::from_slice(&output.stdout).map_err(FacadeError::BadJson)?;
        self.translate_output(command, &value)
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

    /// The prototype on `argv`, started from the root of the dossier through `regia-exec`: an
    /// absolute interpreter, the environment of [`Program::Python`], and limits.
    pub fn launch(&self, argv: &[String]) -> Result<Launch, FacadeError> {
        let python = match &self.interpreter {
            Some(name) => Pin::named(Program::Python, Path::new(name), &Search::of_this_process())?,
            None => Pin::resolve(Program::Python)?,
        };
        let script = DossierPath::parse(SCRIPT).map_err(ExecError::from)?;
        let args = regia_exec::python::prototype(&script, argv).map_err(ExecError::from)?;
        Ok(python.launch(args, &self.dossier).limits(PROTOTYPE_LIMITS))
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
