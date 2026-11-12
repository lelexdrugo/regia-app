<script lang="ts">
  // The brief: what waits for the owner, in the core's order, and the rest of the queue. Text from the tracker and
  // from the machine reaches the DOM only through Svelte's text interpolation, never as markup: titles, projects and
  // reasons are written by people and by agents, and are untrusted (threat model §3.7). They sit in elements marked
  // data-raw, so the catalog proof can tell them from the catalog's strings (app/ui/prove).
  //
  // One answer of the core fills every block, and each block says its own state: waiting, not read, empty, not
  // read by this version (`not_ported`), or its rows. `data-brief` on the screen is the marker of the first brief
  // on screen (lib/brief.ts).
  import { onMount, tick } from "svelte";

  import type { BriefView, DossierView, OutsideTrackerView } from "../lib/bindings";
  import { clock, type Done, finding, groups, type Marker, problem, type Ranked } from "../lib/brief";
  import type { Catalog } from "../lib/i18n";
  import Controller from "./brief/Controller.svelte";
  import Row from "./brief/Row.svelte";

  interface Props {
    strings: Catalog;
    load: () => Promise<BriefView>;
    /** What was decided in the app today: #477 (F34) fills it, and this version decides nothing. */
    done?: Done[];
    /** Called once the brief, or its error, is in the DOM. */
    onsettled?: () => void;
  }

  let { strings, load, done = [], onsettled }: Props = $props();

  type State = { kind: "loading" } | { kind: "ready"; brief: BriefView; read: Date } | { kind: "failed"; reason: string };
  let view: State = $state({ kind: "loading" });
  let marker: Marker = $derived.by(() => view.kind);
  let sorted = $derived.by(() => (view.kind === "ready" ? groups(view.brief) : null));

  async function read(): Promise<void> {
    view = { kind: "loading" };
    try {
      view = { kind: "ready", brief: await load(), read: new Date() };
    } catch (error) {
      view = { kind: "failed", reason: String(error) };
    }
    await tick();
    onsettled?.();
  }

  function unported(brief: BriefView, section: string): boolean {
    return brief.not_ported.includes(section);
  }

  function numbers(list: number[]): string {
    return list.map((number) => `#${number}`).join(", ");
  }

  function count(n: number): string {
    return new Intl.NumberFormat(strings.language).format(n);
  }

  function outside(brief: BriefView): [string, OutsideTrackerView][] {
    return Object.entries(brief.outside_tracker);
  }

  function dossierLine(warning: DossierView): string {
    return problem(warning.problem, warning.ahead, strings);
  }

  onMount(read);
</script>

