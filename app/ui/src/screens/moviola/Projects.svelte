<script lang="ts">
  // The moviola's L0, the portfolio: each project with a folder on this machine, its health and its last
  // milestone, from `list_projects`, in place of the prototype viewer's route `/` (`disegnaL0`). The projects with
  // a story come first and open their lanes; titles, ids and milestones are text from others, in data-raw.
  import { onMount, tick } from "svelte";

  import type { Catalog } from "../../lib/i18n";
  import { day, HEALTH_ID, KIND_ID, portfolioOrder, projectName, timecode } from "../../lib/moviola/text";
  import type { MoviolaSource, ProjectSummary } from "../../lib/moviola/types";
  import { projectHref } from "../../lib/route";

  interface Props {
    strings: Catalog;
    source: MoviolaSource;
    /** The projects as read, for the lanes' title. */
    onread?: (projects: ProjectSummary[]) => void;
    onsettled?: () => void;
  }

  let { strings, source, onread, onsettled }: Props = $props();

  type State = { kind: "loading" } | { kind: "ready"; projects: ProjectSummary[] } | { kind: "failed"; reason: string };
  let view: State = $state({ kind: "loading" });

  async function read(): Promise<void> {
    view = { kind: "loading" };
    try {
      const projects = await source.projects();
      onread?.(projects);
      view = { kind: "ready", projects: portfolioOrder(projects) };
    } catch (error) {
      view = { kind: "failed", reason: String(error) };
    }
    await tick();
    onsettled?.();
  }

  const TONE = { healthy: "positive", at_risk: "negative", waiting_on_owner: "accent" } as const;

  onMount(read);
</script>

<div class="screen" data-moviola={view.kind} aria-busy={view.kind === "loading"}>
  <header class="titlerow">
    <div class="titlerow-main">
      <h1 tabindex="-1">{strings.message("moviola-title")}</h1>
      {#if view.kind === "loading"}
        <p class="titlerow-sub" role="status">{strings.message("loading")}</p>
      {:else if view.kind === "ready"}
        <p class="titlerow-sub">{strings.message("moviola-projects-sub", { n: view.projects.length })}</p>
      {/if}
    </div>
  </header>

  {#if view.kind === "loading"}
    <div class="skeleton" aria-hidden="true"><div class="bar"></div><div class="bar"></div><div class="bar"></div></div>
  {:else if view.kind === "failed"}
    <div class="error-box" role="alert">
      <span class="alert-icon" aria-hidden="true"></span>
      <p>
        <span>{strings.message("moviola-error-lead")}</span>
        <span data-raw>{view.reason}</span>
      </p>
      <button type="button" class="btn" onclick={read}>{strings.message("error-retry")}</button>
    </div>
  {:else if view.projects.length === 0}
    <p class="empty">{strings.message("moviola-projects-empty")}</p>
  {:else}
    <ul class="reels">
      {#each view.projects as project (project.id)}
        <li class="reel" class:reel-none={project.milestones === 0} data-project={project.id}>
          <span class="reel-head">
            {#if project.milestones > 0}
              <a class="reel-name" href={projectHref(project.id)} data-raw title={projectName(project)}>{projectName(project)}</a>
            {:else}
              <span class="reel-name" data-raw title={projectName(project)}>{projectName(project)}</span>
            {/if}
            <span class="reel-id mono" data-raw>{project.id}</span>
            {#if project.health}
              <span class="pill {TONE[project.health]}" data-health={project.health}>{strings.message(HEALTH_ID[project.health])}</span>
            {:else}
              <span class="pill neutral" data-health="none">{strings.message("moviola-project-none")}</span>
            {/if}
          </span>
          {#if project.milestones > 0}
            <span class="reel-facts">
              <span>{strings.message("moviola-project-counts", { milestones: project.milestones, directions: project.directions })}</span>
              {#if project.from && project.to}
                <span class="faint">
                  {strings.message("moviola-project-span", { from: day(strings, Date.parse(project.from)), to: day(strings, Date.parse(project.to)) })}
                </span>
              {/if}
            </span>
          {/if}
          {#if project.last}
            <span class="reel-last" data-last={project.last.id}>
              <span class="faint">{strings.message("moviola-project-last")}</span>
              <span class="reel-kind">{strings.message(KIND_ID[project.last.kind])}</span>
              <span class="reel-title" data-raw title={project.last.title}>{project.last.title}</span>
              <span class="mono faint">{timecode(strings, Date.parse(project.last.ts))}</span>
            </span>
          {/if}
        </li>
      {/each}
    </ul>
    <p class="note">{strings.message("moviola-projects-note")}</p>
  {/if}
</div>
