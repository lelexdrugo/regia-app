//! The resolver of the dossier's references against `tools/tests/test_riferimenti.py` of the dossier (as
//! #530 changed it: forms that are no path, folders that are there, the empty reference), its tests one by
//! one, on folders under the build's own temporary folder. The prototype's sentences for what is not there,
//! and for a form that is no path, are a catalog's, so here they are values the test passes.
//!
//! Links: a symbolic link, or on Windows without the privilege a junction (`mklink /J`, which needs no
//! privilege), as the prototype's `collega_cartella` does. A test is skipped only when neither can be made,
//! and it says so. Its fourteenth test, that a result cannot be changed, holds by the type: `Reference`
//! has no setter and no public field.

use std::fs;
use std::path::{Path, PathBuf};

use regia_core::{NotAPathForm, ReferenceKind, Unresolved, data_root, resolve};

const ABSENT: &str = "not on this machine";

/// The prototype's `motivo` of a `non-percorso`, in the words the test compares the product's forms by.
fn form_name(form: NotAPathForm) -> &'static str {
    match form {
        NotAPathForm::Link => "link",
        NotAPathForm::Command => "comando",
        NotAPathForm::MachineLocation => "sede della macchina",
        NotAPathForm::NamePlaceholder => "segnaposto <nome>",
        NotAPathForm::UnknownPlaceholder => "segnaposto sconosciuto",
    }
}

fn not_a_path_sentence(form: NotAPathForm) -> String {
    format!("not a path ({})", form_name(form))
}

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

/// `test_una_cartella_che_esiste_e_presente`: a folder that is there is there, and says it is a folder.
#[test]
fn a_folder_that_is_there_is_present() {
    let d = dossier("folder");
    for reference in [".regia/report", ".regia/report/", ".regia", ".regia\\report", "."] {
        let r = resolve(reference, &d.dossier);
        assert_eq!((r.kind(), r.exists(), r.is_folder(), r.reason()), (ReferenceKind::Relative, true, true, None), "{reference}");
        assert!(r.path().is_some_and(Path::is_dir), "{reference}");
    }
    assert_eq!(resolve(".regia/report", &d.dossier).render(ABSENT), "`.regia/report/`");
    assert_eq!(resolve(".regia/report/", &d.dossier).render(ABSENT), "`.regia/report/`", "the slash is not doubled");
    assert_eq!(resolve(".regia\\report", &d.dossier).render(ABSENT), "`.regia/report/`");
}

/// `test_un_file_non_e_una_cartella`.
#[test]
fn a_file_is_not_a_folder() {
    let d = dossier("file-not-folder");
    let r = resolve(".regia/report/x.md", &d.dossier);
    assert_eq!((r.exists(), r.is_folder()), (true, false));
    assert!(!resolve(".regia/report/manca.md", &d.dossier).is_folder());
}

/// `test_una_cartella_che_non_c_e_e_assente`.
#[test]
fn a_folder_that_is_not_there_is_absent() {
    let d = dossier("folder-absent");
    let r = resolve(".regia/prove/342/", &d.dossier);
    assert_eq!((r.kind(), r.exists(), r.is_folder(), r.reason()), (ReferenceKind::Relative, false, false, Some(Unresolved::Absent)));
    assert_eq!(r.render(ABSENT), format!("`.regia/prove/342/`: {ABSENT}"));
}

/// `test_non_percorso`: a link, a command, a place of the machine, a placeholder: no path, whatever the
/// disk holds, and a document shows the reference as written.
#[test]
fn a_form_that_is_no_path_does_not_resolve() {
    let d = dossier("not-a-path");
    let cases = [
        ("https://x/y", NotAPathForm::Link),
        ("http://github.com/lelexdrugo/regia/blob/main/x.md", NotAPathForm::Link),
        ("file:///C:/x", NotAPathForm::Link),
        ("file:///home/x/y", NotAPathForm::Link),
        ("python tools/x.py --a", NotAPathForm::Command),
        ("python -m unittest → exit 0", NotAPathForm::Command),
        ("diff -r skills/regia ~/.claude/skills/regia", NotAPathForm::Command),
        ("git status", NotAPathForm::Command),
        ("~/x", NotAPathForm::MachineLocation),
        ("~", NotAPathForm::MachineLocation),
        ("~\\x", NotAPathForm::MachineLocation),
        ("%LOCALAPPDATA%/x", NotAPathForm::MachineLocation),
        ("%LOCALAPPDATA%\\regia\\tracker.json", NotAPathForm::MachineLocation),
        ("%ProgramFiles(x86)%/x", NotAPathForm::MachineLocation),
        ("$HOME/x", NotAPathForm::MachineLocation),
        ("${HOME}/x", NotAPathForm::MachineLocation),
        ("<worktree>/x", NotAPathForm::NamePlaceholder),
        ("<bytesized>", NotAPathForm::NamePlaceholder),
        ("{archivio}/x", NotAPathForm::UnknownPlaceholder),
        ("{archivio}", NotAPathForm::UnknownPlaceholder),
    ];
    for (reference, form) in cases {
        let r = resolve(reference, &d.dossier);
        assert_eq!(r.kind(), ReferenceKind::NotAPath(form), "{reference}");
        assert_eq!((r.path(), r.exists(), r.is_folder(), r.reason()), (None, false, false, Some(Unresolved::NotAPath)), "{reference}");
        // As written, not normalized, and never "not on this machine".
        assert_eq!(r.render_with(ABSENT, not_a_path_sentence), format!("`{reference}`: not a path ({})", form_name(form)), "{reference}");
        assert!(!r.render_with(ABSENT, not_a_path_sentence).contains(ABSENT), "{reference}");
        assert_eq!(r.render(ABSENT), format!("`{reference}`"), "{reference}");
    }
}

