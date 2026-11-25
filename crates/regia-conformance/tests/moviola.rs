//! The moviola, read (#484 of the dossier): the core's portfolio, timelines and sidecars against the prototype's
//! `serve._portafoglio`, `serve._vista` and `dati.sidecar` on the synthetic dossiers of `fixtures/moviola/`, and the
//! dossier's hostile paths (threat model T05, V14) as properties of the product.
//!
//! The prototype's answers are recorded in `fixtures/moviola/oracle.json` by `oracle/moviola_oracle.py`, which the
//! ignored test below runs again on a copy of the prototype:
//!
//! ```text
//! REGIA_HOME=<copy> cargo test -p regia-conformance --test moviola -- --ignored
//! REGIA_HOME=<copy> RECORD_MOVIOLA=1 cargo test -p regia-conformance --test moviola -- --ignored
//! ```
//!
//! The comparison is on parsed JSON, through a table of the prototype's keys and words in the product's names. It
//! is declared on both sides: what the product does not carry is listed in `DROPPED`, and a key the product adds
//! must hold its empty value (`null`, `[]`, an object of empty values) wherever the prototype has none. Paths on this machine are written
//! relative to the fixtures' folder on both sides.

use std::fs;
use std::path::{Path, PathBuf};

use regia_conformance::workspace_root;
use regia_core::TrackerConfigV1;
use regia_core::moviola::{self, LinkSettings, MoviolaError, Project, Range};
use regia_exec::{Pin, Program, python};
use regia_facade::PROTOTYPE_LIMITS;
use regia_identity::prototype_env_var;
use serde_json::{Map, Value, json};

/// The prototype's keys, in the product's names. One name each, whatever the record.
const KEYS: [(&str, &str); 52] = [
    ("progetto", "project"),
    ("project_url", "project_url"),
    ("milestones", "milestones"),
    ("direzioni", "directions"),
    ("aggregati", "aggregates"),
    ("id", "id"),
    ("ts", "ts"),
    ("tipo", "kind"),
    ("titolo", "title"),
    ("direzione", "direction"),
    ("genitori", "parents"),
    ("salute", "health"),
    ("apre_direzione", "opens_direction"),
    ("rif", "refs"),
    ("processo", "process"),
    ("autore", "author"),
    ("link", "links"),
    ("issue", "issue"),
    ("commit", "commit"),
    ("report", "report"),
    ("round", "round"),
    ("controlli", "checks"),
    ("difetti", "defects"),
    ("trovati", "found"),
    ("chiusi", "closed"),
    ("chiude", "closes"),
    ("ruolo", "role"),
    ("agente", "agent"),
    ("modello", "model"),
    ("sessione", "session"),
    ("testo", "text"),
    ("url", "url"),
    ("hash", "hash"),
    ("percorso", "path"),
    ("esiste", "exists"),
    ("aperta_da", "opened_by"),
    ("stato", "status"),
    ("motivo", "reason"),
    ("task", "task"),
    ("finestra", "window"),
    ("sottoagenti_dichiarati", "declared_subagents"),
    ("modelli", "models"),
    ("effort", "effort"),
    ("sessioni", "sessions"),
    ("via", "via"),
    ("esito", "outcome"),
    ("token", "tokens"),
    ("in", "input"),
    ("out", "output"),
    ("cache_lettura", "cache_read"),
    ("cache_scrittura", "cache_write"),
    ("milestone", "milestones"),
];

/// The sidecar's keys and the portfolio's, on top of [`KEYS`].
const MORE_KEYS: [(&str, &str); 13] = [
    ("cambiato", "changed"),
    ("decisioni_aperte", "open_decisions"),
    ("rischi", "risks"),
    ("prossimo", "next"),
    ("in_breve", "in_brief"),
    ("claim_cambiati", "changed_claims"),
    ("deviazioni", "deviations"),
    ("piano", "plan"),
    ("deviazione", "deviation"),
    ("impatto", "impact"),
    ("ultima", "last"),
    ("dal", "from"),
    ("al", "to"),
];

