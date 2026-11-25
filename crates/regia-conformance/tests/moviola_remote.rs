//! The moviola's registry and the remote of each project (#592 of the dossier): the registry composed in
//! `regia-runtime`, and the URL of a commit read from the `origin` of the project's own repo, as the prototype's
//! `fonti.url_remote` and `Rimandi.commit` give it, on a synthetic dossier of three repos: an ssh remote, an https
//! one with credentials in it, and none.
//!
//! The prototype's answer is the constant below; the ignored test runs the prototype again and compares:
//!
//! ```text
//! REGIA_HOME=<copy> cargo test -p regia-conformance --test moviola_remote -- --ignored
//! ```
//!
//! The secret in the second URL must not come out of anything the product returns, the argv of the call that
//! reads it, or what the prototype prints.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use regia_core::moviola::{self, LinkSettings, Project, Range};
use regia_core::{IssueState, RawIssue, Tracker, TrackerBinding, TrackerConfigV1, TrackerError, base_labels, status_option_names};
use regia_exec::{Args, Pin, Program, Text, python};
use regia_facade::PROTOTYPE_LIMITS;
use regia_identity::prototype_env_var;
use regia_runtime::{project_links, project_remote, registry_projects};
use serde_json::{Value, json};

static NEXT: AtomicUsize = AtomicUsize::new(0);

const SECRET: &str = "segreto";
const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

/// The three repos: the folder's name, and the `origin` it is given. What the prototype's `Rimandi.commit` gives
/// for each, recorded below, is the link the product must give.
const REMOTES: [(&str, Option<&str>); 3] = [
    ("ssh", Some("git@github.com:o/r.git")),
    ("https", Some("https://utente:segreto@github.com/o/r.git")),
    ("none", None),
];

/// What the prototype's `Rimandi(folder, config={}).commit(COMMIT)` gave for each repo, URL of the commit included.
fn recorded(name: &str) -> Option<String> {
    match name {
        "ssh" | "https" => Some(format!("https://github.com/o/r/commit/{COMMIT}")),
        "none" => None,
        other => panic!("no repo {other}"),
    }
}

