//! The prompt of a step the controller launches, and of the step that reports a stall: the prototype's
//! `costruisci_prompt` and `SCODARE_PROMPT`, in English and with the English names of the commands (#467).
//! They are text for agents, so they name the contract's commands, never the prototype's; the exit codes
//! they cite are the same. The attached payload is the precheck's, byte for byte, so the step reads the
//! counts and ids the prototype's step reads.

use regia_identity::{BIN_NAME, PRODUCT_NAME};

use super::words::format as words;
use crate::precheck::Json;
use crate::precheck::v1::format as precheck_words;

/// The prompt for `project`, in `folder` as the registry names it, from its `entry` of the precheck's payload
/// and the whole `payload`, at the precheck's `t`. A terminal state for a stall gets the reporting step's
/// prompt; anything else the orchestrator's step, with the backlog's paragraph when that is the reason.
#[must_use]
pub fn prompt(project: &str, folder: &str, entry: &Json, payload: &Json, precheck_t: Option<&str>) -> String {
    let format = precheck_words();
    let outcome = entry.get(&format.project.outcome).and_then(Json::as_str).unwrap_or_default();
    let reason = entry.get(&format.project.reason).and_then(Json::as_str).unwrap_or_default();
    let t = precheck_t.unwrap_or("-");
    let attached = payload.to_ascii();
    if outcome == format.outcomes.terminal_state.word && reason == format.outcomes.terminal_state.reasons.stall {
        return stall(project, folder, t, &attached);
    }
    let backlog = if reason == format.outcomes.new_activity.reasons.backlog { backlog_paragraph() } else { String::new() };
    format!(
        "You are a step of the {PRODUCT_NAME} orchestrator on the project \"{project}\" (folder {folder}), started by the \
         resident controller: nobody follows you in chat and nobody can answer during the step.\n\n\
         Use the {PRODUCT_NAME} skill and run ONE orchestrator step on this project, by the protocol §1: start with \
         `{BIN_NAME} step start --project {project}` (if it exits 3 another orchestrator is already active: stop and say \
         so, do not replan), then observe, replan, dispatch and record, and close with \
         `{BIN_NAME} step finish --project {project} --holder <your id>`. Then stop: do not start another round.\n\
         A step is a wave: dispatch together every task that can run and is independent, up to the project's wip and \
         the budget (§1.3), and let each worker split its own task among subagents. Do not wait for the workers: their \
         reports will start the next step.\n\
         Workers start by the protocol §1.3: with `orca orchestration worker-start` if Orca is reachable and \
         `run-create` succeeds, otherwise with subagents. The protocol's gates hold as always: what is irreversible or \
         goes outside goes through Decision.\n\n\
         The controller found work ({outcome}/{reason}, precheck of {t}). Counts and ids only: read the facts again \
         from the tracker and from {dossier}/.\n{attached}\n{backlog}",
        dossier = dossier_folder(),
    )
}

/// The step that reports a stall: it takes no lock and replans nothing.
fn stall(project: &str, folder: &str, t: &str, attached: &str) -> String {
    let dossier = &words().dossier;
    format!(
        "You are a step that REPORTS the stall of the project \"{project}\" (folder {folder}), started by the resident \
         controller of the {PRODUCT_NAME}: nobody follows you in chat. Use the {PRODUCT_NAME} skill.\n\n\
         The precheck found that the project does not progress (the same situation for several ticks). This step does \
         NOT start with `{BIN_NAME} step start` and takes NEITHER the lock NOR a lease: the stall may be caused by a \
         lock left held (protocol §1.1, exception). Do only this:\n\
         1. find out why it does not progress (`{BIN_NAME} lock status {project}`, `{BIN_NAME} task list --project \
         {project}`, `{folder}/{state}`);\n\
         2. write the stall memo (model in {memo}) with `{BIN_NAME} comment` on the project's Objective issue, and \
         bring it to Decision with `{BIN_NAME} task status`;\n\
         3. do not replan, do not run anything else, and stop.\n\n\
         Precheck outcome ({t}); counts and ids only, read the facts again from the tracker:\n{attached}\n",
        state = dossier.state_file,
        memo = dossier.memo_model,
    )
}

/// The backlog's paragraph (#211): the step drains up to three items of it.
fn backlog_paragraph() -> String {
    let format = precheck_words();
    format!(
        "\nReason `{reason}` (the backlog, protocol §1.2 points 6 and 8): `{tasks}` lists up to 10 items in Inbox or \
         Approved as drafts, in the order of the graph. Take up to 3 of them and bring them out of the backlog, each into \
         exactly one of the protocol's exits (Proposal, ready, Decision, `{BIN_NAME} task block`, or `{parked}` with the \
         reason in a comment). Leave none as it was.\n",
        reason = format.outcomes.new_activity.reasons.backlog,
        tasks = format.project.backlog_tasks,
        parked = format.backlog.parked,
    )
}

/// The dossier's own folder, as the prototype names it: the folder of its state file.
fn dossier_folder() -> &'static str {
    let state = &words().dossier.state_file;
    state.rsplit_once('/').map_or(state.as_str(), |(folder, _)| folder)
}