/// What the product does not carry, by the path of the prototype's key (`[]` for any item of a list). The view's
/// page is the prototype's local server's route, which the product has not (ADR 0001 §9); `snapshot` of a
/// milestone is its sidecar's file, which `get_snapshot` reads by the milestone's id; `progetto` repeats the
/// timeline's own; `schema` is the same on every record.
const DROPPED: [&str; 8] = [
    "milestones/[]/schema",
    "milestones/[]/progetto",
    "milestones/[]/snapshot",
    "milestones/[]/link/pagina",
    "direzioni/[]/schema",
    "aggregati/[]/schema",
    "sidecar/schema",
    "sidecar/progetto",
];

/// The words of the closed lists, by the path of the key that holds them.
fn word(path: &str, word: &str) -> Option<&'static str> {
    let kinds = [
        ("milestone", "milestone"),
        ("decisione", "decision"),
        ("gate", "gate"),
        ("eccezione", "exception"),
        ("esplorazione", "exploration"),
        ("rientro", "rejoin"),
        ("abbandono", "abandonment"),
    ];
    let health = [("in salute", "healthy"), ("a rischio", "at_risk"), ("in attesa del titolare", "waiting_on_owner")];
    let status = [("attiva", "active"), ("abbandonata", "abandoned"), ("rientrata", "rejoined")];
    let references = [("relativo", "relative"), ("dati", "data"), ("assoluto", "absolute"), ("fuori", "outside")];
    let list: &[(&str, &'static str)] = if path.ends_with("link/report/[]/tipo") {
        &references
    } else if path == "milestones/[]/tipo" || path == "ultima/tipo" {
        &kinds
    } else if path == "milestones/[]/salute" || path == "salute" {
        &health
    } else if path == "direzioni/[]/stato" {
        &status
    } else {
        return None;
    };
    Some(list.iter().find(|(known, _)| *known == word).unwrap_or_else(|| panic!("{path}: `{word}` is not in the table")).1)
}

fn product_key(path: &str, key: &str) -> String {
    KEYS.iter()
        .chain(MORE_KEYS.iter())
        .find(|(python, _)| *python == key)
        .map(|(_, product)| (*product).to_owned())
        .unwrap_or_else(|| panic!("{path}/{key}: a key of the prototype neither translated nor dropped"))
}

/// The prototype's JSON in the product's names, its dropped keys left out.
fn translated(value: &Value, path: &str) -> Value {
    let child = |key: &str| if path.is_empty() { key.to_owned() } else { format!("{path}/{key}") };
    match value {
        Value::Object(map) => {
            let mut out = Map::new();
            for (key, item) in map {
                let at = child(key);
                if DROPPED.contains(&at.as_str()) {
                    continue;
                }
                out.insert(product_key(&at, key), translated(item, &at));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(|item| translated(item, &child("[]"))).collect()),
        Value::String(text) => word(path, text).map_or_else(|| value.clone(), |id| json!(id)),
        _ => value.clone(),
    }
}

/// `null`, `[]`, or an object of empty values: what the product writes where the prototype writes nothing.
fn empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.values().all(empty),
        _ => false,
    }
}

/// Whether `actual` says what `expected` says: every key of `expected` with the same value, and every other key
/// of `actual` empty. The differences, each with its path.
fn differences(expected: &Value, actual: &Value, path: &str, out: &mut Vec<String>) {
    match (expected, actual) {
        (Value::Object(expected), Value::Object(actual)) => {
            for (key, value) in expected {
                match actual.get(key) {
                    Some(other) => differences(value, other, &format!("{path}/{key}"), out),
                    None => out.push(format!("{path}/{key}: missing")),
                }
            }
            for (key, value) in actual {
                if !expected.contains_key(key) && !empty(value) {
                    out.push(format!("{path}/{key}: {value} where the prototype has nothing"));
                }
            }
        }
        (Value::Array(expected), Value::Array(actual)) if expected.len() == actual.len() => {
            for (index, (one, other)) in expected.iter().zip(actual).enumerate() {
                differences(one, other, &format!("{path}/{index}"), out);
            }
        }
        _ if expected == actual => {}
        _ => out.push(format!("{path}: {expected} != {actual}")),
    }
}

fn assert_same(expected: &Value, actual: &Value, what: &str) {
    let mut out = Vec::new();
    differences(expected, actual, "", &mut out);
    assert!(out.is_empty(), "{what}:\n{}", out.join("\n"));
}

