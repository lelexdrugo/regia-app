# ADR 0001: Product architecture

- **Status:** Accepted, 2026-11-02, by the owner's decision on task #396 of the dossier, with its two choices: the controller is off by default on every installation until the user consents (section 14), and usage probes run only with the user's consent at setup, with no probe at all if the user refuses (section 6). The renderer of the moviola (section 13) comes from the benchmark of task #398; the owner ratifies it at the gate of the study's synthesis (#400).
- **Scope:** the desktop app, the CLI and the core of the product in this repo. Nothing here is in production, and this ADR moves nothing to production.
- **Evidence:** the walking skeleton in this repo, run on Windows, Linux and macOS (see "Proof"). Claims that were read but not run are marked *not proven*. Claims about what the skeleton does not do yet are marked *not built*.

## Context

The product exists as a Python prototype: about 13 000 lines and a live controller, kept in a private repo called the *dossier*. It works, and it stays the oracle. The product does the same work with mature choices, and it has to reach people who are not its author. It installs on a clean machine, with a guided setup, a control room and a read-only timeline (the *moviola*). Agents keep working through a CLI.

Four facts drive the design.

- **The prototype cannot stop.** Its controller is running, and the two implementations will coexist for months.
- **The CLI is a contract.** Agents call it, so command names and JSON are a contract, and changing them is a gate.
- **The contract moves to English once**, through a facade over the prototype. The prototype itself is not touched.
- **The product's users are not the owner.** They choose their own agent services, and may have no Orca and no Mac.

## Decision

### 1. A rewrite proves itself first

The order is oracle, conformance, slices with a gate, then release. The UI comes last.

- Every slice carries a function from prototype to product (core, CLI, app) with its conformance test, on a frozen contract, and goes to production through a gate.
- The conformance test compares the two implementations through the same facade table the CLI uses (section 11). A bug in the shapes the fixtures exercise therefore fails the test. Today those are the brief's tracker sections; the machine-local shapes have only unit tests.

### 2. Crates

Dependencies point inwards. `regia-core` starts no process and opens no socket; everything external sits behind a port.

| Crate | Role | Status |
|---|---|---|
| `regia-identity` | The product name, and what derives from it | built |
| `regia-core` | Domain rules and ports; reader of the v1 formats | built for the brief |
| `regia-github` | Tracker port, through the user's `gh` | built for reads |
| `regia-i18n` | Language catalogs | built |
| `regia-facade` | The disposable table, and the passthrough to the prototype | built for `brief` |
| `regia-runtime` | Composition root shared by CLI and app | built |
| `regia-cli` | The `regia` binary | built |
| `app/src-tauri` | The Tauri shell: commands, capabilities | built |
| `regia-conformance` | Dev only: harness, fake `gh`, recorder | built |
| `regia-exec` | The only module that starts a process, as the threat model requires: a closed list of programs, typed arguments, an environment allow-list | planned |
| `regia-store` | SQLite state (section 8) | planned |
| `regia-git` | Git port, through the `git` CLI (section 4) | planned |
| `regia-agents` | `AgentService` and `LaunchSurface` (section 6) | planned |
| `regia-controller` | The controller engine (section 7) | planned |

The app and the CLI share `regia-runtime`, so they cannot wire adapters differently. Orca, GitHub and provider specifics live only in adapters.

### 3. Product name

"regia" is provisional. The name lives in one constant, `regia_identity::PRODUCT_NAME`, and from it derive the binary name, the app identifier, the configuration folder, the dossiers folder and the prefix of the product's own environment variables.

Cargo's `[[bin]] name` and the `productName` and `identifier` of `tauri.conf.json` cannot read a Rust constant, so they repeat the value. A test (`the_product_name_has_one_source`) fails if a copy drifts, and another fails if a source file spells the name as an exact string outside the identity crate. That second test is a narrow guard: it does not catch the name inside a longer string. Crate names keep the `regia-` prefix, which ADR 0017 of the dossier allows until the public name is chosen, so renaming the product also means renaming the crates and searching for the bare word by hand.

**The state folder, `REGIA_RUNTIME` and `REGIA_HOME` do not follow the constant.** They are the prototype's contract too, and a rename during coexistence would otherwise open a second `regia.db`, where the shared lock silently stops excluding the Python controller. They hang off a separate `PROTOTYPE_NAME`, pinned by a test, which goes away with the prototype. The state rule is the prototype's, written out by hand: the `directories` crate returns different folders on Windows and has no state folder on Windows or macOS.

