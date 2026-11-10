//! V18 of the threat model, for the agents: the core's environment holds tokens, keys and a foreign
//! host, and the fake agent sees none of them, unless the user's settings name one for that service.
//! A single test in its own binary, because it sets this process's environment.

mod common;

use common::{Trial, env_of};
use regia_agents::{Env, LaunchSurface, ServiceId};
use regia_identity::prototype_env_var;

const CANARIES: [(&str, &str); 11] = [
    ("GH_TOKEN", "canary-gh"),
    ("GITHUB_TOKEN", "canary-github"),
    ("GH_ENTERPRISE_TOKEN", "canary-enterprise"),
    ("GH_HOST", "evil.example"),
    ("ANTHROPIC_API_KEY", "canary-anthropic"),
    ("ANTHROPIC_AUTH_TOKEN", "canary-anthropic-token"),
    ("ANTHROPIC_BASE_URL", "https://evil.example"),
    ("CLAUDE_CODE_OAUTH_TOKEN", "canary-claude"),
    ("OPENAI_API_KEY", "canary-openai"),
    ("AWS_SECRET_ACCESS_KEY", "canary-aws"),
    ("REGIA_SOME_TOKEN", "canary-regia"),
];

/// What binds a process to the session that started it: none reaches the child, whatever the core
/// itself runs under.
const BINDINGS: [&str; 4] = ["ORCA_", "CLAUDE_CODE_", "CLAUDECODE", "CODEX_COMPANION_"];

#[test]
fn an_agent_sees_no_secret_unless_the_settings_name_it() {
    for (name, value) in CANARIES {
        // SAFETY: the only test of this binary: no other thread reads the environment meanwhile.
        unsafe { std::env::set_var(name, value) };
    }
    let trial = Trial::new("environment");
    let env = Env::of_this_process()
        .with("CLAUDE_CONFIG_DIR", trial.claude_home())
        .with("CODEX_HOME", trial.codex_home())
        .with(&prototype_env_var("RUNTIME"), trial.state.clone());
    let cli = trial.cli_over(env);

    cli.launch(&trial.request(ServiceId::Claude)).expect("the launch");
    let seen = env_of(&trial.record("claude", 0));
    for (name, canary) in CANARIES {
        assert_ne!(seen.get(name).map(String::as_str), Some(canary), "the agent sees {name}");
        assert!(!seen.contains_key(name), "the agent sees {name}");
    }
    for name in seen.keys() {
        assert!(!BINDINGS.iter().any(|prefix| name.starts_with(prefix)), "the agent sees {name}");
        assert!(!regia_exec::secret_shaped(name), "the agent sees {name}, shaped like a secret");
        assert!(!name.starts_with("ANTHROPIC_") && !name.starts_with("GH_") && !name.starts_with("OPENAI_"), "the agent sees {name}");
    }
    assert!(seen.contains_key("PATH") && seen.contains_key("REGIA_RUNTIME") && seen.contains_key("CLAUDE_CONFIG_DIR"));

    // Named for this service in the settings, a key passes, by name, and nothing else does.
    let request = trial.request(ServiceId::Claude).secret_env(vec!["ANTHROPIC_API_KEY".to_owned(), "NOT_SET_ANYWHERE_KEY".to_owned()]);
    cli.launch(&request).expect("the launch");
    let seen = env_of(&trial.record("claude", 1));
    assert_eq!(seen.get("ANTHROPIC_API_KEY").map(String::as_str), Some("canary-anthropic"));
    assert!(!seen.contains_key("NOT_SET_ANYWHERE_KEY"), "an unset secret is not set empty");
    for (name, _) in CANARIES.iter().filter(|(name, _)| *name != "ANTHROPIC_API_KEY") {
        assert!(!seen.contains_key(*name), "the agent sees {name}");
    }
}
