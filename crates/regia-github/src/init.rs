//! What `init` reads of GitHub ([`InitReads`]), as the prototype reads it (`tools/inizializza.py` of the dossier):
//! the user and the token's scopes from `gh api -i user`, the repo, the owner's Projects page after page, the repo's
//! labels page after page. Nothing is written. A REST call names its path in argv, built from typed values; the
//! GraphQL query goes on stdin with its variables. Each call is logged before it is made, failed ones too, in the
//! words the prototype logs it with (`_descrivi`): for the query, the form `-f query=… -f o=…` it would take in argv.

use std::collections::BTreeSet;
use std::path::PathBuf;

use regia_core::init::{FieldNode, FieldOption, GhSaid, GhUser, InitError, InitReads, ProjectNode, RepoFacts};
use regia_core::pystr;
use regia_exec::{Args, Pin, Program, RepoSlug, Text};
use serde::Deserialize;
use serde_json::{Value, json};

/// The owner's Projects, as the prototype asks for them: each with its status field by name, and its fields with
/// their kind, id and name, and a single select's options.
const PROJECTS_QUERY: &str = r#"query($o:String!,$after:String){repositoryOwner(login:$o){
  ... on User{projectsV2(first:50,after:$after){pageInfo{hasNextPage endCursor} nodes{number id title url
    field(name:"Status"){__typename ... on ProjectV2SingleSelectField{id name options{id name color description}}}
    fields(first:50){nodes{__typename ... on ProjectV2FieldCommon{id name} ... on ProjectV2SingleSelectField{options{id name color description}}}}}}}
  ... on Organization{projectsV2(first:50,after:$after){pageInfo{hasNextPage endCursor} nodes{number id title url
    field(name:"Status"){__typename ... on ProjectV2SingleSelectField{id name options{id name color description}}}
    fields(first:50){nodes{__typename ... on ProjectV2FieldCommon{id name} ... on ProjectV2SingleSelectField{options{id name color description}}}}}}}}}"#;

/// The labels a page holds; a shorter page is the last.
const LABELS_PAGE: usize = 100;

/// What `gh` says when GitHub has no such thing.
const NOT_FOUND: [&str; 3] = ["HTTP 404", "Not Found", "Could not resolve"];

/// The reads of `init` through the user's `gh`, which holds the token.
pub struct GhInit {
    /// Resolved once, to an absolute path. A gh that cannot be found fails the first call.
    gh: Result<Pin, String>,
    work_dir: PathBuf,
    calls: Vec<String>,
}

impl Default for GhInit {
    fn default() -> Self {
        Self::new()
    }
}

impl GhInit {
    pub fn new() -> Self {
        Self { gh: Pin::resolve(Program::Gh).map_err(|error| error.to_string()), work_dir: regia_exec::neutral_folder(), calls: Vec::new() }
    }

    /// Runs `gh`, logged first as `description`. A failure is what `gh` said, its stderr, else its stdout, else
    /// only its exit code.
    fn run(&mut self, description: String, args: Args, stdin: Option<String>) -> Result<String, InitError> {
        self.calls.push(description);
        let gh = self.gh.as_ref().map_err(|error| InitError::Unavailable(error.clone()))?;
        let mut launch = gh.launch(args, &self.work_dir);
        if let Some(stdin) = stdin {
            launch = launch.stdin(stdin);
        }
        let output = launch.run().map_err(|error| InitError::Unavailable(error.to_string()))?;
        if !output.status.success() {
            let (stderr, stdout) = (String::from_utf8_lossy(&output.stderr), String::from_utf8_lossy(&output.stdout));
            let text = [pystr::strip(&stderr), pystr::strip(&stdout)].into_iter().find(|text| !text.is_empty());
            let not_found = text.is_some_and(|text| NOT_FOUND.iter().any(|needle| text.contains(needle)));
            let said = text.map_or(GhSaid::Exit(output.status.code().unwrap_or(-1)), |text| GhSaid::Text(text.to_owned()));
            return Err(InitError::Gh { said, not_found });
        }
        String::from_utf8(output.stdout).map_err(|error| InitError::Unreadable(error.to_string()))
    }

    /// `gh api [-i] <path>`, a GET.
    fn get(&mut self, path: &str, include: bool) -> Result<String, InitError> {
        let value = Text::parse(path).map_err(|error| InitError::Unavailable(error.to_string()))?;
        let (args, description) = if include {
            (Args::new().word("api").word("-i").value(&value), format!("gh api -i {path}"))
        } else {
            (Args::new().word("api").value(&value), format!("gh api {path}"))
        };
        self.run(description, args, None)
    }
}

impl InitReads for GhInit {
    fn user(&mut self) -> Result<GhUser, InitError> {
        let text = self.get("user", true)?;
        user_of(&text)
    }

