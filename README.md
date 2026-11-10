# regia

A control room for projects worked on by coding agents: what waits for you, what is ready, what is running. "regia" is a provisional name.

This is a walking skeleton. The architecture is accepted in [docs/adr/0001-product-architecture.md](docs/adr/0001-product-architecture.md), and nothing here is in production.

## Layout

| Path | What it is |
|---|---|
| `crates/regia-identity` | The product name in one constant, and what derives from it |
| `crates/regia-core` | Domain model and rules, with ports. Starts no process, opens no socket |
| `crates/regia-github` | Tracker adapter through the user's `gh` |
| `crates/regia-i18n` | The language catalog (Project Fluent) for the CLI and the app |
| `crates/regia-facade` | The disposable table between the English contract and the Python prototype |
| `crates/regia-runtime` | The composition root shared by the CLI and the app |
| `crates/regia-cli` | The `regia` binary, the agents' interface |
| `crates/regia-conformance` | Dev only: the harness that compares the two implementations |
| `app/` | The Tauri 2 app and its static frontend |
| `fixtures/brief/` | Synthetic scenarios and what the prototype answered for them |

## Build and test

Rust 1.90 or later. On Linux, the [Tauri system packages](https://v2.tauri.app/start/prerequisites/) too.

```sh
cargo build --workspace
cargo test --workspace
regia brief --json            # needs an authenticated gh and a tracker.json
```

The conformance test needs no network, no credentials and no Python: it runs on committed fixtures. To watch the app render the same brief as the CLI, in each language:

```sh
cargo build -p regia-app --features smoke
./target/debug/app-smoke
```

## Conformance

The Python prototype is the oracle and lives in a private repo. A fixture case in `fixtures/brief/cases/` holds what the tracker answered and what the prototype printed. The test runs the real `regia` binary against the same answers through a fake `gh`, passes the prototype's output through the facade table, and requires an empty diff. Object key order does not count; array order does.

Fixtures are recorded locally and committed, because CI cannot read the prototype's repo. With a checkout of it, clean in `tools/`:

```sh
cargo run -p regia-conformance --bin record -- --dossier <path>
./target/debug/live-check --dossier <path>     # both implementations on the real tracker; prints no data
```

Run `live-check` directly, not through `cargo run`: cargo changes `PATH`. Due dates count from today, so every fixture is recorded and checked on one fixed day, `FIXTURE_TODAY` in `crates/regia-conformance/src/lib.rs`: `record` runs the prototype through `crates/regia-conformance/oracle/fixed_today.py`, which only replaces the prototype's own date function, and the product reads the day from `REGIA_TODAY`. `live-check` uses the real day on both sides. Fixtures are synthetic on purpose, because this repo goes to other people and later public. Never record real titles, names or URLs into them; `fixtures_are_synthetic` checks the obvious ones.

The brief's three machine-local sections (controller, dossier, outside the tracker) are not ported or compared yet. See the ADR.
