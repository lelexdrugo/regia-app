//! Which service, tier, model and effort a role gets: three layers (ADR 0001 §6). The product's
//! defaults ([`roles.toml`](../policy/roles.toml)), each service's file, and the user's settings,
//! which name the principal, the enabled services and per-role overrides.

use std::collections::BTreeMap;
use std::path::Path;

use regia_exec::{Effort, ModelId};
use serde::{Deserialize, Serialize};

use crate::gates::{GateError, GateSettings, Gates};
use crate::launch::SurfaceKind;
use crate::{AgentService, ServiceId};

/// How strong a model a role needs. A service's file maps each tier to a model, or to none for the
/// service's own default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Light,
    Standard,
    Deep,
}

impl Tier {
    pub fn id(self) -> &'static str {
        match self {
            Tier::Light => "light",
            Tier::Standard => "standard",
            Tier::Deep => "deep",
        }
    }
}

/// The kinds of work the product assigns a service to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Replanning,
    ScoutingAndReading,
    SynthesisAndMaps,
    Writing,
    Code,
    ExperimentsAndProbes,
    Chronicler,
    Reconnaissance,
    AdversarialReview,
    SecondImplementation,
}

impl Role {
    pub const ALL: [Role; 10] = [
        Role::Replanning,
        Role::ScoutingAndReading,
        Role::SynthesisAndMaps,
        Role::Writing,
        Role::Code,
        Role::ExperimentsAndProbes,
        Role::Chronicler,
        Role::Reconnaissance,
        Role::AdversarialReview,
        Role::SecondImplementation,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Role::Replanning => "replanning",
            Role::ScoutingAndReading => "scouting_and_reading",
            Role::SynthesisAndMaps => "synthesis_and_maps",
            Role::Writing => "writing",
            Role::Code => "code",
            Role::ExperimentsAndProbes => "experiments_and_probes",
            Role::Chronicler => "chronicler",
            Role::Reconnaissance => "reconnaissance",
            Role::AdversarialReview => "adversarial_review",
            Role::SecondImplementation => "second_implementation",
        }
    }

    pub fn parse(text: &str) -> Option<Role> {
        Role::ALL.into_iter().find(|role| role.id() == text)
    }
}

/// Which service a role names: the principal, or the diversity rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceRule {
    Principal,
    OtherThanWriter,
    Fixed(ServiceId),
}

impl<'de> Deserialize<'de> for ServiceRule {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        match text.as_str() {
            "principal" => Ok(ServiceRule::Principal),
            "other_than:writer" => Ok(ServiceRule::OtherThanWriter),
            other => ServiceId::parse(other)
                .map(ServiceRule::Fixed)
                .ok_or_else(|| serde::de::Error::custom("a service is `principal`, `other_than:writer` or a known service")),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct FallbackRule {
    service: ServiceRule,
    tier: Tier,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoleDefault {
    service: Option<ServiceRule>,
    tier: Tier,
    effort: String,
    deadline_minutes: Option<u32>,
    fallback: Option<FallbackRule>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Defaults {
    default: RoleDefault,
    roles: BTreeMap<Role, RoleDefault>,
}

/// A role's override in the user's settings. Any key may be left out.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleOverride {
    service: Option<ServiceRule>,
    tier: Option<Tier>,
    model: Option<String>,
    effort: Option<String>,
    deadline_minutes: Option<u32>,
}

/// A service in the user's settings.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceSettings {
    /// A model per tier, over the service's file.
    #[serde(default)]
    tiers: BTreeMap<Tier, String>,
    /// Variables the child gets by name, such as an API key. Only the name is written here: the
    /// value is read from the environment at launch, and never reaches a log or a file.
    #[serde(default)]
    pub secret_env: Vec<String>,
}

/// The user's settings, layer 3: a file in the configuration folder. Every key may be left out.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// The service the orchestrator starts. Without it: claude, the prototype's default.
    pub principal: Option<ServiceId>,
    /// The services the user enabled. Without it: every known one.
    pub services: Option<Vec<ServiceId>>,
    pub surface: Option<SurfaceKind>,
    #[serde(default)]
    roles: BTreeMap<Role, RoleOverride>,
    #[serde(default)]
    service: BTreeMap<ServiceId, ServiceSettings>,
    #[serde(default)]
    gates: GateSettings,
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("cannot read the settings at {path}: {reason}")]
    Read { path: String, reason: String },
    #[error("the settings do not parse: {0}")]
    Parse(String),
    #[error("the principal {0} is not among the enabled services")]
    PrincipalNotEnabled(ServiceId),
    #[error("no service is enabled")]
    NoService,
    #[error("invalid {kind} for {role}")]
    InvalidValue { kind: &'static str, role: &'static str },
    #[error("invalid variable name in the settings of {0}: names are A-Z, 0-9 and _")]
    InvalidEnvName(ServiceId),
    #[error(transparent)]
    Gate(#[from] GateError),
}

/// The file name of the settings in the configuration folder.
pub const SETTINGS_FILE: &str = "agents.toml";

impl Settings {
    pub fn parse(text: &str) -> Result<Settings, SettingsError> {
        toml::from_str(text).map_err(|error| SettingsError::Parse(error.message().to_owned()))
    }

