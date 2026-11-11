//! The property of `--file` (conformance study §9.3 of the dossier): no sequence of components leaves the
//! project's folder or names `.git`, checked against a model that resolves the path by brute force, as
//! the prototype's `Path.resolve` does with no link on the way.

use std::fs;
use std::path::{Component, Path, PathBuf};

use proptest::prelude::*;
use regia_git::{named_files, named_paths};

/// Where a `--file` lands, by the model.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Landing {
    Inside(String),
    Leaves,
    WholeOrGit,
    Untyped,
}

/// The folder the paths are relative to: real, with a `.git` and a `docs` inside, so some parts exist.
fn root() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("regia-git-paths").join("work");
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::canonicalize(&root).unwrap();
    root
}

/// The model: the folder's own components and the given ones, `.` dropped and `..` taking one back,
/// never above the top; then where that lands.
fn model(root: &Path, parts: &[String]) -> Landing {
    let top: Vec<String> = root.components().map(|part| part.as_os_str().to_string_lossy().into_owned()).collect();
    // The drive and the root of the file system: `..` goes no higher.
    let floor = root.components().take_while(|part| matches!(part, Component::Prefix(_) | Component::RootDir)).count();
    let mut path = top.clone();
    for part in parts {
        match part.as_str() {
            "." | "" => {}
            ".." => {
                if path.len() > floor {
                    path.pop();
                }
            }
            other => path.push(other.to_owned()),
        }
    }
    if path.len() < top.len() || path[..top.len()] != top[..] {
        return Landing::Leaves;
    }
    let inside = &path[top.len()..];
    if inside.first().is_none_or(|first| first.to_lowercase() == ".git") {
        return Landing::WholeOrGit;
    }
    let typed = inside.iter().all(|part| {
        !part.starts_with('-')
            && !part.ends_with('.')
            && part.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
            && !matches!(part.split('.').next().unwrap().to_ascii_uppercase().as_str(), "CON" | "PRN" | "AUX" | "NUL")
    });
    if typed { Landing::Inside(inside.join("/")) } else { Landing::Untyped }
}

fn landing(root: &Path, given: &str) -> Landing {
    match named_paths(root, &[given.to_owned()]) {
        Ok(paths) => {
            let extra = paths.len() > named_files().len();
            Landing::Inside(if extra { paths.last().unwrap().as_str().to_owned() } else { "(a named file)".to_owned() })
        }
        Err(reason) if reason.ends_with("leaves the project's folder") => Landing::Leaves,
        Err(reason) if reason.ends_with("names the whole folder or .git") => Landing::WholeOrGit,
        Err(reason) if reason.contains("invalid dossier path") => Landing::Untyped,
        Err(reason) => panic!("unexpected refusal of {given:?}: {reason}"),
    }
}

fn part() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("..".to_owned()),
        Just(".".to_owned()),
        Just(".git".to_owned()),
        Just(".GIT".to_owned()),
        Just("docs".to_owned()),
        Just("work".to_owned()),
        Just("a.md".to_owned()),
        Just(".regia".to_owned()),
        Just("x y".to_owned()),
        Just("è".to_owned()),
        Just("-x".to_owned()),
        "[a-z]{1,3}",
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn no_sequence_leaves_the_folder_or_names_git(parts in prop::collection::vec(part(), 1..7), backslash in any::<bool>()) {
        let root = root();
        let separator = if backslash && cfg!(windows) { "\\" } else { "/" };
        let given = parts.join(separator);
        let expected = model(&root, &parts);
        let got = landing(&root, &given);
        // A named file given again is kept once: the model does not know the named ones.
        let named = named_files().iter().any(|file| matches!(&expected, Landing::Inside(path) if path == file.as_str()));
        if !named {
            prop_assert_eq!(&got, &expected, "{}", given);
        }
        if let Landing::Inside(path) = &got {
            let joined = root.join(path);
            prop_assert!(joined.starts_with(&root), "{} leaves the folder", given);
            prop_assert!(!path.is_empty() && !path.split('/').next().unwrap().eq_ignore_ascii_case(".git"), "{} names .git", given);
            prop_assert!(!path.split('/').any(|part| part == ".." || part == "."), "{} is not resolved", given);
        }
    }
}

#[test]
fn a_named_file_given_again_is_kept_once() {
    let root = root();
    let named = named_files();
    let again = named_paths(&root, &[named[0].as_str().to_owned(), "docs/a.md".to_owned(), "docs/./a.md".to_owned()]).unwrap();
    let names: Vec<&str> = again.iter().map(|path| path.as_str()).collect();
    let mut expected: Vec<&str> = named.iter().map(|path| path.as_str()).collect();
    expected.push("docs/a.md");
    assert_eq!(names, expected);
}
