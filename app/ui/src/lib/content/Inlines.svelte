<script lang="ts">
  // The span nodes of the core's tree. A link is a button, never an <a href>: no navigation of its own,
  // no drag, no middle click, and its host shows beside its text, so the text cannot pass for another
  // address. An image is a placeholder with the host it would have loaded from (threat model §3.7).
  import type { Inline } from "../bindings";
  import type { Catalog } from "../i18n";
  import Inlines from "./Inlines.svelte";

  interface Props {
    content: Inline[];
    strings: Catalog;
    onopenlink: (url: string) => void;
  }

  let { content, strings, onopenlink }: Props = $props();
</script>

{#each content as inline, index (index)}
  {#if inline.kind === "text"}
    <span data-raw>{inline.text}</span>
  {:else if inline.kind === "emphasis"}
    <em><Inlines content={inline.content} {strings} {onopenlink} /></em>
  {:else if inline.kind === "strong"}
    <strong><Inlines content={inline.content} {strings} {onopenlink} /></strong>
  {:else if inline.kind === "code"}
    <code data-raw>{inline.text}</code>
  {:else if inline.kind === "break"}
    <br />
  {:else if inline.kind === "link"}
    {@const target = inline.target}
    {#if target.kind === "external"}
      <button type="button" class="link" onclick={() => onopenlink(target.url)}>
        <Inlines content={inline.content} {strings} {onopenlink} />
        <span class="host" data-raw>{target.host}</span>
      </button>
    {:else}
      <span class="refused"
        ><Inlines content={inline.content} {strings} {onopenlink} />
        <span class="note">{strings.message("content-link-refused", { reason: target.reason })}</span></span
      >
    {/if}
  {:else if inline.kind === "image"}
    <span class="image"
      ><span class="note"
        >{inline.host
          ? strings.message("content-image", { host: inline.host })
          : strings.message("content-image-nohost")}</span
      >{#if inline.alt}
        <span data-raw>{inline.alt}</span>{/if}</span
    >
  {/if}
{/each}

<style>
  .link {
    font: inherit;
    color: inherit;
    text-decoration: underline;
    background: none;
    border: 0;
    padding: 0;
    cursor: pointer;
    text-align: start;
  }

  .host {
    font-size: 0.85em;
    opacity: 0.75;
    margin-inline-start: 0.25em;
  }

  .host::before {
    content: "(";
  }

  .host::after {
    content: ")";
  }

  .note {
    font-size: 0.85em;
    opacity: 0.75;
  }

  .image {
    border: 1px dashed currentColor;
    padding: 0 0.25em;
  }
</style>