### 4. Git

Git goes through the `git` CLI, behind a port. It behaves as the user's own git does: credential helpers, SSH, hooks, commit signing, worktrees.

### 5. GitHub

The tracker goes through `gh` as a subprocess. The product holds no token, and `gh` needs the `project` scope for Projects v2. The setup checks `gh` and guides the user; it does not bundle it. The app finds the status field by its **field id** and reads the value's **option id**, which it resolves to a canonical id (`inbox`, `proposal`, `approved`, `in_progress`, `decision`, `done`, `discarded`). So neither a renamed label nor a field named in another language breaks anything, unlike the prototype, which reads the field by the name `Status`. The Italian labels of the v1 tracker are used once, at load, to bind each option id. An unknown option id is an error, and so is a Project whose items have no value for the configured field: an empty brief there would be a false all-clear.

A finder-launched macOS app gets a minimal `PATH`, which may not contain `gh`. *Not proven;* the setup slice must resolve it.

The OAuth device flow is the second adapter behind the same port, for the public release, for users who do not have `gh`. It needs a registered OAuth App, which is a later gate.

### 6. Agents, and services with a principal (#385)

Two ports, because the prototype already separates the two questions.

- **`AgentService` says which agent:** detect, auth state, login guide, describe a launch, resume, probe usage, classify the end, wake an expired token, and scrub the child's environment, so that a controller started under another agent does not bind its children to that session.
- **`LaunchSurface` says where it runs:** `Cli` (a detached process, with a log, its argv with fixed arguments and the prompt on stdin), `Orca` (optional, an adapter like any other) and `Dry` (starts nothing, for shadow runs and tests).

The two do not meet at an argv, because the prototype's Orca path takes the prompt as an argument and an agent, a model and an effort, has no stream, and finds its session by a marker. So the service produces a neutral launch request and each surface consumes it; stream parsing and transcript lookup belong to the `Cli` surface.

Orca is a surface, not a service. A deadline guard lives in the core and applies to any pair. Fallback is a rule the caller applies, and it is legal only before any effect: a surface returns a typed `FailedBeforeEffect`, so a caller cannot launch twice.

The principal is the service the orchestrator starts. It is chosen at setup and in settings. `policies/models.yaml`, fixed in the prototype's repo today, splits into three layers:

1. **Product defaults**, compiled in: roles with a tier and an effort, without model names (`code = { tier = "standard", effort = "high" }`). A diversity rule names no product: `adversarial_review = { service = "other_than:writer", … }`.
2. **Each service's TOML file:** binaries, auth command, launch template, stream format, tier-to-model map, skill folders.
3. **User settings** in the configuration folder: `principal`, enabled services, per-role overrides, budget thresholds per service.

With only one service, the diversity rule degrades to the same service in a clean context, and the report says so.

**The honest limit of "configurable":** only a CLI that shares a known stream format, transcript rule and usage format is one TOML file. Anything else needs code: today each service has its own argv shape, session-id extraction, transcript rule and usage parsing. The two services known are `claude` and `codex`.

**Usage probes read the tools' OAuth tokens.** The prototype reads Claude's and Codex's access tokens from their files into its process and sends them to two undocumented endpoints, and the `probe usage` port inherits that. It means handling credentials we said we do not hold, on endpoints that can change without notice; and where macOS keeps Claude's credentials is *not proven*, so there the probe may answer "unknown".

**The owner's decision: probes run only with the user's consent at setup, and a user who refuses gets no probe at all.** Nothing then reads the tokens. That state has to be distinct from "unknown": in the prototype an unknown budget sends every candidate to `idle/budget` and keeps waking the token, so a refused probe would leave a controller that never launches. The budget slice defines how the controller behaves with no probe; the dossier's plan proposes the default.

**The skill becomes neutral.** It names no product and asks the CLI: `regia agents show --role <role> --json` answers with service, tier, model, effort, surface, deadline and fallback. That command is a new name in the contract, so it passes the gate with the facade. Regenerating the skill's text from the principal was rejected, because it leaves a second copy to keep aligned in two folders.

*Not built:* this whole section. The design comes from reading `tools/lancio.py` and `tools/usage.py` in the prototype.

