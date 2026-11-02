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