/// `test_non_percorso_non_guarda_il_disco`.
#[test]
fn a_form_that_is_no_path_does_not_look_at_the_disk() {
    let d = dossier("not-a-path-disk");
    fs::create_dir_all(d.dossier.join("~")).unwrap();
    fs::write(d.dossier.join("~").join("x"), "si").unwrap();
    fs::create_dir_all(d.dossier.join("%X%")).unwrap();
    assert!(matches!(resolve("~/x", &d.dossier).kind(), ReferenceKind::NotAPath(NotAPathForm::MachineLocation)));
    assert!(matches!(resolve("%X%/y", &d.dossier).kind(), ReferenceKind::NotAPath(NotAPathForm::MachineLocation)));
}

/// `test_assoluti_con_forme_che_somigliano_ad_altro_restano_assoluti`: the candidates of the dossier's check
/// `percorso-assoluto` keep their kind.
#[test]
fn absolute_paths_that_look_like_something_else_stay_absolute() {
    let d = dossier("absolute-lookalikes");
    for reference in [
        "C://Users/x",
        "C:\\\\Users\\x",
        "C:\\Program Files\\x y",
        "C:/Program Files (x86)/x",
        "/c/Users/x",
        "/home/x y/z",
        "\\\\server\\share\\x",
        "/mnt/c/Users/x",
        "/tmp/x",
    ] {
        assert_eq!(resolve(reference, &d.dossier).kind(), ReferenceKind::Absolute, "{reference}");
    }
}

/// `test_forme_che_restano_relative`: a shape that looks like a form but is not one, and a file with a space
/// in its name that is there is not a command.
#[test]
fn shapes_that_look_like_a_form_stay_relative() {
    let d = dossier("relative-lookalikes");
    fs::create_dir_all(d.dossier.join("docs")).unwrap();
    fs::write(d.dossier.join("docs").join("due parole.md"), "si").unwrap();
    for reference in ["docs/design/x.md", "{dati}-copia/x", "a<b>/x", "<x", "$x.md", "%x", "~x", "100%/x", "x/{archivio}/y"] {
        assert_eq!(resolve(reference, &d.dossier).kind(), ReferenceKind::Relative, "{reference}");
    }
    let r = resolve("docs/due parole.md", &d.dossier);
    assert_eq!((r.kind(), r.exists(), r.is_folder()), (ReferenceKind::Relative, true, false));
    // A known limit of the prototype: the space makes it a command.
    assert_eq!(resolve("docs/manca per davvero.md", &d.dossier).kind(), ReferenceKind::NotAPath(NotAPathForm::Command));
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
    for reference in ["", " ", "a?b", "a\0b", long.as_str()] {
        assert!(!resolve(reference, &d.dossier).exists(), "{}", &reference[..reference.len().min(20)]);
        assert!(resolve(reference, &d.dossier).render(ABSENT).contains(ABSENT));
    }
    // As the prototype's: a NUL names no path, and a name the system refuses is a path with no file.
    assert_eq!(resolve("a\0b", &d.dossier).kind(), ReferenceKind::Outside);
    assert_eq!(resolve("a?b", &d.dossier).kind(), ReferenceKind::Relative);
    // The dot is the dossier itself, the empty reference is not.
    assert!(resolve(".", &d.dossier).is_folder());
}

/// `test_riferimento_vuoto_o_con_caratteri_strani_non_solleva`, the empty case: a relative reference that
/// is not there and has no path, not the dossier's folder.
#[test]
fn the_empty_reference_is_not_the_dossier_itself() {
    let d = dossier("empty");
    for reference in ["", " ", "  ", "\t"] {
        let r = resolve(reference, &d.dossier);
        assert_eq!((r.kind(), r.path(), r.exists(), r.is_folder()), (ReferenceKind::Relative, None, false, false), "{reference:?}");
        assert_eq!(r.reason(), Some(Unresolved::Absent), "{reference:?}");
        assert_eq!(r.render(ABSENT), format!("`{reference}`: {ABSENT}"), "{reference:?}");
    }
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
