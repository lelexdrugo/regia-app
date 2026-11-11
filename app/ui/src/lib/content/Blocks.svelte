<script lang="ts">
  // The block nodes of the core's tree, each as one element of a fixed list. A heading of the content
  // starts at h3, under the view's own h1 and h2.
  import type { Block } from "../bindings";
  import type { Catalog } from "../i18n";
  import Blocks from "./Blocks.svelte";
  import Inlines from "./Inlines.svelte";

  interface Props {
    blocks: Block[];
    strings: Catalog;
    onopenlink: (url: string) => void;
  }

  let { blocks, strings, onopenlink }: Props = $props();
</script>

{#each blocks as block, index (index)}
  {#if block.kind === "paragraph"}
    <p><Inlines content={block.content} {strings} {onopenlink} /></p>
  {:else if block.kind === "heading"}
    {#if block.level <= 1}
      <h3><Inlines content={block.content} {strings} {onopenlink} /></h3>
    {:else if block.level === 2}
      <h4><Inlines content={block.content} {strings} {onopenlink} /></h4>
    {:else if block.level === 3}
      <h5><Inlines content={block.content} {strings} {onopenlink} /></h5>
    {:else}
      <h6><Inlines content={block.content} {strings} {onopenlink} /></h6>
    {/if}
  {:else if block.kind === "quote"}
    <blockquote><Blocks blocks={block.blocks} {strings} {onopenlink} /></blockquote>
  {:else if block.kind === "code"}
    <pre data-raw><code>{block.text}</code></pre>
  {:else if block.kind === "list"}
    {#if block.ordered}
      <ol>
        {#each block.items as item, position (position)}
          <li><Blocks blocks={item} {strings} {onopenlink} /></li>
        {/each}
      </ol>
    {:else}
      <ul>
        {#each block.items as item, position (position)}
          <li><Blocks blocks={item} {strings} {onopenlink} /></li>
        {/each}
      </ul>
    {/if}
  {:else if block.kind === "table"}
    <div class="table">
      <table>
        <thead>
          <tr>
            {#each block.head as cell, column (column)}
              <th><Inlines content={cell} {strings} {onopenlink} /></th>
            {/each}
          </tr>
        </thead>
        <tbody>
          {#each block.rows as row, line (line)}
            <tr>
              {#each row as cell, column (column)}
                <td><Inlines content={cell} {strings} {onopenlink} /></td>
              {/each}
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else if block.kind === "rule"}
    <hr />
  {/if}
{/each}

<style>
  pre {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  li > :global(p) {
    margin: 0;
  }

  /* Each level indents by its rule and a little padding, not by the browser's 40 px on both sides: sixteen
     levels, the core's cap, still leave room for the text. */
  blockquote {
    margin: 0.5rem 0;
    padding-inline-start: 0.75rem;
    border-inline-start: 2px solid currentColor;
  }

  /* A wide table scrolls in its own box instead of squeezing its columns to a word per line. */
  .table {
    overflow-x: auto;
  }

  table {
    border-collapse: collapse;
  }

  th,
  td {
    border: 1px solid currentColor;
    padding: 0.125rem 0.375rem;
    text-align: start;
    overflow-wrap: normal;
    min-width: 8em;
  }
</style>
