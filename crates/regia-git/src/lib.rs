//! The git port (ADR 0001 §4). It is the user's own `git`, started through regia-exec, so credential
//! helpers, hooks, signing and worktrees are the user's. Every call is a typed argv of this crate run
//! by the pinned program in the dossier's folder, with regia-exec's environment for git: no prompt, no
//! optional lock, literal pathspecs and no fsmonitor (threat model §3.6, rules 2, 4 and 8).
//!
//! [`sync`] is the prototype's `dossier_sync`, the commit and push that `checkpoint` and `step finish`
//! make of the dossier's named files:
//! - only the named files go into the commit, never what someone else staged;
//! - a fast-forward comes before the commit, and a divergence keeps the commit local;
//! - commits made before and never pushed are pushed when there is nothing new (#159 of the dossier);
//! - the push goes to the branch's upstream only, never with force and never to another branch.
//!
//! A failure of git is an outcome with its reason, never an error.
//!
//! [`link`] and [`first_push`] are the prototype's `_collega_dossier` and `_primo_push`: a new project's dossier
//! linked to its repo, and its scaffold pushed when the folder was made a repo here.

mod dossier;
mod git;
mod link;

pub use dossier::{Checkpoint, DossierSync, Outcome, named_files, named_paths, sync, sync_with};
pub use git::{Git, GitDate, Run, TIMEOUT};
pub use link::{FirstPush, LinkError, Linked, PushStep, first_push, is_repo, link, other_origin};
