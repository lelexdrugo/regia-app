<script lang="ts">
  // The story as text (#490): the text alternative of the stage, the prototype's `esporta` (the dossier's
  // tools/moviola/esporta.py) in the page. An index of every moment in time order, then a section for each direction,
  // in the order of its first moment, with its record and its moments. Labels M01, M02… number the whole story, as the
  // prototype's do, so «opened by» can point at one. Closed at first and built from the model only when opened, and let
  // go when closed: at x100 it is 2,700 moments twice. The decisions of each period and the references are not here:
  // they come with the core's `esporta` (#485), byte for byte with the prototype's. Titles, reasons and directions' ids
  // are text from others.
  import type { Catalog } from "../../lib/i18n";
  import { type LaneCard, laneCards, type Model } from "../../lib/moviola/model";
  import { HEALTH_ID, KIND_ID, LANE_ID, laneName, timecode } from "../../lib/moviola/text";

  interface Props {
    strings: Catalog;
    model: Model;
  }

  let { strings, model }: Props = $props();

  let open = $state(false);
  const main = $derived(model.timeline.main_direction);
  const digits = $derived(Math.max(2, String(model.nodes.length).length));

  function label(index: number): string {
    return strings.message("moviola-text-label", { n: String(index + 1).padStart(digits, "0") });
  }

  /** A direction's first moment, or none: the prototype's sections go by it, a direction without one last. */
  function first(card: LaneCard): number {
    const index = card.lane.nodes[0];
    return index === undefined ? Infinity : (model.nodes[index]?.t ?? Infinity);
  }

  const sections = $derived(open ? [...laneCards(model)].sort((a, b) => first(a) - first(b)) : []);
</script>

{#snippet moment(index: number, withLane: boolean)}
  {@const node = model.nodes[index]}
  {#if node}
    {@const m = node.milestone}
    {@const lane = model.lanes[node.lane]}
    <span class="mono">{label(index)}</span>
    <span class="sep" aria-hidden="true">·</span>
    <span class="mono">{timecode(strings, node.t)}</span>
    <span class="sep" aria-hidden="true">·</span>
    <span>{strings.message(KIND_ID[m.kind])}</span>
    {#if withLane && lane}
      <span class="sep" aria-hidden="true">·</span>
      {#if lane.id === main}
        <span>{laneName(strings, model, lane)}</span>
      {:else}
        <span class="mono" data-raw>{lane.id}</span>
      {/if}
    {/if}
    <span class="sep" aria-hidden="true">·</span>
    <span data-raw>{m.title}</span>
    {#if m.health !== "healthy"}
      <span class="sep" aria-hidden="true">·</span>
      <span>{strings.message(HEALTH_ID[m.health])}</span>
    {/if}
  {/if}
{/snippet}

<details class="story-text" bind:open data-open={open ? "true" : "false"}>
  <summary>{strings.message("moviola-text-title")}</summary>
  {#if open}
    <p class="faint">{strings.message("moviola-text-lead", { n: model.nodes.length, lanes: model.lanes.length })}</p>
    <h3 id="moviola-text-index">{strings.message("moviola-text-index")}</h3>
    <ol class="story-list" aria-labelledby="moviola-text-index">
      {#each model.nodes as node (node.index)}
        <li data-id={node.milestone.id}>{@render moment(node.index, true)}</li>
      {/each}
    </ol>
    {#each sections as card (card.lane.id)}
      {@const lane = card.lane}
      <section class="story-lane" aria-labelledby="moviola-text-lane-{lane.index}" data-lane={lane.id}>
        <h3 id="moviola-text-lane-{lane.index}">
          <span>{strings.message("moviola-text-direction")}</span>
          {#if lane.id === main}
            <span>{strings.message("moviola-direction-main-title")}</span>
          {:else}
            <span data-raw>{lane.title ?? lane.id}</span>
          {/if}
          <span class="mono faint" data-raw>({lane.id})</span>
        </h3>
        <dl class="story-kv">
          <dt>{strings.message("moviola-text-state")}</dt>
          <dd>{strings.message(LANE_ID[lane.status])}</dd>
          {#if card.openedBy}
            <dt>{strings.message("moviola-direction-opened")}</dt>
            <dd><span class="mono">{label(card.openedBy.index)}</span> <span data-raw>{card.openedBy.milestone.title}</span></dd>
          {/if}
          {#if card.closedBy}
            <dt>{strings.message(lane.status === "abandoned" ? "moviola-direction-abandoned" : "moviola-direction-rejoined")}</dt>
            <dd><span class="mono">{label(card.closedBy.index)}</span> <span data-raw>{card.closedBy.milestone.title}</span></dd>
          {/if}
          <dt>{strings.message("moviola-text-reason")}</dt>
          {#if lane.id === main}
            <dd>{strings.message("moviola-direction-main")}</dd>
          {:else if !lane.recorded}
            <dd class="faint">{strings.message("moviola-direction-no-record")}</dd>
          {:else if lane.reason}
            <dd data-raw>{lane.reason}</dd>
          {:else}
            <dd class="faint">{strings.message("moviola-direction-no-reason")}</dd>
          {/if}
        </dl>
        {#if lane.nodes.length}
          <ol class="story-list" aria-labelledby="moviola-text-lane-{lane.index}">
            {#each lane.nodes as index (index)}
              <li>{@render moment(index, false)}</li>
            {/each}
          </ol>
        {:else}
          <p class="faint">{strings.message("moviola-direction-empty")}</p>
        {/if}
      </section>
    {/each}
  {/if}
</details>
