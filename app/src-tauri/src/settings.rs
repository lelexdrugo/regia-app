//! The app's own settings: the design system's direction, the density and the language (#475). One TOML file
//! in the per-user configuration folder, beside the agents' `agents.toml`, written only by `set_settings` and
//! read before the first paint: `index.html` is served with them on its root element, so the page is drawn in
//! them from its first frame, after a restart as after a reload. Every value is a closed enum, so nothing from
//! the file reaches the page but these names; a value the app does not know reads as the default.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use regia_i18n::Language;
use serde::{Deserialize, Serialize};
use tauri::utils::assets::{AssetKey, AssetsIter, CspHash};
use tauri::webview::Color;
use tauri::{App, Runtime, Theme};

/// The file's name in the configuration folder.
pub const FILE: &str = "interface.toml";

/// The design system's three directions (`app/ui/src/tokens.css`), by the names it gives them: Sala di montaggio
/// by default, Foglio for daylight, Nero as the alternative.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Direction {
    #[default]
    Montaggio,
    Foglio,
    Nero,
}

impl Direction {
    pub fn name(self) -> &'static str {
        match self {
            Direction::Montaggio => "montaggio",
            Direction::Foglio => "foglio",
            Direction::Nero => "nero",
        }
    }

    /// Its `--ground`: the colour of the window until the page draws, and of the title bar's side.
    pub fn ground(self) -> Color {
        match self {
            Direction::Montaggio => Color(0x14, 0x12, 0x0f, 0xff),
            Direction::Foglio => Color(0xf3, 0xf3, 0xf1, 0xff),
            Direction::Nero => Color(0x00, 0x00, 0x00, 0xff),
        }
    }

    pub fn theme(self) -> Theme {
        match self {
            Direction::Foglio => Theme::Light,
            Direction::Montaggio | Direction::Nero => Theme::Dark,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Density {
    #[default]
    Compact,
    Comfortable,
}

impl Density {
    pub fn name(self) -> &'static str {
        match self {
            Density::Compact => "compact",
            Density::Comfortable => "comfortable",
        }
    }
}

/// A language of the catalog, as the settings name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum LanguageTag {
    En,
    It,
}

impl LanguageTag {
    pub fn language(self) -> Language {
        match self {
            LanguageTag::En => Language::En,
            LanguageTag::It => Language::It,
        }
    }
}

/// What `get_settings` answers, and `set_settings` after a change.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Settings {
    pub direction: Direction,
    pub density: Density,
    /// The language chosen in the app, or `null` when none was: then the system's.
    pub language: Option<LanguageTag>,
}

impl Settings {
    /// The language the app speaks: `<PRODUCT>_LANG` when it is set, then the one chosen here, then the
    /// system's. The variable comes first so that `app-smoke` and a terminal can still ask for one.
    pub fn speaks(&self) -> Language {
        let variable = std::env::var(regia_identity::env_var("LANG")).is_ok_and(|value| !value.is_empty());
        match self.language {
            Some(tag) if !variable => tag.language(),
            _ => Language::detect(),
        }
    }
}

/// What `set_settings` takes: only the fields to change. A field the app does not know is refused.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SettingsChange {
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub direction: Option<Direction>,
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub density: Option<Density>,
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub language: Option<LanguageTag>,
}

/// Where the file is. The app manages one: the configuration folder in a release build, the smoke run's own
/// folder in a test build (`main.rs`), a temporary folder in the tests. `None` when the system has no folder.
#[derive(Debug, Clone)]
pub struct Store {
    dir: Option<PathBuf>,
}

/// One key of the file, read as `T`, or `None` when it is missing or holds a value the app does not know.
fn key<T: for<'de> Deserialize<'de>>(table: &toml::Table, name: &str) -> Option<T> {
    table.get(name).cloned().and_then(|value| value.try_into().ok())
}

impl Store {
    pub fn new(dir: Option<PathBuf>) -> Store {
        Store { dir }
    }