fn fixtures() -> PathBuf {
    workspace_root().join("fixtures").join("moviola")
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))).unwrap()
}

/// The drive's form of a Windows verbatim path, as the resolver gives it.
fn strip(path: PathBuf) -> PathBuf {
    let text = path.display().to_string();
    text.strip_prefix(r"\\?\").filter(|rest| rest.chars().nth(1) == Some(':')).map_or(path, PathBuf::from)
}

/// `root`, as the resolver writes the paths under it.
fn canonical(root: &Path) -> PathBuf {
    strip(fs::canonicalize(root).unwrap())
}

/// Every string under `root` written relative to it, as the oracle writes the prototype's.
fn relative(value: Value, root: &str) -> Value {
    match value {
        Value::Object(map) => Value::Object(map.into_iter().map(|(key, item)| (key, relative(item, root))).collect()),
        Value::Array(items) => Value::Array(items.into_iter().map(|item| relative(item, root)).collect()),
        Value::String(text) if text.starts_with(root) => json!(format!("<root>{}", text[root.len()..].replace('\\', "/"))),
        other => other,
    }
}

struct Fixture {
    root: PathBuf,
    settings: Value,
    oracle: Value,
}

fn fixture() -> Fixture {
    let root = canonical(&fixtures());
    Fixture { settings: read_json(&root.join("settings.json")), oracle: read_json(&root.join("oracle.json")), root }
}

impl Fixture {
    fn projects(&self) -> Vec<Project> {
        self.settings["projects"]
            .as_array()
            .unwrap()
            .iter()
            .map(|project| Project {
                id: project["id"].as_str().unwrap().to_owned(),
                folder: self.root.join(project["folder"].as_str().unwrap()),
                title: project["title"].as_str().map(str::to_owned),
            })
            .collect()
    }

    fn project(&self, id: &str) -> Project {
        self.projects().into_iter().find(|project| project.id == id).unwrap()
    }

    fn links(&self, id: &str) -> LinkSettings {
        let link = &self.settings["links"][id];
        let field = |key: &str| link[key].as_str().map(str::to_owned);
        LinkSettings { owner: field("owner"), repo: field("repo"), project_url: field("project_url"), remote: field("remote") }
    }

    fn product<T: serde::Serialize>(&self, value: &T) -> Value {
        relative(serde_json::to_value(value).unwrap(), &self.root.display().to_string())
    }
}

#[test]
fn the_portfolio_is_the_prototype_s() {
    let f = fixture();
    let portfolio = moviola::portfolio(&f.projects()).unwrap();
    assert_eq!(portfolio.iter().map(|summary| summary.id.as_str()).collect::<Vec<_>>(), ["alpha", "beta", "gamma"]);
    let expected: Vec<Value> = f.oracle["portfolio"].as_array().unwrap().iter().map(|entry| translated(entry, "")).collect();
    assert_same(&json!(expected), &f.product(&portfolio), "the portfolio");
}

#[test]
fn each_timeline_is_the_prototype_s_view_without_its_sidecars() {
    let f = fixture();
    for project in f.projects() {
        let mut view = f.oracle["views"][&project.id].clone();
        view.as_object_mut().unwrap().remove("snapshot").expect("the view has its sidecars");
        let timeline = moviola::timeline(&project, &f.links(&project.id), &Range::default()).unwrap();
        // The product's one key of its own: the implicit direction, which the prototype's viewer names in its code.
        let mut product = f.product(&timeline);
        assert_eq!(product.as_object_mut().unwrap().remove("main_direction"), Some(json!("principale")));
        assert_same(&translated(&view, ""), &product, &format!("the timeline of {}", project.id));
    }
}

#[test]
fn each_sidecar_is_the_prototype_s_and_only_a_milestone_s_id_finds_one() {
    let f = fixture();
    let mut read = 0;
    for project in f.projects() {
        let sidecars = f.oracle["views"][&project.id]["snapshot"].as_object().unwrap();
        for (id, sidecar) in sidecars {
            let product = moviola::snapshot(&project.folder, id).unwrap();
            assert_same(&translated(sidecar, "sidecar"), &f.product(&product), &format!("the sidecar {id}"));
            read += usize::from(product.is_some());
        }
    }
    assert_eq!(read, 7, "six sidecars of alpha and the one of beta");
    let alpha = f.project("alpha");
    let known = "01JCM100000000000000000000";
    assert!(moviola::snapshot(&alpha.folder, known).unwrap().is_some());
    for unknown in ["", "01jcm100000000000000000000", "01JCM100000000000000000000.json", "../alpha", "snapshot", "CON", "01JCB100000000000000000000"] {
        assert_eq!(moviola::snapshot(&alpha.folder, unknown).unwrap(), None, "{unknown:?}");
    }
    // A sidecar's file with no milestone of its id is not read.
    let gamma = f.project("gamma");
    assert_eq!(moviola::snapshot(&gamma.folder, known).unwrap(), None);
}

#[test]
fn in_brief_is_read_where_the_sidecar_has_it_and_absent_elsewhere() {
    let f = fixture();
    let alpha = f.project("alpha");
    let with = moviola::snapshot(&alpha.folder, "01JCM100000000000000000000").unwrap().unwrap();
    assert_eq!(with.in_brief.as_deref(), Some("The project starts. Nothing is late."));
    let without = moviola::snapshot(&alpha.folder, "01JCM200000000000000000000").unwrap().unwrap();
    assert_eq!(without.in_brief, None);
    assert_eq!(f.product(&without)["in_brief"], Value::Null, "the key is there, empty");
}

#[test]
fn the_remotes_are_the_prototype_s() {
    let f = fixture();
    for case in f.oracle["remotes"].as_array().unwrap() {
        let url = case[0].as_str().unwrap();
        assert_eq!(moviola::normalize_remote(url).as_deref(), case[1].as_str(), "{url:?}");
    }
}

#[test]
fn the_ranges_are_the_prototype_s() {
    let f = fixture();
    let alpha = f.project("alpha");
    for case in f.oracle["ranges"].as_array().unwrap() {
        let (from, to, expected) = (case[0].as_str(), case[1].as_str(), &case[2]);
        let range = Range::parse(from, to);
        if expected["error"] == json!(true) {
            assert!(range.is_err(), "{from:?} {to:?}");
            continue;
        }
        let range = range.unwrap();
        let from = range.from.map(|at| at.format("%Y-%m-%dT%H:%M:%SZ").to_string());
        let to = range.to.map(|at| at.format("%Y-%m-%dT%H:%M:%SZ").to_string());
        assert_eq!((from.as_deref(), to.as_deref()), (expected["from"].as_str(), expected["to"].as_str()), "{case}");
        let kept: Vec<String> =
            moviola::timeline(&alpha, &LinkSettings::default(), &range).unwrap().milestones.into_iter().map(|m| m.milestone.id).collect();
        assert_eq!(json!(kept), expected["kept"], "{from:?} {to:?}");
    }
}

/// The copy of the prototype in `REGIA_HOME`.
fn prototype() -> PathBuf {
    let home = std::env::var_os(prototype_env_var("HOME")).filter(|value| !value.is_empty()).map(PathBuf::from);
    let home = home.unwrap_or_else(|| panic!("set {} to a copy of the prototype's tools/", prototype_env_var("HOME")));
    assert!(home.join("tools").join("moviola").join("serve.py").is_file(), "no tools/moviola/serve.py in {}", home.display());
    home
}

#[test]
#[ignore = "needs a copy of the prototype in REGIA_HOME"]
fn the_prototype_gives_the_recorded_answers() {
    let home = prototype();
    let script = workspace_root().join("crates").join("regia-conformance").join("oracle").join("moviola_oracle.py");
    let interpreter = Pin::resolve(Program::Python).expect("a Python interpreter");
    let mut argv: Vec<String> = python::FLAGS.map(str::to_owned).to_vec();
    argv.extend([script.display().to_string(), fixtures().display().to_string()]);
    let output = regia_exec::harness::program(&interpreter, &argv, &home)
        .limits(PROTOTYPE_LIMITS)
        .env(&prototype_env_var("HOME"), &home)
        .run()
        .expect("run the oracle");
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    let recorded = fixtures().join("oracle.json");
    if std::env::var_os("RECORD_MOVIOLA").is_some() {
        fs::write(&recorded, &stdout).unwrap();
    }
    let answers: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(answers, read_json(&recorded), "the prototype's answers changed: record them again");
}

// ---------- the dossier's hostile paths (T05, V14) ----------

/// A copy of `alpha` in a clean folder of the build's own: the tests below change it.
fn copy_of_alpha(case: &str) -> (PathBuf, PathBuf) {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("moviola").join(case);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let root = canonical(&root);
    let alpha = root.join("alpha");
    copy_tree(&fixtures().join("projects").join("alpha"), &alpha);
    (root, alpha)
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A link to a folder: symbolic, or on Windows without the privilege a junction, which the prototype's own test
/// makes with `mklink /J`; here Python's `_winapi.CreateJunction` makes it, as no other program may start. `false`
/// when this machine allows neither.
fn link_folder(link: &Path, target: &Path) -> bool {
    #[cfg(unix)]
    return std::os::unix::fs::symlink(target, link).is_ok();
    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_dir(target, link).is_ok() {
            return true;
        }
        let Ok(interpreter) = Pin::resolve(Program::Python) else { return false };
        let code = "import _winapi, sys; _winapi.CreateJunction(sys.argv[1], sys.argv[2])";
        let argv = ["-I".to_owned(), "-c".to_owned(), code.to_owned(), target.display().to_string(), link.display().to_string()];
        let cwd = link.parent().unwrap();
        regia_exec::harness::program(&interpreter, &argv, cwd).run().is_ok_and(|output| output.status.success()) && link.exists()
    }
}

fn link_file(link: &Path, target: &Path) -> bool {
    #[cfg(unix)]
    return std::os::unix::fs::symlink(target, link).is_ok();
    #[cfg(windows)]
    return std::os::windows::fs::symlink_file(target, link).is_ok();
}

/// Appends a milestone to the copy's `milestones.jsonl`.
fn add_milestone(folder: &Path, id: &str, reports: &[String]) {
    let path = folder.join(".regia").join("moviola").join("milestones.jsonl");
    let mut text = fs::read_to_string(&path).unwrap();
    let milestone = json!({"id": id, "ts": "2026-10-22T09:00:00Z", "tipo": "milestone", "direzione": "principale", "genitori": [],
                           "titolo": "Hostile references", "salute": "in salute", "rif": {"report": reports}});
    text.push_str(&format!("{milestone}\n"));
    fs::write(&path, text).unwrap();
}

#[test]
fn a_report_s_path_is_never_followed_out_of_the_dossier() {
    let (root, alpha) = copy_of_alpha("hostile-reports");
    fs::write(root.join("outside.md"), "outside").unwrap();
    let outside = root.join("elsewhere");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("s.md"), "outside").unwrap();
    let mut reports: Vec<String> = [
        "../outside.md",
        "..\\outside.md",
        ".regia/../../outside.md",
        ".regia/report/../../../outside.md",
        "C:\\Windows\\win.ini",
        "C:/Windows/win.ini",
        "c:outside.md",
        "\\\\?\\C:\\Windows\\win.ini",
        "\\\\.\\C:\\Windows\\win.ini",
        "//?/C:/Windows/win.ini",
        "\\\\server\\share\\x.md",
        "/etc/passwd",
        ".regia/report/x.md:stream",
        ".regia/report/x.md::$DATA",
        "CON",
        "NUL",
        ".regia/report/CON.md",
        "aux.md",
        ".regia/report/2026-10-19-12-present.md.",
        ".regia/report/2026-10-19-12-present.md ",
        ".REGIA/REPORT/2026-10-19-12-PRESENT.MD",
        "a\0b",
    ]
    .map(str::to_owned)
    .to_vec();
    reports.push(format!(".regia/report/{}.md", "x".repeat(300)));
    reports.push(root.join("outside.md").display().to_string());
    let linked = link_folder(&alpha.join(".regia").join("report").join("ln"), &outside);
    if linked {
        reports.extend([".regia/report/ln/s.md".to_owned(), ".regia\\report\\ln\\s.md".to_owned()]);
    } else {
        eprintln!("links not allowed on this machine: the case of a link out of the dossier is skipped");
    }
    let file_linked = link_file(&alpha.join(".regia").join("report").join("ln.md"), &root.join("outside.md"));
    if file_linked {
        reports.push(".regia/report/ln.md".to_owned());
    }
    add_milestone(&alpha, "01JCX000000000000000000001", &reports);
    let project = Project { id: "alpha".to_owned(), folder: alpha.clone(), title: None };
    let timeline = moviola::timeline(&project, &LinkSettings::default(), &Range::default()).unwrap();
    let hostile = timeline.milestones.iter().find(|m| m.milestone.id == "01JCX000000000000000000001").unwrap();
    assert_eq!(hostile.links.report.len(), reports.len());
    for (reference, link) in reports.iter().zip(&hostile.links.report) {
        // A path, when there is one, is inside the dossier: nothing out of it is named, let alone read.
        if let Some(path) = &link.path {
            assert!(Path::new(path).starts_with(&alpha), "{reference:?} resolves out of the dossier: {path}");
        }
        assert!(!(link.exists && link.path.is_none()), "{reference:?}");
    }
    let kind = |reference: &str| hostile.links.report[reports.iter().position(|r| r == reference).unwrap()].kind;
    for out in ["../outside.md", "..\\outside.md", ".regia/../../outside.md", ".regia/report/../../../outside.md", "a\0b"] {
        assert_eq!(kind(out), moviola::ReportKind::Outside, "{out:?}");
    }
    for absolute in ["C:\\Windows\\win.ini", "\\\\?\\C:\\Windows\\win.ini", "\\\\.\\C:\\Windows\\win.ini", "/etc/passwd", "c:outside.md"] {
        assert_eq!(kind(absolute), moviola::ReportKind::Absolute, "{absolute:?}");
    }
    assert_eq!(kind(&root.join("outside.md").display().to_string()), moviola::ReportKind::Absolute);
    if linked {
        for through in [".regia/report/ln/s.md", ".regia\\report\\ln\\s.md"] {
            let link = &hostile.links.report[reports.iter().position(|r| r == through).unwrap()];
            assert_eq!((link.kind, link.path.as_deref(), link.exists), (moviola::ReportKind::Outside, None, false), "{through:?}");
        }
    }
    if file_linked {
        assert_eq!(kind(".regia/report/ln.md"), moviola::ReportKind::Outside);
    }
}

