//! `init`'s plan and the local configuration it derives (F008, F009 and F257 of the dossier's inventory): what the
//! tracker and this machine have, what is missing and what `apply` would do about it, item by item, in the
//! prototype's order and with its outcomes (`tools/inizializza.py`). Nothing here writes to the tracker: the reads go
//! through the port [`InitReads`], one call or one paged read per method, in the order the prototype makes them.
//!
//! An item's detail is data, [`Detail`]: the CLI words it in the user's language, and in Italian it is the
//! prototype's sentence. The names of the items and of the local file's keys are format v1 (`formats/init_v1.toml`),
//! and so are the labels and the statuses (`formats/tracker_v1.toml`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::v1::{BaseLabel, base_labels, project_field_name, status_option_names};

const VOCABULARY: &str = include_str!("../formats/init_v1.toml");

/// The scopes a command that only reads needs, `plan` and `derive`: each entry is one of its alternatives, and the
/// last alternative is the one to add.
const READ_SCOPES: [&[&str]; 2] = [&["repo"], &["project", "read:project"]];

/// The scopes `apply` needs, which writes.
const WRITE_SCOPES: [&[&str]; 2] = [&["repo"], &["project"]];

/// The agents' skill folders under the home, as the prototype installs the skills: a short name and the folder.
const SKILL_HOMES: [(&str, &str); 2] = [("claude", ".claude"), ("agents", ".agents")];

#[derive(Debug, Deserialize)]
struct Vocabulary {
    items: ItemNames,
    manual: ManualSteps,
    defaults: Defaults,
    skills: Skills,
    local_file: LocalFileKeys,
}

#[derive(Debug, Deserialize)]
struct ItemNames {
    gh: String,
    repo: String,
    project: String,
    status: String,
    project_field: String,
    label_prefix: String,
    skill_prefix: String,
    skills: String,
    manual_prefix: String,
}

#[derive(Debug, Deserialize)]
struct ManualSteps {
    views: String,
}

#[derive(Debug, Deserialize)]
struct Defaults {
    repo: String,
    project_title: String,
}

#[derive(Debug, Deserialize)]
struct Skills {
    install: String,
}

#[derive(Debug, Deserialize)]
struct LocalFileKeys {
    note_key: String,
    note: String,
    project_field_key: String,
    project_options_key: String,
    generator_key: String,
    generator: String,
}

fn vocabulary() -> Vocabulary {
    toml::from_str(VOCABULARY).expect("the embedded init vocabulary is valid")
}

/// The repo the plan looks for when the command names none: the prototype's tracker.
#[must_use]
pub fn default_repo() -> String {
    vocabulary().defaults.repo
}

/// The title of the Project the plan looks for when the command names none: the prototype's.
#[must_use]
pub fn default_project_title() -> String {
    vocabulary().defaults.project_title
}

/// Where an item belongs: what `ready` counts is the environment and the tracker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Environment,
    Tracker,
    Local,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Present,
    Missing,
    Different,
    Error,
}

impl Outcome {
    pub fn id(self) -> &'static str {
        match self {
            Outcome::Present => "present",
            Outcome::Missing => "missing",
            Outcome::Different => "different",
            Outcome::Error => "error",
        }
    }
}

/// What `apply` would do about an item: nothing, a write of its own, or a step for the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    None,
    Create,
    Update,
    Manual,
    Install,
}

impl Action {
    pub fn id(self) -> &'static str {
        match self {
            Action::None => "none",
            Action::Create => "create",
            Action::Update => "update",
            Action::Manual => "manual",
            Action::Install => "install",
        }
    }
}

/// What `gh` said when it failed: its text, or only its exit code when it printed nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GhSaid {
    Text(String),
    Exit(i32),
}

impl std::fmt::Display for GhSaid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GhSaid::Text(text) => f.write_str(text),
            GhSaid::Exit(code) => write!(f, "gh exited with {code}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InitError {
    /// `gh` cannot be started, or did not finish: there is no plan.
    #[error("{0}")]
    Unavailable(String),
    /// `gh` ended with an error, or GraphQL answered with errors; `not_found` when it said the thing is not there.
    #[error("{said}")]
    Gh { said: GhSaid, not_found: bool },
    /// An answer that does not read as the call's, or a file of a skill that does not read.
    #[error("{0}")]
    Unreadable(String),
}

/// The user `gh` is signed in as, and the token's scopes when the answer shows them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhUser {
    pub login: String,
    pub scopes: Option<BTreeSet<String>>,
}

/// What the plan reads of a repo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoFacts {
    pub private: bool,
    pub node_id: Option<String>,
    pub owner_node_id: Option<String>,
}

/// A Project of the owner, with its status field as GitHub answers a field by name, and its fields.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectNode {
    pub number: u64,
    pub id: String,
    pub title: String,
    pub url: String,
    /// `None` when the Project has no field of that name.
    pub status_field: Option<FieldNode>,
    pub fields: Vec<FieldNode>,
}

/// A field of a Project. A field of a type the query asks nothing of has only its kind.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FieldNode {
    /// The field's GraphQL type.
    pub kind: Option<String>,
    pub id: Option<String>,
    pub name: Option<String>,
    /// The options of a single-select field, in their order; `None` for a field of another type.
    pub options: Option<Vec<FieldOption>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldOption {
    pub id: String,
    pub name: String,
}

