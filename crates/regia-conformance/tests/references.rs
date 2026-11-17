//! The resolver of the dossier's references against `tools/tests/test_riferimenti.py` of the dossier, its
//! fourteen tests one by one, on folders under the build's own temporary folder. The prototype's sentence
//! for what is not there is a catalog's, so here it is a value the test passes.
//!
//! Links: a symbolic link, or on Windows without the privilege a junction (`mklink /J`, which needs no
//! privilege), as the prototype's `collega_cartella` does. A test is skipped only when neither can be made,
//! and it says so. Its fourteenth test, that a result cannot be changed, holds by the type: `Reference`
//! has no setter and no public field.

use std::fs;
use std::path::{Path, PathBuf};

use regia_core::{ReferenceKind, Unresolved, data_root, resolve};

const ABSENT: &str = "not on this machine";

struct Dossier {
    root: PathBuf,
    dossier: PathBuf,
}

/// A clean root with a dossier holding `.regia/report/x.md`.
fn dossier(name: &str) -> Dossier {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("references").join(name);
    let _ = fs::remove_dir_all(&root);
    let dossier = root.join("dossier");
    fs::create_dir_all(dossier.join(".regia").join("report")).unwrap();
    fs::write(dossier.join(".regia").join("report").join("x.md"), "ciao").unwrap();
    let root = fs::canonicalize(&root).map(strip).unwrap();
    let dossier = root.join("dossier");
    Dossier { root, dossier }
}

/// The drive's form of a Windows verbatim path, as the resolver gives it.
fn strip(path: PathBuf) -> PathBuf {
    let text = path.display().to_string();
    text.strip_prefix(r"\\?\").filter(|rest| rest.chars().nth(1) == Some(':')).map_or(path, PathBuf::from)
}

/// What `link_folder` made.
#[derive(Clone, Copy, Debug)]
enum Link {
    Symlink,
    #[cfg(windows)]
    Junction,
}

/// A link to a folder: a symbolic link, or on Windows, when that is not allowed, a junction. `None` when
/// this machine allows neither.
fn link_folder(link: &Path, target: &Path) -> Option<Link> {
    #[cfg(unix)]
    return std::os::unix::fs::symlink(target, link).is_ok().then_some(Link::Symlink);
    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_dir(target, link).is_ok() {
            return Some(Link::Symlink);
        }
        junction(link, target).then_some(Link::Junction)
    }
}

/// `cmd /C mklink /J <link> <target>`: a junction needs no privilege. Started through the harness, the only
/// way a test starts a process (`only_regia_exec_starts_a_process`).
#[cfg(windows)]
fn junction(link: &Path, target: &Path) -> bool {
    let system = std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    let args: Vec<String> =
        ["/C", "mklink", "/J", &link.display().to_string(), &target.display().to_string()].map(str::to_owned).to_vec();
    regia_exec::harness::launch(&system.join("System32").join("cmd.exe"), &args, target)
        .run()
        .is_ok_and(|output| output.status.success())
        && link.is_dir()
}

/// Says which link a test used, or that it is skipped because the machine allows none.
fn report_link(test: &str, made: Option<Link>) -> bool {
    match made {
        Some(link) => {
            eprintln!("{test}: link made as {link:?}");
            true
        }
        None => {
            eprintln!("{test}: neither a symbolic link nor a junction can be made on this machine: the test is skipped");
            false
        }
    }
}

fn link_file(link: &Path, target: &Path) -> bool {
    #[cfg(unix)]
    return std::os::unix::fs::symlink(target, link).is_ok();
    #[cfg(windows)]
    return std::os::windows::fs::symlink_file(target, link).is_ok();
}

#[test]
fn a_relative_reference_that_is_there() {
    let d = dossier("present");
    let r = resolve(".regia/report/x.md", &d.dossier);
    assert_eq!((r.kind(), r.exists(), r.reason()), (ReferenceKind::Relative, true, None));
    assert_eq!(r.path(), Some(d.dossier.join(".regia").join("report").join("x.md").as_path()));
    assert!(r.path().unwrap().is_absolute());
    assert_eq!(r.render(ABSENT), "`.regia/report/x.md`");
}

#[test]
fn a_relative_reference_that_is_not_there() {
    let d = dossier("absent");
    let r = resolve(".regia/report/manca.md", &d.dossier);
    assert_eq!((r.kind(), r.exists(), r.reason()), (ReferenceKind::Relative, false, Some(Unresolved::Absent)));
    assert_eq!(r.render(ABSENT), format!("`.regia/report/manca.md`: {ABSENT}"));
}

#[test]
fn a_folder_is_not_a_file() {
    let d = dossier("folder");
    assert!(!resolve(".regia/report", &d.dossier).exists());
    assert_eq!(resolve(".regia/report", &d.dossier).render(ABSENT), format!("`.regia/report`: {ABSENT}"));
}

#[test]
fn a_data_reference_does_not_resolve() {
    let d = dossier("data");
    for reference in ["{dati}/x", "{dati}\\EXP-1\\x.csv", "{dati}"] {
        let r = resolve(reference, &d.dossier);
        assert_eq!((r.kind(), r.path(), r.exists()), (ReferenceKind::Data, None, false), "{reference}");
        assert_eq!(r.reason(), Some(Unresolved::DataRootNotConfigured));
    }
    assert_eq!(resolve("{dati}/x", &d.dossier).render(ABSENT), format!("`{{dati}}/x`: {ABSENT}"));
    assert_eq!(data_root(), None);
}