    fn path(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|dir| dir.join(FILE))
    }

    /// The file as a table: empty when it is missing, unreadable or not TOML, so the app always opens.
    fn table(&self) -> toml::Table {
        self.path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| text.parse::<toml::Table>().ok())
            .unwrap_or_default()
    }

    pub fn load(&self) -> Settings {
        let table = self.table();
        Settings {
            direction: key(&table, "direction").unwrap_or_default(),
            density: key(&table, "density").unwrap_or_default(),
            language: key(&table, "language"),
        }
    }

    /// Writes the fields of `change` and keeps every other key of the file, then answers what is saved. The
    /// file is replaced whole, through a temporary one, so a crash leaves the old one or the new one.
    pub fn change(&self, change: SettingsChange) -> Result<Settings, String> {
        let path = self.path().ok_or("this system has no configuration folder")?;
        let mut table = self.table();
        let mut set = |name: &str, value: Option<&'static str>| {
            if let Some(value) = value {
                table.insert(name.to_owned(), toml::Value::String(value.to_owned()));
            }
        };
        set("direction", change.direction.map(Direction::name));
        set("density", change.density.map(Density::name));
        set("language", change.language.map(|tag| tag.language().tag()));
        let text = toml::to_string(&table).map_err(|error| error.to_string())?;
        write(&path, &text).map_err(|error| format!("{}: {error}", path.display()))?;
        Ok(self.load())
    }
}

fn write(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let partial = path.with_extension("toml.partial");
    std::fs::write(&partial, text)?;
    std::fs::rename(&partial, path)
}

/// `page` with the settings on its root element, which `index.html` writes as a bare `<html>`. Only the enums'
/// names are written: no text from the file reaches the page.
pub fn first_paint(page: &str, settings: &Settings) -> Option<String> {
    let root = format!(
        "<html lang=\"{}\" data-dir=\"{}\" data-density=\"{}\">",
        settings.speaks().tag(),
        settings.direction.name(),
        settings.density.name()
    );
    page.contains("<html>").then(|| page.replacen("<html>", &root, 1))
}

/// The app's files as Tauri serves them, with `index.html` drawn in the saved settings (`first_paint`). Every
/// other file, and the hashes of the CSP, are the embedded ones.
pub struct Assets<R: Runtime> {
    embedded: Box<dyn tauri::Assets<R>>,
    store: Store,
}

/// Serves `context`'s files through `Assets`.
pub fn draw_first_paint<R: Runtime>(context: &mut tauri::Context<R>, store: Store) {
    let embedded = context.set_assets(Box::new(Moving));
    context.set_assets(Box::new(Assets { embedded, store }));
}

/// Holds the place of the embedded files while they move into `Assets`, for one statement.
struct Moving;

impl<R: Runtime> tauri::Assets<R> for Moving {
    fn get(&self, _: &AssetKey) -> Option<Cow<'_, [u8]>> {
        None
    }

    fn iter(&self) -> Box<AssetsIter<'_>> {
        Box::new(std::iter::empty())
    }

    fn csp_hashes(&self, _: &AssetKey) -> Box<dyn Iterator<Item = CspHash<'_>> + '_> {
        Box::new(std::iter::empty())
    }
}

impl<R: Runtime> tauri::Assets<R> for Assets<R> {
    fn setup(&self, app: &App<R>) {
        self.embedded.setup(app);
    }

