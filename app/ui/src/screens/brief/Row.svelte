<script lang="ts">
  // A task of the queue: its number, its status where a group mixes them, its due mark, its title, its project, and
  // the strip of the work it holds up. The title and the project are written by others: they are text, in elements
  // marked data-raw (app/ui/README.md). The row says where it is in the core's list, for app-smoke (lib/smoke.ts).
  import { due, frames, measure, type Ranked } from "../../lib/brief";
  import type { Catalog } from "../../lib/i18n";

  interface Props {
    strings: Catalog;
    row: Ranked;
    /** Show the status: the waiting groups mix proposals and decisions. */
    status?: boolean;
  }

  let { strings, row, status = false }: Props = $props();

  let task = $derived(row.task);
  let mark = $derived(due(task, strings));
  let strip = $derived(frames(task));
  let measured = $derived(measure(task, strings));
</script>

<li class="qrow" data-section={row.section} data-number={task.number} data-rank={row.rank}>
  <span class="qrow-top">
    <span class="issue">#{task.number}</span>
    {#if status && task.status}
      <span class="pill {task.status === 'decision' ? 'accent' : 'neutral'}">{strings.message(`state-${task.status}`)}</span>
    {/if}
    {#if mark}
      <span class="pill {mark.overdue ? 'negative' : 'accent'}">{mark.text}</span>
    {/if}
  </span>
  <span class="qrow-title" data-raw title={task.title}>{task.title}</span>
  {#if task.project || measured}
    <span class="qrow-foot">
      {#if task.project}
        <span class="project" data-raw title={task.project}>{task.project}</span>
      {/if}
      {#if measured}
        <span
          class="qrow-chain"
          title={strip ? strings.message("queue-chain-title", { unlocks: task.unblocks ?? 0, chain: task.chain ?? 0 }) : null}
        >
          {#if strip}
            <span class="frames" aria-hidden="true">
              {#each strip.lit as lit, index (index)}<i class:lit></i>{/each}
              {#if strip.more > 0}<span class="more">+{strip.more}</span>{/if}
            </span>
          {/if}
          <span class="faint">{measured}</span>
        </span>
      {/if}
    </span>
  {/if}
</li>