{#snippet group(heading: string, rows: Ranked[] | null, empty: string, status: boolean)}
  <section class="qgroup">
    <header class="qgroup-head">
      <h2>{strings.message(heading)}</h2>
      {#if rows}<span class="count">{count(rows.length)}</span>{/if}
    </header>
    {#if view.kind === "loading"}
      <div class="skeleton" aria-hidden="true"><div class="bar"></div><div class="bar"></div><div class="bar"></div></div>
    {:else if view.kind === "failed"}
      <p class="note">{strings.message("brief-not-read")}</p>
    {:else if rows && rows.length > 0}
      <ul class="qlist">
        {#each rows as row (row.task.number)}
          <Row {strings} {row} {status} />
        {/each}
      </ul>
    {:else}
      <p class="empty">{strings.message(empty)}</p>
    {/if}
  </section>
{/snippet}

{#snippet notPorted(section: string)}
  <p class="note">{strings.message("brief-not-ported", { section })}</p>
{/snippet}

<div class="screen" data-brief={marker} aria-busy={view.kind === "loading"}>
  <header class="titlerow">
    <div class="titlerow-main">
      <h1 tabindex="-1">{strings.message("brief-title")}</h1>
      {#if view.kind === "loading"}
        <p class="titlerow-sub" role="status">{strings.message("brief-loading")}</p>
      {:else if view.kind === "ready"}
        <p class="titlerow-sub">{finding(view.brief, strings)}</p>
      {/if}
    </div>
    {#if view.kind === "ready"}
      <p class="stamp">{strings.message("brief-updated", { when: clock(view.read, strings) })}</p>
    {/if}
  </header>

  <div class="queue">
    {#if view.kind === "failed"}
      <div class="error-box" role="alert">
        <span class="alert-icon" aria-hidden="true"></span>
        <p>
          <span>{strings.message("brief-error-lead")}</span>
          <span data-raw>{view.reason}</span>
        </p>
        <button type="button" class="btn" onclick={read}>{strings.message("error-retry")}</button>
      </div>
    {/if}

    {#if view.kind === "ready"}
      {@const brief = view.brief}
      {#if brief.controller || unported(brief, "controller")}
        <section class="qgroup">
          <header class="qgroup-head"><h2>{strings.message("brief-controller")}</h2></header>
          {#if brief.controller}
            <Controller {strings} controller={brief.controller} />
          {:else}
            {@render notPorted("controller")}
          {/if}
        </section>
      {/if}
      {#if brief.graph || brief.due_ignored.length > 0}
        <ul class="warnings">
          {#each brief.graph?.cycles ?? [] as cycle, index (index)}
            <li>{strings.message("brief-cycle", { reason: numbers(cycle) })}</li>
          {/each}
          {#if brief.graph && brief.graph.truncated.length > 0}
            <li>{strings.message("brief-truncated", { reason: numbers(brief.graph.truncated) })}</li>
          {/if}
          {#each brief.due_ignored as ignored (ignored.number)}
            <li>
              <span>{strings.message("brief-due-ignored", { n: String(ignored.number) })}</span>
              <span class="mono" data-raw>{ignored.value}</span>
            </li>
          {/each}
        </ul>
      {/if}
    {/if}

    {@render group("brief-waiting", sorted?.waiting ?? null, sorted && sorted.session.length > 0 ? "brief-empty-sessions" : "brief-empty", true)}
    {#if sorted && sorted.session.length > 0}
      {@render group("brief-session", sorted.session, "brief-empty", true)}
    {/if}

    {#if done.length > 0}
      <section class="qgroup">
        <header class="qgroup-head">
          <h2>{strings.message("brief-done")}</h2>
          <span class="count">{count(done.length)}</span>
        </header>
        <ul class="qlist">
          {#each done as decided (decided.number)}
            <li class="qrow qrow-done">
              <span class="qrow-top">
                <span class="issue">#{decided.number}</span>
                <span class="pill neutral">
                  {strings.message("brief-done-choice", { outcome: decided.outcome, choice: decided.choice ?? "" })}
                </span>
                <span class="qrow-when">
                  {strings.message(decided.written ? "brief-done-written" : "brief-done-writing")}
                </span>
              </span>
              <span class="qrow-title" data-raw title={decided.title}>{decided.title}</span>
            </li>
          {/each}
        </ul>
      </section>
    {/if}

    {@render group("brief-ready", sorted?.ready ?? null, "brief-ready-empty", false)}
    {@render group("brief-in-progress", sorted?.inProgress ?? null, "brief-running-empty", false)}

    {#if view.kind === "ready"}
      {@const brief = view.brief}
      {#if brief.dossier.length > 0 || unported(brief, "dossier")}
        <section class="qgroup">
          <header class="qgroup-head">
            <h2>{strings.message("brief-dossiers")}</h2>
            {#if brief.dossier.length > 0}<span class="count">{count(brief.dossier.length)}</span>{/if}
          </header>
          {#if brief.dossier.length > 0}
            <ul class="mini">
              {#each brief.dossier as warning, index (index)}
                <li>
                  <span class="mini-head">
                    <span class="project" data-raw title={warning.project}>{warning.project}</span>
                    <span class="mini-what">
                      {dossierLine(warning)}
                      {#if warning.branch}
                        {strings.message("dossier-on-branch")} <span class="mono" data-raw>{warning.branch}</span>
                      {/if}
                    </span>
                  </span>
                  <span class="mini-detail mono" data-raw>{warning.folder}</span>
                  {#if warning.detail}<span class="mini-detail" data-raw>{warning.detail}</span>{/if}
                </li>
              {/each}
            </ul>
          {:else}
            {@render notPorted("dossier")}
          {/if}
        </section>
      {/if}
      {#if outside(brief).length > 0 || unported(brief, "outside_tracker")}
        {@const projects = outside(brief)}
        <section class="qgroup">
          <header class="qgroup-head">
            <h2>{strings.message("brief-offtracker")}</h2>
            {#if projects.length > 0}
              <span class="count">{count(projects.reduce((sum, [, entry]) => sum + entry.total, 0))}</span>
            {/if}
          </header>
          {#if projects.length > 0}
            <ul class="mini">
              {#each projects as [project, entry] (project)}
                {#each entry.items as item, index (index)}
                  <li>
                    <span class="mini-head">
                      <span class="mini-what" data-raw>{item.text}</span>
                      <span class="project" data-raw title={item.project}>{item.project}</span>
                    </span>
                    {#if item.why}<span class="mini-detail" data-raw>{item.why}</span>{/if}
                  </li>
                {/each}
                {#if entry.more > 0}
                  <li class="more-line">
                    <span class="project" data-raw>{project}</span>
                    <span>{strings.message("brief-offtracker-more", { n: entry.more })}</span>
                  </li>
                {/if}
              {/each}
            </ul>
            <p class="more-line note">{strings.message("brief-offtracker-note")}</p>
          {:else}
            {@render notPorted("outside_tracker")}
          {/if}
        </section>
      {/if}
    {/if}
  </div>
</div>
