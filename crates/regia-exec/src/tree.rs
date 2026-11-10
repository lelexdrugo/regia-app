//! The process tree of a run, so that it can be killed whole (threat model §3.6, rule 6): a job
//! object on Windows, a process group on Unix. A detached run gets a tree too, which nothing kills
//! unless the holder of its handle asks.

#[cfg(windows)]
pub use windows::{Tree, prepare, spawn_detached};

#[cfg(unix)]
pub use unix::{Tree, prepare, spawn_detached};

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
    /// A detached program does not get the Ctrl+C of the caller's console.
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    /// Out of the caller's job, which a terminal may close with everything in it.
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;

    pub fn prepare(command: &mut Command) {
        command.creation_flags(CREATE_NO_WINDOW);
    }

    /// Starts a program that outlives the caller: no window, a process group of its own, and out of
    /// the caller's job. A job that does not allow it refuses the start; then it starts again without
    /// leaving, and dies with that job, as the prototype's launcher accepts. A refused start created
    /// no process, so the second one cannot run the program twice.
    pub fn spawn_detached(command: &mut Command) -> io::Result<Child> {
        command.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB);
        command.spawn().or_else(|_| {
            command.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
            command.spawn()
        })
    }

    /// A job object holding the program and what it starts. The program joins it just after it
    /// starts, so a grandchild started in that instant would escape: a suspended start would close
    /// the window, and is not done yet.
    pub struct Tree {
        job: OwnedHandle,
    }

    impl Tree {
        /// A tree killed when the job is closed, which is when the run ends.
        pub fn adopt(child: &Child) -> io::Result<Tree> {
            Tree::with_limits(child, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK)
        }

        /// A tree that closing the job leaves running: only [`Tree::kill`] ends it.
        pub fn adopt_detached(child: &Child) -> io::Result<Tree> {
            Tree::with_limits(child, JOB_OBJECT_LIMIT_BREAKAWAY_OK)
        }

        fn with_limits(child: &Child, limits: u32) -> io::Result<Tree> {
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
                info.BasicLimitInformation.LimitFlags = limits;
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
            // the handle, on drop, to kill the job when it was made to.
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

    /// Starts a program in a session of its own, which the caller's terminal does not end, as the
    /// prototype's `start_new_session`. Not with [`prepare`]: a group leader cannot call `setsid`.
    pub fn spawn_detached(command: &mut Command) -> io::Result<Child> {
        // SAFETY: `setsid` is async-signal-safe and touches no memory of the parent.
        unsafe {
            command.pre_exec(|| if libc::setsid() == -1 { Err(io::Error::last_os_error()) } else { Ok(()) });
        }
        command.spawn()
    }

    pub struct Tree {
        group: libc::pid_t,
        /// A detached tree outlives its handle.
        kill_on_drop: bool,
    }

    impl Tree {
        pub fn adopt(child: &Child) -> io::Result<Tree> {
            Ok(Tree { group: group_of(child)?, kill_on_drop: true })
        }

        /// The session of a detached program: it leads its own group.
        pub fn adopt_detached(child: &Child) -> io::Result<Tree> {
            Ok(Tree { group: group_of(child)?, kill_on_drop: false })
        }

        pub fn kill(&self) {
            // SAFETY: kill(2) with a negative id signals that process group; it touches no memory.
            unsafe {
                libc::kill(-self.group, libc::SIGKILL);
            }
        }
    }

    fn group_of(child: &Child) -> io::Result<libc::pid_t> {
        libc::pid_t::try_from(child.id()).map_err(|_| io::Error::other("process id out of range"))
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            if self.kill_on_drop {
                self.kill();
            }
        }
    }
}
