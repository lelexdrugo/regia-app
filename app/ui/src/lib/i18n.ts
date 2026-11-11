import { FluentBundle, FluentResource, type FluentVariable } from "@fluent/bundle";

import type { LanguageTag } from "./bindings";

// Every catalog file of every language, read at build time from the crate the CLI renders from (ADR 0001
// §10): the CSP's connect-src admits the IPC only, so a fetch of a .ftl file would be refused. A new area
// is a new file in a language's folder, picked up here without a change.
const files = import.meta.glob<string>("../../../../crates/regia-i18n/locales/*/*.ftl", {
  query: "?raw",
  import: "default",
  eager: true,
});

const FALLBACK = "en";

function sources(language: string): string[] {
  return Object.entries(files)
    .filter(([path]) => path.split("/").at(-2) === language)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([, source]) => source);
}

export function languages(): string[] {
  const tags = Object.keys(files).map((path) => path.split("/").at(-2) ?? "");
  return [...new Set(tags)].filter((tag) => tag !== "").sort();
}

/** The languages a person can choose, in the order the choice shows them: the settings' `LanguageTag`. */
export const LANGUAGES: readonly LanguageTag[] = ["it", "en"];

export interface Catalog {
  readonly language: string;
  /** The message, or its id when the catalog has none, as the Rust catalog does. */
  message(id: string, args?: Record<string, FluentVariable>): string;
}

/** `product` is the product's name from `get_app_info`: the catalogs leave it out (#226). */
export function catalog(requested: string, product: string): Catalog {
  const language = languages().includes(requested) ? requested : FALLBACK;
  // No bidi isolation marks: the Rust catalog leaves them out too, and both render the same text.
  const bundle = new FluentBundle(language, { useIsolating: false });
  // The term the Rust catalog defines from the same name (regia-i18n's product_term).
  for (const source of [`-product = ${product}\n`, ...sources(language)]) {
    const errors = bundle.addResource(new FluentResource(source));
    if (errors.length > 0) {
      throw new Error(`catalog ${language}: ${errors.map(String).join("; ")}`);
    }
  }
  return {
    language,
    message(id, args) {
      const pattern = bundle.getMessage(id)?.value;
      return pattern ? bundle.formatPattern(pattern, args ?? null, []) : id;
    },
  };
}
