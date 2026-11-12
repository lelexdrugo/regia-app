# The moviola's fixtures

Three synthetic dossiers in the shape the prototype writes (`docs/moviola/CONTRATTO.md` of the dossier, §5): no real
title, path or session. `crates/regia-conformance/tests/moviola.rs` reads them with the core and compares the answers
with the prototype's.

- `projects/alpha`: seven milestones written out of order, two of them on the same instant, one with an offset, one
  hand-written with no reference, process or author; three directions, one lane with no record; three aggregates,
  from the full one to the bare one; six sidecars, two with `in_breve` and one of a paper; report references of every
  kind the two resolvers read alike (present, absent, `{dati}`, absolute, `..`, with `\`).
- `projects/beta`: one milestone and its sidecar, no other file, no tracker and no remote.
- `projects/gamma`: no moviola at all.
- `settings.json`: the projects as a registry gives them, the settings of their links, the remotes and the ranges
  the test checks.
- `oracle.json`: the prototype's answers, recorded by `crates/regia-conformance/oracle/moviola_oracle.py`.

To record again, with a copy of the prototype's `tools/` in `REGIA_HOME` (never the live dossier):

```text
REGIA_HOME=<copy> RECORD_MOVIOLA=1 cargo test -p regia-conformance --test moviola -- --ignored
```
