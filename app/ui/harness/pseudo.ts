// The pseudo-locale "xx" of the catalog proof, as the UX prototype's: every message of the catalog wrapped in
// ⟦ ⟧ and made 40% longer. A text node with letters and without ⟦ ⟧ did not come from the catalog, and a view
// that fits in English but not in a longer language shows it in the limits proof.
import type { Catalog } from "../src/lib/i18n";

export function pseudo(base: Catalog): Catalog {
  return {
    language: base.language,
    message(id, args) {
      const text = base.message(id, args);
      // The catalog answers with the id itself when it has no such message.
      if (text === id) {
        console.error(`the catalog has no message ${id}`);
      }
      return `⟦${text}${"·".repeat(Math.ceil(text.length * 0.4))}⟧`;
    },
  };
}
