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
| [Tracker commands](#tracker-commands) | `brief`, `projects`, `project *`, `task *`, `plan`, `comment`, `setup`, `init` | #441 (F6) |
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

## Tracker commands

The commands that read and write the tracker through `gh`, and the two that prepare it, `setup` and `init`.
By routing group: the reads `projects`, `task show`, `task list` and `plan` pass at #500 (F55); the writes
`task new`, `task status`, `task maturity`, `task block`, `task unblock`, `task parent`, `task move`,
`comment` and `project mirror` at #501 (F56); `brief`, `setup`, `project new`, `project set` and `init` are in
the group `state`, at #502 (F57). `projects` sits in F55 and in F57 in the plan; the table puts it in the
reads (#440/V9).

- **Statuses** are canonical ids, as values and as arguments: `inbox`, `proposal`, `approved`, `in_progress`,
  `decision`, `done`, `discarded`, and `unset` as the key that counts the tasks with none. The labels of the
  owner's Project stay in its catalogs. **Maturity**: `draft`, `ready`.
- **A value that starts with a dash** is written `--name=value` (`--body=- a list item`), the one form in
  which the prototype's parser takes it (#440/V10). Without `=` such a value is refused.
- **Text output.** `setup`, `task status`, `task maturity`, `task block` and `task parent` print a line the
  skill does not read: it reads the exit code (checked by #440 against `skills/regia/`). The other commands
  print text without `--json` too, which passes as it is.

### The issue, as every command gives it

`task list`, `task show`, `brief` and the nodes of `plan` share the keys of the issue (`conformita.md` §3.1):
`number`, `title`, `state` and `state_reason` (GitHub's), `status`, `project` (`progetto`, from the label),
`project_field` (`campo_progetto`, the value of the Project's field «Progetto», #338), `maturity`, `objective`
(`obiettivo`, a boolean: whether it is an Objective), `labels`, `parent`, `sub_issues` (`sub`, pairs `[number,
GitHub state]`), `blocked_by` (`bloccato_da`, pairs), `blocked_by_total`, `url`, `updated_at`, `created_at`,
`closed_at`. `task list` and `brief` add the graph's measures: `unblocks` (`sblocca`, approved tasks
downstream), `unblocks_total` (`sblocca_totale`, every task with a status downstream), `chain` (`catena`, the
longest chain downstream, itself included), `cycle` (`ciclo`). `brief` adds the due date, `due` (`entro`), with
`due_soon` (`entro_vicino`) or `overdue` (`entro_scaduto`). `task show` adds `body` and, with `--comments`,
`comments`.

### `brief`

| Command | Prototype | Arguments | Exit |
|---|---|---|---|
| `brief` | `brief` | `--project` | 0; 1 an error of the tracker |

**Output**: `waiting_on_owner` (`aspetta_me`), `ready` (`pronti`), `in_progress` (`in_corso`), `by_project`
(`per_progetto`, keyed by project id and then by status: both are data), and only when they are not empty
`session` (`sessione`, the items labelled `sessione`), `graph` (`grafo`: `cycles`, `truncated`,
`children_waiting`), `due_ignored` (`entro_ignorati`: `number`, `value`), `controller`, `dossier`,
`outside_tracker` (`fuori_dal_tracker`). **`controller`**: `last_round` (`ultimo_giro`), `stale` (`vecchio`),
`budget` (`rounds`, `since`, `reason`, `alarm`), `to_open` (`da_aprire`: `name`, `day`, `outcome`, `folder`,
`prompt`, `reason`, `attempts`), `to_resume` (`da_riprendere`: `project`, `session`, `resumes`, `how`,
`orca_command`, `reason`, `folder`, `origin`, `adapter`, `agent`, `orca_session`, `transcript`). **`dossier`**:
`project`, `folder`, `problem`, `branch`, `ahead`, `detail`; **problems** `folder_missing`, `not_a_repo`,
`no_remote`, `no_upstream`, `detached_head`, `git_failed`, `not_pushed`. **`outside_tracker`**: keyed by
project, `total`, `items`, `more`; an item as a finding of [`check`](#check).

### `projects`

| Command | Prototype | Arguments | Exit |
|---|---|---|---|
| `projects` | `progetti` | | 0; 1 |

**Output**: a list, one entry per Objective: `id`, `objective` (`obiettivo`, the number of the Objective
issue), `title` (`titolo`), `status`, and the keys of the registry that `project new` writes, `folder`
(`cartella`), `playbook`, `priority` (`priorita`), `wip`, `auto_defaults` (`default_automatici`), `repo`,
`products` (`prodotti`, a list). `priority` and `wip` are the registry's strings. **The entry is open**
(ADR 0001 §11): any other key of the block `<!-- regia:progetto -->`, written by hand, passes as data, with its
name as it is; two keys that land on one name are an error.

### `project`

| Command | Prototype | Arguments | Exit |
|---|---|---|---|
| `project new` | `progetto new` | `--id`, `--title` (`--titolo`), `--folder` (`--cartella`), `--playbook`, `--objective` (`--obiettivo`), `--definition-of-done` (`--fatto`), `--mandate` (`--mandato`), `--priority`, `--wip`, `--auto-defaults` (`--default-automatici`, `no` or hours), `--color` (`--colore`), `--repo`, `--products` | 0; 1 |
| `project set` | `progetto set` | `--project`, `--repo`, `--products`, `--wip`, `--priority`, `--dry-run` | 0; 1 |
| `project mirror` | `progetto riempi` | `--dry-run`, `--pause` (`--pausa`, seconds between two writes) | 0; 1 when an item could not be written, in JSON too |

`--products` takes repos separated by commas, or `none`, which the table translates to the prototype's
`nessuno`: any other value passes as it is.

**`project new`**: `project` (`progetto`), `objective`, `scaffold` (the files written in `.regia/`), and with
`--repo` `repo`, `git` (`init` or `existing`: whether the folder was a repo already), `first_push`
(`primo_push`, a sentence). **`project set`**: `project`, `objective`, `repo`, `products`, `wip` and `priority`
(integers, or null), `written` (`scritto`), `comment` (`commento`, the id of the comment that records the
levers, or null). **`project mirror`**: `dry_run`, `items` (how many issues the Project has), `options_added`
(`opzioni_aggiunte`), `already_right` (`gia_giusti`), `to_write` (`da_scrivere`, triples `[number, field,
label]`), `written` (`scritti`), `unlabeled` (`senza_etichetta`), `unknown_label` (`etichetta_ignota`, pairs
`[number, label]`), `failed` (`falliti`, pairs `[number, reason]`).

### `task`

| Command | Prototype | Arguments | Exit |
|---|---|---|---|
| `task new` | `task new` | `--project`, `--title`, `--body` (`--corpo`), `--body-file`, `--parent` (`--padre`), `--blocked-by` (`--bloccato-da`, numbers separated by commas), `--maturity` (default `draft`), `--status` (default `inbox`) | 0; 1 |
| `task status <n> <status>` | `task stato` | `--comment` (`--commento`), `--on-behalf-of-owner` (`--per-conto-del-titolare`), `--role` (`worker`, `chat`) | 0; 1 |
| `task maturity <n> <maturity>` | `task maturita` | | 0; 1 |
| `task block <n>` | `task blocca` | `--by` (`--da`, the blockers) | 0; 1 |
| `task unblock <n>` | `task sblocca` | `--by`, `--reason` (`--motivo`), `--on-behalf-of-owner`, `--role` | 0; 1 |
| `task parent <n> <parent>` | `task padre` | | 0; 1 |
| `task move <n>` | `task sposta` | `--project`, `--parent` (default: the project's Objective) | 0; 1 |
| `task show <n>` | `task show` | `--short` (`--breve`), `--comments` (`--commenti`), `--since` (`--dal`, an instant; alone it implies `--comments`) | 0; 1 |
| `task list` | `task list` | `--project`, `--status` (a list: `approved,in_progress`), `--ready` (`--pronti`), `--waiting-on-owner` (`--aspetta-me`), `--objectives` (`--obiettivi`), `--include-closed` (`--anche-chiusi`), `--order graph` (`--ordine grafo`) | 0; 1 |

`done` and `discarded` close the issue, any other status reopens it, and after a close the status is written
twice, three seconds apart, because the Project's workflows can rewrite it (`conformita.md` T10, T11).

`task new`, `task parent` and `task move` count the parent's sub-issues before any write, and refuse a parent
at GitHub's cap of 100 (#505). `task parent` gives a parent to a task that has none: it does nothing when the
task is a child of that parent already (#512/V1), and it sends no `replaceParent`, so GitHub refuses a task
that is the child of another issue. `task move` replaces the parent.

**`task new`**: `task`, `parent`, `url`. **`task unblock`**: `number` (`numero`), `unblocked` (`sbloccati`),
`not_blocking` (`non_bloccanti`, the numbers that did not block it: a warning, not an error), `comment` (the
id of the signed comment with the reason, or null). **`task move`**: `task`, `from` (`da`), `to` (`a`),
`parent`. **`task list`**: a list of issues without `body`. **`task show`**: the issue with `body`, and
`comments`, oldest first: `id`, `created_at` (`creato`), `role` (`firma`), `url`, `body`, `shortened`
(`accorciato`: a worker's body is its first line). `role` is the role the comment's signature carries, and
`titolare` for a comment with none: data, as the markers already published are (`conformita.md` M2, §8.4).

### `plan`

| Command | Prototype | Arguments | Exit |
|---|---|---|---|
| `plan` | `piano` | `--project` or `--root` (`--radice`), one of the two; `--mermaid`; `--max-nodes` (`--max-nodi`, default 25) | 0; 1 |

**Output**: `command` (`comando`), `date` (`data`), `root` (`radice`), `project`, `nodes` (`nodi`: `number`,
`title`, `status`, `project`, `parent`, `unblocks`, `chain`, `cycle`, `external` (`esterno`, one step outside
the perimeter)), `edges` (`archi`, pairs `[blocker, blocked]`), `nested` (`annidati`, edges between a parent and
its child, in the note), `main_line` (`linea`), `inbox_hidden` (`inbox_fuori`), `truncated` (`tagliati`),
`edges_lost` (`archi_persi`), `mermaid` (the diagram, or null). `command` is the prototype's command line, and
`mermaid` quotes it: data, in the prototype's words, until the plan is ported.

### `comment`

| Command | Prototype | Arguments | Exit |
|---|---|---|---|
| `comment <n>` | `commento` | `--body`, `--body-file`, `--on-behalf-of-owner`, `--role` | 0; 1 |

**Output**: `number`, `id` (the comment's id, the `databaseId` the step records; null when the URL `gh`
printed does not end with it), `url`. The comment is signed: a blank line, a visible line, and the marker
`<!-- regia:autore ruolo=<role> sessione=<id> -->` as its last line, in the prototype's words (`conformita.md`
M1). The role is `titolare` with `--on-behalf-of-owner`, then `orchestratore` when this session holds the open
step of the issue's project, then `--role`, then `chat` (M3).

### `setup` and `init`

| Command | Prototype | Arguments | Exit |
|---|---|---|---|
| `setup` | `setup` | | 0; 1 |
| `init`, `init plan` | `init`, `init piano` | `--owner` (GitHub's owner of the tracker, default the user of `gh`), `--repo`, `--project-title` (`--progetto-titolo`) | 0; 1 the plan could not be computed |
| `init derive` | `init ricava` | the same, `--destination` (`--destinazione`), `--overwrite` (`--sovrascrivi`) | 0; 1; 3 a different file at the destination; 4 the tracker is not ready |
| `init apply` | `init applica` | the same, `--dry-run` (`--a-secco`) | 0; 1; 3; 4; 5 a write failed |

`init` alone is `init plan` and takes no option. The prototype's `init` exits with 2 on a wrong argument
(argparse of `inizializza`); through the facade a wrong argument is refused before, with 1.

**`init plan`**: `owner`, `repo`, `project_title`, `ready` (`pronto`: every item of the environment and of the
tracker is present), `items` (`voci`), `gh_calls` (`chiamate_gh`, the calls made, as command lines). **An
item**: `item` (`voce`), `scope` (`ambito`: `environment`, `tracker`, `local`, `manual`), `outcome` (`esito`:
`present`, `missing`, `different`, `error`), `action` (`azione`: `none`, `create`, `update`, `manual`,
`install`), `detail` (`dettaglio`, prose). **`init derive`** adds `outcome` (`written`, `would_write`, `same`,
`different`, `not_ready`), `exit_code` (`esito_codice`) and `destination`. **`init apply`**: `owner`, `repo`,
`project_title`, `dry_run` (`a_secco`), `writes` (`scritture`: `item`, `command`), `config` (`configurazione`:
`destination`, `outcome`, also `after_the_writes`), `ready`, `items`, `gh_calls`, `exit_code` (`esito`, an
integer), and `error` after a failed write.

### The ⚠ names of the tracker commands

The names of `conformita.md` §8.1–§8.3 marked ⚠ for these commands, and the five names of `brief` where §8.3
and ADR 0001 §11 differ. The proposal is what the table holds.

| Prototype | Proposed | Left out | Why |
|---|---|---|---|
| `aspetta_me` | `waiting_on_owner` | `awaiting_owner` (§8.3) | the name of ADR 0001 §11, already in the native brief, the app and their tests; the two say the same |
| `per_progetto` | `by_project` | `per_project` (§8.3) | ADR 0001 §11 and the native brief; `by` reads as a grouping, `per` as a rate |
| `sessione` (brief) | `session` | `needs_session` (§8.3) | ADR 0001 §11 and the native brief. `needs_session` is clearer, and `session` is a session id in `declare`: the strongest case for a change at the gate |
| `obiettivo` (issue) | `objective` | `is_objective` (§8.3) | ADR 0001 §11 and the native brief; in `projects` the same word is the number of the Objective, which the shape keeps apart |
| `ultimo_giro` | `last_round` | `last_cycle` (the table before), `last_tick` | a round of the controller, as `controller run --rounds`; `cycles` already means the graph's cycles in the same brief |
| `giri` (controller budget) | `rounds` | `cycles` (the table before) | the same word as `last_round` and `--rounds` |
| `da_aprire`, `da_riprendere` | `to_open`, `to_resume` | `pending_launches`, `stopped_sessions` | what the controller would open and resume, in its own words |
| `task maturita` | `task maturity` | `task readiness` (§8.1) | `maturity` is the name the issue, the labels' prefix and the values `draft`, `ready` already share |
| `abbozzo` | `draft` | `stub` | the spec is a draft to complete, not a placeholder |
| `--obiettivo` | `--objective` | `--goal` | the English of `tipo:obiettivo`, as the key |
| `--fatto` | `--definition-of-done` | `--done` | the full name of the section it fills; `--done` reads as a flag |
| `--mandato` | `--mandate` | `--scope` | the mandate is a term of the regia; `scope` would mix it with `init`'s `scope` |
| `--default-automatici`, `default_automatici` | `--auto-defaults`, `auto_defaults` | `--auto-choices` | the defaults taken alone after a time |
| `--prodotti`, `nessuno` | `--products`, `none` | `--product-repos` | the repos of the products; `none` translates, so a repo called `none` is never written by mistake |
| `progetto riempi` | `project mirror` | `project fill`, `project sync`, `project backfill` | the field is the mirror of the label (the prototype's `rispecchia_progetto`); `fill` says nothing of what, `sync` reads two-way, `backfill` is the budget policy's `riempimento` |
| `--pausa` | `--pause` | `--delay` | seconds between two writes |
| `gia_giusti`, `senza_etichetta`, `etichetta_ignota` | `already_right`, `unlabeled`, `unknown_label` | `ok`, `no_label`, `foreign_label` | `ok` is a key of other outcomes; `unknown` because the label names no project of the registry |
| `--da` (`task block`, `task unblock`) | `--by` | `--blockers` | `task block 12 --by 3` reads as a sentence |
| `non_bloccanti` | `not_blocking` | `skipped` | the numbers that did not block the task |
| `--per-conto-del-titolare` | `--on-behalf-of-owner` | `--on-behalf-of-user` (§8.2) | "owner" is the owner of the regia everywhere in the product (`waiting_on_owner`); GitHub's owner appears only in `init --owner`, where it is the account of the tracker |
| `--aspetta-me` | `--waiting-on-owner` | `--awaiting-me` (§8.2) | the same words as the brief's `waiting_on_owner`; the agents that read it are not "me" |
| `--dal` (`task show`) | `--since` | `--from` | a lower bound with no end; `moviola export --from` opens a range with `--to` |
| `firma` | `role` | `signature`, `signed_by` | the role the signature carries, the same word `--role` writes |
| `accorciato` | `shortened` | `truncated` | `truncated` is a list of the graph and of the plan |
| `linea` | `main_line` | `critical_path` | the longest drawn path, not the critical path of a schedule |
| `inbox_fuori` | `inbox_hidden` | `inbox_without_edges` | the Inbox tasks left out of the drawing; the note says why |
| `annidati` | `nested` | `hidden_edges` | edges between a parent and a child it contains |
| `comando` (`plan`) | `command`, value as it is | translating the command line | the value is a line of the prototype's CLI, quoted in `mermaid` too: translating it needs a parser of the CLI in the facade; the native plan writes the English one |
| `voce` (`init`) | `item`, values as they are | an enum of the items | the items carry prefixes and names of labels and skills (`etichetta:maturità:pronto`, `skill:regia`): data the table cannot translate. The native `init` (#456, #458) names them: `gh`, `repo`, `project`, `status`, `project_field` for `progetto`, `label:`, `skill:`, `manual:` |
| `ambito`, `esito`, `azione` (`init`) | `scope`, `outcome`, `action` | `area`, `status`, `step` | `status` is the tracker's; `outcome` as the dossier's |
| `esito` (`init apply`), `esito_codice` | `exit_code` | `outcome`, `outcome_code` | in `apply` the key holds the exit code, an integer: the value stays as the prototype gives it (ADR 0001 §11), and the name says what it is |
| `da-scrivere`, `dopo-le-scritture` | `would_write`, `after_the_writes` | `to_write`, `pending` | a dry run's outcome, and a configuration that comes after the tracker's writes |
| `--a-secco` | `--dry-run` | `--simulate` | the name `project set` already has |
| `init ricava` | `init derive` | `init read`, `init config` | it derives the configuration from the tracker that exists |

### Limits of the tracker section

- **Free values in Italian.** Tracker labels (`maturità:pronto`, `progetto:<id>`), the prototype's sentences
  (`detail`, `first_push`, `reason`), `plan.command` and `mermaid`, `init`'s item names and gh calls, and the
  roles of signatures pass as they are: data, not contract (`conformita.md` §8.4).
- **`project new --repo`** creates a private repo and pushes the scaffold when the folder was not a repo
  (`first_push`): a case of it runs only on a folder that is a repo already, so nothing reaches a remote.

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

**Native (#448).** The binary answers `check` itself where the routing says so, with the same keys; the findings'
`text` and `why` and the `warnings` are the prototype's sentences, as through the facade. Two checks came after #442,
and their ids are **new proposals** for #223: `absolute-path` (`percorso-assoluto`, #283 of the dossier: an absolute
path of the machine in `stato.md`, a report or the decisions) and `decisions-over-cap` (`decisioni-oltre-tetto`, #36:
`decisioni.md` past its cap). The thresholds are the prototype's `policies/controlli.yaml` while a dossier is there,
else the product's defaults, the same values; a file that does not read gives the defaults and says why on stderr.
`step finish` runs the same checks, the state file of HEAD against the disk, and its `checks` are no longer
`unavailable` for want of a port.

### `path`

The prototype's `percorso` (#530 of the dossier): where a reference of the dossier is, for an agent that must not
join paths by hand. Read only. **New, with names proposed for #223** (#448).

| Command | Prototype | Arguments | Exit |
|---|---|---|---|
| `path <reference>` | `percorso` | `--project` (the dossier of its registry; without it the working folder, if it has `.regia/`) | 0; 2 with no `--project` outside a dossier; 1 an unknown project |

**Output**: `reference` (`riferimento`), `kind` (`tipo`), `path` (`percorso`, on this machine, or null), `exists`
(`esiste`), `is_folder` (`cartella`: whether what exists is a folder; not `folder`, which names a project's folder
elsewhere), `reason` (`motivo`, the prototype's sentence, or the name of the form of a reference that is no path).
**Kinds** (`reference_kind`): `relative` (`relativo`), `data` (`dati`), `absolute` (`assoluto`), `outside`
(`fuori`), `not_a_path` (`non-percorso`, as #593 names it).

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

**Native, and only with consent (#463).** The binary answers `budget` itself where the routing says so
(`REGIA_ENGINE=rust`, or no prototype), with the same keys. The probes run only if the user said yes at setup:
`probes = "granted"` in `agents.toml`. Without an answer, or with `probes = "refused"`, every provider has the
**new state `probe_off`**, with a `reason`, no `windows` and no `plan`; nothing is read, the cache neither, so
`--cache` adds no `cache` key. Exit 0, as for `unknown`. `probe_off` and the `probes` key are proposals for #223,
like the rest. Where the native output differs from the prototype's: the `reason` sentences are in English; the
credentials follow `CLAUDE_CONFIG_DIR` and `CODEX_HOME` when `REGIA_CLAUDE_CREDENTIALS` and `REGIA_CODEX_AUTH`
are not set; on macOS Claude reads `unknown` without reading its credentials; and the text form, without
`--json`, prints the contract's keys as `key: value` lines.

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
