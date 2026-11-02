//! One catalog format for the CLI and the app. The catalogs are compiled in; the app's webview gets
//! its strings through the IPC, so there is a single Fluent runtime, in Rust (ADR 0001, "Languages").

use std::collections::BTreeMap;
use std::fmt;

use fluent_bundle::{FluentArgs, FluentBundle, FluentResource};
use unic_langid::LanguageIdentifier;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    En,
    It,
}

impl Language {
    pub const ALL: [Language; 2] = [Language::En, Language::It];

    pub fn tag(self) -> &'static str {
        match self {
            Language::En => "en",
            Language::It => "it",
        }
    }

    fn source(self) -> &'static str {
        match self {
            Language::En => include_str!("../locales/en/main.ftl"),
            Language::It => include_str!("../locales/it/main.ftl"),
        }
    }

    /// The `<PRODUCT>_LANG` variable, then the system locale, then English.
    pub fn detect() -> Language {
        let requested = std::env::var(regia_identity::env_var("LANG"))
            .ok()
            .filter(|value| !value.is_empty())
            .or_else(sys_locale::get_locale)
            .unwrap_or_default();
        if requested.to_lowercase().starts_with("it") { Language::It } else { Language::En }
    }
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.tag())
    }
}

pub struct Catalog {
    bundle: FluentBundle<FluentResource>,
    ids: Vec<String>,
}

impl Catalog {
    pub fn new(language: Language) -> Self {
        let resource = FluentResource::try_new(language.source().to_owned())
            .unwrap_or_else(|(_, errors)| panic!("invalid {language} catalog: {errors:?}"));
        let ids = resource
            .entries()
            .filter_map(|entry| match entry {
                fluent_syntax::ast::Entry::Message(message) => Some(message.id.name.to_owned()),
                _ => None,
            })
            .collect();
        let tag: LanguageIdentifier = language.tag().parse().expect("valid language tag");
        let mut bundle = FluentBundle::new(vec![tag]);
        bundle.set_use_isolating(false);
        bundle.add_resource(resource).expect("catalog ids are unique");
        Self { bundle, ids }
    }

    pub fn message(&self, id: &str) -> String {
        self.format(id, None)
    }

    pub fn message_with(&self, id: &str, name: &str, value: &str) -> String {
        let mut args = FluentArgs::new();
        args.set(name.to_owned(), value.to_owned());
        self.format(id, Some(&args))
    }

    fn format(&self, id: &str, args: Option<&FluentArgs<'_>>) -> String {
        let Some(pattern) = self.bundle.get_message(id).and_then(|m| m.value()) else {
            return id.to_owned();
        };
        let mut errors = Vec::new();
        self.bundle.format_pattern(pattern, args, &mut errors).into_owned()
    }

    pub fn ids(&self) -> &[String] {
        &self.ids
    }

    /// Every message without arguments filled in, for the webview. `{ $reason }` placeholders are
    /// kept as the literal `{reason}` for the frontend to replace.
    pub fn all(&self) -> BTreeMap<String, String> {
        self.ids
            .iter()
            .map(|id| (id.clone(), self.message_with(id, "reason", "{reason}")))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_has_exactly_the_same_messages() {
        let mut reference = Catalog::new(Language::En).ids().to_vec();
        reference.sort();
        for language in Language::ALL {
            let mut ids = Catalog::new(language).ids().to_vec();
            ids.sort();
            assert_eq!(ids, reference, "catalog {language}");
        }
    }

    #[test]
    fn languages_really_differ_and_arguments_are_filled() {
        let en = Catalog::new(Language::En);
        let it = Catalog::new(Language::It);
        assert_ne!(en.message("brief-ready"), it.message("brief-ready"));
        assert!(en.message_with("brief-error", "reason", "boom").contains("boom"));
    }
}
