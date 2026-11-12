//! A stand-in for `git` that does not answer: every call ends with the exit code 124, as the
//! prototype's deadline gives a git that ran out of time. The real deadline of the product is 120 s,
//! too long for a case, so the case holds the exit code that deadline leaves (#571/V8).

use std::process::ExitCode;

fn main() -> ExitCode {
    eprintln!("fake git: no answer");
    ExitCode::from(124)
}
