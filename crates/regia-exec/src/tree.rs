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

    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_BREAKAWAY_OK, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject,
        TerminateJobObject,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

    /// The release app has no console: without this flag every program would flash a window.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    /// A detached program does not get the Ctrl+C of the caller's console.
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    /// Out of the caller's job, which a terminal may close with everything in it.
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    /// The program is created with its main thread stopped, so that it runs nothing before it is in
    /// its job: [`Tree::adopt`] and [`Tree::adopt_detached`] start it.
    const CREATE_SUSPENDED: u32 = 0x0000_0004;

    /// The program starts suspended: whoever spawns it must hand the child to [`Tree::adopt`], or
    /// kill it. A suspended program never runs, and nothing else ends it.
    pub fn prepare(command: &mut Command) {
        command.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
    }

    /// Starts a program that outlives the caller: no window, a process group of its own, and out of
    /// the caller's job. A job that does not allow it refuses the start; then it starts again without
    /// leaving, and dies with that job, as the prototype's launcher accepts. A refused start created
    /// no process, so the second one cannot run the program twice. Like [`prepare`], the program
    /// starts suspended, and [`Tree::adopt_detached`] starts it.
    pub fn spawn_detached(command: &mut Command) -> io::Result<Child> {
        const DETACHED: u32 = CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP | CREATE_SUSPENDED;
        command.creation_flags(DETACHED | CREATE_BREAKAWAY_FROM_JOB);
        command.spawn().or_else(|_| {
            command.creation_flags(DETACHED);
            command.spawn()
        })
    }

    /// A job object holding the program and what it starts. The program is created suspended and
    /// runs only once it is in the job, so everything it starts, from its first instruction on, is
    /// in the job too: nothing escapes between the start and the assignment.
    pub struct Tree {
        job: OwnedHandle,
    }

    impl Tree {
        /// A tree killed when the job is closed, which is when the run ends. It puts the suspended
        /// program in the job, then starts it. An error leaves the program suspended: the caller
        /// kills it.
        pub fn adopt(child: &Child) -> io::Result<Tree> {
            let tree = Tree::with_limits(child, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK)?;
            resume(child)?;
            Ok(tree)
        }

        /// A tree that closing the job leaves running: only [`Tree::kill`] ends it. A detached
        /// program does not depend on its tree, so one that cannot be set up (`None`) leaves the
        /// program running alone. An error means the program could not be started: it is still
        /// suspended, and the caller kills it.
        pub fn adopt_detached(child: &Child) -> io::Result<Option<Tree>> {
            let tree = Tree::with_limits(child, JOB_OBJECT_LIMIT_BREAKAWAY_OK).ok();
            resume(child)?;
            Ok(tree)
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

    /// Starts the main thread of a program created suspended. `std` gives no handle to it, so it is
    /// found by its owner among the threads of the system: a program that never ran has only that
    /// one, and all of them are resumed.
    fn resume(child: &Child) -> io::Result<()> {
        let pid = child.id();
        // SAFETY: Win32 calls on handles this function owns from their creation (the snapshot and
        // each thread, closed on drop), and on a `THREADENTRY32` whose size is set as the API
        // requires. The program is alive and suspended, so its process id is not reused.
        unsafe {
            let raw = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if raw == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            let snapshot = OwnedHandle::from_raw_handle(raw);
            let mut entry = THREADENTRY32 {
                dwSize: u32::try_from(size_of::<THREADENTRY32>()).expect("a small struct"),
                ..THREADENTRY32::default()
            };
            let mut resumed = false;
            let mut more = Thread32First(snapshot.as_raw_handle(), &mut entry) != 0;
            while more {
                if entry.th32OwnerProcessID == pid {
                    let raw = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                    if raw.is_null() {
                        return Err(io::Error::last_os_error());
                    }
                    let thread = OwnedHandle::from_raw_handle(raw);
                    if ResumeThread(thread.as_raw_handle()) == u32::MAX {
                        return Err(io::Error::last_os_error());
                    }
                    resumed = true;
                }
                more = Thread32Next(snapshot.as_raw_handle(), &mut entry) != 0;
            }
            if resumed { Ok(()) } else { Err(io::Error::other("the program has no thread to start")) }
        }
    }

    /// The tests start this test binary again, in a role named by the environment: the child of a
    /// tree is a real program that starts a real grandchild.
    #[cfg(test)]
    mod tests {
        use std::io::{BufRead, BufReader};
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        use std::process::Stdio;
        use std::sync::mpsc;
        use std::time::Duration;
        use std::{env, fs, thread};

        use windows_sys::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
        use windows_sys::Win32::System::JobObjects::IsProcessInJob;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess,
            WaitForSingleObject,
        };

        use super::{Child, Command, Tree, prepare, spawn_detached};
        use crate::run::new_command;

        const ROLE: &str = "REGIA_TREE_TEST_ROLE";
        const MARKER: &str = "REGIA_TREE_TEST_MARKER";

        /// How long the child of a test waits between its creation and its adoption: a thread
        /// preempted there for this long is what a program that is not suspended would exploit.
        const PREEMPTED: Duration = Duration::from_millis(300);
        const FOREVER: Duration = Duration::from_secs(120);

        /// The part of the test binary the others start: it does nothing unless a role is set.
        #[test]
        fn role() {
            let Ok(role) = env::var(ROLE) else { return };
            match role.as_str() {
                // Starts a grandchild as its very first act, says who it is, and stays.
                "spawner" => {
                    println!("GRANDCHILD={}", sleeper(Stdio::null()).id());
                    thread::sleep(FOREVER);
                }
                "sleeper" => thread::sleep(FOREVER),
                // A caller of `spawn_detached`: it starts a session and ends.
                "caller" => {
                    let mut command = this_binary("sleeper");
                    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
                    // The session is meant to outlive this caller: nobody waits for it.
                    #[allow(clippy::zombie_processes)]
                    let session = spawn_detached(&mut command).expect("the session starts");
                    Tree::adopt_detached(&session).expect("the session starts");
                    println!("SESSION={}", session.id());
                }
                "marker" => fs::write(env::var(MARKER).expect("a marker"), "ran").expect("the marker is written"),
                other => panic!("no role {other}"),
            }
        }

        fn this_binary(role: &str) -> Command {
            let mut command = new_command(&env::current_exe().expect("the test binary"));
            command.args(["--exact", "tree::windows::tests::role", "--nocapture"]).env(ROLE, role);
            command
        }

        fn sleeper(stdio: Stdio) -> Child {
            this_binary("sleeper").stdin(Stdio::null()).stdout(stdio).stderr(Stdio::null()).spawn().expect("a grandchild")
        }

        /// A handle to a process, taken while its pid is certainly its own, that ends it on drop.
        struct Watched(OwnedHandle);

        impl Watched {
            fn open(pid: u32) -> Watched {
                // SAFETY: OpenProcess takes plain values; the handle it returns is owned from here.
                let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
                assert!(!raw.is_null(), "process {pid} cannot be opened: {}", std::io::Error::last_os_error());
                // SAFETY: a non-null handle OpenProcess just returned, owned by no one else.
                Watched(unsafe { OwnedHandle::from_raw_handle(raw) })
            }

            /// Whether the process ended within `wait`.
            fn ends_within(&self, wait: Duration) -> bool {
                let millis = u32::try_from(wait.as_millis()).expect("a short wait");
                // SAFETY: the handle is open and owned by `self`.
                let result = unsafe { WaitForSingleObject(self.0.as_raw_handle(), millis) };
                assert!(result == WAIT_OBJECT_0 || result == WAIT_TIMEOUT, "the wait failed: {result}");
                result == WAIT_OBJECT_0
            }
        }

        impl Drop for Watched {
            fn drop(&mut self) {
                // SAFETY: the handle is open and owned by `self`; ending an ended process fails, harmlessly.
                unsafe {
                    TerminateProcess(self.0.as_raw_handle(), 1);
                }
            }
        }

        /// The first line of the program's stdout that starts with `key=`: libtest writes its own.
        fn announced(child: &mut Child, key: &str) -> u32 {
            let stdout = child.stdout.take().expect("stdout is piped");
            let (sender, receiver) = mpsc::channel();
            thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if sender.send(line).is_err() {
                        return;
                    }
                }
            });
            let prefix = format!("{key}=");
            loop {
                let line = receiver.recv_timeout(Duration::from_secs(30)).expect("the program announces itself");
                if let Some(pid) = line.strip_prefix(&prefix) {
                    return pid.trim().parse().expect("a pid");
                }
            }
        }

        /// The window of #438/V5: the program starts a grandchild at once, and the tree must hold it.
        /// The program is held back between its creation and its adoption, as a preempted thread
        /// would hold it. A program that is not suspended starts its grandchild in that time, out of
        /// the job, and survives the kill: this test fails on the start that came before.
        #[test]
        fn a_grandchild_started_at_once_dies_with_the_tree() {
            for round in 1..=25 {
                let mut command = this_binary("spawner");
                prepare(&mut command);
                command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
                let mut child = command.spawn().expect("the program starts");
                thread::sleep(PREEMPTED);
                let tree = Tree::adopt(&child).expect("the tree is made");
                let grandchild = Watched::open(announced(&mut child, "GRANDCHILD"));
                assert!(!grandchild.ends_within(Duration::ZERO), "round {round}: the grandchild ended by itself");
                tree.kill();
                assert!(grandchild.ends_within(Duration::from_secs(10)), "round {round}: the grandchild escaped the job");
                child.wait().expect("the program is reaped");
            }
        }

        /// A program that is created and not adopted runs nothing, and ends when it is killed.
        #[test]
        fn a_program_runs_nothing_until_it_is_in_its_tree() {
            let marker = env::temp_dir().join(format!("regia-tree-marker-{}", std::process::id()));
            let _ = fs::remove_file(&marker);
            let start = |marker: &std::path::Path| {
                let mut command = this_binary("marker");
                prepare(&mut command);
                command.env(MARKER, marker).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
                command.spawn().expect("the program starts")
            };

            let mut child = start(&marker);
            thread::sleep(PREEMPTED);
            assert!(!marker.exists(), "the program ran before it was in its job");
            let tree = Tree::adopt(&child).expect("the tree is made");
            assert!(child.wait().expect("the program is reaped").success());
            assert!(marker.exists(), "the program did not run once adopted");
            drop(tree);
            fs::remove_file(&marker).expect("the marker is removed");

            let mut child = start(&marker);
            child.kill().expect("a suspended program can be killed");
            child.wait().expect("the program is reaped");
            thread::sleep(PREEMPTED);
            assert!(!marker.exists(), "a killed program ran");
        }

        /// #460/V9: a session started with `spawn_detached` by a caller in a job that allows
        /// breakaway is not in that job, and goes on when the caller's job is closed with its
        /// caller. The caller's job is the one a run of this crate has (breakaway allowed, kill on
        /// close); the session is checked for membership first, so a refused breakaway, which falls
        /// back to staying in the job, fails the test with its own message instead of passing.
        #[test]
        fn a_detached_session_outlives_the_job_of_its_caller() {
            let mut command = this_binary("caller");
            prepare(&mut command);
            command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
            let mut caller = command.spawn().expect("the caller starts");
            let job = Tree::adopt(&caller).expect("the caller's tree is made");
            let session = Watched::open(announced(&mut caller, "SESSION"));
            caller.wait().expect("the caller is reaped");

            let mut inside = 0;
            // SAFETY: both handles are open and owned here; `inside` is a BOOL that the call sets.
            let checked = unsafe { IsProcessInJob(session.0.as_raw_handle(), job.job.as_raw_handle(), &mut inside) };
            assert!(checked != 0, "the membership of the session cannot be read");
            assert_eq!(inside, 0, "the session is in the job of its caller: the breakaway was refused");

            drop(job);
            assert!(!session.ends_within(Duration::from_millis(500)), "the session ended with the job of its caller");
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
        pub fn adopt_detached(child: &Child) -> io::Result<Option<Tree>> {
            Ok(group_of(child).ok().map(|group| Tree { group, kill_on_drop: false }))
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
