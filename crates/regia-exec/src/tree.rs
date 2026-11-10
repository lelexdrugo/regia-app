//! The process tree of a run, so that it can be killed whole (threat model §3.6, rule 6): a job
//! object on Windows, a process group on Unix.

#[cfg(windows)]
pub use windows::{Tree, prepare};

#[cfg(unix)]
pub use unix::{Tree, prepare};

#[cfg(windows)]
mod windows {
    use std::io;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::os::windows::process::CommandExt;
    use std::process::{Child, Command};

    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_BREAKAWAY_OK, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject,
        TerminateJobObject,
    };

    /// The release app has no console: without this flag every program would flash a window.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub fn prepare(command: &mut Command) {
        command.creation_flags(CREATE_NO_WINDOW);
    }

    /// A job object holding the program and what it starts, killed when the job is closed. The
    /// program joins it just after it starts, so a grandchild started in that instant would escape:
    /// a suspended start would close the window, and is not done yet.
    pub struct Tree {
        job: OwnedHandle,
    }

    impl Tree {
        pub fn adopt(child: &Child) -> io::Result<Tree> {
            // SAFETY: Win32 calls on a job handle this function owns from its creation, and on the
            // child's process handle, which `child` keeps open.
            unsafe {
                let raw = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if raw.is_null() {
                    return Err(io::Error::last_os_error());
                }
                let job = OwnedHandle::from_raw_handle(raw);
                let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                // Breakaway stays allowed: a program that detaches on purpose, as the prototype's
                // launcher does, still can.
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK;
                let size = u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>()).expect("a small struct");
                let info = std::ptr::from_ref(&info).cast();
                if SetInformationJobObject(job.as_raw_handle(), JobObjectExtendedLimitInformation, info, size) == 0 {
                    return Err(io::Error::last_os_error());
                }
                if AssignProcessToJobObject(job.as_raw_handle(), child.as_raw_handle()) == 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(Tree { job })
            }
        }

        pub fn kill(&self) {
            // SAFETY: the job handle is owned by `self` and open. A failure leaves the closing of
            // the handle, on drop, to kill the job.
            unsafe {
                TerminateJobObject(self.job.as_raw_handle(), 1);
            }
        }
    }
}

#[cfg(unix)]
mod unix {
    use std::io;
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command};

    /// A new process group led by the program: what it starts stays in it, unless it leaves on
    /// purpose with a session of its own.
    pub fn prepare(command: &mut Command) {
        command.process_group(0);
    }

    pub struct Tree {
        group: libc::pid_t,
    }

    impl Tree {
        pub fn adopt(child: &Child) -> io::Result<Tree> {
            let group = libc::pid_t::try_from(child.id()).map_err(|_| io::Error::other("process id out of range"))?;
            Ok(Tree { group })
        }

        pub fn kill(&self) {
            // SAFETY: kill(2) with a negative id signals that process group; it touches no memory.
            unsafe {
                libc::kill(-self.group, libc::SIGKILL);
            }
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            self.kill();
        }
    }
}
