# ADR 0001: Product architecture

- **Status:** Proposed, 2026-11-02. It waits for the owner's decision: the first product ADR is a mandatory gate of the epic.
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
- The conformance test compares the two implementations through the same facade table the CLI uses (section 11). A bug in the table therefore fails the test.

### 2. Crates

Dependencies point inwards. `regia-core` starts no process and opens no socket; everything external sits behind a port.

| Crate | Role | Status |
|---|---|---|
| `regia-identity` | The product name, and what derives from it | built |
| `regia-core` | Domain rules and ports; reader of the v1 formats | built for the brief |
| `regia-github` | Tracker port, through the user's `gh` | built for reads |
| `regia-i18n` | Language catalogs | built |
| `regia-facade` | The disposable table, and the passthrough to the prototype | built for `brief`, and `projects` by table only |
| `regia-runtime` | Composition root shared by CLI and app | built |
| `regia-cli` | The `regia` binary | built |
| `app/src-tauri` | The Tauri shell: commands, capabilities | built |
| `regia-conformance` | Dev only: harness, fake `gh`, recorder | built |
| `regia-store` | SQLite state (section 8) | planned |
| `regia-git` | Git port, through the `git` CLI (section 4) | planned |
| `regia-agents` | `AgentService` and `LaunchSurface` (section 6) | planned |
| `regia-controller` | The controller engine (section 7) | planned |

The app and the CLI share `regia-runtime`, so they cannot wire adapters differently. Orca, GitHub and provider specifics live only in adapters.

### 3. Product name

"regia" is provisional. The name lives in one constant, `regia_identity::PRODUCT_NAME`, and from it derive the binary name, the app identifier, the configuration folder, the dossiers folder, the state folder and the prefix of the environment variables.

Cargo's `[[bin]] name` and the `productName` and `identifier` of `tauri.conf.json` cannot read a Rust constant, so they repeat the value. A test (`the_product_name_has_one_source`) fails if a copy drifts, and another fails if a Rust or JS source spells the name as a string outside the identity crate. Crate names keep the `regia-` prefix, which ADR 0017 of the dossier allows until the public name is chosen, so renaming the product also means renaming the crates and repeating the search.

While the prototype runs, `REGIA_RUNTIME`, `REGIA_HOME` and the state folder keep their names, because they are the prototype's contract too. The state rule is the prototype's, written out by hand: the `directories` crate returns different folders on Windows and has no state folder on Windows or macOS.

### 4. Git

Git goes through the `git` CLI, behind a port. It behaves as the user's own git does: credential helpers, SSH, hooks, commit signing, worktrees.

### 5. GitHub

The tracker goes through `gh` as a subprocess. The product holds no token, and `gh` needs the `project` scope for Projects v2. The setup checks `gh` and guides the user; it does not bundle it. The app reads a status by the **option id** of the field and resolves it to a canonical id (`inbox`, `proposal`, `approved`, `in_progress`, `decision`, `done`, `discarded`), so a renamed label breaks nothing. The Italian labels of the v1 tracker are used once, at load, to bind each option id. An unknown option id is an error, not a guess.

The OAuth device flow is the second adapter behind the same port, for the public release, for users who do not have `gh`. It needs a registered OAuth App, which is a later gate.

### 6. Agents, and services with a principal (#385)

Two ports, because the prototype already separates the two questions.

- **`AgentService` says which agent:** detect, auth state, login guide, build a launch plan, parse the stream, find the transcript, resume, probe usage, classify the end. Its output is an argv with fixed arguments; a prompt is untrusted text and goes on stdin.
- **`LaunchSurface` says where it runs:** `Cli` (a detached process, with a log), `Orca` (optional, an adapter like any other) and `Dry` (starts nothing, for shadow runs and tests).

Orca is a surface, not a service. A deadline guard lives in the core and applies to any pair, and fallback is a rule the caller applies.

The principal is the service the orchestrator starts. It is chosen at setup and in settings. `policies/models.yaml`, fixed in the prototype's repo today, splits into three layers:

1. **Product defaults**, compiled in: roles with a tier and an effort, without model names (`code = { tier = "standard", effort = "high" }`). A diversity rule names no product: `adversarial_review = { service = "other_than:writer", … }`.
2. **Each service's TOML file:** binaries, auth command, launch template, stream format, tier-to-model map, skill folders.
3. **User settings** in the configuration folder: `principal`, enabled services, per-role overrides, budget thresholds per service.

With only one service, the diversity rule degrades to the same service in a clean context, and the report says so.

