//! Where a program is, resolved once to an absolute path and pinned (threat model §3.6, rule 1,
//! and T06). The prototype calls `shutil.which` before every launch; here the search runs once per
//! pin, and a launch refuses a file that changed since.

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::{Args, ExecError, Launch, Program};

/// Where programs are looked for, in order: the absolute folders of `PATH`, then
/// [`usual_folders`]. A relative entry, which would mean the current folder, and a folder inside
/// the dossier are never searched.
#[derive(Debug, Clone)]
pub struct Search {
    folders: Vec<PathBuf>,
}

impl Search {
    /// `PATH` of this process and the usual folders, without the dossier named by `REGIA_HOME`.
    pub fn of_this_process() -> Search {
        let dossier = env::var_os(regia_identity::prototype_env_var("HOME"))
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        Search::from_parts(env::var_os("PATH").as_deref(), usual_folders(), dossier.as_slice())
    }

    pub fn from_parts(path: Option<&OsStr>, usual: Vec<PathBuf>, exclude: &[PathBuf]) -> Search {
        let mut folders: Vec<PathBuf> = Vec::new();
        for folder in path.map(env::split_paths).into_iter().flatten().chain(usual) {
            if folder.is_absolute() && !exclude.iter().any(|root| inside(&folder, root)) && !folders.contains(&folder) {
                folders.push(folder);
            }
        }
        Search { folders }
    }

    pub fn folders(&self) -> &[PathBuf] {
        &self.folders
    }

    pub fn find(&self, program: Program) -> Option<PathBuf> {
        program.file_names().iter().find_map(|name| self.find_named(name))
    }

    /// On Windows, `name.exe` in any folder wins over a `.cmd` or `.bat` in an earlier one: a batch
    /// file takes a narrower set of arguments (threat model §3.6, rule 9).
    pub fn find_named(&self, name: &str) -> Option<PathBuf> {
        let passes: &[&[&str]] = if cfg!(windows) { &[&["exe"], &["cmd", "bat"]] } else { &[&[]] };
        for extensions in passes {
            for folder in &self.folders {
                let mut candidates: Vec<PathBuf> =
                    extensions.iter().map(|extension| folder.join(name).with_extension(extension)).collect();
                if extensions.is_empty() {
                    candidates.push(folder.join(name));
                }
                if let Some(found) = candidates.into_iter().find(|candidate| executable(candidate)) {
                    return Some(found);
                }
            }
        }
        None
    }
}

/// Install folders searched after `PATH`. They answer the minimal `PATH` of an app started without
/// a shell: on macOS from the Finder it is `/usr/bin:/bin:/usr/sbin:/sbin` (ADR 0001 §5).
pub fn usual_folders() -> Vec<PathBuf> {
    let home = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf());
    let mut out: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        for (variable, folder) in [("ProgramFiles", "GitHub CLI"), ("ProgramFiles", "Git\\cmd"), ("APPDATA", "npm")] {
            if let Some(base) = env::var_os(variable).filter(|value| !value.is_empty()) {
                out.push(PathBuf::from(base).join(folder));
            }
        }
        out.extend(home.map(|home| home.join(".local").join("bin")));
    } else {
        if cfg!(target_os = "macos") {
            out.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
        }
        out.extend(home.map(|home| home.join(".local/bin")));
        out.extend(["/usr/local/bin", "/usr/bin", "/bin", "/home/linuxbrew/.linuxbrew/bin", "/snap/bin"].map(PathBuf::from));
    }
    out
}

/// Path prefixes compare without case on Windows, as the file system does.
fn inside(folder: &Path, root: &Path) -> bool {
    if cfg!(windows) {
        let lower = |path: &Path| PathBuf::from(path.to_string_lossy().to_lowercase());
        lower(folder).starts_with(lower(root))
    } else {
        folder.starts_with(root)
    }
}

fn executable(path: &Path) -> bool {
    match fs::metadata(path) {
        #[cfg(unix)]
        Ok(meta) => {
            use std::os::unix::fs::PermissionsExt;
            meta.is_file() && meta.permissions().mode() & 0o111 != 0
        }
        #[cfg(not(unix))]
        Ok(meta) => meta.is_file(),
        Err(_) => false,
    }
}

/// What a pinned file looked like. Size and modification time catch a replaced file; they are a
/// tripwire, not a defence against a process of the same user, which could restore both (§4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fingerprint {
    len: u64,
    modified: Option<SystemTime>,
}

impl Fingerprint {
    fn of(path: &Path) -> Option<Fingerprint> {
        let meta = fs::metadata(path).ok()?;
        meta.is_file().then(|| Fingerprint { len: meta.len(), modified: meta.modified().ok() })
    }
}

/// A program at an absolute path, and what its file looked like when it was resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    program: Program,
    path: PathBuf,
    seen: Fingerprint,
}

impl Pin {
    /// `REGIA_<PROGRAM>` when it is set, as an absolute path or a file name to look up; otherwise
    /// the search of this process. An explicit value never falls back to the search.
    pub fn resolve(program: Program) -> Result<Pin, ExecError> {
        let search = Search::of_this_process();
        match env::var_os(program.env_name()).filter(|value| !value.is_empty()) {
            Some(value) => Pin::named(program, Path::new(&value), &search),
            None => Pin::search(program, &search),
        }
    }

    pub fn search(program: Program, search: &Search) -> Result<Pin, ExecError> {
        let path = search.find(program).ok_or_else(|| not_found(program))?;
        Pin::at(program, &path)
    }

    /// A value the user gave: an absolute path as it is, a bare file name looked up in `search`.
    pub fn named(program: Program, value: &Path, search: &Search) -> Result<Pin, ExecError> {
        if value.is_absolute() {
            return Pin::at(program, value);
        }
        let bare = value.components().count() == 1;
        let found = value.to_str().filter(|_| bare).and_then(|name| search.find_named(name));
        Pin::at(program, &found.ok_or_else(|| not_found(program))?)
    }

    /// The file at an absolute path, as the setup will confirm it, or as a test names a fake.
    pub fn at(program: Program, path: &Path) -> Result<Pin, ExecError> {
        let seen = Fingerprint::of(path).filter(|_| path.is_absolute() && executable(path));
        let seen = seen.ok_or_else(|| ExecError::NotAFile { program, path: path.to_path_buf() })?;
        Ok(Pin { program, path: path.to_path_buf(), seen })
    }

    pub fn program(&self) -> Program {
        self.program
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The file is still the one that was resolved.
    pub(crate) fn check(&self) -> Result<(), ExecError> {
        if Fingerprint::of(&self.path) == Some(self.seen) {
            Ok(())
        } else {
            Err(ExecError::Changed { program: self.program, path: self.path.clone() })
        }
    }

    /// Starts nothing: describes a run of this program with `args`, in `cwd`.
    pub fn launch(&self, args: Args, cwd: &Path) -> Launch {
        Launch::program(self.clone(), args, cwd)
    }
}

fn not_found(program: Program) -> ExecError {
    ExecError::NotFound { program, variable: program.env_name() }
}
