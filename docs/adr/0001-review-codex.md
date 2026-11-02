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
