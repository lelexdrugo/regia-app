<script lang="ts">
  // Text from the tracker reaches the DOM only through Svelte's text interpolation, never as markup:
  // titles are written by people and by agents, and are untrusted (threat model §3.7).
  import { onMount, tick } from "svelte";

  import type { BriefView, TaskView } from "./lib/bindings";
  import type { Catalog } from "./lib/i18n";

  interface Props {
    strings: Catalog;
    load: () => Promise<BriefView>;
    /** Called once the brief, or its error, is in the DOM. */
    onsettled?: () => void;
  }

  let { strings, load, onsettled }: Props = $props();

  type State = { kind: "loading" } | { kind: "ready"; brief: BriefView } | { kind: "failed"; reason: string };
  let view: State = $state({ kind: "loading" });

  type SectionKey = "waiting_on_owner" | "ready" | "in_progress";
  const sections: { key: SectionKey; heading: string }[] = [
    { key: "waiting_on_owner", heading: "brief-waiting" },
    { key: "ready", heading: "brief-ready" },
    { key: "in_progress", heading: "brief-in-progress" },
  ];

  function numbers(list: number[]): string {
    return list.map((number) => `#${number}`).join(", ");
  }

  function warnings(brief: BriefView): string[] {
    if (!brief.graph) {
      return [];
    }
    const out = brief.graph.cycles.map((cycle) => strings.message("brief-cycle", { reason: numbers(cycle) }));
    if (brief.graph.truncated.length > 0) {
      out.push(strings.message("brief-truncated", { reason: numbers(brief.graph.truncated) }));
    }
    return out;
  }

  function section(brief: BriefView, key: SectionKey): TaskView[] {
    return brief[key];
  }

  onMount(async () => {
    try {
      view = { kind: "ready", brief: await load() };
    } catch (error) {
      view = { kind: "failed", reason: String(error) };
    }
    await tick();
    onsettled?.();
  });
</script>

<h1>{strings.message("app-title")}</h1>
{#if view.kind === "loading"}
  <p>{strings.message("brief-loading")}</p>
{:else if view.kind === "failed"}
  <p role="alert">{strings.message("brief-error", { reason: view.reason })}</p>
{:else}
  <p class="note">{strings.message("brief-partial")}</p>
  {#each warnings(view.brief) as warning, index (index)}
    <p class="warning">{warning}</p>
  {/each}
  {#each sections as { key, heading } (key)}
    <section>
      <h2>{strings.message(heading)}</h2>
      {#if section(view.brief, key).length === 0}
        <p class="note">{strings.message("brief-nothing")}</p>
      {:else}
        <ul>
          {#each section(view.brief, key) as task (task.number)}
            <li data-section={key} data-number={task.number}>
              <span class="number">#{task.number}</span>
              {task.title}
            </li>
          {/each}
        </ul>
      {/if}
    </section>
  {/each}
{/if}