**The honest limit of "configurable":** a new service that speaks a known stream format is one TOML file. A new format needs a parser in code. The two services known today are `claude` and `codex`.

**The skill becomes neutral.** It names no product and asks the CLI: `regia agents show --role <role> --json` answers with service, tier, model, effort, surface, deadline and fallback. That command is a new name in the contract, so it passes the gate with the facade. Regenerating the skill's text from the principal was rejected, because it leaves a second copy to keep aligned in two folders.

*Not built:* this whole section. The design comes from reading `tools/lancio.py` and `tools/usage.py` in the prototype.

### 7. Controller in Rust, in the first version

One engine in a library crate, a `Controller` with a `tick()` and injectable clock, sleep, launch and tracker, so the prototype's test cases can be carried over as cases. It has two hosts:

- `regia controller run [--once] [--shadow]` in the CLI: the unit a user service registers, and what shadow runs use;
- a thread inside the app, which starts when the app opens and the lock is free. If another process holds the lock, the app says so and starts nothing.

Closing the window stops the app's controller. A tray comes later, in its own slice.

**Threads, not tokio.** The load is one cycle every 15 minutes, subprocesses, synchronous SQLite and two HTTP calls. The core does not depend on tokio, so the CLI does not compile it. Tauri commands call the core off the main thread.

**Only one controller, across languages.** The only lock two languages can share is the prototype's: the row `(lock, controller)` of the `documenti` table in `regia.db`, taken with `BEGIN IMMEDIATE`, with the same document fields and the same 600-second TTL. The Rust side writes its own monotonic-clock tag, so a Python reader falls back to the wall-clock expiry, which is the behaviour ADR 0008 of the dossier defines for a differing clock. `tauri-plugin-single-instance` only stops two windows, and does not exclude a headless controller or the Python one.

*Not proven, and the largest risk of the transition:* Python and Rust on the same `regia.db` holding that lock. It is the first thing the runtime slice must test.

**Shadow before the gate.** The Rust controller runs with `--shadow` and the `Dry` surface on a consistent copy of the state (`VACUUM INTO`), never on the live file, against recorded tracker answers and a fixed clock, and its decisions are compared with `--una-volta --lancio finto` of the prototype. Then it runs live, read-only, against each fresh copy. The owner sets how many cycles without an unexplained logic divergence are enough.

**Does this change #103? In part.**

| Part of #103 | Outcome |
|---|---|
| Distributing Python and its dependencies; a launcher for `python tools/controller.py` | Falls away |
| A user service with restart on failure (Windows at logon, Linux `systemd --user`) | Stays, targeting `regia controller run`, and gains macOS (`launchd`; *not proven*) |
| The gate on installing a service | Stays |
| The server notes (dedicated user, port not exposed) | Stay |
| Start at login | New, as an explicit setting. `tauri-plugin-autostart` starts the app and does not supervise it, so it is not a service |

The system service stays out of v0.1, as the epic says. A controller that lives in the app covers a personal machine.

### 8. Local storage and keychain

- **State:** `rusqlite` with `bundled`, the same SQLite on three systems and no system dependency.
- **During the transition:** the same `regia.db` and the same `documenti` schema, with the prototype's rules: `BEGIN IMMEDIATE`, rollback journal, `synchronous=FULL`, a new connection per operation, no WAL. New tables take new names, and the existing schema is not altered while the Python runs. Typed tables can wait until `migrate`.
- **Keychain:** nothing goes in it in v0.1, because `gh`, `claude` and `codex` hold their own credentials. `keyring` is therefore not a dependency yet, which also removes the Linux-without-Secret-Service risk from v0.1. A `SecretStore` port waits for the first secret of our own, which is the device-flow token. No credentials in the package, ever.

### 9. IPC between app and core

In-process Tauri commands. There is no sidecar and no local server.

- **Commands and capabilities.** `build.rs` lists the commands with `AppManifest::commands`, which turns on Tauri's ACL for them, and `capabilities/main.json` allows exactly those. The skeleton's webview can call `bootstrap` and `get_brief` and nothing else; it has no `core:default`, no plugin permissions and no remote origin. A test keeps it so.
- **Subprocesses start from Rust, never from a webview permission.** `tauri-plugin-shell` and `tauri-plugin-fs` are forbidden, and a test checks the manifest.
- **Types.** `ts-rs` generates TypeScript from the Rust types, with hand-written command wrappers, from the first UI slice. `tauri-specta` was rejected because it has been in release candidate for years.
- **CSP.** `default-src 'none'`, with `script-src`, `style-src`, `img-src` and `font-src` set to `'self'`, and `connect-src` limited to the IPC. Everything from the tracker reaches the DOM as text. Markdown written by agents becomes a tree built in the core, with no HTML passthrough. The skeleton renders a hostile title on all three systems and checks that no element comes out of it.

