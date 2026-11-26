# The precheck's oracle

What the Python prototype's precheck printed on synthetic cases: its outcome line, the last line of its stdout, which the controller reads, with the exit code (#465). The cases transcribe the prototype's `tools/tests/test_precheck.py` and the precheck's side of #406 and of the comment signatures, on events and decision; the budget, the stall, the backlog and the caps of the Proposals are #466's.

Everything here is synthetic: the projects are `p`, `a` and `b`, the tracker is `example/tracker` of `fixtures/brief/tracker.json`, and every URL is under `https://tracker.invalid/`.

## Layout

The cases are under `events/`, not `cases/`: the harness reads every family's `cases/` as cases of the CLI, and these drive another command. `events/<case>/` holds two files:

- `case.json`: the issues, compactly, with the defaults of the prototype's test helper (a task of `p`, approved and ready; an Objective whose registry names the folder `{dossier}/<project>`, wip 2, priority 3); the comments, by issue, some signed by the product's own signer; and the runs, each with its arguments, the events cursor of each project, the locks, the leases, the reports on disk with their dates and, for a worker's report, its text, the real repos of its dossiers (#743), a stand-in where git should be, and the failures to build;
- `python.it.json`: for each run, the exit code and the line, with `t` as `{precheck-t}` and the sentence of an error, or the reason of the policy's fallback (`politica_motivo`, which names the file it could not read), as `{detail}`.

A run, or a tick of a run, can also have `faults` (#600), set up on the state database before the tick and taken away after it, the same for both sides: `database-held`, another process's write transaction held for the whole tick (`store-probe hold-tx`); `cursor-write`, every write of a cursor aborted by a trigger; `budget-cache`, the read of the budget's cache failing, its last attempt past the probes' interval and the claim of a new probe aborted by a trigger, so no side ever asks a provider. The harness checks that each trigger aborts its write before any side runs. A run can have no policy file (`no_policy_file`), a lock `renewed` at the tick's instant, and a 5-hour window with neither a reset nor the flag of a window not open (`no_reset`).

## Replaying a run

`crates/regia-conformance/tests/precheck.rs` builds, from `case.json`, what both sides read:

1. the answers of the fake `gh`: the list of the open issues in one page, and the comments of every issue; the read of the recent comments (#393 of the dossier) has every issue with comments, an issue marked `closed` too, its last twenty, which the product asks as the checks do, every label at once, and the prototype by the project's label;
2. the state folder: the synthetic `tracker.json`, and the rows of `regia.db` (the cursor's value with its date, a lock or a lease that expires in 2999 or in 2000, a row that is not a document);
3. a dossier folder, with each report at `<project>/.regia/report/<name>` and its date, to the 100 nanoseconds, and the run's repos built in it first: a project's dossier at `<project>`, a product's clone beside it or where its site says, each on `main` with one commit and its bare remote under `remotes/`, then its branches, pushed or not, and its commits not pushed. A dossier the run does not build is the folder of its reports, or no folder at all: in the old cases `cartella-mancante` or `non-e-repo`.

The product runs its `precheck` binary there, at `2026-01-10T09:00:00Z`. The prototype runs its precheck in process, with its own tracker reader behind the fake `gh`, its runtime on the state folder and its reports on disk; only the usage probe answers a known budget of 10 %. The dossier's warnings are its real `_dossier_da_regia`'s, with the git of its `PATH`; the product's are `regia-git`'s, with the git it finds; a run's stand-in for git, if any, goes to both. The workers of the tasks in progress (#210 and #641 of the dossier) come from each side's own adapter of Orca behind the fake `orca` of the run's folder, which answers its four reads from the run's `workers`, with the thresholds of the run's `policies/worker.yaml` (the defaults without one) and the run's instant: the prototype's own reader would ask the machine's clock. The product finds the fake by `REGIA_ORCA`, the prototype's adapter where it looks the program up; neither ever starts the machine's Orca.

## The comparison

The line must be the prototype's, key order included, each project's `dossier` too (#743): nothing is taken out of either side. Every reason is ported. The detector's rows must be the prototype's too, the digests of the series byte for byte.

## Recording again

Locally, with a copy of the dossier's `tools/` and `policies/` at the commit `RECORDED.json` cites:

```
REGIA_HOME=<copy> RECORD_PRECHECK_CASES=1 cargo test -p regia-conformance --test precheck the_prototype_prints_the_recorded_lines -- --ignored
```

Without the variable the same test checks the recording against the prototype.