### 7. Controller in Rust, in the first version

**This reverses a premise of the dossier's ADR 0017,** which assumed controller, step and launch stay in Python beyond v0.1. The source is the owner's decision of 2 November, in the epic's mandate. The conformance study written before it (`conformita.md`) says the controller does not change in v0.1, and carries a note saying so.

One engine in a library crate, a `Controller` with a `tick()` and injectable clock, sleep, launch and tracker, so the prototype's test cases can be carried over as cases. It has two hosts:

- `regia controller run [--once] [--shadow]` in the CLI: the unit a user service registers, and what shadow runs use;
- a thread inside the app, **only after the user has switched it on** (section 14). If another process holds the lock, the app says so and starts nothing.

Closing the window stops the app's controller. A tray comes later, in its own slice.

**Threads, not tokio.** The load is one cycle every 15 minutes, subprocesses, synchronous SQLite and two HTTP calls. The core does not depend on tokio, so the CLI does not compile it. Tauri commands call the core off the main thread.

**Only one controller, across languages.** The only lock two languages can share is the prototype's: the row `(lock, controller)` of the `documenti` table in `regia.db`, taken with `BEGIN IMMEDIATE`, with the same document fields and the same 600-second TTL. `tauri-plugin-single-instance` only stops two windows, and does not exclude a headless controller or the Python one.

- **Clocks.** `std::time::Instant` cannot be serialized, so a Rust monotonic tag needs per-OS calls. The Rust side writes a tag of its own and always writes the wall-clock expiry; each side reads the other's lock by the wall clock, which is what ADR 0008 of the dossier defines for a differing tag. The known cost returns for this one case: a forward clock jump over 600 s lets either side steal a live lock.
- **Keys.** The database documents and the JSON of `lock`, `lease` and `step` share their Italian keys. The table cannot apply inside the core, so `regia-store` needs a v1 codec with those keys as data, or the Python stops reading the rows.
- **Not only the database.** The state also has `controller-stato.json`, `da-riprendere.json` and `politica-locale.json`, which the Python brief and controller read. The transition contract covers them, by the inventory in the dossier.

*Not proven, and the largest risk of the transition:* Python and Rust on the same `regia.db` holding that lock. It is the first thing the runtime slice must test. **Plan B if it fails:** `lock`, `lease` and `step` stay a Python subprocess for as long as the Python controller exists. The risk and plan B concern only the owner's machine: a colleague's machine has no Python, so nothing there shares the lock and `regia-store` works alone.

**Three defects the Rust controller does not inherit.** A review of the prototype's controller (`0001-review-codex.md`, round 2) found them, as static counterexamples that were not executed. The dossier fixes the Python separately; the fix is not a condition of this section.

- **D1, launching after losing the lock.** The lock is checked again right before every launch, at every launch point: a step, the token wake-up, a chat, the chronicler, a resume. The check sits outside the launch's own error handling, so a lost lock cannot turn into a launch error.
- **D2, losing step records.** The step registry is loaded after the lock is taken, never in a constructor, and saved record by record with a version check, so it cannot overwrite records it did not load.
- **D3, the cursor of `step start`.** The cursor is written in the same transaction that checks the lock's owner, as `step end` already does.

The engine's tests cover all three with an injected clock, and the prototype's own tests serve as an oracle if the dossier's fix lands.

**Shadow before the gate.** The Rust controller runs with `--shadow` and the `Dry` surface on a copy, never on live files: the whole state folder, with the database through `VACUUM INTO`. The comparison needs a driver, because the prototype's CLI cannot do it as it stands.

- The copied lock row is held by the live controller, so the oracle would exit at once: the copy drops that row.
- `--lancio finto` does not cover everything: resuming suspended sessions needs `--no-ripresa`, and the moviola calls need a stub.
- The CLI has no clock option, so a small driver outside `tools/` calls the prototype's hooks with a fixed clock and recorded tracker answers.

Then the Rust controller runs live, read-only, against each fresh copy. The owner sets how many cycles without an unexplained logic divergence are enough. *Not built, not proven.*

**Does this change #103? In part.**

