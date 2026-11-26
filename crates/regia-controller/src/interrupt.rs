//! Ctrl+C for a resident a command line hosts (#468/V3): the prototype's controller stops at a `KeyboardInterrupt`,
//! releases its lock and exits with 0. Here the handler only raises the flag the resident looks at
//! ([`crate::resident::HostPorts::stop`]): the wait ends at once ([`crate::resident::StopSleep`]), a tick under way
//! ends first, and the resident releases the lock and exits with 0.
//!
//! - On Unix, SIGINT and SIGTERM: the second is how a user service is stopped.
//! - On Windows, the console's Ctrl+C and Ctrl+Break.
//!
//! The handler does one thing a handler may do: an atomic store. Only a command line installs it; the app stops
//! its controller through the flag alone.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

/// The flag the handler raises, once installed.
static STOP: OnceLock<Arc<AtomicBool>> = OnceLock::new();

/// The flag a stop raises, with the handler installed for this process: the same flag on every call.
///
/// # Errors
///
/// When the system refuses the handler.
pub fn stop_on_interrupt() -> Result<Arc<AtomicBool>, String> {
    if let Some(stop) = STOP.get() {
        return Ok(Arc::clone(stop));
    }
    let stop = Arc::clone(STOP.get_or_init(|| Arc::new(AtomicBool::new(false))));
    install()?;
    Ok(stop)
}

fn raise() {
    if let Some(stop) = STOP.get() {
        stop.store(true, Ordering::SeqCst);
    }
}

#[cfg(unix)]
extern "C" fn on_signal(_signal: libc::c_int) {
    raise();
}

#[cfg(unix)]
fn install() -> Result<(), String> {
    let handler = on_signal as extern "C" fn(libc::c_int);
    for signal in [libc::SIGINT, libc::SIGTERM] {
        // SAFETY: the handler is a plain function that lives as long as the process and only stores an atomic.
        let previous = unsafe { libc::signal(signal, handler as libc::sighandler_t) };
        if previous == libc::SIG_ERR {
            return Err(format!("signal {signal}: {}", std::io::Error::last_os_error()));
        }
    }
    Ok(())
}

#[cfg(windows)]
unsafe extern "system" fn on_console(event: u32) -> windows_sys::core::BOOL {
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT};
    if event == CTRL_C_EVENT || event == CTRL_BREAK_EVENT {
        raise();
        return 1;
    }
    0
}

#[cfg(windows)]
fn install() -> Result<(), String> {
    use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
    // SAFETY: the handler is a plain function that lives as long as the process and only stores an atomic.
    if unsafe { SetConsoleCtrlHandler(Some(on_console), 1) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn install() -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The flag is one per process, and the handler only raises it.
    #[test]
    fn the_flag_is_the_same_on_every_call_and_the_handler_raises_it() {
        let first = stop_on_interrupt().expect("the handler");
        let second = stop_on_interrupt().expect("the handler");
        assert!(Arc::ptr_eq(&first, &second));
        assert!(!first.load(Ordering::SeqCst));
        raise();
        assert!(second.load(Ordering::SeqCst));
        first.store(false, Ordering::SeqCst);
    }
}
