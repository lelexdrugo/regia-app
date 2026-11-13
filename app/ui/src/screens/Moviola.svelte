<script lang="ts">
  // The moviola, read only: the portfolio (L0) at #/moviola, a project's lanes at #/moviola/<id>. The names read
  // with the portfolio title the lanes; a project opened by its address alone is titled by its id.
  import { SvelteMap } from "svelte/reactivity";

  import type { Catalog } from "../lib/i18n";
  import { projectName } from "../lib/moviola/text";
  import type { MoviolaSource, ProjectSummary } from "../lib/moviola/types";
  import Projects from "./moviola/Projects.svelte";
  import Timeline from "./moviola/Timeline.svelte";

  interface Props {
    strings: Catalog;
    source: MoviolaSource;
    /** The project the address names, or none for the portfolio. */
    project: string | null;
    /** The moment the address opens, if any. */
    moment?: string | null;
    observeTheme?: boolean;
    onsettled?: () => void;
  }

  let { strings, source, project, moment = null, observeTheme = true, onsettled }: Props = $props();

  const names = new SvelteMap<string, string>();

  function read(projects: ProjectSummary[]): void {
    for (const summary of projects) names.set(summary.id, projectName(summary));
  }
</script>

{#if project === null}
  <Projects {strings} {source} onread={read} {...onsettled ? { onsettled } : {}} />
{:else}
  {#key project}
    <Timeline {strings} {source} {project} {moment} name={names.get(project) ?? project} {observeTheme} {...onsettled ? { onsettled } : {}} />
  {/key}
{/if}