#[test]
fn a_link_on_the_way_to_the_moviola_s_files_is_refused() {
    let (root, alpha) = copy_of_alpha("hostile-links");
    let elsewhere = root.join("elsewhere");
    copy_tree(&fixtures().join("projects").join("beta").join(".regia").join("moviola"), &elsewhere);
    // The moviola's folder is a link to another project's.
    let other = root.join("other");
    fs::create_dir_all(other.join(".regia")).unwrap();
    if !link_folder(&other.join(".regia").join("moviola"), &elsewhere) {
        eprintln!("links not allowed on this machine: the test is skipped, as the prototype's");
        return;
    }
    assert!(matches!(moviola::read_milestones(&other), Err(MoviolaError::Link(_))));
    let project = Project { id: "other".to_owned(), folder: other.clone(), title: None };
    assert!(moviola::timeline(&project, &LinkSettings::default(), &Range::default()).is_err());
    assert!(moviola::portfolio(&[project]).is_err());
    // The folder of the sidecars is a link.
    let snapshots = alpha.join(".regia").join("moviola").join("snapshot");
    fs::rename(&snapshots, root.join("moved")).unwrap();
    assert!(link_folder(&snapshots, &root.join("moved")));
    assert!(matches!(moviola::snapshot(&alpha, "01JCM100000000000000000000"), Err(MoviolaError::Link(_))));
    // A sidecar or a JSONL file that is a link to a file.
    // The link itself, never what it leads to.
    fs::remove_dir(&snapshots).or_else(|_| fs::remove_file(&snapshots)).unwrap();
    fs::rename(root.join("moved"), &snapshots).unwrap();
    let sidecar = snapshots.join("01JCM100000000000000000000.json");
    fs::rename(&sidecar, root.join("sidecar.json")).unwrap();
    if !link_file(&sidecar, &root.join("sidecar.json")) {
        eprintln!("links to files not allowed on this machine: their cases are skipped");
        return;
    }
    assert!(matches!(moviola::snapshot(&alpha, "01JCM100000000000000000000"), Err(MoviolaError::Link(_))));
    let directions = alpha.join(".regia").join("moviola").join("direzioni.jsonl");
    fs::rename(&directions, root.join("direzioni.jsonl")).unwrap();
    assert!(link_file(&directions, &root.join("direzioni.jsonl")));
    assert!(matches!(moviola::read_directions(&alpha), Err(MoviolaError::Link(_))));
}

