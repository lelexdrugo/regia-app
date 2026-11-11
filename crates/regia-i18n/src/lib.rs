//! One catalog format for the CLI and the app (ADR 0001 §10). The catalogs are compiled in, one `.ftl`
//! file per area in each language's folder (`build.rs`); the app's webview renders the same files with
//! `@fluent/bundle`, imported at build time (`app/ui/src/lib/i18n.ts`).
//!
//! The product's name is not in the catalogs: both runtimes define the term `-product` from
//! `regia-identity` (#226).

use std::fmt;

use fluent_bundle::{FluentArgs, FluentBundle, FluentResource};
use unic_langid::LanguageIdentifier;

include!(concat!(env!("OUT_DIR"), "/sources.rs"));

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

    /// The language's catalog files, by name.
    fn sources(self) -> &'static [(&'static str, &'static str)] {
        sources(self.tag())
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

/// The Fluent term every catalog uses for the product's name, as the webview defines it too.
pub fn product_term() -> String {
    format!("-product = {}\n", regia_identity::PRODUCT_NAME)
}

pub struct Catalog {
    bundle: FluentBundle<FluentResource>,
    ids: Vec<String>,
}

impl Catalog {
    /// Every file of the language. A file that does not parse, or an id two files define, is a bug in
    /// the catalog, and the tests of this crate load every language.
    pub fn new(language: Language) -> Self {
        let tag: LanguageIdentifier = language.tag().parse().expect("valid language tag");
        let mut bundle = FluentBundle::new(vec![tag]);
        bundle.set_use_isolating(false);
        let term = FluentResource::try_new(product_term()).expect("the product term");
        bundle.add_resource(term).expect("the product term is defined once");
        let mut ids = Vec::new();
        for (name, source) in language.sources() {
            let resource = FluentResource::try_new((*source).to_owned())
                .unwrap_or_else(|(_, errors)| panic!("invalid catalog {language}/{name}: {errors:?}"));
            ids.extend(resource.entries().filter_map(|entry| match entry {
                fluent_syntax::ast::Entry::Message(message) => Some(message.id.name.to_owned()),
                _ => None,
            }));
            bundle.add_resource(resource).unwrap_or_else(|errors| panic!("{language}/{name} repeats an id: {errors:?}"));
        }
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
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use fluent_syntax::ast::{Entry, Expression, InlineExpression, Pattern, PatternElement};

    use super::*;

    /// What a translation must keep: the message's attributes, and every variable and function it uses.
    #[derive(Debug, Default, PartialEq, Eq)]
    struct Shape {
        attributes: BTreeSet<String>,
        variables: BTreeSet<String>,
        functions: BTreeSet<String>,
    }

    fn pattern(pattern: &Pattern<&str>, shape: &mut Shape) {
        for element in &pattern.elements {
            if let PatternElement::Placeable { expression } = element {
                self::expression(expression, shape);
            }
        }
    }

    fn expression(expression: &Expression<&str>, shape: &mut Shape) {
        match expression {
            Expression::Inline(inline) => self::inline(inline, shape),
            Expression::Select { selector, variants } => {
                inline(selector, shape);
                variants.iter().for_each(|variant| pattern(&variant.value, shape));
            }
        }
    }

    fn inline(inline: &InlineExpression<&str>, shape: &mut Shape) {
        match inline {
            InlineExpression::VariableReference { id } => {
                shape.variables.insert(id.name.to_owned());
            }
            InlineExpression::FunctionReference { id, arguments } => {
                shape.functions.insert(id.name.to_owned());
                arguments.positional.iter().for_each(|argument| self::inline(argument, shape));
                arguments.named.iter().for_each(|argument| self::inline(&argument.value, shape));
            }
            InlineExpression::TermReference { arguments: Some(arguments), .. } => {
                arguments.positional.iter().for_each(|argument| self::inline(argument, shape));
            }
            InlineExpression::Placeable { expression } => self::expression(expression, shape),
            _ => {}
        }
    }

    fn shapes(language: Language) -> BTreeMap<String, Shape> {
        let mut out = BTreeMap::new();
        for (_, source) in language.sources() {
            let resource = fluent_syntax::parser::parse(*source).expect("the catalog parses");
            for entry in &resource.body {
                if let Entry::Message(message) = entry {
                    let mut shape = Shape::default();
                    if let Some(value) = &message.value {
                        pattern(value, &mut shape);
                    }
                    for attribute in &message.attributes {
                        shape.attributes.insert(attribute.id.name.to_owned());
                        pattern(&attribute.value, &mut shape);
                    }
                    assert!(out.insert(message.id.name.to_owned(), shape).is_none(), "{language}: {} twice", message.id.name);
                }
            }
        }
        out
    }

    #[test]
    fn every_language_has_the_same_messages_attributes_and_variables() {
        let reference = shapes(Language::En);
        assert!(reference.len() > 400, "the catalog is read: {}", reference.len());
        for language in Language::ALL {
            let shapes = shapes(language);
            let missing: Vec<&String> = reference.keys().filter(|id| !shapes.contains_key(*id)).collect();
            let extra: Vec<&String> = shapes.keys().filter(|id| !reference.contains_key(*id)).collect();
            assert!(missing.is_empty() && extra.is_empty(), "{language}: missing {missing:?}, extra {extra:?}");
            for (id, shape) in &shapes {
                assert_eq!(shape, &reference[id], "{language}: {id}");
            }
            let mut ids = Catalog::new(language).ids().to_vec();
            ids.sort();
            assert_eq!(ids, reference.keys().cloned().collect::<Vec<_>>(), "{language}: the bundle loads every file");
        }
    }

    #[test]
    fn every_area_has_a_file_in_every_language() {
        let names = |language: Language| language.sources().iter().map(|(name, _)| *name).collect::<Vec<_>>();
        assert!(names(Language::En).len() >= 12, "{:?}", names(Language::En));
        for language in Language::ALL {
            assert_eq!(names(language), names(Language::En), "{language}");
        }
    }

    /// Rust formats every message but the dates, which only the webview has a DATETIME for: the CLI and
    /// the app read the same files.
    #[test]
    fn every_message_formats_in_rust_but_the_dates() {
        for language in Language::ALL {
            let catalog = Catalog::new(language);
            for (id, shape) in shapes(language) {
                if shape.functions.contains("DATETIME") {
                    continue;
                }
                let mut args = FluentArgs::new();
                for variable in &shape.variables {
                    args.set(variable.clone(), if variable == "n" { "2".into() } else { format!("[{variable}]") });
                }
                let message = catalog.bundle.get_message(&id).unwrap();
                let mut errors = Vec::new();
                catalog.bundle.format_pattern(message.value().unwrap(), Some(&args), &mut errors);
                assert!(errors.is_empty(), "{language}/{id}: {errors:?}");
            }
        }
    }

    #[test]
    fn the_product_name_comes_from_the_identity_crate() {
        let en = Catalog::new(Language::En);
        assert_eq!(en.message("setup-welcome-title"), format!("Let's set up {}", regia_identity::PRODUCT_NAME));
        for language in Language::ALL {
            for (name, source) in language.sources() {
                assert!(!source.contains("-product ="), "{language}/{name} defines the product's name");
            }
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
