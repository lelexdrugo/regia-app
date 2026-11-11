<script lang="ts">
  // The languages of the catalog, each by its own name in its own language, so that someone who landed in the
  // other one still finds theirs. A group of radio buttons: the arrow keys move within it, and a choice applies
  // at once, without reloading (App.svelte swaps the catalog).
  import type { LanguageTag } from "../lib/bindings";
  import { type Catalog, LANGUAGES } from "../lib/i18n";

  interface Props {
    strings: Catalog;
    /** Each language's catalog, for its own name. */
    catalogFor: (language: LanguageTag) => Catalog;
    /** Tells the radio groups of the page apart. */
    name: string;
    /** The heading that names the group, or none: then the catalog's "Language". */
    labelledby?: string;
    onlanguage: (language: LanguageTag) => void;
  }

  let { strings, catalogFor, name, labelledby, onlanguage }: Props = $props();
</script>

<div
  class="choices"
  role="radiogroup"
  aria-labelledby={labelledby}
  aria-label={labelledby ? undefined : strings.message("nav-language")}
>
  {#each LANGUAGES as language (language)}
    <label class="choice">
      <input
        type="radio"
        {name}
        value={language}
        checked={strings.language === language}
        onchange={() => onlanguage(language)}
      />
      <span lang={language}>{catalogFor(language).message("language-self")}</span>
    </label>
  {/each}
</div>
