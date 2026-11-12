//! The budget policy across the two languages (#464, Acceptance 1 and 2). Local, as the prototype is: a
//! copy of the dossier's `tools/` and `policies/`, named by `REGIA_HOME`, plays the prototype, so its tests'
//! helpers write nothing into the dossier.
//!
//!     REGIA_HOME=<copy> cargo test -p regia-conformance --test policy_cross -- --ignored
//!
//! - **The precheck applies the override the product writes.** An Inbox and three Proposals, the case of
//!   the prototype's `TestPrecheck`: with the caps on, the precheck holds the Inbox back; once the product
//!   has written `proposal_caps=off`, it lets it through, with `politica: file+locale` and the override
//!   active. An override the product wrote until 12 hours later still holds at 11 and no longer at 13.
//! - **The same command writes the same file and prints the same text.** `policy set`, `show` and `reset`
//!   of the product, in Italian, against `imposta`, `politica` and `azzera` of `politica_budget.py`, each on
//!   its own state folder at the same instant: the same bytes of the override's file (CRLF on Windows, as
//!   Python's text mode writes it), the same exit, the same text, and the same JSON through the names this
//!   slice proposes for #223. A refusal's reason is each side's own: only its frame is compared.

use std::fs;
use std::path::{Path, PathBuf};

use regia_conformance::{fixture_for_python, fixture_for_rust, lf, prepare_state_dir, regia_binary};
use regia_core::policy::local_file_name;
use regia_exec::{Pin, Program, python};
use regia_facade::PROTOTYPE_LIMITS;
use regia_identity::{env_var, prototype_env_var};
use serde_json::Value;

/// `T0` of the prototype's `test_precheck.py`, which its precheck runs at.
const T0: &str = "2026-10-20T12:00:00Z";

/// The instants the runs read the policy at, in hours after [`T0`], as `REGIA_NOW` takes them.
const AFTER: [(u32, &str); 5] =
    [(0, T0), (1, "2026-10-20T13:00:00Z"), (11, "2026-10-20T23:00:00Z"), (13, "2026-10-21T01:00:00Z"), (96, "2026-10-24T12:00:00Z")];

/// The prototype's side, written into the run's folder. `precheck <hours>` runs the case of `TestPrecheck`
/// with the policy read `hours` after T0; `cli <hours> <argv>` runs `politica_budget.main` at that instant.
/// It prints one JSON line.
const PROTOTYPE_SIDE: &str = r#"import contextlib
import io
import json
import sys
from datetime import timedelta
from pathlib import Path

copy = Path(sys.argv[1])
sys.path[:0] = [str(copy / "tools"), str(copy / "tools" / "tests")]
import politica_budget as PB  # noqa: E402
from test_precheck import T0, base, crea_ambiente, issue, lancia  # noqa: E402

command, adesso = sys.argv[2], T0 + timedelta(hours=float(sys.argv[3]))
if command == "precheck":
    issues = base(issue(2, "Inbox", maturita=None), *[issue(n, "Proposta", maturita=None) for n in (10, 11, 12)])
    amb = crea_ambiente(issues)
    amb.politica = PB.carica(copy / "policies" / "budget.yaml", adesso=adesso)
    codice, j, _ = lancia(amb)
    locale = j["payload"].get("politica_locale") or {}
    print(json.dumps({"t0": T0.isoformat(), "codice": codice, "motivo": j["motivo"], "politica": j["payload"]["politica"],
                      "locale": locale.get("stato")}))
else:
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stderr(err):
        codice = PB.main(sys.argv[4:], adesso=adesso, out=out)
    print(json.dumps({"codice": codice, "out": out.getvalue(), "err": err.getvalue()}, ensure_ascii=False))
"#;

/// The prototype's names of the policy's JSON, and the product's this slice proposes for #223.
const KEYS: &[(&str, &str)] = &[
    ("automatico_max_pct", "automatic_max_pct"),
    ("riempimento_max_pct", "backfill_max_pct"),
    ("aperte_per_progetto", "open_per_project"),
    ("nuove_per_settimana", "new_per_week"),
    ("tetti_proposte", "proposal_caps"),
    ("riempimento", "backfill"),
    ("scodare", "drain"),
    ("fonte", "source"),
    ("motivo", "reason"),
    ("locale", "local"),
    ("stato", "status"),
    ("fino", "until"),
    ("valori", "values"),
    ("nota", "note"),
    ("impostato", "set_at"),
];

/// The prototype's values of `fonte` and of the override's `stato`, and the product's.
const VALUES: &[(&str, &str)] = &[
    ("file+locale", "file+local"),
    ("default+locale", "default+local"),
    ("attivo", "active"),
    ("scaduto", "expired"),
    ("illeggibile", "unreadable"),
    ("ignorato", "ignored"),
];

