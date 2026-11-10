//! Stands in for any program regia-exec starts, in `tests/exec.rs`. By default it prints, as JSON,
//! what it received: its own path, argv, stdin, folder and environment. `REGIA_PROBE_MODE`, which a
//! test sets by name, asks for something else:
//!
//! - `tree`: start a grandchild in `beat` mode, then sleep;
//! - `beat`: append a byte to the file named by `REGIA_PROBE_BEAT` every 50 ms, without end;
//! - `flood`: write to stdout without end.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::time::Duration;

const MODE: &str = "REGIA_PROBE_MODE";
const BEAT: &str = "REGIA_PROBE_BEAT";

fn main() {
    match std::env::var(MODE).unwrap_or_default().as_str() {
        "tree" => {
            let exe = std::env::current_exe().expect("own path");
            let beat = std::env::var_os(BEAT).expect("REGIA_PROBE_BEAT");
            regia_exec::harness::spawn_background(&exe, &[], &[(MODE, OsStr::new("beat")), (BEAT, &beat)])
                .expect("the grandchild starts");
            loop {
                std::thread::sleep(Duration::from_secs(60));
            }
        }
        "beat" => {
            let path = std::env::var_os(BEAT).expect("REGIA_PROBE_BEAT");
            loop {
                if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
                    let _ = file.write_all(b".");
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        "flood" => {
            let chunk = vec![b'x'; 64 * 1024];
            let mut out = std::io::stdout().lock();
            while out.write_all(&chunk).is_ok() {}
        }
        _ => echo(),
    }
}

fn echo() {
    let mut stdin = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut stdin);
    let lossy = |value: &OsStr| value.to_string_lossy().into_owned();
    let env: BTreeMap<String, String> = std::env::vars_os().map(|(name, value)| (lossy(&name), lossy(&value))).collect();
    let report = serde_json::json!({
        "exe": std::env::current_exe().map(|path| path.display().to_string()).unwrap_or_default(),
        "argv": std::env::args_os().skip(1).map(|arg| lossy(&arg)).collect::<Vec<_>>(),
        "stdin": String::from_utf8_lossy(&stdin),
        "cwd": std::env::current_dir().map(|path| path.display().to_string()).unwrap_or_default(),
        "env": env,
    });
    print!("{report}");
}
