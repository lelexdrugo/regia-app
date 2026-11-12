//! What the tests of the surfaces share: a scratch folder under `target/` with the fake agent copied
//! as `claude`, `codex` and `orca`, a work folder, a state folder, and the surfaces pinned to the
//! fakes, so that no test can reach a real agent or a real Orca. A surface without a pin, such as
//! the Orca surface for its wake-up, gets an empty search ([`Trial::nowhere`]) in the tests.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use regia_agents::{Cli, Env, LaunchError, LaunchRequest, LaunchSurface, Orca, OrcaWaits, Receipt, ServiceId, Waits};
use regia_exec::{Pin, Program, Search};
use regia_identity::prototype_env_var;
use serde_json::Value;

pub const SESSION: &str = "11111111-2222-3333-4444-555555555555";
pub const PROMPT: &str = "do the step\nline two: è l'ora";

pub fn exe(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

/// The fake, as `cargo test` builds the examples: `target/<profile>/examples/`, found from the test
/// itself, which is in `<profile>/deps/` before Cargo 1.100 and in `<profile>/build/<package>/<hash>/out/` since.
pub fn fake() -> PathBuf {
    let test = std::env::current_exe().expect("own path");
    let name = exe("fake_agent");
    test.ancestors()
        .skip(1)
        .map(|dir| dir.join("examples").join(&name))
        .find(|fake| fake.is_file())
        .unwrap_or_else(|| {
            panic!(
                "examples/{name} not found above {}: `cargo test` builds the examples, `cargo test --test <name>` alone does not (add --examples)",
                test.display()
            )
        })
}

pub struct Trial {
    pub dir: PathBuf,
    pub bin: PathBuf,
    pub folder: PathBuf,
    pub state: PathBuf,
}

impl Trial {
    pub fn new(label: &str) -> Trial {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("agents-cli").join(label);
        let _ = fs::remove_dir_all(&dir);
        let trial = Trial { bin: dir.join("bin"), folder: dir.join("work"), state: dir.join("state"), dir };
        for folder in [&trial.bin, &trial.folder, &trial.state] {
            fs::create_dir_all(folder).expect("a scratch folder");
        }
        let fake = fake();
        for name in ["claude", "codex", "orca"] {
            fs::copy(&fake, trial.bin.join(exe(name))).expect("a copy of the fake");
        }
        // Where the agents that the fake Orca's app starts write their sessions.
        fs::write(trial.bin.join("claude-home"), trial.claude_home().display().to_string()).expect("claude's home");
        fs::write(trial.bin.join("codex-home"), trial.codex_home().display().to_string()).expect("codex's home");
        trial
    }

    /// What the fakes do: `dies` or `slow`.
    pub fn mode(&self, mode: &str) {
        fs::write(self.bin.join("mode"), mode).expect("the mode");
    }

    /// What the fake Orca does: words of `examples/fake_agent.rs`.
    pub fn orca_mode(&self, modes: &str) {
        fs::write(self.bin.join("orca-mode"), modes).expect("the mode");
    }

    /// The Orca surface over the trial's environment, pinned to the fake, with short waits.
    pub fn orca(&self) -> Orca {
        self.orca_waiting(Duration::from_secs(4))
    }

    pub fn orca_waiting(&self, session: Duration) -> Orca {
        let waits = OrcaWaits {
            check: Duration::from_secs(20),
            retry: Duration::from_millis(50),
            launch: Duration::from_secs(20),
            session,
            tick: Duration::from_millis(50),
        };
        Orca::new(self.env()).with_state(self.state.clone()).with_pin(self.pin(Program::Orca, "orca")).with_waits(waits)
    }

    /// Every call the fake Orca got, in order.
    pub fn orca_calls(&self) -> Vec<Vec<String>> {
        (0..).map_while(|n| fs::read_to_string(self.bin.join(format!("orca.{n}.json"))).ok()).map(|text| args(&serde_json::from_str(&text).expect("a record"))).collect()
    }

    /// The arguments of each `orca agent launch`.
    pub fn agent_launches(&self) -> Vec<Vec<String>> {
        self.orca_calls().into_iter().filter(|args| args.len() > 1 && args[0] == "agent" && args[1] == "launch").collect()
    }

    /// The files of the state folder for the trial's launches.
    pub fn steps(&self) -> Vec<PathBuf> {
        fs::read_dir(self.state.join("passi").join("trial")).map(|entries| entries.filter_map(Result::ok).map(|entry| entry.path()).collect()).unwrap_or_default()
    }

    pub fn claude_home(&self) -> PathBuf {
        self.dir.join("claude")
    }

    pub fn codex_home(&self) -> PathBuf {
        self.dir.join("codex")
    }

    /// The agents' folders, the state folder, and what binds the core to the session that started
    /// it, which the child must not see.
    pub fn env(&self) -> Env {
        Env::from_pairs([
            ("CLAUDE_CONFIG_DIR".to_owned(), OsString::from(self.claude_home())),
            ("CODEX_HOME".to_owned(), self.codex_home().into()),
            (prototype_env_var("RUNTIME"), self.state.clone().into()),
            ("ORCA_TERMINAL_HANDLE".to_owned(), "x".into()),
            ("CLAUDE_CODE_SESSION_ID".to_owned(), "parent".into()),
            ("CLAUDECODE".to_owned(), "1".into()),
        ])
    }

    pub fn pin(&self, program: Program, name: &str) -> Pin {
        Pin::at(program, &self.bin.join(exe(name))).expect("the fake")
    }

    /// A search of an empty folder: a program without a pin is not found, never this machine's.
    pub fn nowhere(&self) -> Search {
        let empty = self.dir.join("empty");
        fs::create_dir_all(&empty).expect("an empty folder");
        Search::from_parts(Some(empty.as_os_str()), Vec::new(), &[])
    }

    /// The surface over `env`, pinned to the fakes, with the prototype test's short waits.
    pub fn cli_over(&self, env: Env) -> Cli {
        Cli::new(env)
            .with_state(self.state.clone())
            .with_pin(ServiceId::Claude, self.pin(Program::Claude, "claude"))
            .with_pin(ServiceId::Codex, self.pin(Program::Codex, "codex"))
            .with_waits(Waits {
                session: Duration::from_secs(20),
                transcript: Duration::from_secs(4),
                wake: Duration::from_secs(20),
                tick: Duration::from_millis(50),
            })
    }

    pub fn cli(&self) -> Cli {
        self.cli_over(self.env())
    }

    pub fn request(&self, service: ServiceId) -> LaunchRequest {
        LaunchRequest::new(service, "trial", &self.folder, PROMPT).expect("a valid request")
    }

    pub fn launch(&self, request: &LaunchRequest) -> Result<Receipt, LaunchError> {
        self.cli().launch(request)
    }

    /// What the fake named `name` received at its `n`-th start.
    pub fn record(&self, name: &str, n: usize) -> Value {
        let text = fs::read_to_string(self.bin.join(format!("{name}.{n}.json"))).expect("the fake started");
        serde_json::from_str(&text).expect("a record")
    }

    pub fn started(&self, name: &str) -> bool {
        self.bin.join(format!("{name}.0.json")).exists()
    }

    pub fn wait_for(&self, what: impl Fn() -> bool, limit: Duration) {
        let start = Instant::now();
        while !what() {
            assert!(start.elapsed() < limit, "not reached in {} s", limit.as_secs());
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

pub fn args(record: &Value) -> Vec<String> {
    record["args"].as_array().expect("args").iter().map(|arg| arg.as_str().expect("text").to_owned()).collect()
}

pub fn env_of(record: &Value) -> BTreeMap<String, String> {
    record["env"]
        .as_object()
        .expect("env")
        .iter()
        .map(|(name, value)| (name.to_ascii_uppercase(), value.as_str().unwrap_or_default().to_owned()))
        .collect()
}