| Part of #103 | Outcome |
|---|---|
| Distributing Python and its dependencies | Already absorbed by the epic, independently of this choice |
| A launcher for `python tools/controller.py` | Falls away |
| Dependencies on `gh`, `claude`, `codex` | Stay |
| A user service with restart on failure (Windows at logon, Linux `systemd --user`) | Stays, targeting `regia controller run`, and gains macOS (`launchd`; *not proven*) |
| `regia install` and `uninstall` | Stay, under a name that passes the contract gate (for example `regia controller install`) |
| The gate on installing a service | Stays |
| The server notes (dedicated user, port not exposed) | Stay |
| Start at login | New, as an explicit setting. `tauri-plugin-autostart` starts the app and does not supervise it, so it is not a service |

The system service stays out of v0.1, as the epic says. A controller that lives in the app covers a personal machine.

### 8. Local storage and keychain

- **State:** `rusqlite` with `bundled`, the same SQLite on three systems and no system dependency.
- **During the transition:** the same `regia.db` and the same `documenti` schema, with the prototype's rules: `BEGIN IMMEDIATE`, rollback journal, `synchronous=FULL`, a new connection per operation, no WAL. New tables take new names, and the existing schema is not altered while the Python runs. Typed tables can wait until `migrate`.
- **Keychain:** nothing goes in it in v0.1, because `gh`, `claude` and `codex` hold their own credentials (though see section 6 on usage probes, which read them). `keyring` is therefore not a dependency yet, which also removes the Linux-without-Secret-Service risk from v0.1. A `SecretStore` port waits for the first secret of our own, which is the device-flow token. No credentials in the package, ever.

### 9. IPC between app and core

In-process Tauri commands. There is no sidecar and no local server.

- **Commands and capabilities.** `build.rs` lists the commands with `AppManifest::commands`, which turns on Tauri's ACL for them, and `capabilities/main.json` allows exactly those. The skeleton's webview can call `bootstrap` and `get_brief` and nothing else; it has no `core:default`, no plugin permissions and no remote origin. A test keeps it so.
- **Subprocesses start from Rust, never from a webview permission.** `tauri-plugin-shell` and `tauri-plugin-fs` are forbidden, and a test checks the lockfile, so a plugin arriving through another crate counts.
- **Types.** `ts-rs` generates TypeScript from the Rust types, with hand-written command wrappers, from the first UI slice. `tauri-specta` was rejected because it has been in release candidate for years.
- **CSP.** `default-src 'none'`, with `script-src`, `style-src`, `img-src` and `font-src` set to `'self'`, and `connect-src` limited to the IPC; a test requires this exact string. Everything from the tracker reaches the DOM as text. Markdown written by agents becomes a tree built in the core, with no HTML passthrough. The skeleton renders a hostile title on all three systems and checks that no element comes out of it.

*Not built:* `ts-rs` (the skeleton has one small hand-read shape) and the markdown tree.

### 10. Languages

**Project Fluent**, one catalog format for the CLI and the app, in `crates/regia-i18n/locales/<lang>/`. Italian needs genders and elisions, and Fluent handles them through term attributes; ICU and gettext were rejected for that, and JSON for lacking any grammar.

- The core will return codes and parameters, not sentences, and the CLI renders them in Rust. *Not built:* its errors are English sentences today.
- From the first UI slice the webview renders the same `.ftl` files with `@fluent/bundle` (about 4.4 kB gzipped, measured). The skeleton has no npm, so for now Rust sends it the resolved strings at start. That shortcut cannot do plurals and goes away with the first UI slice.
- Ids use hyphens: Fluent rejects dots. A test requires the same ids in every language; it must grow to attributes and variables.
- The catalog of the UX track maps onto `.ftl` mechanically.

### 11. Facade

The binary `regia`, in English, is the only entry point. A command that is not ported goes to `python tools/regia.py` through the table in `crates/regia-facade/table/facade.toml`. The crate and the table disappear when porting ends.

The passthrough exists only on the owner's machine. Colleagues enter this repo and never the dossier, and the prototype is never published, so on their machines an unported command has nowhere to go: it fails with a clear error. v0.1 therefore ships every command of its contract in Rust.

The table has three properties that a key-for-key rename would not.

- **It walks by shape.** Project ids used as keys of `by_project` are data and stay as they are, even a project called like a key.
- **It fails closed on keys and listed enums.** An unknown key or value there is an error, so a new key in the prototype's JSON is noticed. Other values pass as they are: tracker labels (`maturità:pronto`) and the prototype's prose stay in its language, as data. A key that goes missing is caught by the conformance diff, not by the table.
- **Its code is shared.** The passthrough and the harness call the same `translate_output`.