*Not built:* `ts-rs` (the skeleton has one small hand-read shape) and the markdown tree.

### 10. Languages

**Project Fluent**, one catalog format for the CLI and the app, in `crates/regia-i18n/locales/<lang>/`. Italian needs genders and elisions, and Fluent handles them through term attributes; ICU and gettext were rejected for that, and JSON for lacking any grammar.

- The core returns codes and parameters, not sentences. The CLI renders them in Rust.
- From the first UI slice the webview renders the same `.ftl` files with `@fluent/bundle` (about 4.4 kB gzipped, measured). The skeleton has no npm, so for now Rust sends it the resolved strings at start. That shortcut cannot do plurals and goes away with the first UI slice.
- Ids use hyphens: Fluent rejects dots. A test requires the same ids in every language; it must grow to attributes and variables.
- The catalog of the UX track maps onto `.ftl` mechanically.

### 11. Facade

The binary `regia`, in English, is the only entry point. A command that is not ported goes to `python tools/regia.py` through the table in `crates/regia-facade/table/facade.toml`. The crate and the table disappear when porting ends.

The table has three properties that a key-for-key rename would not.

- **It walks by shape.** Project ids used as keys of `by_project` are data and stay as they are, even a project called like a key.
- **It fails closed.** An unknown key or enum value is an error, so Italian cannot leak through, and a new key in the prototype's JSON is noticed.
- **Its code is shared.** The passthrough and the harness call the same `translate_output`.

The skeleton carries `brief`, and `projects` in the table. *Not built:* the remaining commands, their arguments and output shapes; that is #223. `REGIA_ENGINE=python` routes `brief` through the facade today, for checks. Translating the machine-local brief sections is tested on hand-written samples only.

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

- **Fixtures are synthetic** and recorded locally with `record`, which refuses to store a URL outside the fixture's fake host. CI cannot read the dossier, and this repo goes to colleagues and later public, so real titles and project names cannot be in it.
- **Isolation.** Each run gets a state folder with only a synthetic `tracker.json`, so the prototype cannot see the machine.
- **Negative controls run in CI:** four alterations of the expected output, one of the tracker's answers, and one unknown key that must fail closed.
- **`live-check`** runs both implementations on the real tracker, locally, and prints only paths and counts.

**What the proof does not cover.** The brief's three machine-local sections (`controller`, `dossier`, `outside_tracker`) are not ported. Synthetic fixtures make them absent, and `live-check` excludes them by name, so an empty diff says nothing about them. The Rust CLI warns on stderr that they are missing. The contract inventory and the v1 format inventory are in the dossier's study, because they describe private material; what a slice needs from them is copied here with the commit.

**The grep of the epic needs a wider exception.** "No Italian identifier outside catalogs and the table" cannot hold as written, because the v1 tracker strings have to live somewhere. They are in `crates/regia-core/formats/tracker_v1.toml`, as data, and the oracle's fixtures are data too. The guard test scans sources against a word list, and the facade's and harness's tests are exempt because they feed the table Italian keys on purpose.

### 13. Frontend for the real screens

Svelte 5, Vite and TypeScript, as a single-page app without SvelteKit, with `bits-ui` for accessible components. It costs the least to write and review, has an accessibility check at compile time, and ships a small bundle (12.7 kB gzipped, against 68.7 kB for React 19, on the same small app, one measurement). React 19 is the fallback, and wins if React Aria's accessible collections turn out to be needed. That agents write Svelte as easily is an assumption, not a measurement. The moviola's renderer is a separate choice, made by its own benchmark, and any of these frameworks can host it.

The skeleton stays static, without npm, to keep CI cheap. The choice applies from the first UI slice and is reversible: changing framework rewrites components, not the IPC contract. *Not built.*

### 14. Security

The threat model is in the dossier's study, with countermeasures mapped to Tauri 2 capabilities. Tauri must be at least 2.11.6, for two recent IPC advisories; the workspace is on 2.12.5.

**The app cannot fix the most serious threat, because it sits outside the app.** Workers run unattended with their permission prompts disabled, so an injected prompt in an issue or a web page can reach whatever the user's shell can. No capability limits that. The proposal is that on colleagues' machines the controller stays off until they consent explicitly. The owner decides this.

### 15. Release

