<script lang="ts" module>
  /** The open decisions of the last moment's notes: no moment yet, still coming, read, or refused. */
  export type Decisions = { kind: "none" } | { kind: "loading" } | { kind: "ready"; list: string[] } | { kind: "failed" };
</script>

<script lang="ts">
  // The project at the playhead (#598): the prototype viewer's panel «alla data». The health of the last
  // moment up to the playhead, how many before and after it, that moment, the directions open then (derived: the
  // records have no closing date) and the open decisions of that moment's notes, which the screen reads one moment at
  // a time. It changes only when the playhead passes a moment.
  import type { Catalog } from "../../lib/i18n";
  import type { AtDate, Model } from "../../lib/moviola/model";
  import { HEALTH_ID, laneName, timecode } from "../../lib/moviola/text";

  interface Props {
    strings: Catalog;
    model: Model;
    state: AtDate;
    decisions: Decisions;
    /** Another moment: a node's index in the model. */
    onselect: (index: number) => void;
    /** Off while the stage holds the focus: its list says the moment chosen, and this panel would say it again (#490). */
    live?: boolean;
  }

  let { strings, model, state, decisions, onselect, live = true }: Props = $props();

  const TONE = { healthy: "positive", at_risk: "negative", waiting_on_owner: "accent" } as const;
  const main = $derived(model.timeline.main_direction);
</script>

<section
  class="at-date"
  aria-labelledby="moviola-at"
  aria-live={live ? "polite" : "off"}
  data-last={state.last ? state.last.milestone.id : ""}
  data-before={state.before}
  data-after={state.after}
  data-open={state.open.map((lane) => lane.id).join(",")}
  data-decisions={decisions.kind === "ready" ? decisions.list.length : decisions.kind}
>
  <h2 id="moviola-at">{strings.message("moviola-at")}</h2>
  <dl class="at-kv">
    <dt>{strings.message("moviola-health")}</dt>
    <dd>
      {#if state.last}
        <span class="pill {TONE[state.last.milestone.health]}" data-health={state.last.milestone.health}>{strings.message(HEALTH_ID[state.last.milestone.health])}</span>
      {:else}
        <span class="faint">{strings.message("moviola-at-none")}</span>
      {/if}
      <span class="faint">{strings.message("moviola-at-counts", { before: state.before, after: state.after })}</span>
    </dd>
    {#if state.last}
      {@const last = state.last}
      <dt>{strings.message("moviola-at-last")}</dt>
      <dd>
        <button type="button" class="link jump" onclick={() => onselect(last.index)}>
          <!-- A flex box keeps no space between its items: the hidden comma keeps the button's name in two words (#490). -->
          <span class="mono">{timecode(strings, last.t)}</span><span class="sr">,&nbsp;</span>
          <span data-raw>{last.milestone.title}</span>
        </button>
      </dd>
    {/if}
    <dt title={strings.message("moviola-at-open-hint")}>{strings.message("moviola-at-open")}</dt>
    <dd>
      {#each state.open as lane, i (lane.id)}
        {@const comma = i < state.open.length - 1 ? "," : ""}
        {#if lane.id === main}
          <span>{laneName(strings, model, lane)}{comma}</span>
        {:else}
          <span data-raw>{lane.id}{comma}</span>
        {/if}
      {:else}
        <span class="faint">{strings.message("moviola-at-open-none")}</span>
      {/each}
    </dd>
    <dt>{strings.message("moviola-at-decisions")}</dt>
    <dd>
      {#if decisions.kind === "ready" && decisions.list.length > 0}
        <span data-raw title={decisions.list.join("\n")}>{strings.message("moviola-at-decisions-count", { n: decisions.list.length, first: decisions.list[0] ?? "" })}</span>
      {:else if decisions.kind === "loading"}
        <span class="faint">{strings.message("moviola-at-decisions-reading")}</span>
      {:else if decisions.kind === "failed"}
        <span class="faint">{strings.message("moviola-at-decisions-failed")}</span>
      {:else}
        <span class="faint">{strings.message("moviola-at-decisions-none")}</span>
      {/if}
    </dd>
  </dl>
</section>
