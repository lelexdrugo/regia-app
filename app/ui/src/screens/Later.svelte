<script lang="ts">
  // A section whose screen comes with a later slice: the sidebar and the keyboard already lead here, and the page
  // says so instead of showing an empty frame.
  import type { Catalog } from "../lib/i18n";

  interface Props {
    strings: Catalog;
    /** The message id of the section's name. */
    title: string;
  }

  let { strings, title }: Props = $props();
</script>

<div class="screen">
  <header class="titlerow">
    <h1 tabindex="-1">{strings.message(title)}</h1>
  </header>
  <p class="note">{strings.message("shell-later")}</p>
</div>
