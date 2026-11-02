//! Domain model and rules of the product. Dependencies point inwards: adapters (GitHub, git, agent
//! services) implement the ports defined here, and this crate never starts a process or opens a
//! socket. See `docs/adr/0001-product-architecture.md`.

pub mod brief;
pub mod graph;
pub mod model;
pub mod tracker;
pub mod v1;

pub use brief::{Brief, BriefOptions, BriefTask, GraphWarnings, build_brief};
pub use graph::{GraphOrder, Metrics, graph_order};
pub use model::{IssueState, Maturity, Status, Task};
pub use tracker::{RawIssue, Tracker, TrackerError};
pub use v1::{TrackerBinding, TrackerConfigV1, V1Error};
