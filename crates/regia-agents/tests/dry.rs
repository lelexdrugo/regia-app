//! A `Dry` launch records the neutral request and starts nothing: the request is a golden.

use std::fs;
use std::path::PathBuf;

use regia_agents::{Dry, LaunchRequest, LaunchSurface, Policy, Role, ServiceId, Settings, SurfaceKind};
use serde_json::Value;

fn golden(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("golden").join(name)
}

#[test]
fn a_dry_launch_records_the_neutral_request() {
    let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let policy = Policy::new(Settings::default()).unwrap();
    let assignment = policy.assign(Role::Code, None);
    let request = LaunchRequest::new(assignment.service, "trial", &folder, "do the step\nline two: è l'ora")
        .unwrap()
        .model(assignment.model_id())
        .effort(Some(assignment.effort_level()))
        .secret_env(vec!["ANTHROPIC_API_KEY".to_owned()]);
    let dry = Dry::new();
    let receipt = dry.launch(&request).unwrap();
    assert_eq!((receipt.surface, receipt.service, receipt.pid, receipt.session.as_deref()), (SurfaceKind::Dry, ServiceId::Claude, None, None));

    let mut recorded = dry.requests().pop().unwrap();
    assert_eq!(receipt.request.as_ref(), Some(&recorded));
    // The folder is this machine's: the golden names it by a placeholder.
    let resolved = request.folder.display().to_string();
    assert_eq!(recorded["folder"], Value::String(resolved));
    recorded["folder"] = Value::String("<folder>".to_owned());
    let expected: Value = serde_json::from_str(&fs::read_to_string(golden("dry-request.json")).unwrap()).unwrap();
    assert_eq!(recorded, expected);

    // The receipt is the same shape as the Cli surface's, with nothing started.
    let receipt = serde_json::to_value(&receipt).unwrap();
    for key in ["surface", "service", "model", "effort", "session", "transcript", "transcript_exists", "pid", "log", "started", "prompt_file", "resume", "warnings"] {
        assert!(receipt.get(key).is_some(), "{key}");
    }
}
