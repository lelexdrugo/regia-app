//! A stand-in for the Python interpreter, where the prototype is not, as in CI. The facade starts it as
//! it starts the prototype; it refuses an argv that is not the one the case expects, flags and script
//! included, and otherwise replays what the prototype printed and its exit code. It reads the replay
//! from a file beside it, so it never passes for the real interpreter by accident.

use std::io::Write;
use std::process::ExitCode;

use regia_conformance::{FAKE_PYTHON_SIDECAR, FAKE_PYTHON_WRONG_ARGV, Replay, read_json};

fn main() -> ExitCode {
    let exe = std::env::current_exe().expect("own path");
    let replay: Replay = read_json(&exe.with_file_name(FAKE_PYTHON_SIDECAR));
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv != replay.argv {
        eprintln!("fake python: started with {argv:?}, the case expects {:?}", replay.argv);
        return ExitCode::from(FAKE_PYTHON_WRONG_ARGV);
    }
    let _ = std::io::stdout().write_all(replay.stdout.as_bytes());
    let _ = std::io::stderr().write_all(replay.stderr.as_bytes());
    ExitCode::from(replay.exit)
}
