// The catalog's text, for the proofs that read the page: a proof asks for a message by its id, so its source spells
// no word of another language (regia-conformance refuses Italian words in the app's code).
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

/** Every one-line message of a language's catalog, by id. */
export function messages(ui, language) {
  const folder = join(ui, "../../crates/regia-i18n/locales", language);
  const found = {};
  for (const file of readdirSync(folder).filter((name) => name.endsWith(".ftl"))) {
    for (const match of readFileSync(join(folder, file), "utf8").matchAll(/^([a-z][\w-]*) = (.+)$/gm)) {
      found[match[1]] = match[2].trim();
    }
  }
  return found;
}

/** A one-line message with its variables, written as Fluent writes them: { $name }. */
export function format(text, args) {
  return text.replace(/\{ \$(\w+) \}/g, (_, name) => String(args[name]));
}
