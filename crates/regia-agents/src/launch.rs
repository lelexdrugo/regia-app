//! The neutral launch request, the surfaces that consume it, and the fallback rule (ADR 0001 §6).
//! A surface says whether it failed before any effect, so a caller can fall back without launching
//! twice.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use regia_exec::{Effort, ModelId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::ServiceId;

/// Where a launch runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceKind {
    /// A detached process with its log; the prompt on stdin.
    Cli,
    /// Starts nothing: shadow runs and tests.
    Dry,
}

/// Nothing started: the caller may fall back to another surface or service.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{reason}")]
pub struct FailedBeforeEffect {
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LaunchError {
    #[error(transparent)]
    BeforeEffect(#[from] FailedBeforeEffect),
    /// Something may have started. Launching again could run the same work twice.
    #[error("{0}")]
    AfterEffect(String),
}

impl LaunchError {
    pub fn before(reason: impl Into<String>) -> LaunchError {
        LaunchError::BeforeEffect(FailedBeforeEffect { reason: reason.into() })
    }

    pub fn before_effect(&self) -> bool {
        matches!(self, LaunchError::BeforeEffect(_))
    }
}

/// What the launch is for, as the prototype names it: a project id, `[a-z0-9][a-z0-9-]*`. It names
/// the folder of the logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label(String);

impl Label {
    pub fn parse(text: &str) -> Result<Label, LaunchError> {
        let valid = text.len() <= 64
            && text.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
            && text.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if valid { Ok(Label(text.to_owned())) } else { Err(LaunchError::before("invalid project id: lowercase letters, digits and dashes")) }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A folder as the prototype resolves it: absolute and without the `\\?\` prefix Windows gives a
/// canonical path, which would also change Claude's transcript folder.
pub fn resolved_folder(folder: &Path) -> Result<PathBuf, LaunchError> {
    if !folder.is_dir() {
        return Err(LaunchError::before("the folder does not exist"));
    }
    let canonical = folder.canonicalize().map_err(|error| LaunchError::before(format!("cannot resolve the folder: {error}")))?;
    Ok(plain(canonical))
}

fn plain(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path,
    }
}

/// A launch, described by the service and consumed by a surface. It holds no argv.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchRequest {
    pub service: ServiceId,
    pub label: Label,
    /// Absolute, existing, resolved.
    pub folder: PathBuf,
    pub model: Option<ModelId>,
    pub effort: Option<Effort>,
    /// Never an argument on the Cli surface: it goes on stdin.
    pub prompt: String,
    pub clean_context: bool,
    /// Names of variables the child gets, from the user's settings. Values are read at launch.
    pub secret_env: Vec<String>,
}

impl LaunchRequest {
    /// The checks the prototype makes before any effect.
    pub fn new(service: ServiceId, label: &str, folder: &Path, prompt: &str) -> Result<LaunchRequest, LaunchError> {
        let label = Label::parse(label)?;
        if prompt.trim().is_empty() {
            return Err(LaunchError::before("empty prompt"));
        }
        let folder = resolved_folder(folder)?;
        Ok(LaunchRequest {
            service,
            label,
            folder,
            model: None,
            effort: None,
            prompt: prompt.to_owned(),
            clean_context: false,
            secret_env: Vec::new(),
        })
    }

    #[must_use]
    pub fn model(mut self, model: Option<ModelId>) -> LaunchRequest {
        self.model = model;
        self
    }

    #[must_use]
    pub fn effort(mut self, effort: Option<Effort>) -> LaunchRequest {
        self.effort = effort;
        self
    }

    #[must_use]
    pub fn clean_context(mut self, clean: bool) -> LaunchRequest {
        self.clean_context = clean;
        self
    }

    #[must_use]
    pub fn secret_env(mut self, names: Vec<String>) -> LaunchRequest {
        self.secret_env = names;
        self
    }

    /// The request as data: what a `Dry` launch records, and its golden.
    pub fn to_json(&self) -> Value {
        json!({
            "service": self.service,
            "label": self.label.as_str(),
            "folder": self.folder.display().to_string(),
            "model": self.model.as_ref().map(ModelId::as_str),
            "effort": self.effort.map(|effort| effort.as_str()),
            "prompt": self.prompt,
            "clean_context": self.clean_context,
            "secret_env": self.secret_env,
        })
    }
}

/// What a surface answers once the launch has started. It does not wait for the end.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Receipt {
    pub surface: SurfaceKind,
    pub service: ServiceId,
    pub model: Option<String>,
    pub effort: Option<&'static str>,
    pub session: Option<String>,
    pub transcript: Option<String>,
    pub transcript_exists: bool,
    pub pid: Option<u32>,
    pub log: Option<String>,
    pub started: String,
    pub prompt_file: Option<String>,
    pub resume: Option<String>,
    pub warnings: Vec<String>,
    /// Only on `Dry`: the request it would have launched.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<Value>,
}

impl Receipt {
    pub(crate) fn new(surface: SurfaceKind, request: &LaunchRequest) -> Receipt {
        Receipt {
            surface,
            service: request.service,
            model: request.model.as_ref().map(|model| model.as_str().to_owned()),
            effort: request.effort.map(|effort| effort.as_str()),
            session: None,
            transcript: None,
            transcript_exists: false,
            pid: None,
            log: None,
            started: now(),
            prompt_file: None,
            resume: None,
            warnings: Vec::new(),
            request: None,
        }
    }
}

/// The time of a launch as the prototype writes it: UTC, to the second.
pub(crate) fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}

/// Where a launch runs.
pub trait LaunchSurface {
    fn kind(&self) -> SurfaceKind;

    /// Starts the launch and returns once it is running, or fails, saying whether before any effect.
    fn launch(&self, request: &LaunchRequest) -> Result<Receipt, LaunchError>;
}

/// Starts nothing. It keeps every request it is handed, for shadow runs and tests.
#[derive(Debug, Default)]
pub struct Dry {
    requests: Mutex<Vec<Value>>,
}

impl Dry {
    pub fn new() -> Dry {
        Dry::default()
    }

    pub fn requests(&self) -> Vec<Value> {
        self.requests.lock().map(|requests| requests.clone()).unwrap_or_default()
    }
}

impl LaunchSurface for Dry {
    fn kind(&self) -> SurfaceKind {
        SurfaceKind::Dry
    }

    fn launch(&self, request: &LaunchRequest) -> Result<Receipt, LaunchError> {
        let value = request.to_json();
        if let Ok(mut requests) = self.requests.lock() {
            requests.push(value.clone());
        }
        let mut receipt = Receipt::new(SurfaceKind::Dry, request);
        receipt.request = Some(value);
        Ok(receipt)
    }
}

/// The fallback rule (ADR 0001 §6): the second surface runs only when the first failed before any
/// effect, and the receipt says why. A failure after an effect is returned as it is.
pub fn launch_with_fallback(
    first: &dyn LaunchSurface,
    then: &dyn LaunchSurface,
    request: &LaunchRequest,
) -> Result<Receipt, LaunchError> {
    match first.launch(request) {
        Err(LaunchError::BeforeEffect(failed)) => {
            let mut receipt = then.launch(request)?;
            let first = serde_json::to_value(first.kind()).ok().and_then(|kind| kind.as_str().map(str::to_owned));
            receipt.warnings.push(format!("{} unavailable, used {}: {failed}", first.unwrap_or_default(), receipt_kind(&receipt)));
            Ok(receipt)
        }
        other => other,
    }
}

fn receipt_kind(receipt: &Receipt) -> &'static str {
    match receipt.surface {
        SurfaceKind::Cli => "cli",
        SurfaceKind::Dry => "dry",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Failing(LaunchError, Mutex<u32>);

    impl LaunchSurface for Failing {
        fn kind(&self) -> SurfaceKind {
            SurfaceKind::Cli
        }

        fn launch(&self, _: &LaunchRequest) -> Result<Receipt, LaunchError> {
            *self.1.lock().unwrap() += 1;
            Err(self.0.clone())
        }
    }

    fn request() -> LaunchRequest {
        LaunchRequest::new(ServiceId::Claude, "trial", &std::env::current_dir().unwrap(), "do the step").unwrap()
    }

    #[test]
    fn a_request_is_checked_before_any_effect() {
        let here = std::env::current_dir().unwrap();
        for (label, folder, prompt) in [
            ("Trial X", here.clone(), "p"),
            ("", here.clone(), "p"),
            ("-x", here.clone(), "p"),
            ("trial", here.clone(), "  "),
            ("trial", here.join("does-not-exist"), "p"),
        ] {
            let refused = LaunchRequest::new(ServiceId::Claude, label, &folder, prompt).unwrap_err();
            assert!(refused.before_effect(), "{label:?} {prompt:?}");
        }
        let folder = request().folder;
        assert!(folder.is_absolute() && !folder.to_string_lossy().starts_with(r"\\?\"), "{}", folder.display());
    }

    /// The prototype's `auto`: a fallback only before any effect, with the reason in the receipt.
    #[test]
    fn the_fallback_runs_only_before_an_effect() {
        let dry = Dry::new();
        let before = Failing(LaunchError::before("not reachable"), Mutex::new(0));
        let receipt = launch_with_fallback(&before, &dry, &request()).unwrap();
        assert_eq!(receipt.surface, SurfaceKind::Dry);
        assert!(receipt.warnings.iter().any(|warning| warning.contains("not reachable")), "{:?}", receipt.warnings);

        let after = Failing(LaunchError::AfterEffect("launch refused".to_owned()), Mutex::new(0));
        let error = launch_with_fallback(&after, &dry, &request()).unwrap_err();
        assert_eq!(error, LaunchError::AfterEffect("launch refused".to_owned()));
        assert_eq!(dry.requests().len(), 1, "the second surface ran only for the first case");
        assert_eq!((*before.1.lock().unwrap(), *after.1.lock().unwrap()), (1, 1));
    }

    #[test]
    fn plain_drops_only_the_verbatim_prefix_of_a_drive() {
        assert_eq!(plain(PathBuf::from(r"\\?\C:\Users\x")), PathBuf::from(r"C:\Users\x"));
        assert_eq!(plain(PathBuf::from(r"\\?\UNC\server\share")), PathBuf::from(r"\\?\UNC\server\share"));
        assert_eq!(plain(PathBuf::from("/home/x")), PathBuf::from("/home/x"));
    }
}
