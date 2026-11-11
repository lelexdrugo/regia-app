<script lang="ts">
  // Text written by others, from the core's tree (threat model §3.7): every node is an element of a fixed
  // list, built by Svelte, and every string of the content reaches the DOM as a text node. No attribute
  // comes from the content. Raw text sits in elements marked data-raw, so the catalog proof can tell it
  // from the catalog's strings.
  import type { Document } from "../bindings";
  import type { Catalog } from "../i18n";
  import Blocks from "./Blocks.svelte";

  interface Props {
    document: Document;
    origin: "agent" | "owner" | "unknown";
    strings: Catalog;
    /** A click on a link the core may open: its URL goes back to the core, which checks it again. */
    onopenlink: (url: string) => void;
  }

  let { document, origin, strings, onopenlink }: Props = $props();
</script>

<section class="document">
  <p class="origin">{strings.message("content-origin", { origin })}</p>
  <div class="content">
    <Blocks blocks={document.blocks} {strings} {onopenlink} />
  </div>
  {#if document.truncated}
    <p class="truncated">{strings.message("content-truncated", { limit: "256 KiB" })}</p>
  {/if}
</section>

<style>
  .document {
    border-left: 2px solid currentColor;
    padding-left: 0.75rem;
    overflow-wrap: anywhere;
  }

  .origin,
  .truncated {
    font-size: 0.85em;
    opacity: 0.75;
    margin: 0 0 0.5rem;
  }

  .content {
    max-width: 72ch;
  }
</style>
