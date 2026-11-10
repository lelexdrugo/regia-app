# Codex review of ADR 0001: not concluded

- **Date:** 2026-11-02, read-only, requested by task #403 of the dossier.
- **Reviewer:** Codex (plus plan), through `codex:codex-rescue`, job `task-mvzcb759-1hk9pf`, started 14:14Z.
- **Result:** `non-concluso`. At 14:31Z Codex stopped with "You've hit your usage limit" (5-hour quota 45 % at launch, 100 % after the job). The job produced **no defect list**. No fallback to another model was used, by the rule of the task.
- **What Codex said before stopping** (messages recovered from the job log, verbatim in substance):
  - The prior review `0001-review.md` (D1-D32) was readable and noted so as not to repeat it. Controller and shared lock are marked "not built", so their absence is not a defect.
  - The facade keys it traced so far **match the current Python producers**.
  - Two candidate defects, **stated without `file:line` and not verified**: (1) a controller can resume after its lease was stolen and still launch work; (2) a successor can overwrite newer step records because it loads them before acquiring the lock. Codex called both static counterexamples, with the runtime behaviour untested. They concern the Python prototype controller, which §7 says the Rust controller will share a lock with.
  - Sandbox access to the Python `site.py` was denied; no finding depends on it.
- **Fate of the two candidates:** not verified by the worker (no location to check, and the task asked for Codex's evidence). To decide in #400: re-run this review when quota returns (reset 20:00Z), or have someone read the Python `lease`/`step` code for these two points.

## Round 2

- **Date:** 2026-11-02, read-only, task #403 round 2. **Reviewer:** Codex (plus plan) via `codex:codex-rescue`, one job of about 8 minutes (quota 0 % of 5 h at launch). Scope: §7 and §14 of the ADR and the code that proves them; §11 left out. Part 2 (prototype) not done, see the dossier report.
- **Result:** concluded. Codex answered the two leads with `file:line`. Line numbers are relative to `projects/regia/tools/`. Counterexamples are static, not executed. The worker re-read the code for the two grave ones (verification below).
- None of D1-D3 appears in `0001-review.md` (D1-D32 there).

### Codex text (verbatim in substance)

**D1 · grave · lead 1 confirmed · `controller.py:694` (launch), `controller.py:552`/`:643` (last renewals), `runtime.py:393-401`, `lancio.py:274-275`.** The controller lock has TTL 600 s (`controller.py:65-66`). A renews, prepares the launch, is suspended before `self.lancia`. After 600 s B takes the expired lock ("rubato", `runtime.py:399`). A resumes and launches without re-checking ownership; the CLI path starts the process. The owner check exists in `rinnova` but is missing between the last renewal and the launch.

**D2 · grave · lead 2 confirmed · `controller.py:490-492` (load), `:1306-1307` (lock after construction), `:1052-1054`, `:1002`/`:950`, `sospese.py:482-486`, `runtime.py:582-583`.** The constructor loads the step registry and rebuilds the steps before the lock is taken. A holds the lock; B builds a `Controller` and reads an empty registry, then is suspended before `prendi_lock`. A launches a step and saves its record, ends, releases. B takes the lock; in its first round `_sospese` calls `_aggiorna_registro`, which does not recover A's record (nothing to update, no opt-in), then saves its own empty dict. `Registro.salva` replaces the whole document, with no version check. A's record is deleted even without any lock theft.

**D3 · media · new · `regia.py:1973-1983`, `runtime.py:575-583`, `regia.py:1985-1989`, `:2001-2013`.** `passo inizio` takes the lock, reads and prepares the cursor, and saves it in separate operations. A prepares its cursor and stalls past the TTL. B finds the lock expired and completes `passo inizio`, saving B's cursor. A resumes at `:1983`: `scrivi_cursore` replaces the document without checking the lock owner, and A still returns `ok:true`. B keeps the lock, but its later `passo fine` fails because the cursor carries A's name.

### Worker verification

- **D1:** confirmed by reading. `rinnova_lock` (`controller.py:524-527`) raises `LockPerso` if the lock is not ours, but between the last renewal before the launch loop (`:643`) and `self.lancia` (`:694`) there is no check, and the launch itself (Orca, "can last minutes", comment at `:823`) is not covered by a lease re-check before it starts. Codex cites `:552` as the last renewal; `:643` (sveglia) also renews and may lie on the path, so the exact window was not measured, but no renewal sits right before `:694`. Fate: **da decidere in #400**.
- **D2:** confirmed by reading. `self.record = self.registro.carica()` at `:491` runs in `__init__`; `prendi_lock()` follows only at `:1307`. `_salva_registro` (`:949-950`) saves the in-memory dict through `Registro.salva`, which writes the whole document (`sospese.py:482-486`). Needs a second controller constructed while the first holds the lock and then ends before the lock is taken: narrow but needs no clock fault. Fate: **da decidere in #400**.
- **D3:** media, not verified in depth. Fate: **da decidere in #400**.
- **False positives:** none among the three. Not already in `0001-review.md`.

## Round 3

- **Date:** 2026-11-10, read-only, task #439 of the dossier (slice F4). **Reviewer:** Codex (`gpt-6-astra`, effort `high` instead of the default `xhigh`, to fit three reviews of #439 in the 5-hour quota) through `codex:codex-rescue`, job `task-mwaacg7c-sv6it1`, 06:04Z to 06:10Z. Quota: 0 % of 5 h at launch, 29 % after; 7 days from 7 % to 11 %.
- **Commit reviewed:** `eaa7be6`, the `main` of 2026-11-10, with `regia-store` (#437) and the new brief oracle (#435).
- **Scope:** what no earlier review covered: Status, Context, sections 1–6, 8–13 and 15, the answers to ADR 0017, Alternatives, Proof and Consequences; in section 7 only "Three defects the Rust controller does not inherit", written after round 2. The rest of section 7 and section 14 are out of scope.
- **Known lists given to Codex:** `0001-review.md` (D1–D32), round 2 (D1–D3), and the drift already open in the dossier: `regia-exec` "planned", §12's counts and §11's names after #435, the fake `gh` ignoring `first:N`, the clock tag of §7.
- **IDs.** This round's findings are **#439/D1–D7**. They are not round 2's D1–D3, which section 7 and the plan's slices cite.
- **Result:** concluded. Seven findings: one high, five medium, one low. Counterexamples are static, not executed. The worker re-read the code for all seven; none is a false positive.

### Codex text (verbatim in substance)

**#439/D1 · high · ADR `:126` · `crates/regia-store/src/lib.rs:9`.** The ADR says injected-clock engine tests cover all three controller fixes, although §2 still lists the controller as planned (`:46`). No test found after inspecting the crates' manifests and Rust sources, `app/src-tauri/src` and `crates/regia-conformance/tests`; the existing controller-lock test exercises exclusion, not launch fencing, registry updates or cursor transactions (`store_cross.rs:315`). A controller slice cannot treat these safety properties as implemented or tested. *Extends round 2's D1–D3.*

**#439/D2 · medium · ADR `:64` · `crates/regia-github/src/lib.rs:18`.** The protection against a false empty brief depends on seeing the configured Project: `projectItems` reads only `first:5`, without pagination, while the error requires `items_seen > 0` (`:180`). If every relevant issue has the configured Project beyond those five entries, status extraction finds nothing, the error is bypassed, and the task sections omit those issues (`crates/regia-core/src/brief.rs:109`). *Extends `0001-review.md` D4.*

**#439/D3 · medium · ADR `:196` · `crates/regia-facade/src/lib.rs:225`.** The ADR says the facade reproduces exit 3, which agents consume as part of the contract (`:198`). Every nonzero Python exit becomes `PythonFailed`, discarding stdout, and the CLI turns that error into exit 1 (`crates/regia-cli/src/main.rs:35`). A slice adding an idle or refused command through the facade would silently change its exit-code contract. *Known: no.*

**#439/D4 · medium · ADR `:154` · `crates/regia-store/src/db.rs:93`.** The ADR promises a rollback journal and no WAL, but opening a database sets `synchronous=FULL` without checking or setting `journal_mode`. An existing WAL database stays in WAL; the journal test checks only freshly created databases (`store_cross.rs:215`). A storage or backup slice cannot rely on the rollback-journal guarantee. *Known: no.*

**#439/D5 · medium · ADR `:223` · `crates/regia-conformance/src/bin/live_check.rs:55`.** The ADR says `live-check` prints only paths and counts, but the native command's failure assertion prints its stderr verbatim. An unknown status option puts the tracker's value in the error (`crates/regia-core/src/v1.rs:24`), and the CLI forwards it to stderr, so a shared failing run can expose tracker values despite the redaction on the success path. *Extends `0001-review.md` D32.*

**#439/D6 · medium · ADR `:220` · `crates/regia-conformance/src/bin/record.rs:259`.** The recorder claims to refuse URLs outside the synthetic host, but it only checks values of the JSON key `"url"`. A scenario title holding `https://private.example/document` passes through (`:123`) and is written into `python.it.json` (`:262`); the fixture test's fixed list misses that host too (`crates/regia-conformance/tests/brief.rs:240`). *Known: no.*

**#439/D7 · low · ADR `:48` · `crates/regia-cli/src/main.rs:47`.** Sharing `regia-runtime` does not stop the two frontends from wiring different adapters: `REGIA_ENGINE=python` sends the CLI through Python, while the app always calls the native `load_brief` (`app/src-tauri/src/main.rs:34`). §11 supports this explicitly, so §2's unconditional claim contradicts it. *Known: no.*

**Checked and holds** (Codex): product-name derivation and the manifest checks; status read by field id and option id; an unknown option rejected; bundled SQLite; `BEGIN IMMEDIATE` and `synchronous=FULL`; one `translate_output` for passthrough and harness; array order kept in the comparison; capabilities and CSP of the normal build; text rendered with `textContent`; the same catalog ids in every language; the operating systems CI selects. **Not checked:** builds and tests, historical logs, external claims (benchmarks, platforms, prices, governance), the rest of §7, §14.

### Worker verification and fate

| Id | Codex | Verified by reading | Worker's weight | Fate |
|---|---|---|---|---|
| #439/D1 | high | True. There is no controller crate on `eaa7be6`. The paragraph at `:126` has no *not built* mark, unlike the rest of the ADR. The only controller test is `the_controller_lock_excludes_both_ways`, which needs the dossier and is ignored in CI. | medium: the plan already gives the three tests to #449 (F14), #467 (F25), #468 (F26). What is wrong is the missing mark. | #560: mark the paragraph *not built* and name the three slices |
| #439/D2 | medium | True. The prototype reads `projectItems(first:5)` too (dossier `tools/regia.py:453`), so conformance holds and the limit is shared. It bites only an issue in more than five Projects. | low | #559, next to the guard of #435/V7: say the limit, or page through it in both implementations |
| #439/D3 | medium | True. `run_python_json` returns an error on any nonzero exit and `main` maps every error to `ExitCode::FAILURE`. `brief` exits 0, so nothing breaks today. | medium | #440 (F5): its point 4 covers exit 3 in the harness, and this extends it to the passthrough, with stdout kept |
| #439/D4 | medium | True. The prototype does not set `journal_mode` either (dossier `tools/runtime.py:230` sets only `synchronous`). Both rely on SQLite's default, and nothing in either repo turns WAL on. | low | #561: check `journal_mode` on open, or set it on both sides |
| #439/D5 | medium | True. `assert!(output.status.success(), "regia failed: {}", …)` prints the native stderr. Errors can carry tracker values and `gh` messages. | medium | #559: redact the failure path too, as `0001-review.md` D32 did for the success path |
| #439/D6 | medium | True. The check counts `"url"` keys only. `fixtures_are_synthetic` has a fixed list of words. | medium | #559: scan every string for `://` outside the fake host |
| #439/D7 | low | True. `REGIA_ENGINE` is a check path that §11 documents. | low | #560: qualify §2 ("on the native path") |