The skeleton carries `brief` only. *Not built:* the remaining commands, their arguments and output shapes; that is #223. `REGIA_ENGINE=python` routes `brief` through the facade today, for checks. Translating the machine-local brief sections is tested on hand-written samples only. `projects` needs a design of its own: its registry is a free block in the Objective's body, so one new line there would break a closed shape for every project.

Until a section is ported and its slice has passed the gate, the agents' `brief` goes through the facade. The native brief says what it lacks with a `not_ported` key, because absence means "nothing to report" in this contract and would read as an all-clear when the controller has an alarm.

Four conventions, answering the conformance study's open questions:

- **Names:** `snake_case` everywhere, GitHub's fields too. **Statuses:** canonical ids in the contract; labels only in catalogs.
- **The prototype's inconsistencies** (`esito` as a string or an integer, exit 3 meaning both idle and refused) are reproduced by the facade, so the diff stays honest. A slice may correct one only by passing the contract gate.
- **Line endings:** the Rust side writes LF. The Python readers accept both, and the harness compares parsed values.
- **Text output** is not a contract: agents read `--json` and exit codes. It goes to the catalog. Each slice checks this against the skill for its commands.

**The names are a contract, so approving this ADR does not approve them for production;** that gate stays with the facade going to production. The names the skeleton proposes for `brief --json`:

| Python | English | Python | English |
|---|---|---|---|
| `aspetta_me` | `waiting_on_owner` | `stato` | `status` |
| `pronti` | `ready` | `progetto` | `project` |
| `in_corso` | `in_progress` | `maturita` | `maturity` (`draft`, `ready`) |
| `per_progetto` | `by_project` | `obiettivo` | `objective` |
| `sessione` | `session` | `sub` | `sub_issues` |
| `grafo` | `graph` (`cycles`, `truncated`) | `bloccato_da` | `blocked_by` |
| `sblocca` | `unblocks` | `bloccato_da_totale` | `blocked_by_total` |
| `catena` | `chain` | `stateReason`, `updatedAt`… | `state_reason`, `updated_at`… |
| `ciclo` | `cycle` | a task with no status, `—` | `unset` |

### 12. Conformance

A case in `fixtures/brief/cases/` holds what the tracker answered and what the prototype printed, in Italian and untranslated. The test runs the real `regia` binary against those answers through a fake `gh` (a real executable, placed first on `PATH`), passes the recorded output through the table, and requires an empty diff. Object key order does not count; array order does, because the order of the sections is the logic under test.

The fake behaves like a server: it returns only the fields the query selects, errors on an unknown field, and checks variables and the `states` and `orderBy` arguments. A query that drops `optionId` therefore fails in CI. It is still a fixture: field names real GitHub would refuse are caught only by `live-check`, which is run by hand.

- **Fixtures are synthetic** and recorded locally with `record`, which refuses to store a URL outside the fixture's fake host. CI cannot read the dossier, and this repo goes to colleagues and later public, so real titles and project names cannot be in it.
- **Isolation.** Each run gets a state folder with only a synthetic `tracker.json`, so the prototype cannot see the machine.
- **Negative controls run in CI:** four alterations of the expected output, one of the tracker's answers, and one unknown key that must fail closed.
- **`live-check`** runs both implementations on the real tracker, locally, and prints only paths and counts.

**What the proof does not cover.** The brief's three machine-local sections (`controller`, `dossier`, `outside_tracker`) are not ported. Synthetic fixtures make them absent, and `live-check` excludes them by the `not_ported` declaration, so an empty diff says nothing about them. Human text output is outside the proof too: every case uses `--json`, and the text differs from the prototype's (no graph suffixes, for one). The contract inventory and the v1 format inventory are in the dossier's study, because they describe private material; what a slice needs from them is copied here with the commit.

**The grep of the epic needs a wider exception.** "No Italian identifier outside catalogs and the table" cannot hold as written, because the v1 tracker strings have to live somewhere. They are in `crates/regia-core/formats/tracker_v1.toml`, as data, and the oracle's fixtures are data too. The guard test scans sources against a word list, and the facade's and harness's tests are exempt because they feed the table Italian keys on purpose.

