<script lang="ts">
  // The help, on ? or from the sidebar: the keys, then the words. A modal dialog, so Esc closes it and the focus
  // stays inside while it is open; the shell puts the focus back where it was.
  import { onMount } from "svelte";

  import { GLOSSARY, KEYS, MOVIOLA_KEYS, MOVIOLA_WORDS } from "../lib/glossary";
  import type { Catalog } from "../lib/i18n";

  interface Props {
    strings: Catalog;
    onclose: () => void;
  }

  let { strings, onclose }: Props = $props();
  let dialog: HTMLDialogElement | undefined = $state();

  onMount(() => dialog?.showModal());
</script>

<dialog class="help" aria-labelledby="help-keys" bind:this={dialog} {onclose}>
  <h2 id="help-keys">{strings.message("keys-title")}</h2>
  <table class="keys">
    <tbody>
      {#each KEYS as [description, keys] (description)}
        <tr>
          <td>{strings.message(description)}</td>
          <td class="keys-col"><kbd data-raw>{keys}</kbd></td>
        </tr>
      {/each}
    </tbody>
  </table>
  <h2 class="spaced">{strings.message("words-title")}</h2>
  <dl class="words">
    {#each GLOSSARY as [term, meaning] (term)}
      <dt>{strings.message(term)}</dt>
      <dd>{strings.message(meaning)}</dd>
    {/each}
  </dl>
  <h2 class="spaced" id="help-moviola">{strings.message("moviola-help-title")}</h2>
  <table class="keys" aria-labelledby="help-moviola">
    <tbody>
      {#each MOVIOLA_KEYS as [description, keys] (description)}
        <tr>
          <td>{strings.message(description)}</td>
          <td class="keys-col"><kbd data-raw>{keys}</kbd></td>
        </tr>
      {/each}
    </tbody>
  </table>
  <dl class="words">
    {#each MOVIOLA_WORDS as [term, meaning] (term)}
      <dt>{strings.message(term)}</dt>
      <dd>{strings.message(meaning)}</dd>
    {/each}
  </dl>
  <div class="help-foot">
    <button class="btn" type="button" onclick={() => dialog?.close()}>{strings.message("keys-close")}</button>
  </div>
</dialog>