#[test]
fn the_data_placeholder_is_a_whole_segment() {
    let d = dossier("segment");
    fs::create_dir_all(d.dossier.join("{dati}-copia")).unwrap();
    fs::write(d.dossier.join("{dati}-copia").join("x.md"), "si").unwrap();
    let r = resolve("{dati}-copia/x.md", &d.dossier);
    assert_eq!((r.kind(), r.exists()), (ReferenceKind::Relative, true));
}

#[test]
fn a_data_reference_does_not_look_in_the_dossier() {
    let d = dossier("not-in-dossier");
    fs::create_dir_all(d.dossier.join("{dati}")).unwrap();
    fs::write(d.dossier.join("{dati}").join("x"), "no").unwrap();
    assert!(!resolve("{dati}/x", &d.dossier).exists());
}

#[test]
fn absolute_paths_of_windows_and_posix_do_not_resolve() {
    let d = dossier("absolute");
    let present = d.dossier.join(".regia").join("report").join("x.md").display().to_string();
    for reference in ["C:\\Users\\x\\f.md", "C:/Users/x/f.md", "c:f.md", "\\\\server\\share\\f.md", "/home/x/f.md", "/", present.as_str()] {
        let r = resolve(reference, &d.dossier);
        // Even where this machine has the file: it is to migrate to a relative one (#268).
        assert_eq!((r.kind(), r.path(), r.exists()), (ReferenceKind::Absolute, None, false), "{reference}");
        assert!(r.render(ABSENT).contains(ABSENT));
    }
}

#[test]
fn dot_dot_leads_out_of_the_dossier() {
    let d = dossier("dot-dot");
    fs::write(d.root.join("fuori.md"), "no").unwrap();
    for reference in ["../fuori.md", "..\\fuori.md", ".regia/../../fuori.md", ".."] {
        let r = resolve(reference, &d.dossier);
        assert_eq!((r.kind(), r.path(), r.exists()), (ReferenceKind::Outside, None, false), "{reference}");
    }
}

#[test]
fn a_link_out_of_the_dossier_leads_out_of_it() {
    let d = dossier("link-out");
    let outside = d.root.join("esterna");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("s.md"), "no").unwrap();
    if !report_link("a_link_out_of_the_dossier_leads_out_of_it", link_folder(&d.dossier.join("ln"), &outside)) {
        return;
    }
    let mut references = vec!["ln/s.md", "ln\\s.md"];
    if link_file(&d.dossier.join("ln.md"), &outside.join("s.md")) {
        references.push("ln.md");
    }
    for reference in references {
        let r = resolve(reference, &d.dossier);
        assert_eq!((r.kind(), r.path(), r.exists()), (ReferenceKind::Outside, None, false), "{reference}");
        assert_eq!(r.render(ABSENT), format!("`{}`: {ABSENT}", reference.replace('\\', "/")));
    }
}

#[test]
fn a_link_inside_the_dossier_stays_relative() {
    let d = dossier("link-in");
    let made = link_folder(&d.dossier.join("ln"), &d.dossier.join(".regia").join("report"));
    if !report_link("a_link_inside_the_dossier_stays_relative", made) {
        return;
    }
    let r = resolve("ln/x.md", &d.dossier);
    assert_eq!((r.kind(), r.exists()), (ReferenceKind::Relative, true));
}

#[test]
fn slashes_and_backslashes_are_the_same() {
    let d = dossier("slashes");
    let (a, b) = (resolve(".regia/report/x.md", &d.dossier), resolve(".regia\\report\\x.md", &d.dossier));
    assert_eq!((a.kind(), a.path(), a.exists(), a.normalized()), (b.kind(), b.path(), b.exists(), b.normalized()));
    assert_eq!(b.original(), ".regia\\report\\x.md");
    assert_eq!(b.render(ABSENT), a.render(ABSENT));
    assert_eq!(resolve(".regia\\report\\manca.md", &d.dossier).render(ABSENT), format!("`.regia/report/manca.md`: {ABSENT}"));
    assert_eq!(resolve("..\\x", &d.dossier).kind(), resolve("../x", &d.dossier).kind());
}

#[test]
fn an_empty_or_strange_reference_does_not_fail() {
    let d = dossier("strange");
    let long = "x".repeat(5000);
    for reference in ["", ".", "a?b", "a\0b", long.as_str()] {
        assert!(!resolve(reference, &d.dossier).exists(), "{}", &reference[..reference.len().min(20)]);
        assert!(resolve(reference, &d.dossier).render(ABSENT).contains(ABSENT));
    }
    // As the prototype's: a NUL names no path, and a name the system refuses is a path with no file.
    assert_eq!(resolve("a\0b", &d.dossier).kind(), ReferenceKind::Outside);
    assert_eq!(resolve("a?b", &d.dossier).kind(), ReferenceKind::Relative);
}

#[test]
fn a_reference_from_a_path_normalizes_its_separators() {
    let d = dossier("from-path");
    let reference = Path::new(".regia").join("report").join("x.md").display().to_string();
    assert_eq!(resolve(&reference, &d.dossier).normalized(), ".regia/report/x.md");
}

/// The prototype accepts a trailing separator, which its paths drop, and reads `X:` as a drive whatever `X`
/// is: one character, then a colon.
#[test]
fn a_trailing_separator_and_any_drive_letter_read_as_the_prototype_s() {
    let d = dossier("edges");
    assert!(resolve(".regia/report/x.md/", &d.dossier).exists());
    assert!(!resolve(".regia/report/x.md/more", &d.dossier).exists());
    for reference in ["a:b", "1:x", "é:x"] {
        assert_eq!(resolve(reference, &d.dossier).kind(), ReferenceKind::Absolute, "{reference}");
    }
}