### 13. Frontend for the real screens

Svelte 5, Vite and TypeScript, as a single-page app without SvelteKit, with `bits-ui` for accessible components. It costs the least to write and review, has an accessibility check at compile time, and ships a small bundle (12.7 kB gzipped, against 68.7 kB for React 19, on the same small app, one measurement). React 19 is the fallback, and wins if React Aria's accessible collections turn out to be needed. That agents write Svelte as easily is an assumption, not a measurement. The moviola's renderer is a separate choice, made by its own benchmark, and any of these frameworks can host it. That benchmark (task #398 of the dossier, `docs/app/moviola/scelta-renderer.md`) chose hand-written Canvas 2D with no library, and a level of detail still to design. It holds in all three engines and with no GPU, where WebGL2 is twice as slow. Hand-written WebGL2 is the next step if crowded views or effects demand it, with Canvas 2D as its required fallback. A physical Mac and native Linux with a GPU are *not measured*.

The skeleton stays static, without npm, to keep CI cheap. The choice applies from the first UI slice and is reversible: changing framework rewrites components, not the IPC contract. *Not built.*

### 14. Security

The threat model is in the dossier's study, with countermeasures mapped to Tauri 2 capabilities. Tauri must be at least 2.11.6, for two recent IPC advisories; the workspace is on 2.12.5.

**The app cannot fix the most serious threat, because it sits outside the app.** Workers run unattended with their permission prompts disabled, so an injected prompt in an issue or a web page can reach whatever the user's shell can. No capability limits that. **By the owner's decision, the controller is off by default on every installation, and starts only after explicit consent.** Usage probes likewise run only with consent at setup (section 6).

*Not built,* and required by the threat model for the first UI slice: `withGlobalTauri` off (the skeleton needs it on because it has no npm), the window built from Rust with navigation handlers, the isolation pattern (to evaluate there), `cargo audit` and `cargo deny` in CI. Actions are already pinned to commits.

### 15. Release

The release study is in the dossier. In short:

- **Channel:** one `vX.Y.Z` tag for CLI and app. CI makes a draft release and the owner publishes it after a clean-machine test.
- **Installers:** NSIS per user on Windows, a universal bundle on macOS, AppImage and deb on Linux, built in an Ubuntu 22.04 container for a low glibc floor.
- **Signing:** none in v0.1, so colleagues pass SmartScreen and Gatekeeper by hand, with written steps. Later spending goes through a gate: Apple is 99 USD a year; a standard certificate is about 150–300 USD a year (indicative); Microsoft's Artifact Signing is not available to an individual in Italy.
- **Updates:** `tauri-plugin-updater`. Private assets need authentication and the package holds no token, so the app reads one at runtime with `gh auth token` and passes it as a header. *Not proven end to end.*
- **Keys and access:** collaborators on a personal private repo always have write access, so any colleague could read the update-signing key from the repo's secrets. The owner must decide this before the first invitation.
- **CI quota:** the Free plan's allowance is 12 USD a month, at 0.006 USD a minute on Linux, 0.010 on Windows and 0.062 on macOS. Pushes to `main` that touch the build run Linux and Windows; the full matrix runs by hand and, once it exists, at a tag. The MSRV (1.90) and the release profile are not built by CI yet, and neither are pull requests, which nobody opens so far.

### Answers to "Open for #155" in the dossier's ADR 0017

| Open point | Answer |
|---|---|
| Language catalog | Project Fluent, section 10 |
| Facade in the crates, table format | `regia-facade`, a shape-walking TOML table, section 11 |
| Name constant | `regia_identity::PRODUCT_NAME`, plus consistency tests, section 3 |
| Harness | Translates the recorded Python output in the test, through the CLI's own table, section 12 |
| Format v2 | The complete v1 inventory is in the dossier's study (`inventario-formati-v1.md`). Its map to v2 names is a proposal with open rows; `migrate` and those rows stay with their sub-issue |
| Skill in the dossier | After the move, `skills/` keeps a short pointer to the product; the Italian text stays in git history. Nothing changes before the gate |
| Owner's policies | Product defaults compiled in; the owner's choices in user settings, in the configuration folder, outside the binary and this repo. While the Python runs, `policies/` of the dossier stays authoritative. `models.yaml` splits as in section 6. `budget.yaml` and `controlli.yaml` follow the same pattern: defaults plus user thresholds, *not designed in detail*. **`gates.yaml` is different: a user setting can add a gate but never remove one of the product's** |

## Alternatives rejected

| Alternative | Why not |
|---|---|
| A Python sidecar | It is the pact with the past; the epic excludes it, and the prototype is never published |
| Electron | Excluded by the epic; a heavy runtime for a tool that lives beside other tools |
| An app server on localhost instead of Tauri IPC | Opens a port and an auth problem for no gain |
| The core as async on tokio | No concurrent load justifies it, and it would make the CLI compile tokio |
| `gix` for git | No push, hooks or signing today |
| `git2` for git | Does not run hooks, and brings libgit2 and OpenSSL to three systems |
| A token read with `gh auth token`, then direct HTTP | The token enters our process for every call, for no gain over the subprocess; kept only for update downloads |
| A GitHub App | Its documentation offers no Projects permission for a personal account |
| The OAuth device flow as the first adapter | Needs a registered app before anyone has asked for it; it comes second |
| A controller only in the app | Dies with the window, cannot run on a server, cannot be tested headless |
| A controller only as a headless command | Asks a v0.1 user to run a second process |
| A separate `regia.db` during the transition | The lock could not exclude the Python controller, and cursors would fork |
| `lock`, `lease` and `step` as a Python subprocess during the transition | Rejected as the first choice, because it keeps Python in the product's hot path; kept as plan B (section 7) |
| A closed table shape for `projects` | Its registry is a free block, so one new line would break the command for every project |
| Reading the status field by its name, as the prototype does | Breaks with a localized or renamed field |
| `keyring` in v0.1 | Nothing to store yet; adds a Linux failure mode for nothing |
| `product.toml` with a generated constant | One more build step, and Cargo and Tauri still repeat the name, so the test is needed anyway |
| Generating manifests from the name | A machine to maintain for an event that happens once |
| A flat key-for-key rename table | Corrupts data used as keys, and lets unknown Italian through |
| Translating the Python output at recording time | CI would never exercise the table |
| Fixtures from a snapshot of the real tracker, anonymized | Real structure and leftovers in a repo that goes public; `live-check` covers real shape without storing it |
| Monkeypatching the prototype's `graphql` in the recorder | The real entry point stays untested, and the fake `gh` also exercises the Rust adapter, up to what a fixture can show (section 12) |
| Fluent strings only through IPC, or only in the webview | The first has no plurals; the second leaves the CLI out |
| gettext, ICU MessageFormat, JSON catalogs | See section 10 |
| `tauri-specta` | Still a release candidate |
| Tauri 3 | Alpha only |
| React 19, Solid, Vue, no framework | Section 13; React is the fallback, the others offer no decisive gain, and no framework leaves accessible components to us |

## Proof

Run by the skeleton on 2026-11-02. CI run links and logs are in the worker's report in the dossier, with the live output in `docs/app/architettura/prove/live-check.log`.

- **Conformance:** empty diff on four recorded cases, on Windows, Linux and macOS.
- **Live, by hand, on Windows:** an empty diff on the real tracker, with `outside_tracker` excluded by the `not_ported` declaration.
- **App:** the debug build of the real Tauri window, started from a shell on a fixture in English and in Italian, renders the same waiting, ready and in-progress tasks in the same order as the CLI, shows the catalog's headings, and renders a hostile title as text.

**Measured afterwards, on Windows only, by the study's synthesis:** installer size, memory at rest and start-up, with the targets set on those numbers, because no reliable published ones were found. They are in `docs/app/piano/traguardi.md` of the dossier. Linux and macOS have no numbers yet.

## Consequences

- **Risks to test first:** Python and Rust on one `regia.db`; update downloads from private releases; everything on macOS beyond a CI runner, since the owner is assumed to have no Mac; the app launched as a real GUI program, in a release build, not from a shell.
- **The prototype can move under us.** A slice freezes its contract and re-records. `RECORDED.json` names the oracle's commit.
- **Corrections for the epic:** its text cites Tauri 2.11.5, which is out of date; the current minimum Rust is 1.90. Its grep needs the exception in section 12. Its mention of Windows Sandbox does not apply to Windows 11 Home.
- **Gates that stay:** command names and JSON, each slice going to production, the skill's move to English together with the facade, the public name, spending, public releases, installing services.
