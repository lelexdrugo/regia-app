# The command contract

The English names of the CLI: commands, arguments, JSON keys, values, exit codes and variables. The agents
read them, so they are the contract of the product (ADR 0017 §4 of the dossier, ADR 0001 §11). The facade
table in `crates/regia-facade/table/` states them, one file per group of commands, and the fixtures in
`fixtures/` prove them against the Python prototype. This page explains the table and gives, for every name
whose English is not obvious, the alternative left out and why.

**Nothing here is decided.** Every name is a proposal for the gate of the names (#223, ✋C), which takes the
map as a whole. The proposals start from the conformance study of the dossier (`docs/app/architettura/conformita.md`
§8); the names it marks ⚠ are listed again below with the choice made.

## Who fills which section

| Section | Commands | Slice |
|---|---|---|
| [Conventions](#conventions), [Exit codes](#exit-codes), [Variables](#variables) | all | #442 (F7), with what #440 (F5) set |
| Tracker commands | `brief`, `projects`, `project *`, `task *`, `plan`, `comment`, `setup`, `init` | #441 (F6) |
| [State, dossier and budget](#state-dossier-and-budget) | `lock *`, `lease *`, `step *`, `checkpoint`, `declare`, `check`, `budget` | #442 (F7) |
| [Names with no Python equivalent](#names-with-no-python-equivalent) | `controller run`, `moviola export`, `agents show`, `launch` | #442 (F7), with the names of #460 (F19) |

Of the two commands of the facade's engine (#440), `lock renew` is in its group here and `task maturity` in
#441's.

## Conventions

- **Keys** are snake_case. The keys the prototype already has in English stay (`ok`, `task`, `branch`, `sha`,
  `url`, `ttl_s`, `t`, `v`, `ts`).
- **`--json`** is global and goes first in the prototype's argv, wherever the user wrote it (#440). A command
  with no JSON output prints text, which passes as it is: the agents read `--json` and the exit code.
- **Data are never translated.** Project ids, report names, the providers `claude` and `codex`, the windows
  `5h` and `7d`, and every sentence of the prototype (`reason`, `why`, `text`, `note`) pass as they are, in the
  prototype's language. The table translates by shape, not by name (`conformita.md` §8.4).
- **An unknown key or value fails closed.** A key or an enum value the table does not list is an error,
  exit 1, never a key passed through in Italian.
- **Values of options** pass as they are unless the table names an enum for them. A value never starts with a
  dash. An option with many values (`--reports-processed a b`) takes every value that follows it, as the
  prototype's `nargs="*"` does.

## Exit codes

| Exit | Meaning |
|---|---|
| 0 | done; also `lock status` on a held lock, `check` with findings, `budget` with an unknown value, `step finish` whose checks or dossier failed (the outcome says so), `checkpoint` whose push failed |
| 1 | error: a wrong argument, a step that cannot close (`step finish` on your lock with no step open), a `checkpoint` that could not even try (a project with no folder), a `declare` with no session id, a key or value the table does not know |
| 3 | **refused** (`regia`): a lock or a lease that is someone else's, a lock busy for more than 3 s, `step start` on a lock held by another orchestrator |

**3 has two meanings, and the table does not normalize it.** In `regia` it means *refused*: the operation was
not done, and it is not an error. The orchestrator's prompt relies on it (`step start` exits 3 when another
orchestrator is active), and so does the precheck. The prototype's precheck (`tools/precheck.py`), which stays
in Python past v0.1, uses 3 for *nothing to do*. The facade passes every exit code of the prototype as it is,
3 included (#440); a native command keeps the same codes. A caller reads 3 by the program that gave it.

## Variables

| Variable | Meaning | Status |
|---|---|---|
| `REGIA_ENGINE` | `python` runs the command on the prototype, `rust` in the binary; another value is an error | #440, new value `rust` |
| `REGIA_NOW` | the instant the product's clock reads, ISO 8601 with its offset; set by the harness to the fixtures' instant, `2026-01-10T09:00:00Z` | **new** (#440/V3): the harness sets it on every run; a native lock, lease or step must read it, and none does yet |
| `REGIA_TODAY` | the day the brief counts due dates from | existing |
| `REGIA_HOME`, `REGIA_RUNTIME`, `REGIA_PYTHON`, `REGIA_CLAUDE_CREDENTIALS`, `REGIA_CODEX_AUTH` | the prototype's, unchanged | prototype |

The prefix follows the product's name (ADR 0017 §6); the prototype's own variables keep `REGIA_`.

## State, dossier and budget

The commands of `regia.db` (locks, leases, cursors), of the dossier's repo and of the budget. All are in
the routing group `state`, which passes at #502 (F57), except `declare`, a write (#501, F56).

### `lock`: the project lock

One orchestrator at a time on a project.

| Command | Prototype | Arguments | Exit |
|---|---|---|---|
| `lock acquire <project>` | `lock prendi` | `--holder`, `--ttl` (seconds) | 0; 3 busy |
| `lock renew <project>` | `lock rinnova` | `--holder`, `--ttl` | 0; 3 someone else's or absent |
| `lock release <project>` | `lock rilascia` | `--holder` | 0 (also when absent: `free`); 3 someone else's |
| `lock status <project>` | `lock stato` | | 0 always |

**Outcome** (`acquire`, `renew`, `release`; `step renew` too): `ok`, `status`, `reason` (`motivo`), `current`
(`attuale`, the lock as it is, on a refusal), `project`, `holder` (`proprietario`), `taken_at` (`preso`),
`ttl_s`, `expires_at` (`scade`), `replaced` (`al_posto_di`, the expired holder taken over), `stolen_at`
(`rubato_il`), `renewed_at` (`rinnovato`), `expired` (`scaduto`), `expires_in_s` (`scade_tra_s`).

**Status** (`lock status`): `project`, `busy` (`occupato`, a lock that has not expired), `status`, and the view
of the lock when there is one, with the keys of the outcome.

**States** (`status`, one enum for locks and leases): `taken` (`preso`), `renewed` (`rinnovato`), `stolen`
(`rubato`), `busy` (`occupato`), `released` (`rilasciato`), `foreign` (`altrui`), `absent` (`assente`), `free`
(`libero`), `expired` (`scaduto`), `unreadable` (`illeggibile`), `heartbeat` (`battito`).

### `lease`: the lease of a task In progress

| Command | Prototype | Arguments | Exit |
|---|---|---|---|
| `lease acquire <task>` | `lease prendi` | `--project`, `--holder`, `--ttl` | 0; 3 busy |
| `lease heartbeat <task>` | `lease battito` | `--holder`, `--ttl` (default: the ttl it was taken with) | 0; 3 someone else's or absent |
| `lease release <task>` | `lease rilascia` | `--holder` | 0; 3 someone else's |
| `lease list` | `lease list` | `--expired` (`--scadute`) | 0 |

**Outcome**: the lock's keys, plus `task` and `heartbeat_at` (`battito`). **List**: the views, each with
`expired` and `expires_in_s`; a row that is not a JSON object lists with `unreadable: true`.

### `step`: an orchestrator's step

The prototype's `passo`. A step opens with the project's lock and closes by writing the `events` cursor,
which tells the precheck what is new.

| Command | Prototype | Arguments | Exit |
|---|---|---|---|
| `step start` | `passo inizio` | `--project`, `--holder` (default: the session id; at `start` only, a generated one), `--ttl` (default 1800) | 0; **3 when another orchestrator holds the lock** |
| `step renew` | `passo rinnova` | `--project`, `--holder`, `--ttl` | 0; 3 |
| `step finish` | `passo fine` | `--project`, `--holder`, `--file` (repeatable), `--folder` (`--cartella`), `--no-push`, `--reports-processed` (`--report-elaborati`, zero or more names) | 0, also when the checks or the dossier failed; 3 someone else's lock; 1 no step open |

**`step start`**: `ok`, `project`, `status`, `holder`, `holder_generated` (`proprietario_generato`), `t` (when
the step began to observe), `resumed` (`ripreso`, the open step of the same holder goes on), `expires_at`,
`expires_in_s`, `cursor` (`cursore`, the `events` cursor). A refusal has `ok`, `status`, `reason`, `current`.

**`step finish`**: `ok`, `project`, `holder`, `cursor`, `lock` (the state of the release), `checks`
(`controlli`), `dossier` (the [dossier outcome](#checkpoint)). A refusal has `ok`, `status`, `reason`,
`current`. `--no-push` leaves out the checks and the dossier.

**The `events` cursor**: `v`, `t`, `own` (`propri`: what the step wrote or processed itself), with `comments`
(`commenti`, the ids of the comments it wrote) and **`reports`** (`report`, new with #406: the reports passed
to `--reports-processed` that exist, with their dates; the names are data). A name that does not exist is a
warning on stderr and is not stored; the exit code does not change.

**Checks of the step**: `total` (`totale`), `per_check`, `findings`, `warnings`, as [`check`](#check), or
`unavailable` (`non_disponibile`) with the reason.

### `checkpoint`

Commits and pushes the named files of the dossier, with no lock: `.regia/stato.md`, `.regia/decisioni.md`,
`.regia/journal`, and each `--file`. Never a force push, never another branch.

| Command | Prototype | Arguments | Exit |
|---|---|---|---|
| `checkpoint` | `checkpoint` | `--project`, `--file` (repeatable), `--folder` | 0; 1 only when it could not even try |

**Outcome**: `ok`, `project`, `dossier` with `outcome` (`esito`), `folder` (`cartella`), `branch`, `sha`,
`files` (`file`), `reason` (`motivo`), `ahead` (`avanti`). **Outcomes**: `pushed` (`pushato`), `local_commit`
(`commit_locale`, the push did not go), `nothing` (`niente`), `no_remote` (`nessun_remoto`), `unavailable`
(`non_disponibile`).

### `declare`

The prototype's `dichiara`: which project, and task, this session works on. Local only.

| Command | Prototype | Arguments | Exit |
|---|---|---|---|
| `declare` | `dichiara` | `--project`, `--task`, `--role` | 0; 1 with no session id |

**Output**: `ok`, `session` (`sessione`), `project`, `task`, `ts`. The role passes as it is: the activity log
stores it, and the moviola reads the log in the prototype's words (`conformita.md` §8.4).

### `check`

The prototype's `controlla`: the commitments left outside the tracker, read only.

| Command | Prototype | Arguments | Exit |
|---|---|---|---|
| `check` | `controlla` | `--project`, `--against-disk` (`--contro-il-disco`) | 0, findings included; 1 an unknown project |

**Output**: `findings` (`voci`), `per_check` (`per_controllo`, the count of each check, keyed by check id),
`warnings` (`avvisi`). **A finding**: `check` (`controllo`), `project`, `where` (`dove`), `text` (`testo`),
`why` (`perche`), `url`. **Check ids**: `state-without-issue`, `state-lines-removed`,
`open-questions-without-issue`, `remaining-unreconciled`, `comment-after-close`, `decision-without-flag`,
`inbox-stale`, `approved-never-started`, `in-progress-stalled`, `project-field`.

### `budget`

What is left of Claude's and Codex's windows, read only.

| Command | Prototype | Arguments | Exit |
|---|---|---|---|
| `budget` | `budget` | `--cache`, `--min-interval` (`--min-intervallo`, seconds, default 300) | 0 always |

**Output**: `generated` (`generato`), `provider` (keyed by provider), `cache` (with `--cache`). **A provider**:
`provider`, `status`, `plan` (`piano`), `windows` (`finestre`, keyed by window), `reason`, `token`, and from the
cache `age_s` (`eta_s`), `from_cache` (`da_cache`), `update_failed` (`aggiornamento_fallito`). **A window**:
`status`, `used_pct`, `remaining_pct`, `reset`, `reset_in_s` (`reset_tra_s`), `open` (`aperta`), `reason`.
**Codex's token**: `status`, `expires_at`, `expires_in_s`, `reason`, `http_status` (`codice_http`). **The
cache**: `from_cache`, `min_interval_s`, `attempt_age_s` (`eta_tentativo_s`), `note` (`nota`). **States**: `ok`,
`unknown` (`sconosciuto`); of the token `valid`, `expiring` (`in_scadenza`), `expired`, `unknown`, `refused`
(`rifiutato`).

## Names with no Python equivalent

Commands the product adds, which `conformita.md` §8.1 does not name. They have no line in the facade table:
the table is for what the prototype answers.

### `controller run`

The controller as a command of the binary (F27, #469), in place of `python tools/controller.py`.

| Name | Prototype | Meaning |
|---|---|---|
| `controller run` | `controller.py` | runs the rounds, every `--interval` seconds |
| `--once` | `--una-volta` | one round, then exits |
| `--shadow` | none | the round decides and writes its trace beside the Python controller, and launches nothing: the shadow run of #436 as a mode of the product |
| `--rounds <n>` | `--giri` | exits after n rounds; 0 for ever |
| `--interval <s>` | `--intervallo` | seconds between two rounds, default 900 |
| `--launcher <auto\|cli\|orca\|fake>` | `--lancio` (`finto`) | how a step starts |
| `--project <id>` | `--progetto` | only this project, repeatable |
| `--model`, `--effort` | `--modello`, `--effort` | the model of a step, default the `replanning` role of the agents' settings |
| `--max-steps <n>` | `--max-passi` | steps launched per round |
| `--step-limit-s <s>` | `--tetto-passo-s` | past this a step no longer counts as alive |
| `--precheck-timeout <s>` | `--precheck-timeout` | |
| `--no-resume` | `--no-ripresa` | does not look at the sessions stopped by the usage limit |
| `--no-wake` | `--no-sveglia` | |
| `--opt-in <file>` | `--optin` | the sessions and folders allowed to resume |
| `--config <file>` | `--config` | the scheduled operations |
| `--only-project-of-resumed` | `--solo-progetto-su-da-riprendere` | |

Exit: 0; 1 an error; 3 another controller is active (a refusal, as in `regia`).

### `moviola export`

The history of a project as deterministic Markdown, read only: the prototype's `python -m tools.moviola esporta`.
Arguments: `--project`, `--folder` (`--cartella`), `--from` (`--dal`), `--to` (`--al`), `--direction`
(`--direzione`), `--output` (`--uscita`). `moviola serve`, the viewer on localhost, is not ported: the app and
its IPC commands replace it (ADR 0001 §9).

### `agents show` and `launch`

The names #460 (F19) chose to write its code, provisional, in its report: `.regia/report/2026-11-10-460-servizi-agenti.md`
of the dossier, section «Nomi nuovi». The table has no line for them: `crates/regia-facade/table/agents.toml`,
if it comes, is #460's.

- `agents show --role <role> [--json]`: who serves a role. **Roles**: `replanning`, `scouting_and_reading`,
  `synthesis_and_maps`, `writing`, `code`, `experiments_and_probes`, `chronicler`, `reconnaissance`,
  `adversarial_review`, `second_implementation`. **Output**: `role`, `service`, `tier` (`light`, `standard`,
  `deep`), `model`, `effort`, `surface`, `deadline_minutes`, `clean_context`, `diversity` (`not_required`,
  `met`, `degraded`), `fallback` (`service`, `tier`, `model`, `effort`, `clean_context`).
- `launch`: starts a session. **Receipt**: `surface` (`cli`, `dry`; the prototype's `adapter`), `service`,
  `model`, `effort`, `session`, `transcript`, `transcript_exists`, `pid`, `log`, `started`, `prompt_file`,
  `resume`, `warnings`, and `request` on a dry run.

## The ⚠ names

| Prototype | Proposed | Left out | Why |
|---|---|---|---|
| `passo` | `step` | `turn`, `pass` | one replanning of the orchestrator; `turn` is a chat's turn, `pass` reads as a verb |
| `passo fine` | `step finish` | `step end`, `step close` | `finish` says it completes the step: cursor, release, dossier |
| `controlla` | `check` | `audit` | short, and `per_check`, `check` say what each finding came from; `audit` suggests a review by someone |
| `lease battito` | `lease heartbeat` | `lease beat`, `lease ping` | the usual name for a signal that keeps a lease alive |
| `--proprietario` | `--holder` | `--owner` | `owner` collides with the GitHub owner of the tracker and with the owner of the regia |
| `--cartella`, `cartella` | `--folder`, `folder` | `--dossier-dir`, `--path` | the project's folder, as the registry names it; `--path` says nothing of what it is, and `--dossier-dir` repeats the `dossier` of the outcome |
| `--ruolo` (`declare`), its values | `--role`, values as they are | translating the values through an enum (`orchestratore` → `orchestrator`, `titolare` → `owner`) | the activity log stores the role and the moviola reads the stored words: a translated value would change data written since #146 |
| `esito` | `outcome` | `result` | what happened to the dossier; `result` reads as the return of any call |
| `dossier` | `dossier` | `repo`, `project_repo` | the regia's word for a project's repo of state and decisions; `repo` would mix it with the repos of the products |
| `motivo` | `reason` | `cause`, `message` | always a sentence that says why |
| `rubato` | `stolen` | `taken_over` | a lock taken from an expired holder; `stolen` is what the prototype says |
| `altrui` | `foreign` | `not_yours`, `other` | someone else's: one word, as the other states |
| `al_posto_di` | `replaced` | `instead_of`, `previous_holder` | the expired holder replaced; a past participle like `stolen_at` |
| `propri` | `own` | `mine`, `self` | what the step wrote or processed itself |
| `--report-elaborati` | `--reports-processed` | `--processed`, `--reports-done` | the reports the step read and closed, by name |
| `report` (cursor) | `reports` | `processed` | a map of report names to dates |
| `occupato` (`lock status`) | `busy` | `held` | the same word as the refusal `busy`; `held` would be one more name for one state |
| `perche` | `why` | `reason` | `reason` is the cause of a refusal; `why` is the explanation of a finding |
| `resta-non-riconciliato` | `remaining-unreconciled` | `resta-unreconciled` | `resta` is a field of the report; the id says what it checks |
| `decisione-senza-flag` | `decision-without-flag` | `decision-unflagged` | parallel to the other ids |
| `--contro-il-disco` | `--against-disk` | `--on-disk` | the state of HEAD against the file on disk, as `step finish` does |
| `in_scadenza` | `expiring` | `expiring_soon` | the token expires within the threshold |
| `controller --una-volta` | `--once` | `--single` | the name the plan uses (F27) |
| `controller --giri` | `--rounds` | `--ticks`, `--cycles` | a round of the controller as its user sees it; `tick` is the engine's method (F25) |
| `controller --lancio finto` | `--launcher fake` | `--launcher dry` | `dry` is a surface of `launch` (#460) with another meaning: a request not sent |
| `moviola esporta` | `moviola export` | `moviola dump` | the history as a document |

## Known limits

- **A legacy `events` cursor.** The prototype still reads an `events` cursor written as a bare ISO string, the
  form before #406. The table expects an object, so `step start --json` on such a cursor would exit 1 after the
  lock is taken. No writer of today writes the string form; the fixtures do not hold one.
- **The fixtures' instant.** Every case is recorded and checked at `2026-01-10T09:00:00Z` with local time UTC:
  the prototype through `crates/regia-conformance/oracle/fixed_now.py`, the product through `REGIA_NOW`, which
  no native command reads yet.
