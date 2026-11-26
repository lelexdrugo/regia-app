//! The controller engine (ADR 0001, section 7).
//!
//! - [`precheck`]: from the tracker, the state and the reports on disk, what happened in each project since its
//!   last step, and whether a step has work. It reads through [`precheck::Sources`], so the prototype's cases
//!   run on fixed answers; the words of its outcome line are data of format v1, in `formats/precheck_v1.toml`.
//! - [`engine`]: the tick, [`engine::Controller::tick`], with its clock, precheck, launch surface, lock and
//!   registry injected; the words of its line, of the postponements and of the registry are data of format
//!   v1, in `formats/controller_v1.toml`.
//! - [`resident`]: the tick's host, [`resident::Resident::run`]: the controller's lock, the registry read after
//!   it, the restart, the ticks and the wait between them, the log and the state file.
//! - [`schedule`]: the scheduled operations, once a day at the first tick after their hour, as a hook before
//!   the precheck; the words of their configuration and cursors are data of format v1, in
//!   `formats/operations_v1.toml`.
//! - [`shadow`]: the shadow run's fresh copy of the state folder, so a run that launches nothing never writes
//!   the live files; [`interrupt`]: Ctrl+C for a command line's resident.

pub mod engine;
pub mod interrupt;
pub mod precheck;
pub mod resident;
pub mod schedule;
pub mod shadow;
