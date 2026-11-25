//! The controller engine (ADR 0001, section 7).
//!
//! - [`precheck`]: from the tracker, the state and the reports on disk, what happened in each project since its
//!   last step, and whether a step has work. It reads through [`precheck::Sources`], so the prototype's cases
//!   run on fixed answers; the words of its outcome line are data of format v1, in `formats/precheck_v1.toml`.
//! - [`engine`]: the tick, [`engine::Controller::tick`], with its clock, precheck, launch surface, lock and
//!   registry injected; the words of its line, of the postponements and of the registry are data of format
//!   v1, in `formats/controller_v1.toml`.

pub mod engine;
pub mod precheck;