/// What the plan reads of GitHub, through the user's `gh`, and nothing else: no method writes. Each call is logged,
/// in the order made, failed ones too, as the prototype logs its calls.
pub trait InitReads {
    /// The signed-in user.
    fn user(&mut self) -> Result<GhUser, InitError>;
    /// The repo `owner/repo`; [`InitError::Gh`] with `not_found` when there is none.
    fn repository(&mut self, owner: &str, repo: &str) -> Result<RepoFacts, InitError>;
    /// Every Project of `owner`, page after page; `None` when GitHub knows no such owner.
    fn projects(&mut self, owner: &str) -> Result<Option<Vec<ProjectNode>>, InitError>;
    /// The names of the labels of `owner/repo`, page after page.
    fn label_names(&mut self, owner: &str, repo: &str) -> Result<BTreeSet<String>, InitError>;
    /// The calls made so far, one line each.
    fn calls(&self) -> Vec<String>;
}

/// The state of an agent's copy of a skill, against the skill's folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillCopy {
    Missing,
    Same,
    Different,
}

/// What an item says, as data. Where it holds a repo, it is `owner/repo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detail {
    /// `gh` failed on the user: what it said, and the scopes to sign in with.
    GhSignedOut { said: GhSaid, scopes: Vec<String> },
    /// The answer about the user does not read.
    GhUnreadable { error: String },
    /// Signed in, and the token does not show its scopes (a fine-grained token?): they are not checked.
    GhScopesHidden { login: String },
    /// Signed in without some scope the command needs: each missing one by its alternatives, the token's scopes in
    /// order, and the scopes to add.
    GhScopesMissing { login: String, missing: Vec<Vec<String>>, scopes: Vec<String>, add: Vec<String> },
    GhSignedIn { login: String, scopes: Vec<String> },
    /// An item of the tracker that cannot be checked: `gh` is not signed in.
    UncheckedSignedOut,
    RepoMissing { repo: String },
    RepoFailed { repo: String, said: GhSaid },
    RepoPublic { repo: String },
    RepoPrivate { repo: String },
    ProjectFailed { title: String, owner: String, said: GhSaid },
    /// An item of the Project that cannot be checked: the Project did not read.
    UncheckedProjectUnread,
    ProjectMissing { title: String, owner: String },
    OwnerUnknown { owner: String },
    StatusNeedsProject,
    /// The project field is born with the first project: a single-select field needs an option.
    ProjectFieldWithFirstProject { field: String },
    ProjectsAmbiguous { title: String, numbers: Vec<u64> },
    /// An item of the Project that cannot be checked: more than one Project has the title.
    UncheckedAmbiguous,
    ProjectFound { number: u64, id: String, url: String },
    StatusFieldMissing,
    StatusFieldKind { kind: String },
    /// Options of the status field missing or named otherwise: those missing, those found, those beyond the format.
    StatusOptionsMissing { missing: Vec<String>, found: Vec<String>, extra: Vec<String> },
    StatusPresent { field_id: String, options: usize, extra: Vec<String> },
    ProjectFieldKind { field: String, kind: String },
    ProjectFieldPresent { field_id: String, options: usize },
    /// The project field would be created with an option for each project the repo's labels name.
    ProjectFieldToCreate { field: String, projects: Vec<String> },
    /// A label that cannot be checked: the repo did not read.
    UncheckedRepoUnread,
    LabelNeedsRepo,
    LabelsFailed { repo: String, said: GhSaid },
    LabelPresent { description: String },
    LabelMissing { description: String, color: String },
    /// No skill to compare: none in the folder, or no folder to read them from.
    NoSkills { folder: Option<PathBuf> },
    SkillCopiesSame,
    SkillCopies { name: String, claude: SkillCopy, agents: SkillCopy, install: String },
    ProjectViews,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub name: String,
    pub scope: Scope,
    pub outcome: Outcome,
    pub action: Action,
    pub detail: Detail,
}

fn item(name: impl Into<String>, scope: Scope, outcome: Outcome, action: Action, detail: Detail) -> Item {
    Item { name: name.into(), scope, outcome, action, detail }
}

/// What the plan looks at: the tracker, and the skills of this machine.
#[derive(Debug, Clone)]
pub struct PlanOptions {
    /// Without it: the user `gh` is signed in as.
    pub owner: Option<String>,
    pub repo: String,
    pub project_title: String,
    /// The command writes to the tracker (`apply`), and needs the scope to write the Projects.
    pub writes: bool,
    /// The folder of the skills to compare with the agents' copies; `None` when there is none to read.
    pub skills: Option<PathBuf>,
    /// The home that holds the agents' copies of the skills.
    pub home: PathBuf,
}

/// The ids the plan found, as the items that find them fill them in.
#[derive(Debug, Clone, Default)]
struct Found {
    repo: Option<(String, String)>,
    repo_node_id: Option<String>,
    project: Option<(u64, String, String)>,
    status: Option<(String, Vec<(String, String)>)>,
    project_field: Option<(String, Vec<(String, String)>)>,
}

/// The plan: the items in the prototype's order, and the ids they found.
#[derive(Debug, Clone)]
pub struct Plan {
    /// `None` when none was given and `gh` is not signed in.
    pub owner: Option<String>,
    pub repo: String,
    pub project_title: String,
    pub items: Vec<Item>,
    found: Found,
}

impl Plan {
    /// Every item of the environment and of the tracker is present: the local configuration can be derived.
    pub fn ready(&self) -> bool {
        self.items.iter().filter(|item| matches!(item.scope, Scope::Environment | Scope::Tracker)).all(|item| item.outcome == Outcome::Present)
    }

    /// The local configuration, when the plan is ready.
    pub fn local_config(&self) -> Option<LocalConfig> {
        if !self.ready() {
            return None;
        }
        let found = &self.found;
        let (owner, repo) = found.repo.clone()?;
        let (project_number, project_id, project_url) = found.project.clone()?;
        let (status_field_id, status_options) = found.status.clone()?;
        Some(LocalConfig { owner, repo, project_number, project_id, project_url, status_field_id, status_options, project_field: found.project_field.clone() })
    }
}