    fn repository(&mut self, owner: &str, repo: &str) -> Result<RepoFacts, InitError> {
        let text = self.get(&format!("repos/{}", slug(owner, repo)?.as_str()), false)?;
        let value = parse(&text)?;
        let object = value.as_object().ok_or_else(|| InitError::Unreadable("the repo is not an object".to_owned()))?;
        let text_of = |value: Option<&Value>| value.and_then(Value::as_str).map(str::to_owned);
        Ok(RepoFacts {
            private: truthy(object.get("private")),
            node_id: text_of(object.get("node_id")),
            owner_node_id: text_of(object.get("owner").and_then(|owner| owner.get("node_id"))),
        })
    }

    fn projects(&mut self, owner: &str) -> Result<Option<Vec<ProjectNode>>, InitError> {
        let mut nodes = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let mut variables = json!({"o": owner});
            let mut description = format!("gh api graphql -f {} -f o={owner}", described(PROJECTS_QUERY));
            if let Some(cursor) = &after {
                variables["after"] = json!(cursor);
                description.push_str(&format!(" -f after={cursor}"));
            }
            let body = json!({"query": PROJECTS_QUERY, "variables": variables}).to_string();
            let page = parse(&self.run(description, regia_exec::gh::graphql(), Some(body))?)?;
            if let Some(errors) = page.get("errors").filter(|errors| truthy(Some(errors))) {
                let messages: Vec<&str> =
                    errors.as_array().into_iter().flatten().map(|error| error.get("message").and_then(Value::as_str).unwrap_or_default()).collect();
                return Err(InitError::Gh { said: GhSaid::Text(messages.join("; ")), not_found: false });
            }
            let found = page.get("data").and_then(|data| data.get("repositoryOwner")).filter(|found| truthy(Some(found)));
            let Some(found) = found else { return Ok(None) };
            let connection = found.get("projectsV2").filter(|connection| truthy(Some(connection)));
            let listed = connection.and_then(|connection| connection.get("nodes")).and_then(Value::as_array);
            for node in listed.into_iter().flatten().filter(|node| truthy(Some(node))) {
                let wire: ProjectWire = serde_json::from_value(node.clone()).map_err(|error| InitError::Unreadable(error.to_string()))?;
                nodes.push(wire.into_node());
            }
            let info = connection.and_then(|connection| connection.get("pageInfo"));
            if !truthy(info.and_then(|info| info.get("hasNextPage"))) {
                return Ok(Some(nodes));
            }
            let cursor = info.and_then(|info| info.get("endCursor")).and_then(Value::as_str);
            after = Some(cursor.ok_or_else(|| InitError::Unreadable("a page of Projects with more and no cursor".to_owned()))?.to_owned());
        }
    }

    fn label_names(&mut self, owner: &str, repo: &str) -> Result<BTreeSet<String>, InitError> {
        let slug = slug(owner, repo)?;
        let mut names = BTreeSet::new();
        let mut page = 1;
        loop {
            let text = self.get(&format!("repos/{}/labels?per_page={LABELS_PAGE}&page={page}", slug.as_str()), false)?;
            let labels: Vec<LabelWire> = serde_json::from_str(&text).map_err(|error| InitError::Unreadable(error.to_string()))?;
            let count = labels.len();
            names.extend(labels.into_iter().map(|label| label.name));
            if count < LABELS_PAGE {
                return Ok(names);
            }
            page += 1;
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.clone()
    }
}

#[derive(Deserialize)]
struct LabelWire {
    name: String,
}

#[derive(Deserialize)]
struct ProjectWire {
    number: u64,
    id: String,
    title: String,
    url: String,
    #[serde(default)]
    field: Option<FieldWire>,
    #[serde(default)]
    fields: Option<FieldsWire>,
}

#[derive(Deserialize)]
struct FieldsWire {
    #[serde(default)]
    nodes: Vec<Option<FieldWire>>,
}

#[derive(Deserialize)]
struct FieldWire {
    #[serde(rename = "__typename", default)]
    kind: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    options: Option<Vec<OptionWire>>,
}

#[derive(Deserialize)]
struct OptionWire {
    id: String,
    name: String,
}

impl ProjectWire {
    fn into_node(self) -> ProjectNode {
        let fields = self.fields.map(|fields| fields.nodes.into_iter().flatten().map(FieldWire::into_node).collect()).unwrap_or_default();
        ProjectNode { number: self.number, id: self.id, title: self.title, url: self.url, status_field: self.field.map(FieldWire::into_node), fields }
    }
}

impl FieldWire {
    fn into_node(self) -> FieldNode {
        let options = self.options.map(|options| options.into_iter().map(|option| FieldOption { id: option.id, name: option.name }).collect());
        FieldNode { kind: self.kind, id: self.id, name: self.name, options }
    }
}

fn parse(text: &str) -> Result<Value, InitError> {
    serde_json::from_str(text).map_err(|error| InitError::Unreadable(error.to_string()))
}