    fn get(&self, key: &AssetKey) -> Option<Cow<'_, [u8]>> {
        let file = self.embedded.get(key)?;
        if key.as_ref() != "/index.html" {
            return Some(file);
        }
        let drawn = std::str::from_utf8(&file).ok().and_then(|page| first_paint(page, &self.store.load()));
        Some(drawn.map_or(file, |page| Cow::Owned(page.into_bytes())))
    }

    fn iter(&self) -> Box<AssetsIter<'_>> {
        self.embedded.iter()
    }

    fn csp_hashes(&self, html_path: &AssetKey) -> Box<dyn Iterator<Item = CspHash<'_>> + '_> {
        self.embedded.csp_hashes(html_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of its own under the workspace's `target/`, emptied first: never the user's configuration.
    fn folder(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-settings").join(name);
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn page() -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../ui/dist/index.html");
        std::fs::read_to_string(path).expect("app/ui/dist/index.html: build.rs refuses to build without it")
    }

    #[test]
    fn without_a_file_the_app_is_in_the_defaults() {
        let store = Store::new(Some(folder("none")));
        assert_eq!(store.load(), Settings { direction: Direction::Montaggio, density: Density::Compact, language: None });
        assert_eq!(Store::new(None).load(), Settings::default());
        assert!(Store::new(None).change(SettingsChange::default()).is_err(), "no folder, nothing to write");
    }

    #[test]
    fn a_change_is_saved_and_read_back_and_other_keys_stay() {
        let dir = folder("change");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE), "future = \"kept\"\ndensity = \"comfortable\"\n").unwrap();
        let store = Store::new(Some(dir.clone()));
        let saved = store.change(SettingsChange { direction: Some(Direction::Nero), ..Default::default() }).unwrap();
        assert_eq!(saved, Settings { direction: Direction::Nero, density: Density::Comfortable, language: None });
        let saved = store.change(SettingsChange { language: Some(LanguageTag::It), ..Default::default() }).unwrap();
        assert_eq!(saved.language, Some(LanguageTag::It));
        assert_eq!(saved.direction, Direction::Nero, "a change leaves the other fields as they were");
        let text = std::fs::read_to_string(dir.join(FILE)).unwrap();
        assert!(text.contains("future = \"kept\""), "{text}");
        assert!(!dir.join("interface.toml.partial").exists());
    }

    #[test]
    fn a_value_the_app_does_not_know_reads_as_the_default() {
        let dir = folder("unknown");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE), "direction = \"purple\"\ndensity = 3\nlanguage = \"fr\"\n").unwrap();
        assert_eq!(Store::new(Some(dir.clone())).load(), Settings::default());
        std::fs::write(dir.join(FILE), "not toml at all [").unwrap();
        assert_eq!(Store::new(Some(dir)).load(), Settings::default());
    }

    #[test]
    fn the_change_refuses_a_field_it_does_not_know() {
        let known: SettingsChange = serde_json::from_str(r#"{"direction":"foglio"}"#).unwrap();
        assert_eq!(known.direction, Some(Direction::Foglio));
        for refused in [r#"{"theme":"nero"}"#, r#"{"direction":"purple"}"#, r#"{"direction":"<script>"}"#, r#"{"path":"/x"}"#] {
            assert!(serde_json::from_str::<SettingsChange>(refused).is_err(), "{refused}");
        }
    }

    #[test]
    fn the_served_page_is_drawn_in_the_saved_settings() {
        let page = page();
        assert_eq!(page.matches("<html").count(), 1, "one root element, written bare: {page}");
        for (direction, density) in [
            (Direction::Montaggio, Density::Compact),
            (Direction::Foglio, Density::Comfortable),
            (Direction::Nero, Density::Compact),
        ] {
            let settings = Settings { direction, density, language: Some(LanguageTag::It) };
            let drawn = first_paint(&page, &settings).expect("index.html has a bare <html>");
            let start = drawn.find("<html").unwrap();
            let root = &drawn[start..=start + drawn[start..].find('>').unwrap()];
            assert!(root.ends_with(&format!("data-dir=\"{}\" data-density=\"{}\">", direction.name(), density.name())), "{root}");
            assert_eq!(drawn.replacen(root, "<html>", 1), page, "only the root element changes");
        }
    }

    #[test]
    fn nothing_from_the_file_reaches_the_page() {
        let dir = folder("hostile");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE), "direction = \"nero\\\"><script>alert(1)</script>\"\nlanguage = \"it\\\" onload=\\\"x\"\n").unwrap();
        let settings = Store::new(Some(dir)).load();
        assert_eq!(settings, Settings::default());
        let drawn = first_paint(&page(), &settings).unwrap();
        assert!(!drawn.contains("script>alert") && !drawn.contains("onload"), "{drawn}");
        assert!(drawn.contains("data-dir=\"montaggio\""));
    }

    #[test]
    fn the_window_colour_is_the_ground_of_tokens_css() {
        let css = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../ui/src/tokens.css")).unwrap();
        for direction in [Direction::Montaggio, Direction::Foglio, Direction::Nero] {
            let block = css.split(&format!("[data-dir=\"{}\"]", direction.name())).nth(1).expect("the direction's block");
            let ground = block.split("--ground:").nth(1).unwrap().trim_start()[..7].to_lowercase();
            let Color(r, g, b, _) = direction.ground();
            assert_eq!(ground, format!("#{r:02x}{g:02x}{b:02x}"), "{}", direction.name());
        }
    }
}
