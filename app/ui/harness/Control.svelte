<script lang="ts">
  // The negative control of the catalog and limits proofs (prove/): the mistakes each one must find, on a page
  // of the harness only. A string written in the code, an element wider than any window, a raw value and a
  // parameter left in braces.
  import type { Catalog } from "../src/lib/i18n";

  interface Props {
    strings: Catalog;
  }

  let { strings }: Props = $props();
  const missing: string | undefined = undefined;
</script>

<h1>{strings.message("app-title")}</h1>
<p>Written in the code, not in the catalog</p>
<div class="wide"></div>
<p>{String(missing)} {"{reason}"}</p>

<style>
  .wide {
    width: 4000px;
    height: 1rem;
  }
</style>
