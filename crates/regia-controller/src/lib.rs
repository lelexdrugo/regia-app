//! The controller engine (ADR 0001, section 7). This slice holds its precheck: from the tracker, the
//! state and the reports on disk, what happened in each project since its last step, and whether a step
//! has work. The engine reads through [`precheck::Sources`], so the prototype's cases run on fixed answers;
//! the words of its outcome line are data of format v1, in `formats/precheck_v1.toml`.

pub mod precheck;