/// The plan, as the prototype computes it: `gh`, then the repo, the Project with its status and project fields, the
/// labels, the skills and the steps by hand. An error is a plan that cannot be computed: `gh` cannot run, or an
/// answer does not read where the prototype stops on it.
pub fn plan(options: &PlanOptions, reads: &mut dyn InitReads) -> Result<Plan, InitError> {
    let vocabulary = vocabulary();
    let names = &vocabulary.items;
    let labels = base_labels();
    let mut plan = Plan {
        owner: options.owner.clone(),
        repo: options.repo.clone(),
        project_title: options.project_title.clone(),
        items: Vec::new(),
        found: Found::default(),
    };
    if plan.gh(names, options.writes, reads)? {
        let exists = plan.repository(names, reads)?;
        plan.projects(names, reads)?;
        plan.labels(names, &labels, exists, reads)?;
    } else {
        for name in [&names.repo, &names.project, &names.status, &names.project_field] {
            plan.items.push(item(name, Scope::Tracker, Outcome::Error, Action::Manual, Detail::UncheckedSignedOut));
        }
        for label in &labels {
            plan.items.push(item(label_item(names, label), Scope::Tracker, Outcome::Error, Action::Manual, Detail::UncheckedSignedOut));
        }
    }
    plan.items.extend(skill_items(names, &vocabulary.skills.install, options.skills.as_deref(), &options.home)?);
    let views = format!("{}{}", names.manual_prefix, vocabulary.manual.views);
    plan.items.push(item(views, Scope::Manual, Outcome::Missing, Action::Manual, Detail::ProjectViews));
    Ok(plan)
}

fn label_item(names: &ItemNames, label: &BaseLabel) -> String {
    format!("{}{}", names.label_prefix, label.name)
}

impl Plan {
    fn owner(&self) -> String {
        self.owner.clone().unwrap_or_default()
    }

    fn slug(&self) -> String {
        format!("{}/{}", self.owner(), self.repo)
    }

    /// The item of `gh`, first; `true` when it is signed in, scopes or not. Without an owner it takes the user's.
    fn gh(&mut self, names: &ItemNames, writes: bool, reads: &mut dyn InitReads) -> Result<bool, InitError> {
        let needed = if writes { WRITE_SCOPES } else { READ_SCOPES };
        let gh = |outcome, action, detail| item(&names.gh, Scope::Environment, outcome, action, detail);
        let user = match reads.user() {
            Ok(user) => user,
            Err(InitError::Unavailable(cause)) => return Err(InitError::Unavailable(cause)),
            Err(InitError::Gh { said, .. }) => {
                let scopes = needed.iter().map(|alternatives| last(alternatives)).collect();
                self.items.push(gh(Outcome::Error, Action::Manual, Detail::GhSignedOut { said, scopes }));
                return Ok(false);
            }
            Err(InitError::Unreadable(error)) => {
                self.items.push(gh(Outcome::Error, Action::Manual, Detail::GhUnreadable { error }));
                return Ok(false);
            }
        };
        let login = user.login;
        if self.owner.is_none() {
            self.owner = Some(login.clone());
        }
        let Some(scopes) = user.scopes else {
            self.items.push(gh(Outcome::Present, Action::None, Detail::GhScopesHidden { login }));
            return Ok(true);
        };
        let missing: Vec<&[&str]> = needed.into_iter().filter(|alternatives| !alternatives.iter().any(|scope| scopes.contains(*scope))).collect();
        let sorted: Vec<String> = scopes.into_iter().collect();
        if missing.is_empty() {
            self.items.push(gh(Outcome::Present, Action::None, Detail::GhSignedIn { login, scopes: sorted }));
        } else {
            let add = missing.iter().map(|alternatives| last(alternatives)).collect();
            let missing = missing.iter().map(|alternatives| alternatives.iter().map(|scope| (*scope).to_owned()).collect()).collect();
            self.items.push(gh(Outcome::Error, Action::Manual, Detail::GhScopesMissing { login, missing, scopes: sorted, add }));
        }
        Ok(true)
    }

