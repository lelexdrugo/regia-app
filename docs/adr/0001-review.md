# Adversarial review of ADR 0001

- **Reviewed:** commit `98dc641`, 2026-11-02, read-only.
- **Reviewer:** Claude Opus, in a clean context, without the author's notes. **Not Codex, which the plan asked for:** the Codex job started and died with `ENOSPC` because the disk was full, without producing findings, and Codex's 5-hour quota then read 100 %. So this review did not come from a different model family, which is the point of the rule. A Codex pass remains to be done when quota allows.
- **Result:** 32 defects (D1–D32), 4 high. The reviewer found no defect in the port of the brief's rules, in `translate_output` being one code path, or in the negative controls.
- **Fixes verified by:** the conformance tests, the app smoke test and `live-check`, all rerun after the fixes.

| Id | Sev. | Defect | Fate |
|---|---|---|---|
| D1 | high | The shadow's oracle cannot run as written and is not side-effect free | ADR corrected (§7): drop the lock row, `--no-ripresa`, moviola stub. Not built |
| D2 | high | The table lacked real `controller` keys, so the facade failed exactly when the controller had something to report | **Fixed:** keys added, unit test on non-empty samples. Still no recorded case with these sections |
| D3 | high | The fake `gh` ignored the query, so the Rust query was untested in CI | **Fixed:** the fake projects each answer on the selection set and checks variables and arguments |
| D4 | high | The `"Status"` field name was hard-coded, with silent empty briefs on a localized field | **Fixed:** read by field id, with an error when nothing matches. Verified on the real tracker |
| D5 | medium | "Fails closed" overstated: missing keys and opaque values pass | ADR and table comment narrowed. Enumerating more values goes to #223 |
| D6 | medium | `projects` treated a free registry as a closed contract, with no test | **Fixed by removal:** out of the table; the design note goes to #223 |
| D7 | medium | "A bug in the table fails the test" holds only for exercised shapes | ADR corrected |
| D8 | medium | The `filtered` case could not tell a graph computed after the filter | **Fixed:** cross-project edge added, and a test asserts the count |
| D9 | medium | Unported sections were invisible in the JSON: a false all-clear | **Fixed:** `not_ported` key; agents stay on the facade until a slice is ported (§11) |
| D10 | medium | Human output and the app diverged, and were untested | **Partly fixed:** session tasks no longer doubled; graph warnings in CLI and app. Exclusion from the proof stated (§12) |
| D11 | medium | The security guard checked less than the ADR said | **Fixed:** every capability file, the exact CSP, forbidden keys, plugins via the lockfile |
| D12 | medium | The skeleton contradicted the threat model, unmarked | ADR lists them as *not built* (§14); actions **pinned** |
| D13 | medium | The usage probe's token handling was hidden | ADR states it (§6, §14), and it goes to the owner |
| D14 | medium | "One TOML file" was false | ADR restated (§6) |
| D15 | medium | The two ports did not compose as stated; fallback lost its safety condition | ADR redesigned (§6): neutral request, `FailedBeforeEffect` |
| D16 | medium | Wake-up and environment scrub were missing from the port | ADR added (§6) |
| D17 | medium | The lock plan had gaps, and no plan B | ADR added (§7): clocks, keys, runtime files, plan B |
| D18 | medium | A fixed clock was not achievable through the named command | ADR corrected (§7): driver outside `tools/`, whole-folder copy |
| D19 | medium | A rename would have opened a second `regia.db` | **Fixed:** `PROTOTYPE_NAME`, pinned by a test |
| D20 | medium | Open inputs unanswered | ADR answers Q4, Q5, Q6 and the policies per file; the v2 map is stated as a proposal |
| D21 | medium | The ADR reversed ADR 0017's premise silently | ADR states it (§7); note added to `conformita.md` |
| D22 | medium | §7 and §14 gave two start rules | ADR fixed: the controller starts only after consent |
| D23 | medium | The real GUI launch path was never exercised | `CREATE_NO_WINDOW` **fixed**. The macOS `PATH` and release-build launch stay as risks in the ADR, not fixed |
| D24 | medium | Two statements in Proof had no artifact | **Fixed:** `prove/live-check.log` committed, and links in the report |
| D25 | low | "The core returns codes" stated as fact | Marked *not built* |
| D26 | low | The passthrough dropped Python's stderr | **Fixed** |
| D27 | low | The no-Italian guard missed plain cases | **Fixed:** longer list, camelCase split, exemption limited to one file |
| D28 | low | The name guard matches only the exact string | ADR narrowed; **no code change,** because a bare-word search needs an allowlist of the crate names that a rename replaces anyway |
| D29 | low | Two divergences on an empty project | **Fixed**, with a case in the scenario |
| D30 | low | The #103 answer was incomplete | ADR table completed |
| D31 | low | CI gaps | **Partly fixed:** no cancellation on `main`, `compile_error!` on smoke in release. PR trigger, MSRV and release jobs are stated as not built |
| D32 | low | `live-check` could print private data | **Fixed:** project ids and table errors redacted |