    /// The settings in `dir`, or the defaults when the file is not there.
    pub fn load(dir: &Path) -> Result<Settings, SettingsError> {
        let path = dir.join(SETTINGS_FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => Settings::parse(&text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(error) => Err(SettingsError::Read { path: path.display().to_string(), reason: error.to_string() }),
        }
    }

    pub fn service(&self, service: ServiceId) -> Option<&ServiceSettings> {
        self.service.get(&service)
    }
}

/// Whether a role that asks for another service than the writer's got one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Diversity {
    /// The role does not ask for it.
    NotRequired,
    /// Another service than the writer's.
    Met,
    /// Only one service is enabled: the same service, in a clean context. The report says so.
    Degraded,
}

/// Where a role goes past its deadline, or when its service does not answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fallback {
    pub service: ServiceId,
    pub tier: Tier,
    pub model: Option<String>,
    pub effort: &'static str,
    pub clean_context: bool,
}

/// What a role gets: the answer of `agents show`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Assignment {
    pub role: Role,
    pub service: ServiceId,
    pub tier: Tier,
    pub model: Option<String>,
    pub effort: &'static str,
    pub surface: SurfaceKind,
    pub deadline_minutes: Option<u32>,
    pub clean_context: bool,
    pub diversity: Diversity,
    pub fallback: Option<Fallback>,
}

impl Assignment {
    pub fn model_id(&self) -> Option<ModelId> {
        self.model.as_deref().and_then(|model| ModelId::parse(model).ok())
    }

    pub fn effort_level(&self) -> Effort {
        Effort::parse(self.effort).expect("an assignment holds a known effort")
    }
}

/// The three layers, checked once.
#[derive(Debug, Clone)]
pub struct Policy {
    defaults: Defaults,
    settings: Settings,
    principal: ServiceId,
    enabled: Vec<ServiceId>,
    gates: Gates,
}

impl Policy {
    /// The compiled defaults under the user's settings.
    pub fn new(settings: Settings) -> Result<Policy, SettingsError> {
        let defaults: Defaults =
            toml::from_str(include_str!("../policy/roles.toml")).expect("the compiled role defaults are valid");
        let principal = settings.principal.unwrap_or(ServiceId::Claude);
        let mut enabled = settings.services.clone().unwrap_or_else(|| ServiceId::ALL.to_vec());
        enabled.sort();
        enabled.dedup();
        if enabled.is_empty() {
            return Err(SettingsError::NoService);
        }
        if !enabled.contains(&principal) {
            return Err(SettingsError::PrincipalNotEnabled(principal));
        }
        for (role, entry) in &settings.roles {
            if let Some(model) = &entry.model {
                ModelId::parse(model).map_err(|_| SettingsError::InvalidValue { kind: "model", role: role.id() })?;
            }
            if let Some(effort) = &entry.effort {
                Effort::parse(effort).map_err(|_| SettingsError::InvalidValue { kind: "effort", role: role.id() })?;
            }
        }
        for (service, entry) in &settings.service {
            for model in entry.tiers.values() {
                ModelId::parse(model).map_err(|_| SettingsError::InvalidValue { kind: "model", role: "a tier" })?;
            }
            let valid = |name: &String| {
                !name.is_empty() && name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
            };
            if !entry.secret_env.iter().all(valid) {
                return Err(SettingsError::InvalidEnvName(*service));
            }
        }
        let gates = Gates::product().with_settings(&settings.gates)?;
        Ok(Policy { defaults, settings, principal, enabled, gates })
    }