fn git(dir: &Path, words: &[&'static str], values: &[&str]) {
    let args = words.iter().fold(Args::new(), |args, word| args.word(word));
    let args = values.iter().fold(args, |args, value| args.value(&Text::parse(value).unwrap()));
    let output = Pin::resolve(Program::Git).expect("git").launch(args, dir).run().expect("git runs");
    assert!(output.status.success(), "git {words:?}: {}", String::from_utf8_lossy(&output.stderr));
}

/// A dossier's root in a clean folder of the build's own.
fn root(case: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("moviola-remote")
        .join(format!("{case}-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst)));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root.canonicalize().unwrap()
}

/// A project: a repo with the given `origin` and a milestone that refers to a commit.
fn project(root: &Path, name: &str, origin: Option<&str>) -> Project {
    let folder = root.join(name);
    let moviola = folder.join(".regia").join("moviola");
    fs::create_dir_all(&moviola).unwrap();
    git(&folder, &["init", "-q", "-b", "main"], &[]);
    if let Some(url) = origin {
        git(&folder, &["remote", "add"], &["origin", url]);
    }
    let milestone = json!({"schema": "moviola.milestone/v0", "id": "01JCM100000000000000000000", "ts": "2026-10-14T10:00:00Z",
        "progetto": name, "tipo": "milestone", "direzione": "principale", "genitori": [], "titolo": "A step",
        "salute": "in salute", "rif": {"issue": [], "commit": [COMMIT], "report": []}});
    fs::write(moviola.join("milestones.jsonl"), format!("{milestone}\n")).unwrap();
    Project { id: name.to_owned(), folder, title: None }
}

/// The URL of the only commit of the project's timeline, as the product builds it, and the whole timeline written.
fn product_link(project: &Project, tracker: &LinkSettings) -> (Option<String>, String) {
    let settings = project_links(tracker, &project.folder);
    let timeline = moviola::timeline(project, &settings, &Range::parse(None, None).unwrap()).expect("a timeline");
    let written = serde_json::to_string(&timeline).unwrap();
    let value: Value = serde_json::from_str(&written).unwrap();
    let link = &value["milestones"][0]["links"]["commit"][0];
    assert_eq!(link["hash"], COMMIT);
    (link["url"].as_str().map(str::to_owned), written)
}

#[test]
fn a_commit_s_link_is_the_prototype_s_and_the_secret_does_not_come_out() {
    let root = root("links");
    for (name, origin) in REMOTES {
        let project = project(&root, name, origin);
        let (url, written) = product_link(&project, &LinkSettings::default());
        assert_eq!(url, recorded(name), "{name}");
        assert!(!written.contains(SECRET), "{name}: the timeline carries the secret");
        let remote = project_remote(&project.folder);
        assert_eq!(remote.as_deref(), recorded(name).map(|url| url.trim_end_matches(&format!("/commit/{COMMIT}")).to_owned()).as_deref());
        assert!(!remote.unwrap_or_default().contains(SECRET), "{name}");
    }
}

/// The remote is the project's, not the app's: two projects of one run have each its own, and the tracker's part of
/// the settings is kept whole.
#[test]
fn each_project_has_its_own_remote_and_the_tracker_s_part_is_kept() {
    let root = root("own");
    let tracker = LinkSettings {
        owner: Some("acme".to_owned()),
        repo: Some("tasks".to_owned()),
        project_url: Some("https://github.com/orgs/acme/projects/1".to_owned()),
        remote: Some("https://example.invalid/not-this-one".to_owned()),
    };
    let (ssh, none) = (project(&root, "ssh", REMOTES[0].1), project(&root, "none", None));
    let (ssh_links, none_links) = (project_links(&tracker, &ssh.folder), project_links(&tracker, &none.folder));
    assert_eq!(ssh_links.remote.as_deref(), Some("https://github.com/o/r"));
    assert_eq!(none_links.remote, None, "the tracker's remote is nobody's: a project without one has none");
    for links in [&ssh_links, &none_links] {
        assert_eq!((&links.owner, &links.repo, &links.project_url), (&tracker.owner, &tracker.repo, &tracker.project_url));
    }
    assert_eq!(product_link(&none, &tracker).0, None);
}

/// A folder that is no repo, one that is not there, and an `origin` that is no web page: no URL, no error.
#[test]
fn no_remote_is_none_and_never_an_error() {
    let root = root("absent");
    let plain = root.join("plain");
    fs::create_dir_all(&plain).unwrap();
    // A fresh folder under the build's own can sit inside the workspace's repo, so the plain one is not asked.
    assert_eq!(project_remote(&root.join("missing")), None);
    let local = project(&root, "local", Some("/srv/p.git"));
    assert_eq!(project_remote(&local.folder), None, "an origin that is a path is no web page");
    let file = project(&root, "file", Some("file:///srv/p.git"));
    assert_eq!(project_remote(&file.folder), None);
}

// ---------- the registry ----------

/// A tracker that answers with the issues it is given.
struct Issues(Vec<RawIssue>);

impl Tracker for Issues {
    fn open_issues(&self) -> Result<Vec<RawIssue>, TrackerError> {
        Ok(self.0.clone())
    }
}

fn issue(number: u64, objective: bool, body: String) -> RawIssue {
    let labels = if objective { vec![base_labels()[0].name.clone()] } else { Vec::new() };
    RawIssue {
        number,
        title: format!("Issue {number}"),
        state: IssueState::Open,
        state_reason: None,
        body,
        url: format!("https://github.com/acme/tasks/issues/{number}"),
        updated_at: "2026-10-14T10:00:00Z".to_owned(),
        created_at: None,
        closed_at: None,
        labels,
        parent: None,
        sub_issues: Vec::new(),
        blocked_by: Vec::new(),
        blocked_by_total: 0,
        // An option the configuration does not bind: it plays no part in the registry.
        status_option_id: Some("opt-unknown".to_owned()),
        field_values: Vec::new(),
    }
}

fn entry(id: &str, folder: &Path) -> String {
    format!("<!-- regia:progetto\nid: {id}\ncartella: {}\n-->\n", folder.display())
}

#[test]
fn the_registry_keeps_the_objectives_with_a_folder_here_and_the_first_of_an_id() {
    let root = root("registry");
    let (alpha, beta, other) = (root.join("alpha"), root.join("beta"), root.join("other"));
    for folder in [&alpha, &beta, &other] {
        fs::create_dir_all(folder).unwrap();
    }
    let issues = Issues(vec![
        issue(1, true, entry("alpha", &alpha)),
        issue(2, true, entry("beta", &root.join("not-here"))),
        issue(3, false, entry("gamma", &other)),
        issue(4, true, entry("alpha", &other)),
        issue(5, true, entry("beta", &beta)),
        issue(6, true, "no registry block".to_owned()),
    ]);
    let options = status_option_names().into_iter().enumerate().map(|(index, name)| (name, format!("opt-{index}"))).collect();
    let config = TrackerConfigV1 {
        owner: "acme".to_owned(),
        repo: "tasks".to_owned(),
        project_id: "PVT_1".to_owned(),
        status_field_id: "PVTSSF_1".to_owned(),
        status_options: options,
        project_url: None,
        project_field_configured: false,
    };
    let binding = TrackerBinding::new(&config).expect("a binding");
    let projects = registry_projects(&issues, &binding, "acme").expect("a registry");
    let found: Vec<(&str, &Path)> = projects.iter().map(|project| (project.id.as_str(), project.folder.as_path())).collect();
    assert_eq!(found, [("alpha", alpha.as_path()), ("beta", beta.as_path())]);
}

// ---------- the prototype ----------

/// The prototype's own remote and commit link for each repo, printed as JSON: what `vista.Rimandi` makes the
/// commit's URL from. It prints no URL as git gave it.
const ORACLE: &str = r#"
import json, sys
sys.path.insert(0, sys.argv[1])
from tools.moviola import fonti, vista
answers = {}
for folder in sys.argv[3:]:
    rimandi = vista.Rimandi(folder, config={})
    answers[folder.replace("\\", "/").rsplit("/", 1)[-1]] = [fonti.url_remote(folder), rimandi.commit(sys.argv[2])[1]]
print(json.dumps(answers, sort_keys=True))
"#;

#[test]
#[ignore = "needs a copy of the prototype in REGIA_HOME"]
fn the_prototype_gives_the_recorded_links() {
    let home = std::env::var_os(prototype_env_var("HOME")).filter(|value| !value.is_empty()).map(PathBuf::from);
    let home = home.unwrap_or_else(|| panic!("set {} to a copy of the prototype", prototype_env_var("HOME")));
    assert!(home.join("tools").join("moviola").join("fonti.py").is_file(), "no tools/moviola/fonti.py in {}", home.display());
    let root = root("oracle");
    let projects: Vec<Project> = REMOTES.iter().map(|(name, origin)| project(&root, name, *origin)).collect();
    let script = root.join("oracle.py");
    fs::write(&script, ORACLE).unwrap();
    let interpreter = Pin::resolve(Program::Python).expect("a Python interpreter");
    let mut argv: Vec<String> = python::FLAGS.map(str::to_owned).to_vec();
    argv.extend([script.display().to_string(), home.display().to_string(), COMMIT.to_owned()]);
    argv.extend(projects.iter().map(|project| project.folder.display().to_string()));
    let output = regia_exec::harness::program(&interpreter, &argv, &home)
        .limits(PROTOTYPE_LIMITS)
        .env(&prototype_env_var("HOME"), &home)
        .run()
        .expect("run the prototype");
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(!stdout.contains(SECRET) && !String::from_utf8_lossy(&output.stderr).contains(SECRET), "the prototype let the secret out");
    let answers: Value = serde_json::from_str(&stdout).unwrap();
    for project in &projects {
        let [remote, link] = answers[&project.id].as_array().unwrap().as_slice() else { panic!("{}: {answers}", project.id) };
        assert_eq!(link.as_str().map(str::to_owned), recorded(&project.id), "{}: the recorded link is not the prototype's", project.id);
        assert_eq!(remote.as_str().map(str::to_owned), project_remote(&project.folder), "{}", project.id);
        assert_eq!(product_link(project, &LinkSettings::default()).0, link.as_str().map(str::to_owned), "{}", project.id);
    }
}
