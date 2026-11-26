# The precheck's oracle

What the Python prototype's precheck printed on synthetic cases: its outcome line, the last line of its stdout, which the controller reads, with the exit code (#465). The cases transcribe the prototype's `tools/tests/test_precheck.py` and the precheck's side of #406 and of the comment signatures, on events and decision; the budget, the stall, the backlog and the caps of the Proposals are #466's.

Everything here is synthetic: the projects are `p`, `a` and `b`, the tracker is `example/tracker` of `fixtures/brief/tracker.json`, and every URL is under `https://tracker.invalid/`.

## Layout

The cases are under `events/`, not `cases/`: the harness reads every family's `cases/` as cases of the CLI, and these drive another command. `events/<case>/` holds two files:

- `case.json`: the issues, compactly, with the defaults of the prototype's test helper (a task of `p`, approved and ready; an Objective whose registry names the folder `{dossier}/<project>`, wip 2, priority 3); the comments, by issue, some signed by the product's own signer; and the runs, each with its arguments, the events cursor of each project, the locks, the leases, the reports on disk with their dates, and the failures to build;
- `python.it.json`: for each run, the exit code and the line, with `t` as `{precheck-t}` and the sentence of an error as `{detail}`.

## Replaying a run

`crates/regia-conformance/tests/precheck.rs` builds, from `case.json`, what both sides read:

1. the answers of the fake `gh`: the list of the open issues in one page, and the comments of every issue; the read of the recent comments (#393 of the dossier) has every issue with comments, an issue marked `closed` too, its last twenty, which the product asks as the checks do, every label at once, and the prototype by the project's label;
2. the state folder: the synthetic `tracker.json`, and the rows of `regia.db` (the cursor's value with its date, a lock or a lease that expires in 2999 or in 2000, a row that is not a document);
3. a dossier folder, with each report at `<project>/.regia/report/<name>` and its date, to the 100 nanoseconds.

The product runs its `precheck` binary there, at `2026-01-10T09:00:00Z`. The prototype runs its precheck in process, with its own tracker reader behind the fake `gh`, its runtime on the state folder and its reports on disk; only the usage probe answers a known budget of 10 %, and the dossier's warnings and the workers have no data.

## The comparison

The line must be the prototype's, key order included, once these are taken out of the prototype's side only: the payload's `budget`, `politica`, `politica_motivo`, `politica_locale`, and each project's `serie`, `rinviato`, `arretrato`, `arretrato_task`, `proposte`, `dossier`, `silenti`, `silenti_task`. No recorded run ends with a reason of #466, and the product's line must have none of those keys.

## Recording again

Locally, with a copy of the dossier's `tools/` and `policies/` at the commit `RECORDED.json` cites:

```
REGIA_HOME=<copy> RECORD_PRECHECK_CASES=1 cargo test -p regia-conformance --test precheck the_prototype_prints_the_recorded_lines -- --ignored
```

Without the variable the same test checks the recording against the prototype.