The release study is in the dossier. In short:

- **Channel:** one `vX.Y.Z` tag for CLI and app. CI makes a draft release and the owner publishes it after a clean-machine test.
- **Installers:** NSIS per user on Windows, a universal bundle on macOS, AppImage and deb on Linux, built in an Ubuntu 22.04 container for a low glibc floor.
- **Signing:** none in v0.1, so colleagues pass SmartScreen and Gatekeeper by hand, with written steps. Later spending goes through a gate: Apple is 99 USD a year; a standard certificate is about 150–300 USD a year (indicative); Microsoft's Artifact Signing is not available to an individual in Italy.
- **Updates:** `tauri-plugin-updater`. Private assets need authentication and the package holds no token, so the app reads one at runtime with `gh auth token` and passes it as a header. *Not proven end to end.*
- **Keys and access:** collaborators on a personal private repo always have write access, so any colleague could read the update-signing key from the repo's secrets. The owner must decide this before the first invitation.
- **CI quota:** the Free plan's allowance is 12 USD a month, at 0.006 USD a minute on Linux, 0.010 on Windows and 0.062 on macOS. Pushes to `main` that touch the build run Linux and Windows; the full matrix runs by hand and, once it exists, at a tag.

### Answers to "Open for #155" in the dossier's ADR 0017

| Open point | Answer |
|---|---|
| Language catalog | Project Fluent, section 10 |
| Facade in the crates, table format | `regia-facade`, a shape-walking TOML table, section 11 |
| Name constant | `regia_identity::PRODUCT_NAME`, plus consistency tests, section 3 |
| Harness | Translates the recorded Python output in the test, through the CLI's own table, section 12 |
| Format v2 | The complete v1 inventory and the map to v2 names are in the dossier's study (`inventario-formati-v1.md`). `migrate` stays with its sub-issue |
| Skill in the dossier | After the move, `skills/` keeps a short pointer to the product; the Italian text stays in git history. Nothing changes before the gate |
| Owner's policies | Product defaults compiled in; the owner's choices in user settings, in the configuration folder, outside the binary and this repo. While the Python runs, `policies/` of the dossier stays authoritative |

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
| `keyring` in v0.1 | Nothing to store yet; adds a Linux failure mode for nothing |
| `product.toml` with a generated constant | One more build step, and Cargo and Tauri still repeat the name, so the test is needed anyway |
| Generating manifests from the name | A machine to maintain for an event that happens once |
| A flat key-for-key rename table | Corrupts data used as keys, and lets unknown Italian through |
| Translating the Python output at recording time | CI would never exercise the table |
| Fixtures from a snapshot of the real tracker, anonymized | Real structure and leftovers in a repo that goes public; `live-check` covers real shape without storing it |
| Monkeypatching the prototype's `graphql` in the recorder | The real entry point stays untested, and the fake `gh` also exercises the Rust adapter |
| Fluent strings only through IPC, or only in the webview | The first has no plurals; the second leaves the CLI out |
| gettext, ICU MessageFormat, JSON catalogs | See section 10 |
| `tauri-specta` | Still a release candidate |
| Tauri 3 | Alpha only |
| React 19, Solid, Vue, no framework | Section 13; React is the fallback, the others offer no decisive gain, and no framework leaves accessible components to us |

## Proof

Run by the skeleton on 2026-11-02. Links to CI runs are in the worker's report in the dossier.

- **Conformance:** empty diff on four recorded cases, on Windows, Linux and macOS.
- **Live:** an empty diff on the real tracker, with `outside_tracker` excluded by name.
- **App:** the real Tauri window, started on a fixture in English and in Italian, renders the same tasks in the same order as the CLI, shows the catalog's headings, and renders a hostile title as text.

**Not measured yet:** installer size (the skeleton does not bundle), memory at rest and cold start on the product. Targets belong to the study's synthesis and should be set on measurements of this skeleton, because no reliable published numbers were found.

## Consequences

- **Risks to test first:** Python and Rust on one `regia.db`; update downloads from private releases; everything on macOS beyond a CI runner, since the owner is assumed to have no Mac.
- **The prototype can move under us.** A slice freezes its contract and re-records. `RECORDED.json` names the oracle's commit.
- **Corrections for the epic:** its text cites Tauri 2.11.5, which is out of date; the current minimum Rust is 1.90. Its grep needs the exception in section 12. Its mention of Windows Sandbox does not apply to Windows 11 Home.
- **Gates that stay:** command names and JSON, each slice going to production, the skill's move to English together with the facade, the public name, spending, public releases, installing services.
