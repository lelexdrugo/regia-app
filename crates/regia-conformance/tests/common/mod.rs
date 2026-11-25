//! Shared by the integration tests that run a binary of another package.
//!
//! `CARGO_BIN_EXE_<name>` exists only for the binaries of the package under test, and `cargo test` builds
//! no others: `cargo test -p regia-conformance --test precheck` on a clean target has no `precheck` (it
//! belongs to `regia-controller`) and no `regia` (`regia-cli`). CI builds the workspace first, so it never
//! met this; a worker who runs one test file does. [`built`] makes the test independent of that order: it
//! builds what it needs, once per test process, with the profile and the target folder of the test that
//! is running, and says what to run if it cannot.

use std::env;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use regia_exec::Limits;

/// A binary of another package: the package that holds it, and its name.
#[derive(Clone, Copy)]
pub struct Bin {
    pub package: &'static str,
    pub name: &'static str,
}

/// What `cargo build` may take on a cold target: a deadline, not a measure.
const BUILD_LIMITS: Limits = Limits {
    timeout: Duration::from_secs(1800),
    stdout: 8 << 20,
    stderr: 8 << 20,
};

/// The folder of the running test's profile (`target/debug`), where cargo leaves the binaries it builds. The
/// test itself is below it, but not always in `deps/`: a newer cargo keeps it in `build/<package>/<hash>/out/`
/// (CI on Linux). So the folder is found as the one inside the target folder, which holds `CACHEDIR.TAG`.
fn profile_dir() -> PathBuf {
    let exe = env::current_exe().expect("current executable");
    exe.ancestors()
        .skip(1)
        .find(|dir| {
            dir.parent()
                .is_some_and(|parent| parent.join("CACHEDIR.TAG").is_file())
        })
        .unwrap_or_else(|| {
            panic!(
                "no target folder (with a CACHEDIR.TAG) above {}",
                exe.display()
            )
        })
        .to_path_buf()
}

fn file_of(dir: &Path, bin: Bin) -> PathBuf {
    dir.join(format!("{}{}", bin.name, env::consts::EXE_SUFFIX))
}

/// The arguments that build `wanted` where the running test lives: the same profile, the same lockfile,
/// the same target folder. A profile other than `debug` and `release` is a custom one, named by its folder.
fn build_args(profile: &Path, wanted: &[Bin]) -> Vec<String> {
    let mut args = vec!["build".to_owned(), "--locked".to_owned()];
    match profile.file_name().and_then(|name| name.to_str()) {
        Some("debug") => {}
        Some("release") => args.push("--release".to_owned()),
        Some(custom) => args.extend(["--profile".to_owned(), custom.to_owned()]),
        None => panic!("no profile name in {}", profile.display()),
    }
    args.extend([
        "--target-dir".to_owned(),
        profile
            .parent()
            .expect("the target folder")
            .display()
            .to_string(),
    ]);
    for bin in wanted {
        args.extend([
            "--package".to_owned(),
            bin.package.to_owned(),
            "--bin".to_owned(),
            bin.name.to_owned(),
        ]);
    }
    args
}

/// The folder of the running test's profile, with every `wanted` binary in it, freshly built, once per
/// process. Parallel tests of the process wait on the one build.
///
/// # Panics
///
/// If the build fails or leaves a binary elsewhere, with the command to run by hand.
pub fn built(wanted: &[Bin]) -> PathBuf {
    static BUILD: Mutex<()> = Mutex::new(());
    static PROFILE: OnceLock<PathBuf> = OnceLock::new();
    if let Some(done) = PROFILE.get() {
        return done.clone();
    }
    let _only_one = BUILD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(done) = PROFILE.get() {
        return done.clone();
    }
    let profile = profile_dir();
    // Always, not only when a binary is missing: a binary left by an earlier build would be a stale one after
    // an edit, and a test of the product must run the product as it is. A build with nothing to do is quick.
    let cargo = PathBuf::from(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    let args = build_args(&profile, wanted);
    let workspace = regia_conformance::workspace_root();
    let by_hand = format!("run `cargo {}` in {}", args.join(" "), workspace.display());
    let output = regia_exec::harness::launch(&cargo, &args, &workspace)
        .limits(BUILD_LIMITS)
        .run();
    let output = output.unwrap_or_else(|error| {
        panic!("could not start cargo to build what the test runs ({error}): {by_hand}")
    });
    assert!(
        output.status.success(),
        "building what the test runs failed: {by_hand}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for bin in wanted {
        assert!(
            file_of(&profile, *bin).is_file(),
            "built {} but not in {}: {by_hand}",
            bin.name,
            profile.display()
        );
    }
    PROFILE.get_or_init(|| profile).clone()
}
