# The controller's oracle

What the Python prototype's controller decided, tick by tick, on synthetic cases with a fixed clock and recorded tracker answers (ADR 0001 §7, "Shadow before the gate"). The Rust engine uses these traces as golden. They are recorded locally by a driver in the private dossier, at the commit that `RECORDED.json` cites, because the driver imports the prototype. CI only reads them.

Everything here is synthetic: the projects are `alpha` and `beta`, the tracker is `example/tracker`, and every URL is under `https://tracker.invalid/`. The prompts name the prototype's commands (`regia passo inizio`, "la skill regia"): that word is the prototype's own name (`PROTOTYPE_NAME`), not a project.

## Layout

`cases/<scenario>/` holds four files:

- `case.json`: the start of the clock, the controller's arguments, the usage probe's answer, the reports on disk, and the change the negative control makes;
- `state.json`: the live state folder before the copy, as rows of the `documenti` table and as files;
- `responses.json`: what the tracker answered, server side;
- `python.it.json`: the trace, with the prototype's data in Italian and untranslated.

## Replaying a case

1. **Build the state folder.**
   - Write the files of `state.json`. Then create a `regia.db` with the `documenti` schema and insert each row with `gen` 1 and `doc` as JSON.
   - First replace `{dossier}` with the absolute path, in forward slashes, of a folder that holds one subfolder per project.
2. **Apply the shadow's cleanup to that folder.** Drop the row `(lock, controller)`, which the live controller always holds. Drop the records of the steps registry (cursor `controller:passi`) whose `stato` is `sospesa` or `in-ripresa`.
3. **Place the reports.** Put each entry of `case.json` `reports` at `{dossier}/<project>/.regia/report/<name>`, with that modification time. No case has reports yet.
4. **Serve the tracker.** Answer from `responses.json` the way the fake `gh` answers `fixtures/brief/cases/*/responses.json`, with `{dossier}` replaced there too. There are two additions:
   - `comments`, keyed by issue number, answers `comments(last:100)`. An issue without an entry has no comments.
   - `issue_pages` given as a string means that reading the list fails.
5. **Answer the usage probe** with `case.json` `usage` on every read. It is what `runtime.sonda_cache` returns.
6. **Run the controller** with `case.json` `args`, the prototype's own option names. The clock starts at `start` and only moves when the controller sleeps between ticks (`--intervallo` seconds, in slices).

**The clock rule.** The prototype judges the expiry of locks and leases on the real wall clock, not the injected one. A lock that a case holds therefore expires in 2999 and has no `_m`, so both sides read it as held whatever the day.

## The trace

In `python.it.json`, only the envelope keys are in English. Each entry of `ticks` has:

- `precheck`: the precheck's JSON line, that is, every project evaluated, with its outcome and reason;
- `log`: the tick's line of `controller.log`, with the decision, the candidates and their action, the launches, the budget, the wake-up, and the steps alive and finished;
- `launches`: each launch the controller asked for, with folder, options and the whole prompt;
- `cursors`: the cursors written in the tick, with their new value (`null` if removed), without `aggiornato`;
- `documents`: other documents that changed, without `_m`, only when there are any;
- `status`: `controller-stato.json` after the tick.

At the top level:

- `cleanup`: what step 2 removed;
- `controller_lock`: absent after the cleanup, held during the run, released at exit;
- `config`: the model and budget policies as the prototype read them from its own files. Use them as inputs.

**Placeholders.** Each stands for a value that changes from run to run.

- `{dossier}`, `{runtime}` and `{run}` replace the run's folders, inside prompts too.
- `{controller}` is the lock owner, and `{pid}` the process id.
- `{rinvio-N}` replaces the random ids of the `rinvio` signals, numbered in order of appearance; equal ids stay equal.
- `{precheck-t}` replaces the `t` of the precheck's line. The prototype takes it from the wall clock, not the injected one.

The other side normalizes its own run the same way before diffing. Object key order does not count; array order does.

## Not covered

Some parts are left out by construction:

- resuming suspended sessions, because the runs use `--no-ripresa`;
- scheduled operations and the chronicler, because no operation is configured;
- Orca's workers;
- the dossier's warnings, which are always empty.

The fake `gh` in `crates/regia-conformance` does not know `comments` yet. On the other 33 calls the prototype makes in these cases, it answered with the same JSON values as the driver's replay, on these `responses.json` files (checked on 10 November 2026). The text of an error, `payload.dettaglio` in the precheck's line, comes from the harness and differs between the two sides, so it is not compared.

## Recording again

This runs locally only, because it needs the private dossier. From the dossier:

```
python docs/app/ombra/driver.py --verifica
python docs/app/ombra/driver.py --esporta <this checkout>
```

`--esporta` refuses to run when the dossier's `tools/`, `policies/` or `docs/app/ombra/` have local changes. It rewrites `cases/` and `RECORDED.json` and leaves this file alone.