/// A run: exit code, stdout and stderr, with LF line breaks.
#[derive(Debug)]
struct Ran {
    exit: Option<i32>,
    stdout: String,
    stderr: String,
}

fn copy() -> PathBuf {
    let home = std::env::var_os(prototype_env_var("HOME")).filter(|value| !value.is_empty()).map(PathBuf::from);
    let home = home.unwrap_or_else(|| panic!("set {} to a copy of the prototype's tools/ and policies/", prototype_env_var("HOME")));
    assert!(home.join("tools").join("politica_budget.py").is_file(), "no tools/politica_budget.py in {}", home.display());
    home
}

fn now_after(hours: u32) -> &'static str {
    AFTER.iter().find(|(after, _)| *after == hours).map_or_else(|| panic!("no instant {hours} hours after T0"), |(_, at)| *at)
}

fn empty_responses(state: &Path) -> PathBuf {
    let path = state.join("no-responses.json");
    fs::write(&path, "{}").expect("empty responses");
    path
}

/// The product, in Italian, on the state folder `state`, `hours` after T0, with the copy as its dossier.
fn product(home: &Path, state: &Path, hours: u32, args: &[&str]) -> Ran {
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    let (launch, _) = fixture_for_rust(regia_exec::harness::launch(&regia_binary(), &args, state), &empty_responses(state), state);
    let launch = launch.env(&prototype_env_var("HOME"), home).env(&env_var("NOW"), now_after(hours)).env(&env_var("LANG"), "it");
    let output = launch.run().expect("run the product");
    Ran {
        exit: output.status.code(),
        stdout: lf(&String::from_utf8_lossy(&output.stdout)),
        stderr: lf(&String::from_utf8_lossy(&output.stderr)),
    }
}

/// The prototype's side on the state folder `state`: its JSON line.
fn prototype(home: &Path, state: &Path, args: &[String]) -> Value {
    let script = state.join("policy_side.py");
    fs::write(&script, PROTOTYPE_SIDE).expect("the prototype's side");
    let interpreter = Pin::resolve(Program::Python).expect("a Python interpreter");
    let mut argv: Vec<String> = python::FLAGS.map(str::to_owned).to_vec();
    argv.extend([script.display().to_string(), home.display().to_string()]);
    argv.extend(args.iter().cloned());
    let launch = regia_exec::harness::program(&interpreter, &argv, home).limits(PROTOTYPE_LIMITS);
    let output = fixture_for_python(launch, &empty_responses(state), state).env(&prototype_env_var("HOME"), home).run().expect("run the prototype");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "the prototype's side failed\nstdout: {stdout}\nstderr: {}", String::from_utf8_lossy(&output.stderr));
    let last = stdout.lines().last().unwrap_or_default();
    serde_json::from_str(last).unwrap_or_else(|error| panic!("{error}: {stdout}"))
}

/// The precheck of `TestPrecheck`'s case, the policy read `hours` after T0: exit, reason, the policy's
/// source and the override's status, as the prototype names them.
fn precheck(home: &Path, state: &Path, hours: u32) -> (i64, String, String, Option<String>) {
    let out = prototype(home, state, &["precheck".to_owned(), hours.to_string()]);
    assert_eq!(out["t0"], "2026-10-20T12:00:00+00:00", "the product's T0 is the prototype's");
    let text = |key: &str| out[key].as_str().unwrap_or_else(|| panic!("no {key}: {out}")).to_owned();
    (out["codice"].as_i64().expect("the exit"), text("motivo"), text("politica"), out["locale"].as_str().map(str::to_owned))
}

/// `politica_budget.py` with `argv`, `hours` after T0.
fn prototype_cli(home: &Path, state: &Path, hours: u32, argv: &[&str]) -> Ran {
    let mut args = vec!["cli".to_owned(), hours.to_string()];
    args.extend(argv.iter().map(|arg| (*arg).to_owned()));
    let out = prototype(home, state, &args);
    let text = |key: &str| lf(out[key].as_str().unwrap_or_else(|| panic!("no {key}: {out}")));
    Ran { exit: out["codice"].as_i64().and_then(|code| i32::try_from(code).ok()), stdout: text("out"), stderr: text("err") }
}

/// The prototype's JSON of the policy with the product's names.
fn in_english(python: &Value) -> Value {
    let rename = |pairs: &[(&str, &str)], text: &str| pairs.iter().find(|(theirs, _)| *theirs == text).map_or(text, |(_, ours)| *ours).to_owned();
    match python {
        Value::Object(fields) => Value::Object(fields.iter().map(|(key, value)| (rename(KEYS, key), in_english(value))).collect()),
        Value::String(text) => Value::String(rename(VALUES, text)),
        other => other.clone(),
    }
}