#[test]
fn a_moviola_that_does_not_read_says_where() {
    let (_, alpha) = copy_of_alpha("unreadable");
    let file = alpha.join(".regia").join("moviola").join("aggregati.jsonl");
    let good = fs::read_to_string(&file).unwrap();
    let cases = [
        ("{not json\n", "aggregati.jsonl line 1: invalid JSON"),
        ("\n\n[1, 2]\n", "aggregati.jsonl line 3: not a JSON object"),
        ("{\"task\": \"12\"}\n", "line 1: `task` is not a whole number"),
        ("{\"round\": 1}\n", "line 1: `task` is missing"),
    ];
    for (text, message) in cases {
        fs::write(&file, text).unwrap();
        let error = moviola::read_aggregates(&alpha).unwrap_err().to_string();
        assert!(error.contains(message), "{error}");
    }
    fs::write(&file, good).unwrap();
    let milestones = alpha.join(".regia").join("moviola").join("milestones.jsonl");
    let text = fs::read_to_string(&milestones).unwrap();
    // A value outside a closed list: the prototype's `registra` refuses it, and the product does not read it.
    fs::write(&milestones, text.replacen("\"salute\": \"in salute\"", "\"salute\": \"verde\"", 1)).unwrap();
    let error = moviola::read_milestones(&alpha).unwrap_err().to_string();
    assert!(error.contains("`salute` is `verde`"), "{error}");
    // A folder in the place of a file.
    fs::remove_file(&milestones).unwrap();
    fs::create_dir_all(&milestones).unwrap();
    assert!(matches!(moviola::read_milestones(&alpha), Err(MoviolaError::NotAFile(_))));
}