    /// The item of the repo: it must exist and be private. `Some(exists)`, or `None` when it did not read.
    fn repository(&mut self, names: &ItemNames, reads: &mut dyn InitReads) -> Result<Option<bool>, InitError> {
        let repo = |outcome, action, detail| item(&names.repo, Scope::Tracker, outcome, action, detail);
        let slug = self.slug();
        let facts = match reads.repository(&self.owner(), &self.repo) {
            Ok(facts) => facts,
            Err(InitError::Gh { not_found: true, .. }) => {
                self.items.push(repo(Outcome::Missing, Action::Create, Detail::RepoMissing { repo: slug }));
                return Ok(Some(false));
            }
            Err(InitError::Gh { said, .. }) => {
                self.items.push(repo(Outcome::Error, Action::Manual, Detail::RepoFailed { repo: slug, said }));
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        self.found.repo_node_id = facts.node_id.filter(|id| !id.is_empty());
        if !facts.private {
            self.items.push(repo(Outcome::Different, Action::Manual, Detail::RepoPublic { repo: slug }));
            return Ok(Some(true));
        }
        self.found.repo = Some((self.owner(), self.repo.clone()));
        self.items.push(repo(Outcome::Present, Action::None, Detail::RepoPrivate { repo: slug }));
        Ok(Some(true))
    }

    /// The items of the Project, of its status field and of its project field.
    fn projects(&mut self, names: &ItemNames, reads: &mut dyn InitReads) -> Result<(), InitError> {
        let field = project_field_name();
        let (title, owner) = (self.project_title.clone(), self.owner());
        let tracker = |name: &str, outcome, action, detail| item(name, Scope::Tracker, outcome, action, detail);
        let born = || tracker(&names.project_field, Outcome::Present, Action::None, Detail::ProjectFieldWithFirstProject { field: field.clone() });
        let nodes = match reads.projects(&owner) {
            Ok(nodes) => nodes,
            Err(InitError::Gh { said, .. }) => {
                self.items.extend([
                    tracker(&names.project, Outcome::Error, Action::Manual, Detail::ProjectFailed { title, owner, said }),
                    tracker(&names.status, Outcome::Error, Action::Manual, Detail::UncheckedProjectUnread),
                    tracker(&names.project_field, Outcome::Error, Action::Manual, Detail::UncheckedProjectUnread),
                ]);
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        let Some(nodes) = nodes else {
            self.items.extend([
                tracker(&names.project, Outcome::Missing, Action::Create, Detail::OwnerUnknown { owner }),
                tracker(&names.status, Outcome::Missing, Action::Create, Detail::StatusNeedsProject),
                born(),
            ]);
            return Ok(());
        };
        let mut found: Vec<ProjectNode> = nodes.into_iter().filter(|node| node.title == title).collect();
        if found.is_empty() {
            self.items.extend([
                tracker(&names.project, Outcome::Missing, Action::Create, Detail::ProjectMissing { title, owner }),
                tracker(&names.status, Outcome::Missing, Action::Create, Detail::StatusNeedsProject),
                born(),
            ]);
            return Ok(());
        }
        if found.len() > 1 {
            let mut numbers: Vec<u64> = found.iter().map(|node| node.number).collect();
            numbers.sort_unstable();
            self.items.extend([
                tracker(&names.project, Outcome::Different, Action::Manual, Detail::ProjectsAmbiguous { title, numbers }),
                tracker(&names.status, Outcome::Error, Action::Manual, Detail::UncheckedAmbiguous),
                tracker(&names.project_field, Outcome::Error, Action::Manual, Detail::UncheckedAmbiguous),
            ]);
            return Ok(());
        }
        let node = found.remove(0);
        self.found.project = Some((node.number, node.id.clone(), node.url.clone()));
        self.items.push(tracker(&names.project, Outcome::Present, Action::None, Detail::ProjectFound { number: node.number, id: node.id, url: node.url }));
        let status = self.status_item(node.status_field.as_ref())?;
        self.items.push(tracker(&names.status, status.0, status.1, status.2));
        let mirror = node.fields.iter().find(|candidate| candidate.name.as_deref() == Some(field.as_str()));
        let (outcome, action, detail) = self.project_field_item(&field, mirror, reads)?;
        self.items.push(tracker(&names.project_field, outcome, action, detail));
        Ok(())
    }

    /// The status field must be a single select with an option for each status; options beyond those are left alone.
    fn status_item(&mut self, field: Option<&FieldNode>) -> Result<(Outcome, Action, Detail), InitError> {
        let Some(field) = field.filter(|field| field.kind.is_some()) else {
            return Ok((Outcome::Missing, Action::Create, Detail::StatusFieldMissing));
        };
        let Some(options) = &field.options else {
            return Ok((Outcome::Different, Action::Manual, Detail::StatusFieldKind { kind: field.kind.clone().unwrap_or_default() }));
        };
        let by_name = by_name(options);
        let wanted = status_option_names();
        let missing: Vec<String> = wanted.iter().filter(|name| !by_name.iter().any(|(found, _)| found == *name)).cloned().collect();
        let extra: Vec<String> = by_name.iter().map(|(name, _)| name.clone()).filter(|name| !wanted.contains(name)).collect();
        if !missing.is_empty() {
            let found = by_name.into_iter().map(|(name, _)| name).collect();
            return Ok((Outcome::Different, Action::Update, Detail::StatusOptionsMissing { missing, found, extra }));
        }
        let field_id = field.id.clone().ok_or_else(|| InitError::Unreadable("the status field has no id".to_owned()))?;
        let bound = wanted.iter().map(|name| (name.clone(), by_name.iter().find(|(found, _)| found == name).map(|(_, id)| id.clone()).unwrap_or_default())).collect();
        self.found.status = Some((field_id.clone(), bound));
        Ok((Outcome::Present, Action::None, Detail::StatusPresent { field_id, options: wanted.len(), extra }))
    }

    /// The project field (ADR 0021 of the dossier). Without it, it is created only if the repo's labels already name
    /// projects, to give it its options; else it is born with the first project, and the item is present.
    fn project_field_item(&mut self, field: &str, mirror: Option<&FieldNode>, reads: &mut dyn InitReads) -> Result<(Outcome, Action, Detail), InitError> {
        if let Some(mirror) = mirror.filter(|mirror| mirror.kind.is_some()) {
            let Some(options) = &mirror.options else {
                return Ok((Outcome::Different, Action::Manual, Detail::ProjectFieldKind { field: field.to_owned(), kind: mirror.kind.clone().unwrap_or_default() }));
            };
            let field_id = mirror.id.clone().ok_or_else(|| InitError::Unreadable("the project field has no id".to_owned()))?;
            self.found.project_field = Some((field_id.clone(), by_name(options)));
            return Ok((Outcome::Present, Action::None, Detail::ProjectFieldPresent { field_id, options: options.len() }));
        }
        let projects = self.projects_of_labels(reads)?;
        if projects.is_empty() {
            return Ok((Outcome::Present, Action::None, Detail::ProjectFieldWithFirstProject { field: field.to_owned() }));
        }
        Ok((Outcome::Missing, Action::Create, Detail::ProjectFieldToCreate { field: field.to_owned(), projects }))
    }

    /// The projects the repo's labels name, sorted, to give the project field its options; none if the repo did not
    /// read or its labels do not.
    fn projects_of_labels(&mut self, reads: &mut dyn InitReads) -> Result<Vec<String>, InitError> {
        if self.found.repo_node_id.is_none() {
            return Ok(Vec::new());
        }
        let names = match reads.label_names(&self.owner(), &self.repo) {
            Ok(names) => names,
            Err(InitError::Gh { .. }) => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let prefix = crate::v1::project_label_prefix();
        let mut projects: Vec<String> = names.iter().filter_map(|name| name.strip_prefix(prefix.as_str())).filter(|project| !project.is_empty()).map(str::to_owned).collect();
        projects.sort();
        Ok(projects)
    }

    /// The labels of the format, each present or to create.
    fn labels(&mut self, names: &ItemNames, labels: &[BaseLabel], exists: Option<bool>, reads: &mut dyn InitReads) -> Result<(), InitError> {
        let each = |plan: &mut Plan, outcome, action, detail: &dyn Fn(&BaseLabel) -> Detail| {
            for label in labels {
                plan.items.push(item(label_item(names, label), Scope::Tracker, outcome, action, detail(label)));
            }
        };
        match exists {
            None => each(self, Outcome::Error, Action::Manual, &|_| Detail::UncheckedRepoUnread),
            Some(false) => each(self, Outcome::Missing, Action::Create, &|_| Detail::LabelNeedsRepo),
            Some(true) => match reads.label_names(&self.owner(), &self.repo) {
                Ok(present) => {
                    for label in labels {
                        let (outcome, action, detail) = if present.contains(&label.name) {
                            (Outcome::Present, Action::None, Detail::LabelPresent { description: label.description.clone() })
                        } else {
                            (Outcome::Missing, Action::Create, Detail::LabelMissing { description: label.description.clone(), color: label.color.clone() })
                        };
                        self.items.push(item(label_item(names, label), Scope::Tracker, outcome, action, detail));
                    }
                }
                Err(InitError::Gh { said, .. }) => {
                    let repo = self.slug();
                    each(self, Outcome::Error, Action::Manual, &|_| Detail::LabelsFailed { repo: repo.clone(), said: said.clone() });
                }
                Err(error) => return Err(error),
            },
        }
        Ok(())
    }
}

fn last(alternatives: &[&str]) -> String {
    alternatives.last().map(|scope| (*scope).to_owned()).unwrap_or_default()
}

/// The options by name, as a dict built from them: each name at its first place, with the id of its last option.
fn by_name(options: &[FieldOption]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for option in options {
        match out.iter_mut().find(|(name, _)| *name == option.name) {
            Some((_, id)) => id.clone_from(&option.id),
            None => out.push((option.name.clone(), option.id.clone())),
        }
    }
    out
}

/// An item for each skill of the folder, one with `SKILL.md`, in the order of their names: present when both agents'
/// copies are the same as the folder, else to install.
fn skill_items(names: &ItemNames, install: &str, source: Option<&Path>, home: &Path) -> Result<Vec<Item>, InitError> {
    let skills = match source {
        Some(folder) if folder.is_dir() => skills_in(folder)?,
        _ => Vec::new(),
    };
    if skills.is_empty() {
        return Ok(vec![item(&names.skills, Scope::Local, Outcome::Error, Action::Manual, Detail::NoSkills { folder: source.map(Path::to_path_buf) })]);
    }
    let mut items = Vec::new();
    for skill in skills {
        let name = skill.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let source = fingerprint(&skill)?;
        let mut copies = Vec::new();
        for (_, folder) in SKILL_HOMES {
            let copy = home.join(folder).join("skills").join(&name);
            copies.push(if !copy.is_dir() {
                SkillCopy::Missing
            } else if fingerprint(&copy)? == source {
                SkillCopy::Same
            } else {
                SkillCopy::Different
            });
        }
        let item_name = format!("{}{name}", names.skill_prefix);
        if copies.iter().all(|copy| *copy == SkillCopy::Same) {
            items.push(item(item_name, Scope::Local, Outcome::Present, Action::None, Detail::SkillCopiesSame));
            continue;
        }
        let outcome = if copies.iter().all(|copy| *copy == SkillCopy::Missing) { Outcome::Missing } else { Outcome::Different };
        let detail = Detail::SkillCopies { name, claude: copies[0], agents: copies[1], install: install.to_owned() };
        items.push(item(item_name, Scope::Local, outcome, Action::Install, detail));
    }
    Ok(items)
}

/// The short names of the agents whose copies of the skills the plan compares, in order.
#[must_use]
pub fn skill_homes() -> [&'static str; 2] {
    SKILL_HOMES.map(|(name, _)| name)
}

/// The skills of `folder`: its entries that hold a `SKILL.md`, by name, as Python sorts paths (without case on
/// Windows).
fn skills_in(folder: &Path) -> Result<Vec<PathBuf>, InitError> {
    let entries = std::fs::read_dir(folder).map_err(|error| unreadable(folder, &error))?;
    let mut skills = Vec::new();
    for entry in entries {
        let path = entry.map_err(|error| unreadable(folder, &error))?.path();
        if path.join("SKILL.md").exists() {
            skills.push(path);
        }
    }
    let key = |path: &PathBuf| {
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        if cfg!(windows) { name.to_lowercase() } else { name }
    };
    skills.sort_by_key(key);
    Ok(skills)
}

/// Every file under `folder`, by its path relative to it with `/`, and its bytes; Python's caches do not count.
fn fingerprint(folder: &Path) -> Result<BTreeMap<String, Vec<u8>>, InitError> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) -> Result<(), InitError> {
        for entry in std::fs::read_dir(dir).map_err(|error| unreadable(dir, &error))? {
            let entry = entry.map_err(|error| unreadable(dir, &error))?;
            let path = entry.path();
            if entry.file_type().map_err(|error| unreadable(&path, &error))?.is_dir() {
                walk(root, &path, out)?;
            } else if path.is_file() {
                let cache = path.components().any(|part| part.as_os_str() == "__pycache__");
                if cache || path.extension().is_some_and(|extension| extension == "pyc") {
                    continue;
                }
                let relative: Vec<String> = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy().into_owned())
                    .collect();
                out.insert(relative.join("/"), std::fs::read(&path).map_err(|error| unreadable(&path, &error))?);
            }
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    walk(folder, folder, &mut out)?;
    Ok(out)
}

fn unreadable(path: &Path, error: &std::io::Error) -> InitError {
    InitError::Unreadable(format!("{}: {error}", path.display()))
}

/// The local configuration a ready plan derives: the keys of format v1 and, when the Project has it, the project
/// field with its options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalConfig {
    pub owner: String,
    pub repo: String,
    pub project_number: u64,
    pub project_id: String,
    pub project_url: String,
    pub status_field_id: String,
    /// In the order of the statuses.
    pub status_options: Vec<(String, String)>,
    /// The project field's id and its options by name, in their order.
    pub project_field: Option<(String, Vec<(String, String)>)>,
}

/// A value of the local file, in the order it is written.
enum Written {
    Text(String),
    Number(u64),
    Map(Vec<(String, String)>),
}

impl LocalConfig {
    /// The keys a derived file is compared on, and their values; the project field's two only when the Project has it.
    fn compared(&self) -> Vec<(String, Written)> {
        let keys = vocabulary().local_file;
        let mut out = vec![
            ("owner".to_owned(), Written::Text(self.owner.clone())),
            ("repo".to_owned(), Written::Text(self.repo.clone())),
            ("project_number".to_owned(), Written::Number(self.project_number)),
            ("project_id".to_owned(), Written::Text(self.project_id.clone())),
            ("project_url".to_owned(), Written::Text(self.project_url.clone())),
            ("status_field_id".to_owned(), Written::Text(self.status_field_id.clone())),
            ("status_options".to_owned(), Written::Map(self.status_options.clone())),
        ];
        if let Some((id, options)) = &self.project_field {
            out.push((keys.project_field_key, Written::Text(id.clone())));
            out.push((keys.project_options_key, Written::Map(options.clone())));
        }
        out
    }

    /// The file as the prototype writes it: `json.dumps(indent=2, ensure_ascii=False)` of the note, the keys and the
    /// writer, in that order, and a line break.
    pub fn file_text(&self) -> String {
        let keys = vocabulary().local_file;
        let mut entries = vec![(keys.note_key, Written::Text(keys.note))];
        entries.extend(self.compared());
        entries.push((keys.generator_key, Written::Text(keys.generator)));
        let mut out = String::from("{\n");
        for (at, (key, value)) in entries.iter().enumerate() {
            out.push_str(&format!("  {}: ", quoted(key)));
            match value {
                Written::Text(text) => out.push_str(&quoted(text)),
                Written::Number(number) => out.push_str(&number.to_string()),
                Written::Map(pairs) if pairs.is_empty() => out.push_str("{}"),
                Written::Map(pairs) => {
                    out.push_str("{\n");
                    let lines: Vec<String> = pairs.iter().map(|(name, id)| format!("    {}: {}", quoted(name), quoted(id))).collect();
                    out.push_str(&lines.join(",\n"));
                    out.push_str("\n  }");
                }
            }
            out.push_str(if at + 1 < entries.len() { ",\n" } else { "\n" });
        }
        out.push_str("}\n");
        out
    }

    /// Whether `existing`, a file read as JSON, says the same: an object whose every compared key is equal as Python
    /// compares values, a key that is missing equal to one that is null.
    pub fn same_as(&self, existing: &Value) -> bool {
        let Some(object) = existing.as_object() else { return false };
        let keys = vocabulary().local_file;
        let mine: BTreeMap<String, Value> = self
            .compared()
            .into_iter()
            .map(|(key, value)| {
                let value = match value {
                    Written::Text(text) => Value::String(text),
                    Written::Number(number) => Value::from(number),
                    Written::Map(pairs) => Value::Object(pairs.into_iter().map(|(name, id)| (name, Value::String(id))).collect()),
                };
                (key, value)
            })
            .collect();
        let all = ["owner", "repo", "project_number", "project_id", "project_url", "status_field_id", "status_options"]
            .map(str::to_owned)
            .into_iter()
            .chain([keys.project_field_key, keys.project_options_key]);
        all.into_iter().all(|key| {
            let theirs = object.get(&key).filter(|value| !value.is_null());
            match (mine.get(&key), theirs) {
                (None, None) => true,
                (Some(mine), Some(theirs)) => python_equal(mine, theirs),
                _ => false,
            }
        })
    }
}

/// A JSON string as Python's `json.dumps(ensure_ascii=False)` writes it: the same escapes as `serde_json`'s.
fn quoted(text: &str) -> String {
    serde_json::to_string(text).expect("a string serializes")
}

/// `a == b` on two values Python read from JSON: a number equals a number of the same value, `true` equals 1.
fn python_equal(a: &Value, b: &Value) -> bool {
    /// A number as Python holds it: an exact integer or a float; `true` and `false` are 1 and 0.
    enum Number {
        Int(i128),
        Float(f64),
    }
    fn number(value: &Value) -> Option<Number> {
        match value {
            Value::Bool(flag) => Some(Number::Int(i128::from(*flag))),
            Value::Number(number) => number
                .as_i64()
                .map(i128::from)
                .or_else(|| number.as_u64().map(i128::from))
                .map(Number::Int)
                .or_else(|| number.as_f64().map(Number::Float)),
            _ => None,
        }
    }
    #[allow(clippy::cast_precision_loss)]
    fn float(number: &Number) -> f64 {
        match number {
            Number::Int(int) => *int as f64,
            Number::Float(float) => *float,
        }
    }
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::String(a), Value::String(b)) => a == b,
        (Value::Array(a), Value::Array(b)) => a.len() == b.len() && a.iter().zip(b).all(|(a, b)| python_equal(a, b)),
        (Value::Object(a), Value::Object(b)) => a.len() == b.len() && a.iter().all(|(key, value)| b.get(key).is_some_and(|other| python_equal(value, other))),
        _ => match (number(a), number(b)) {
            (Some(Number::Int(a)), Some(Number::Int(b))) => a == b,
            (Some(a), Some(b)) => float(&a) == float(&b),
            _ => false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tracker answered from memory, for the order of the reads and the outcomes the answers give.
    #[derive(Default)]
    struct Memory {
        user: Option<Result<GhUser, InitError>>,
        repo: Option<Result<RepoFacts, InitError>>,
        projects: Option<Result<Option<Vec<ProjectNode>>, InitError>>,
        labels: Option<Result<BTreeSet<String>, InitError>>,
        calls: Vec<String>,
    }

    impl InitReads for Memory {
        fn user(&mut self) -> Result<GhUser, InitError> {
            self.calls.push("user".to_owned());
            self.user.clone().expect("a user")
        }
        fn repository(&mut self, owner: &str, repo: &str) -> Result<RepoFacts, InitError> {
            self.calls.push(format!("repo {owner}/{repo}"));
            self.repo.clone().expect("a repo")
        }
        fn projects(&mut self, owner: &str) -> Result<Option<Vec<ProjectNode>>, InitError> {
            self.calls.push(format!("projects {owner}"));
            self.projects.clone().expect("the projects")
        }
        fn label_names(&mut self, owner: &str, repo: &str) -> Result<BTreeSet<String>, InitError> {
            self.calls.push(format!("labels {owner}/{repo}"));
            self.labels.clone().expect("the labels")
        }
        fn calls(&self) -> Vec<String> {
            self.calls.clone()
        }
    }

    fn select(id: &str, name: &str, options: &[String]) -> FieldNode {
        FieldNode {
            kind: Some("ProjectV2SingleSelectField".to_owned()),
            id: Some(id.to_owned()),
            name: Some(name.to_owned()),
            options: Some(options.iter().enumerate().map(|(at, option)| FieldOption { id: format!("o{at}"), name: option.clone() }).collect()),
        }
    }

    /// Everything there: gh with both scopes, a private repo, one Project with the title, its status field and its
    /// project field, the five labels.
    fn ready() -> Memory {
        let status = select("S", "Status", &status_option_names());
        let mirror = select("P", &project_field_name(), &["alpha".to_owned()]);
        let node = ProjectNode { number: 2, id: "PVT".into(), title: "T".into(), url: "u".into(), status_field: Some(status.clone()), fields: vec![status, mirror] };
        Memory {
            user: Some(Ok(GhUser { login: "me".into(), scopes: Some(["project".to_owned(), "repo".to_owned()].into()) })),
            repo: Some(Ok(RepoFacts { private: true, node_id: Some("R".into()), owner_node_id: Some("U".into()) })),
            projects: Some(Ok(Some(vec![node]))),
            labels: Some(Ok(base_labels().into_iter().map(|label| label.name).collect())),
            calls: Vec::new(),
        }
    }

    fn options(home: &Path) -> PlanOptions {
        PlanOptions { owner: None, repo: "r".into(), project_title: "T".into(), writes: false, skills: None, home: home.to_path_buf() }
    }

    fn outcome(plan: &Plan, name: &str) -> (Outcome, Action) {
        let item = plan.items.iter().find(|item| item.name == name).unwrap_or_else(|| panic!("no item {name}"));
        (item.outcome, item.action)
    }

    #[test]
    fn a_tracker_with_everything_is_ready_and_derives_its_config() {
        let names = vocabulary().items;
        let mut reads = ready();
        let plan = plan(&options(Path::new("no-home")), &mut reads).unwrap();
        assert_eq!(plan.owner.as_deref(), Some("me"), "the owner is the user's");
        assert!(plan.ready());
        assert_eq!(reads.calls, ["user", "repo me/r", "projects me", "labels me/r"], "one read of the labels: the project field is there");
        let config = plan.local_config().unwrap();
        assert_eq!((config.project_number, config.status_field_id.as_str()), (2, "S"));
        assert_eq!(config.status_options.iter().map(|(name, _)| name.clone()).collect::<Vec<_>>(), status_option_names());
        assert_eq!(config.project_field, Some(("P".to_owned(), vec![("alpha".to_owned(), "o0".to_owned())])));
        assert_eq!(outcome(&plan, &names.skills), (Outcome::Error, Action::Manual), "no folder of skills");
        assert_eq!(plan.items.last().unwrap().detail, Detail::ProjectViews);
    }

    #[test]
    fn without_the_project_field_the_labels_are_read_twice_and_name_its_options() {
        let names = vocabulary().items;
        let mut reads = ready();
        let Some(Ok(Some(nodes))) = &mut reads.projects else { unreachable!() };
        nodes[0].fields.pop();
        let prefix = crate::v1::project_label_prefix();
        if let Some(Ok(labels)) = &mut reads.labels {
            labels.extend([format!("{prefix}beta"), format!("{prefix}alpha"), prefix.clone()]);
        }
        let plan = plan(&options(Path::new("no-home")), &mut reads).unwrap();
        assert_eq!(reads.calls, ["user", "repo me/r", "projects me", "labels me/r", "labels me/r"]);
        let field = plan.items.iter().find(|item| item.name == names.project_field).unwrap();
        assert_eq!(field.detail, Detail::ProjectFieldToCreate { field: project_field_name(), projects: vec!["alpha".into(), "beta".into()] });
        assert!(!plan.ready() && plan.local_config().is_none());
    }

    #[test]
    fn a_signed_out_gh_leaves_the_tracker_unchecked_and_reads_nothing_else() {
        let mut reads = Memory { user: Some(Err(InitError::Gh { said: GhSaid::Exit(4), not_found: false })), ..Memory::default() };
        let plan = plan(&options(Path::new("no-home")), &mut reads).unwrap();
        assert_eq!(reads.calls, ["user"]);
        assert_eq!(plan.owner, None);
        let tracker: Vec<&Item> = plan.items.iter().filter(|item| item.scope == Scope::Tracker).collect();
        assert_eq!(tracker.len(), 9);
        assert!(tracker.iter().all(|item| item.detail == Detail::UncheckedSignedOut));
        assert_eq!(plan.items[0].detail, Detail::GhSignedOut { said: GhSaid::Exit(4), scopes: vec!["repo".into(), "read:project".into()] });
    }

    #[test]
    fn reading_needs_read_project_and_writing_needs_project() {
        let mut reads = ready();
        reads.user = Some(Ok(GhUser { login: "me".into(), scopes: Some(["read:project".to_owned(), "repo".to_owned()].into()) }));
        assert!(plan(&options(Path::new("no-home")), &mut reads).unwrap().ready());
        let mut writing = options(Path::new("no-home"));
        writing.writes = true;
        let mut reads = ready();
        reads.user = Some(Ok(GhUser { login: "me".into(), scopes: Some(["read:project".to_owned(), "repo".to_owned()].into()) }));
        let plan = plan(&writing, &mut reads).unwrap();
        let expected = Detail::GhScopesMissing { login: "me".into(), missing: vec![vec!["project".into()]], scopes: vec!["read:project".into(), "repo".into()], add: vec!["project".into()] };
        assert_eq!(plan.items[0].detail, expected);
    }

    #[test]
    fn a_gh_that_cannot_run_is_no_plan() {
        let mut reads = Memory { user: Some(Err(InitError::Unavailable("no gh".into()))), ..Memory::default() };
        assert_eq!(plan(&options(Path::new("no-home")), &mut reads).unwrap_err(), InitError::Unavailable("no gh".into()));
    }

    #[test]
    fn the_file_is_written_as_python_writes_it() {
        let keys = vocabulary().local_file;
        let mut config = LocalConfig {
            owner: "o".into(),
            repo: "r".into(),
            project_number: 7,
            project_id: "P".into(),
            project_url: "https://x.invalid/\"q\"\\".into(),
            status_field_id: "S".into(),
            status_options: vec![("à".into(), "1".into()), ("\u{1}".into(), "2".into())],
            project_field: Some(("F".into(), Vec::new())),
        };
        let text = config.file_text();
        let expected_start = format!("{{\n  {}: {},\n  \"owner\": \"o\",\n  \"repo\": \"r\",\n  \"project_number\": 7,\n", quoted(&keys.note_key), quoted(&keys.note));
        assert!(text.starts_with(&expected_start), "{text}");
        assert!(text.contains("  \"project_url\": \"https://x.invalid/\\\"q\\\"\\\\\",\n"), "{text}");
        assert!(text.contains("  \"status_options\": {\n    \"à\": \"1\",\n    \"\\u0001\": \"2\"\n  },\n"), "{text}");
        assert!(text.contains(&format!("  {}: {{}},\n", quoted(&keys.project_options_key))), "an empty map: {text}");
        assert!(text.ends_with(&format!("  {}: {}\n}}\n", quoted(&keys.generator_key), quoted(&keys.generator))), "{text}");
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert!(config.same_as(&parsed));
        config.project_field = None;
        assert!(!config.file_text().contains(&keys.project_field_key));
        assert!(!config.same_as(&parsed), "the project field is compared too");
    }

    #[test]
    fn a_file_is_the_same_as_python_compares_it() {
        let config = LocalConfig {
            owner: "o".into(),
            repo: "r".into(),
            project_number: 2,
            project_id: "P".into(),
            project_url: "u".into(),
            status_field_id: "S".into(),
            status_options: vec![("a".into(), "1".into()), ("b".into(), "2".into())],
            project_field: None,
        };
        let mut value: Value = serde_json::from_str(&config.file_text()).unwrap();
        value["project_number"] = serde_json::json!(2.0);
        value["status_options"] = serde_json::json!({"b": "2", "a": "1"});
        value[vocabulary().local_file.note_key] = serde_json::json!("by hand");
        value[vocabulary().local_file.project_field_key] = Value::Null;
        assert!(config.same_as(&value), "a float of the same value, another order, another note, a null for a missing key");
        value["project_number"] = serde_json::json!("2");
        assert!(!config.same_as(&value));
        assert!(!config.same_as(&serde_json::json!([1])));
    }
}