#[test]
#[ignore = "needs a copy of the prototype in REGIA_HOME"]
fn the_precheck_applies_the_override_the_product_writes() {
    let home = copy();
    let state = prepare_state_dir("policy-cross-precheck");
    let held = (3, "proposte-esaurite".to_owned(), "file".to_owned(), None);
    assert_eq!(precheck(&home, &state, 0), held, "with the caps on, three Proposals hold the Inbox back");

    let set = product(&home, &state, 0, &["policy", "set", "proposal_caps=off"]);
    assert_eq!(set.exit, Some(0), "{}", set.stderr);
    let applied = (0, "inbox".to_owned(), "file+locale".to_owned(), Some("attivo".to_owned()));
    assert_eq!(precheck(&home, &state, 0), applied, "the override the product wrote applies");
    assert_eq!(precheck(&home, &state, 13), applied, "without an expiry it still holds");

    // Acceptance 2 across the two languages: until 12 hours later, it holds at 11 and no longer at 13.
    let set = product(&home, &state, 0, &["policy", "set", "proposal_caps=off", "--until", "12h"]);
    assert_eq!(set.exit, Some(0), "{}", set.stderr);
    assert_eq!(precheck(&home, &state, 11), applied);
    assert_eq!(precheck(&home, &state, 13), (3, "proposte-esaurite".to_owned(), "file".to_owned(), Some("scaduto".to_owned())));
}

#[test]
#[ignore = "needs a copy of the prototype in REGIA_HOME"]
fn the_same_commands_write_the_same_file_and_print_the_same_text() {
    let home = copy();
    let (ours, theirs) = (prepare_state_dir("policy-cross-product"), prepare_state_dir("policy-cross-prototype"));
    let file = |state: &Path| fs::read(state.join(local_file_name())).ok();
    // Hours after T0, the product's argv and the prototype's.
    let steps: [(u32, &[&str], &[&str]); 14] = [
        (0, &["policy", "show"], &["politica"]),
        (0, &["policy", "show", "--json"], &["politica", "--json"]),
        (0, &["policy", "set", "proposal_caps=off", "backfill=off", "--until", "3d", "--note", "solo regia"], &[
            "imposta",
            "tetti_proposte=off",
            "riempimento=off",
            "--fino",
            "3g",
            "--nota",
            "solo regia",
        ]),
        (0, &["policy", "set", "automatic_max_pct=80"], &["imposta", "automatico_max_pct=80"]),
        (1, &["policy", "show"], &["politica"]),
        (1, &["policy", "show", "--json"], &["politica", "--json"]),
        // Above the automatic threshold once backfill is on again: refused, nothing written.
        (1, &["policy", "set", "backfill_max_pct=90", "backfill=on"], &["imposta", "riempimento_max_pct=90", "riempimento=on"]),
        (96, &["policy", "show"], &["politica"]),
        (96, &["policy", "show", "--json"], &["politica", "--json"]),
        // Over an expired override: neither its values nor its expiry nor its note stay.
        (96, &["policy", "set", "open_per_project=2"], &["imposta", "aperte_per_progetto=2"]),
        (96, &["policy", "show", "--json"], &["politica", "--json"]),
        (96, &["policy", "reset"], &["azzera"]),
        (96, &["policy", "reset"], &["azzera"]),
        (96, &["policy", "show"], &["politica"]),
    ];
    for (hours, argv, python) in steps {
        let (rust, proto) = (product(&home, &ours, hours, argv), prototype_cli(&home, &theirs, hours, python));
        assert_eq!(rust.exit, proto.exit, "{argv:?}: the exit\nproduct: {rust:?}\nprototype: {proto:?}");
        if argv.contains(&"--json") {
            let json: Value = serde_json::from_str(&rust.stdout).unwrap_or_else(|error| panic!("{argv:?}: {error}: {}", rust.stdout));
            let python: Value = serde_json::from_str(&proto.stdout).unwrap_or_else(|error| panic!("{python:?}: {error}: {}", proto.stdout));
            assert_eq!(json, in_english(&python), "{argv:?}: the JSON");
        } else {
            assert_eq!(rust.stdout, proto.stdout, "{argv:?}: the text");
            assert_eq!(rust.stdout.is_empty(), rust.exit == Some(2), "{argv:?}: text, unless refused");
        }
        if rust.exit == Some(2) {
            let frame = "override non scritto: ";
            assert!(rust.stderr.starts_with(frame) && proto.stderr.starts_with(frame), "{argv:?}\nproduct: {}\nprototype: {}", rust.stderr, proto.stderr);
        } else {
            assert_eq!((rust.stderr.as_str(), proto.stderr.as_str()), ("", ""), "{argv:?}: stderr");
        }
        assert_eq!(file(&ours), file(&theirs), "{argv:?}: the override's bytes");
    }
    assert!(file(&ours).is_none(), "reset removed it");
}