#[test]
fn text_written_by_agents_reaches_the_view_normalized() {
    let (_, alpha) = copy_of_alpha("text");
    let milestones = alpha.join(".regia").join("moviola").join("milestones.jsonl");
    let text = fs::read_to_string(&milestones).unwrap();
    fs::write(&milestones, text.replacen("First synthetic step", "First\\u202e step\\n\\u001b[31mred", 1)).unwrap();
    let first = moviola::read_milestones(&alpha).unwrap().into_iter().next().unwrap();
    assert_eq!(first.title, "First‹U+202E› step red");
}

/// `fonti.config_tracker` as the links read it: no configuration, or the dossier's unreadable, is none; a local
/// file that does not read is an error, never the dossier's in its place (#203 of the dossier).
#[test]
fn the_links_take_the_tracker_s_configuration_by_the_prototype_s_rule() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("moviola").join("config");
    let _ = fs::remove_dir_all(&root);
    let (state, dossier) = (root.join("state"), root.join("dossier"));
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(dossier.join("config")).unwrap();
    let load = || TrackerConfigV1::load_for_links(Some(&state), Some(&dossier), None);
    assert!(load().unwrap().is_none());
    let shared = dossier.join("config").join("tracker.json");
    fs::write(&shared, "{not json").unwrap();
    assert!(load().unwrap().is_none());
    let config = json!({"owner": "acme", "repo": "tasks", "project_id": "p", "status_field_id": "f", "status_options": {},
                        "project_url": "https://github.com/users/acme/projects/7"});
    fs::write(&shared, config.to_string()).unwrap();
    let read = load().unwrap().unwrap();
    assert_eq!((read.owner.as_str(), read.project_url.as_deref()), ("acme", Some("https://github.com/users/acme/projects/7")));
    fs::write(&shared, json!({"owner": "acme", "repo": "tasks", "project_id": "p", "status_field_id": "f", "status_options": {},
                              "project_url": 7}).to_string()).unwrap();
    assert_eq!(load().unwrap().unwrap().project_url, None, "a Project that is not a string is none");
    fs::write(state.join("tracker.json"), "{}").unwrap();
    assert!(load().is_err(), "a local file that does not read");
}
