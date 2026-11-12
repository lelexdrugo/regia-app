//! Domain model and rules of the product. Dependencies point inwards: adapters (GitHub, git, agent
//! services) implement the ports defined here, and this crate never starts a process or opens a
//! socket. See `docs/adr/0001-product-architecture.md`.

pub mod brief;
pub mod comment;
pub mod deadline;
pub mod due;
pub mod graph;
pub mod link;
pub mod markdown;
pub mod model;
pub mod policy;
pub mod pystr;
pub mod reads;
pub mod reference;
pub mod registry;
pub mod text;
pub mod tracker;
pub mod v1;

pub use brief::{Brief, BriefOptions, BriefTask, GraphWarnings, UNSET_STATUS, build_brief};
pub use comment::{Comment, InstantError, comments, parse_instant, signature_role, summary_line, without_signature};
pub use deadline::{Deadline, FallbackReason, JobState, Verdict};
pub use due::{DueMark, IgnoredDue, parse_date};
pub use graph::{GraphOrder, Metrics, graph_order};
pub use link::{LinkRefusal, LinkTarget, check_link};
pub use markdown::{Block, Document, Inline, parse_document};
pub use model::{DueDate, IssueState, Maturity, Status, Task};
pub use policy::{Lever, LeverValue, Local, LocalStatus, Origin, Policy, PolicyError, Room, Values};
pub use reads::{ListOptions, ListedTask, ShownTask, task_list};
pub use reference::{Reference, ReferenceKind, Unresolved, data_root, resolve};
pub use registry::{EntryError, Registry, full_repo, project_entry, registry_with};
pub use text::{Normalized, normalize, normalize_document, normalize_title};
pub use tracker::{RawComment, RawIssue, StatusHistory, Tracker, TrackerError, add_children_from_parents};
pub use v1::{TrackerBinding, TrackerConfigV1, V1Error};