/// `owner/repo` as a typed value: what reaches argv is a name GitHub allows.
fn slug(owner: &str, repo: &str) -> Result<RepoSlug, InitError> {
    RepoSlug::parse(&format!("{owner}/{repo}")).map_err(|error| InitError::Unavailable(format!("not a repo of GitHub: {error}")))
}

/// Whether Python reads `value` as true: not null, false, zero, empty text, an empty list or an empty object.
fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|number| number != 0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(fields)) => !fields.is_empty(),
    }
}

/// The user and the token's scopes from what `gh api -i user` printed: the head, the first blank line, the body.
/// The scopes are `None` when the head does not show them.
fn user_of(text: &str) -> Result<GhUser, InitError> {
    let (head, body) = split_head(text);
    let mut scopes = None;
    for line in pystr::split_lines(head) {
        let (name, value) = line.split_once(':').unwrap_or((line, ""));
        if pystr::strip(name).to_lowercase() == "x-oauth-scopes" {
            scopes = Some(value.split(',').map(pystr::strip).filter(|scope| !scope.is_empty()).map(str::to_owned).collect());
        }
    }
    let login = parse(body)?.get("login").and_then(Value::as_str).map(str::to_owned);
    let login = login.ok_or_else(|| InitError::Unreadable("the answer names no login".to_owned()))?;
    Ok(GhUser { login, scopes })
}

/// The head and the body of a response printed with `-i`, split at the first blank line, `\r\n` or `\n`; all body
/// when there is none.
fn split_head(text: &str) -> (&str, &str) {
    let bytes = text.as_bytes();
    for start in 0..bytes.len() {
        let mut at = start;
        if bytes[at] == b'\r' {
            at += 1;
        }
        if bytes.get(at) != Some(&b'\n') {
            continue;
        }
        at += 1;
        if bytes.get(at) == Some(&b'\r') {
            at += 1;
        }
        if bytes.get(at) == Some(&b'\n') {
            return (&text[..start], &text[at + 1..]);
        }
    }
    ("", text)
}

/// The query as the prototype logs it: its blanks as one space, and past 120 characters cut at 117 with `…`.
fn described(query: &str) -> String {
    let words: Vec<&str> = query.split(pystr::is_space).filter(|word| !word.is_empty()).collect();
    let text = format!("query={}", words.join(" "));
    if text.chars().count() > 120 { format!("{}…", text.chars().take(117).collect::<String>()) } else { text }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_query_is_logged_as_the_prototype_logs_it() {
        let expected = "query=query($o:String!,$after:String){repositoryOwner(login:$o){ ... on User{projectsV2(first:50,after:$after){pageIn…";
        assert_eq!(described(PROJECTS_QUERY), expected);
        assert_eq!(described(" a\n\t b "), "query=a b");
    }

    #[test]
    fn the_scopes_come_from_the_head_and_the_login_from_the_body() {
        let user = user_of("HTTP/2.0 200 OK\r\nContent-Type: application/json\r\nx-oauth-scopes: gist,  repo ,project,\r\n\r\n{\"login\":\"me\"}").unwrap();
        assert_eq!(user, GhUser { login: "me".into(), scopes: Some(["gist", "project", "repo"].map(str::to_owned).into()) });
        let hidden = user_of("HTTP/2.0 200 OK\nContent-Type: application/json\n\n{\"login\":\"me\"}").unwrap();
        assert_eq!(hidden.scopes, None, "a head without them");
        let empty = user_of("HTTP/2.0 200 OK\nX-Oauth-Scopes: \n\n{\"login\":\"me\"}").unwrap();
        assert_eq!(empty.scopes, Some(BTreeSet::new()), "shown, and none");
        assert_eq!(user_of("{\"login\":\"me\"}").unwrap().scopes, None, "no head");
        assert!(matches!(user_of("HTTP/2.0 200 OK\n\nnot json"), Err(InitError::Unreadable(_))));
        assert!(matches!(user_of("HTTP/2.0 200 OK\n\n{\"name\":\"x\"}"), Err(InitError::Unreadable(_))));
    }

    #[test]
    fn the_head_ends_at_the_first_blank_line() {
        assert_eq!(split_head("a\r\n\r\nb\n\nc"), ("a", "b\n\nc"));
        assert_eq!(split_head("a\n\r\nb"), ("a", "b"));
        assert_eq!(split_head("a\r\rb"), ("", "a\r\rb"));
    }

    #[test]
    fn truth_is_python_s() {
        for value in [json!(null), json!(false), json!(0), json!(0.0), json!(""), json!([]), json!({})] {
            assert!(!truthy(Some(&value)), "{value}");
        }
        for value in [json!(true), json!(1), json!("x"), json!([0]), json!({"a": null})] {
            assert!(truthy(Some(&value)), "{value}");
        }
        assert!(!truthy(None));
    }
}
