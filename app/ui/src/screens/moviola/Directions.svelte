<script lang="ts">
  // The project's directions (#598, L1): the prototype viewer's `disegnaL1`, beside the stage while no moment is open.
  // A card per lane: its title, state and reason, how many moments, the moment that opened it and the one that closed
  // it. Choosing a card dims the other lanes on the stage and lists its moments; choosing it again lets it go. A
  // record's title and reason, and a lane's id, are text from others. The legend of the prototype's L1 is the help's,
  // on ? (#487).
  import type { Catalog } from "../../lib/i18n";
  import { type LaneCard, laneCards, type Model } from "../../lib/moviola/model";
  import { LANE_ID, timecode } from "../../lib/moviola/text";

  interface Props {
    strings: Catalog;
    model: Model;
    /** The lane chosen, an index, or -1. */
    chosen: number;
    onchoose: (lane: number) => void;
    /** A moment: a node's index in the model. */
    onselect: (index: number) => void;
  }

  let { strings, model, chosen, onchoose, onselect }: Props = $props();

  const cards = $derived(laneCards(model));
  const main = $derived(model.timeline.main_direction);

  /** The card's closing moment, as the prototype says it: abandoned with, rejoined with. */
  function closing(card: LaneCard): string {
    return strings.message(card.lane.status === "abandoned" ? "moviola-direction-abandoned" : "moviola-direction-rejoined");
  }
</script>

{#snippet jump(index: number)}
  {@const target = model.nodes[index]}
  {#if target}
    <button type="button" class="link jump" onclick={() => onselect(index)}>
      <span class="mono">{timecode(strings, target.t)}</span>
      <span data-raw>{target.milestone.title}</span>
    </button>
  {/if}
{/snippet}

<aside class="frame frame-empty overview" aria-labelledby="moviola-frame-none">
  <h2 id="moviola-frame-none">{strings.message("moviola-nothing-title")}</h2>
  <p class="faint">{strings.message("moviola-nothing-body")}</p>
  <section aria-labelledby="moviola-directions">
    <h3 id="moviola-directions">{strings.message("moviola-directions")}</h3>
    <ul class="lane-cards">
      {#each cards as card (card.lane.id)}
        {@const lane = card.lane}
        {@const open = chosen === lane.index}
        <li class="lane-card lane-{lane.status}" class:chosen={open} data-lane={lane.id}>
          <button type="button" class="lane-toggle" aria-expanded={open} onclick={() => onchoose(open ? -1 : lane.index)}>
            {#if lane.id === main}
              <span class="lane-name">{strings.message("moviola-direction-main-title")}</span>
            {:else}
              <span class="lane-name" data-raw>{lane.title ?? lane.id}</span>
            {/if}
            <span class="pill neutral">{strings.message(LANE_ID[lane.status])}</span>
          </button>
          <p class="lane-facts faint">
            <span class="mono" data-raw>{lane.id}</span>
            <span aria-hidden="true">·</span>
            <span>{strings.message("moviola-direction-count", { n: lane.nodes.length })}</span>
          </p>
          {#if lane.id === main}
            <p class="lane-reason">{strings.message("moviola-direction-main")}</p>
          {:else if !lane.recorded}
            <p class="lane-reason faint">{strings.message("moviola-direction-no-record")}</p>
          {:else if lane.reason}
            <p class="lane-reason" data-raw>{lane.reason}</p>
          {:else}
            <p class="lane-reason faint">{strings.message("moviola-direction-no-reason")}</p>
          {/if}
          {#if card.openedBy}
            <p class="frame-line"><span class="frame-label">{strings.message("moviola-direction-opened")}</span>{@render jump(card.openedBy.index)}</p>
          {/if}
          {#if card.closedBy}
            <p class="frame-line"><span class="frame-label">{closing(card)}</span>{@render jump(card.closedBy.index)}</p>
          {/if}
          {#if open}
            {#if lane.nodes.length}
              <ol class="lane-moments" aria-label={strings.message("moviola-moments")}>
                {#each lane.nodes as index (index)}
                  <li>{@render jump(index)}</li>
                {/each}
              </ol>
            {:else}
              <p class="faint">{strings.message("moviola-direction-empty")}</p>
            {/if}
          {/if}
        </li>
      {/each}
    </ul>
  </section>
</aside>