    pub fn principal(&self) -> ServiceId {
        self.principal
    }

    pub fn enabled(&self) -> &[ServiceId] {
        &self.enabled
    }

    pub fn gates(&self) -> &Gates {
        &self.gates
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    fn model(&self, service: ServiceId, tier: Tier) -> Option<String> {
        let user = self.settings.service.get(&service).and_then(|entry| entry.tiers.get(&tier)).cloned();
        user.or_else(|| AgentService::known(service).model_for(tier).map(|model| model.as_str().to_owned()))
    }

    /// The other enabled service, if there is one.
    fn other_than(&self, writer: ServiceId) -> Option<ServiceId> {
        self.enabled.iter().copied().find(|service| *service != writer)
    }

    /// What `role` gets. The writer, for the diversity rule, is the principal unless the caller says
    /// which service wrote.
    pub fn assign(&self, role: Role, writer: Option<ServiceId>) -> Assignment {
        let base = self.defaults.roles.get(&role).unwrap_or(&self.defaults.default);
        let user = self.settings.roles.get(&role).cloned().unwrap_or_default();
        let writer = writer.unwrap_or(self.principal);
        let rule = user.service.or(base.service).unwrap_or(ServiceRule::Principal);
        let (service, diversity) = match rule {
            ServiceRule::Principal => (self.principal, Diversity::NotRequired),
            ServiceRule::Fixed(service) if self.enabled.contains(&service) => (service, Diversity::NotRequired),
            ServiceRule::Fixed(_) => (self.principal, Diversity::NotRequired),
            ServiceRule::OtherThanWriter => match self.other_than(writer) {
                Some(other) => (other, Diversity::Met),
                None => (writer, Diversity::Degraded),
            },
        };
        let tier = user.tier.unwrap_or(base.tier);
        let effort = known_effort(user.effort.as_deref().unwrap_or(&base.effort));
        let model = user.model.clone().or_else(|| self.model(service, tier));
        let fallback = base.fallback.as_ref().map(|rule| {
            let service = match rule.service {
                ServiceRule::Principal | ServiceRule::OtherThanWriter => self.principal,
                ServiceRule::Fixed(service) => service,
            };
            Fallback { service, tier: rule.tier, model: self.model(service, rule.tier), effort, clean_context: true }
        });
        Assignment {
            role,
            service,
            tier,
            model,
            effort,
            surface: self.settings.surface.unwrap_or(SurfaceKind::Cli),
            deadline_minutes: user.deadline_minutes.or(base.deadline_minutes),
            clean_context: diversity == Diversity::Degraded,
            diversity,
            fallback,
        }
    }
}

/// The static name of a valid effort; the defaults and the settings are checked before.
fn known_effort(text: &str) -> &'static str {
    Effort::parse(text).map_or("medium", |effort| effort.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(text: &str) -> Policy {
        Policy::new(Settings::parse(text).unwrap()).unwrap()
    }

    #[test]
    fn the_defaults_carry_the_prototype_policy_without_model_names() {
        let text = include_str!("../policy/roles.toml");
        for model in ["opus", "sonnet", "haiku", "gpt"] {
            assert!(!text.contains(model), "the product defaults name a model: {model}");
        }
        let both = policy("");
        let code = both.assign(Role::Code, None);
        assert_eq!((code.service, code.tier, code.model.as_deref(), code.effort), (ServiceId::Claude, Tier::Standard, Some("sonnet"), "high"));
        let replanning = both.assign(Role::Replanning, None);
        assert_eq!((replanning.model.as_deref(), replanning.effort), (Some("opus"), "high"));
        for role in Role::ALL {
            let assignment = both.assign(role, None);
            assert_eq!(Role::parse(role.id()), Some(role));
            assert!(Effort::parse(assignment.effort).is_ok(), "{role:?}");
        }
    }

    #[test]
    fn the_review_goes_to_the_other_service_when_there_is_one() {
        let both = policy("");
        let review = both.assign(Role::AdversarialReview, None);
        assert_eq!((review.service, review.diversity, review.clean_context), (ServiceId::Codex, Diversity::Met, false));
        // Codex's file names no model: codex runs with its own default, as in the prototype.
        assert_eq!((review.model.as_deref(), review.effort, review.deadline_minutes), (None, "high", Some(20)));
        let fallback = review.fallback.unwrap();
        assert_eq!((fallback.service, fallback.model.as_deref(), fallback.clean_context), (ServiceId::Claude, Some("opus"), true));
        // A codex writer gets a claude reviewer.
        assert_eq!(both.assign(Role::AdversarialReview, Some(ServiceId::Codex)).service, ServiceId::Claude);
    }

    /// Acceptance 2 of #460: with one service, the same service in a clean context.
    #[test]
    fn with_one_service_the_review_degrades_to_a_clean_context() {
        let one = policy("principal = \"claude\"\nservices = [\"claude\"]\n");
        let review = one.assign(Role::AdversarialReview, None);
        assert_eq!((review.service, review.diversity, review.clean_context), (ServiceId::Claude, Diversity::Degraded, true));
        assert_eq!(review.model.as_deref(), Some("sonnet"));
        assert_eq!(one.assign(Role::SecondImplementation, None).diversity, Diversity::Degraded);
        assert_eq!(one.assign(Role::Code, None).diversity, Diversity::NotRequired);
    }

    #[test]
    fn the_settings_override_by_role_and_by_tier() {
        let text = "principal = \"codex\"\n\
            [roles.code]\ntier = \"deep\"\neffort = \"xhigh\"\n\
            [roles.writing]\nservice = \"claude\"\nmodel = \"claude-opus-5-5\"\n\
            [service.codex.tiers]\ndeep = \"gpt-5.4-codex\"\n";
        let custom = policy(text);
        let code = custom.assign(Role::Code, None);
        assert_eq!((code.service, code.model.as_deref(), code.effort), (ServiceId::Codex, Some("gpt-5.4-codex"), "xhigh"));
        let writing = custom.assign(Role::Writing, None);
        assert_eq!((writing.service, writing.model.as_deref()), (ServiceId::Claude, Some("claude-opus-5-5")));
        assert_eq!(custom.assign(Role::AdversarialReview, None).service, ServiceId::Claude);
    }

    #[test]
    fn invalid_settings_are_refused() {
        for text in [
            "principal = \"gemini\"",
            "surface = \"ssh\"",
            "unknown = 1",
            "[roles.code]\nmodel = \"--x\"",
            "[roles.code]\neffort = \"alto; rm\"",
            "[roles.unknown_role]\ntier = \"deep\"",
            "[service.claude]\nsecret_env = [\"bad name\"]",
            "[service.claude.tiers]\ndeep = \"a b\"",
            "services = []",
            "principal = \"codex\"\nservices = [\"claude\"]",
        ] {
            let refused = Settings::parse(text).and_then(Policy::new);
            assert!(refused.is_err(), "{text}");
        }
    }

    #[test]
    fn missing_settings_are_the_defaults() {
        let dir = std::env::current_dir().unwrap().join("no-such-settings-folder");
        let settings = Settings::load(&dir).unwrap();
        assert!(settings.principal.is_none());
        assert_eq!(Policy::new(settings).unwrap().enabled(), ServiceId::ALL);
    }
}
