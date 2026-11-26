<script lang="ts">
  // The frame of the moment chosen (L3): the prototype viewer's `disegnaL3` (lib/moviola/frame.ts) in the order of
  // the design brief: the summary first (#409, R17), then the why, what changed, the open decisions, the risks and
  // the next step; then the milestone's process and how it got there, by task, from the aggregates (R05); then the
  // references and the moments around it. Every text of the frame is the chronicler's or an agent's: it reaches
  // the DOM as text, in data-raw, and a `<` stays a character. A `#N`, an issue, a commit or the Project is a button,
  // never an <a href> (threat model §3.7); the app has no command yet that opens the browser
  // (`open_external_link`), so the button shows the address the core resolved, to copy.
  import { tick } from "svelte";

  import type { Catalog } from "../../lib/i18n";
  import type { Frame, Notes, Run } from "../../lib/moviola/frame";
  import type { Model } from "../../lib/moviola/model";
  import { localDay, pack, packReferences, packSummary, type PackWords, sidecarPath } from "../../lib/moviola/pack";
  import { HEALTH_ID, KIND_ID, LANE_ID, laneName, timecode } from "../../lib/moviola/text";
  import type { Check, ModelUse, Snapshot, Tokens } from "../../lib/moviola/types";

  interface Props {
    strings: Catalog;
    model: Model;
    frame: Frame;
    notes: Notes;
    /** The moment's sidecar, for the chat package's summary. */
    snapshot: Snapshot | null;
    /** Another moment: a node's index in the model. */
    onselect: (index: number) => void;
    onclose: () => void;
  }

  let { strings, model, frame, notes, snapshot, onselect, onclose }: Props = $props();

  // The address of the last link pressed, and the chat package asked for, until another moment opens.
  let opened: string | null = $state(null);
  let packText: string | null = $state(null);
  let packArea: HTMLTextAreaElement | undefined = $state();
  let shown = "";
  $effect.pre(() => {
    if (frame.node.milestone.id !== shown) {
      shown = frame.node.milestone.id;
      opened = null;
      packText = null;
    }
  });

  function packWords(): PackWords {
    return {
      head: (args) => strings.message("moviola-pack-head", args),
      looking: (what) => strings.message("moviola-pack-looking", { what }),
      references: (list) => strings.message("moviola-pack-references", { list }),
      none: strings.message("moviola-pack-none"),
      more: (n) => strings.message("moviola-pack-more", { n }),
      question: strings.message("moviola-pack-question"),
      skill: strings.message("moviola-pack-skill"),
    };
  }

  /** «Copy for chat»: the package, selected for the system's copy. The app has no clipboard of its own yet: the
   * threat model has the core write it (`copy_context_pack`), so the webview never touches the clipboard. */
  async function copyPack(): Promise<void> {
    const m = frame.node.milestone;
    const moment = { project: model.timeline.project, id: m.id, kind: m.kind, day: localDay(frame.node.t), title: m.title };
    const refs = packReferences(m.links, notes.kind === "ready" ? sidecarPath(m.id) : null);
    packText = pack(moment, packSummary(notes.kind === "ready" ? snapshot : null), refs, packWords());
    await tick();
    packArea?.focus();
    packArea?.select();
  }

  const TONE = { healthy: "positive", at_risk: "negative", waiting_on_owner: "accent" } as const;

  /** A value someone may not have declared: the catalog's word for null, never 0. */
  function declared(value: number | string | null | undefined): number | string {
    return value === null || value === undefined ? strings.message("moviola-not-declared") : value;
  }

  function modelLine(use: ModelUse): string {
    const args = { agent: declared(use.agent), model: declared(use.model), effort: declared(use.effort), sessions: declared(use.sessions) };
    return strings.message(use.via === "report" ? "moviola-model-cited" : "moviola-model-use", args);
  }

  function checkLine(check: Check): string {
    const args = { kind: declared(check.kind), outcome: declared(check.outcome) };
    return check.model ? strings.message("moviola-check-model", { ...args, model: check.model }) : strings.message("moviola-check", args);
  }

  function tokenLine(tokens: Tokens): string {
    return strings.message("moviola-tokens", {
      input: declared(tokens.input),
      output: declared(tokens.output),
      read: declared(tokens.cache_read),
      written: declared(tokens.cache_write),
    });
  }

  const sidecarSections = $derived([
    { title: "moviola-changed", lines: frame.changed },
    { title: "moviola-open-decisions", lines: frame.openDecisions },
    { title: "moviola-risks", lines: frame.risks },
  ]);
</script>

{#snippet runs(line: Run[])}
  {#each line as run, i (i)}
    {#if run.url}
      {@const url = run.url}
      <button type="button" class="link" data-raw data-url={url} onclick={() => (opened = url)}>{run.text}</button>
    {:else}
      <span data-raw>{run.text}</span>
    {/if}
  {/each}
{/snippet}

{#snippet jump(index: number)}
  {@const target = model.nodes[index]}
  {#if target}
    <button type="button" class="link jump" onclick={() => onselect(index)}>
      <!-- A flex box keeps no space between its items: the hidden comma keeps the button's name in two words (#490). -->
      <span class="mono">{timecode(strings, target.t)}</span><span class="sr">,&nbsp;</span>
      <span data-raw>{target.milestone.title}</span>
    </button>
  {/if}
{/snippet}

<!-- Esc anywhere in the frame lets the moment go, as Esc on the stage does. -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<aside
  class="frame"
  aria-labelledby="moviola-frame-title"
  data-frame={notes.kind}
  data-moment={frame.node.milestone.id}
  onkeydown={(event) => {
    // Esc in the package only leaves it, as Esc leaves a field elsewhere.
    if (event.key === "Escape" && !(event.target instanceof HTMLTextAreaElement)) {
      event.preventDefault();
      onclose();
    }
  }}
>
  <div class="frame-meta">
    <span class="reel-kind">{strings.message(KIND_ID[frame.node.milestone.kind])}</span>
    <span class="mono faint">{timecode(strings, frame.node.t)}</span>
    <button type="button" class="btn frame-copy" onclick={copyPack}>{strings.message("moviola-copy")}</button>
    <button type="button" class="btn icon-btn frame-close" aria-label={strings.message("moviola-frame-close")} title={strings.message("moviola-frame-close")} onclick={onclose}>
      <span class="icon icon-close" aria-hidden="true"></span>
    </button>
  </div>
  <h2 id="moviola-frame-title" data-raw>{frame.node.milestone.title}</h2>
  <p class="frame-tags">
    <span class="pill neutral" data-raw>{laneName(strings, model, frame.lane)}</span>
    <span class="pill {TONE[frame.node.milestone.health]}">{strings.message(HEALTH_ID[frame.node.milestone.health])}</span>
  </p>
  {#if packText !== null}
    <div class="frame-pack">
      <p class="faint" role="status">{strings.message("moviola-pack-note")}</p>
      <textarea bind:this={packArea} readonly rows="7" aria-label={strings.message("moviola-pack-label")} value={packText}></textarea>
    </div>
  {/if}

  {#if notes.kind === "loading"}
    <p class="faint" role="status">{strings.message("moviola-notes-loading")}</p>
  {:else if notes.kind === "failed"}
    <p class="frame-note" role="alert"><span>{strings.message("moviola-notes-failed")}</span> <span data-raw>{notes.reason}</span></p>
  {:else if notes.kind === "none"}
    <p class="faint">{strings.message("moviola-notes-none")}</p>
  {:else}
    {#if frame.inBrief}
      <section class="frame-brief" aria-label={strings.message("moviola-in-brief")}>
        <p data-raw>{frame.inBrief}</p>
      </section>
    {/if}
    <section>
      <h3>{strings.message("moviola-why")}</h3>
      {#if frame.why.direction}
        <p class="frame-why" data-raw>{frame.why.direction}</p>
      {:else if !frame.why.opens && frame.why.closes.length === 0}
        <p class="faint">{strings.message("moviola-none")}</p>
      {/if}
      {#if frame.why.opens}
        <p class="faint">{strings.message("moviola-opens-lane", { lane: frame.why.opens.title ?? frame.why.opens.id, reason: frame.why.opens.reason ?? "" })}</p>
      {/if}
      {#each frame.why.closes as closed (closed.id)}
        <p class="faint">
          {strings.message("moviola-closes-lane", { lane: closed.title ?? closed.id, status: strings.message(LANE_ID[closed.status]), reason: closed.reason ?? "" })}
        </p>
      {/each}
    </section>
    {#each sidecarSections as section (section.title)}
      <section>
        <h3>{strings.message(section.title)}</h3>
        {#if section.lines.length}
          <ul class="frame-list">
            {#each section.lines as line, i (i)}
              <li>{@render runs(line)}</li>
            {/each}
          </ul>
        {:else}
          <p class="faint">{strings.message("moviola-none")}</p>
        {/if}
      </section>
    {/each}
    <section>
      <h3>{strings.message("moviola-next-step")}</h3>
      {#if frame.next}
        <p data-raw>{frame.next}</p>
      {:else}
        <p class="faint">{strings.message("moviola-none")}</p>
      {/if}
    </section>
    {#if frame.claims.length}
      <section>
        <h3>{strings.message("moviola-claims")}</h3>
        <ul class="frame-list">
          {#each frame.claims as claim, i (i)}
            <li data-raw>{claim}</li>
          {/each}
        </ul>
      </section>
    {/if}
    {#if frame.deviations.length}
      <section>
        <h3>{strings.message("moviola-deviations")}</h3>
        <ul class="frame-list">
          {#each frame.deviations as d, i (i)}
            <li>
              {strings.message("moviola-deviation", { plan: declared(d.plan), deviation: declared(d.deviation), reason: declared(d.reason), impact: declared(d.impact) })}
            </li>
          {/each}
        </ul>
      </section>
    {/if}
  {/if}

  <section>
    <h3>{strings.message("moviola-process")}</h3>
    <dl class="frame-kv">
      <dt>{strings.message("moviola-round")}</dt>
      <dd>{declared(frame.process.round)}</dd>
      <dt>{strings.message("moviola-checks")}</dt>
      <dd>
        {#if frame.process.checks.length}<span data-raw>{frame.process.checks.join(", ")}</span>{:else}{strings.message("moviola-not-declared")}{/if}
      </dd>
      <dt>{strings.message("moviola-found")}</dt>
      <dd>{declared(frame.process.found)}</dd>
      <dt>{strings.message("moviola-closed")}</dt>
      <dd>{declared(frame.process.closed)}</dd>
    </dl>
  </section>

  <section class="frame-work">
    <h3>{strings.message("moviola-work")}</h3>
    <p class="faint">{strings.message("moviola-work-sub")}</p>
    {#if frame.work.length === 0}
      <p class="faint">{strings.message("moviola-no-tasks")}</p>
    {:else}
      <ul class="frame-tasks">
        {#each frame.work as { task, aggregate } (task)}
          <li data-task={task}>
            {#if aggregate === null}
              <p><span class="mono" data-raw>#{task}</span> <span class="faint">{strings.message("moviola-no-aggregate")}</span></p>
            {:else}
              <p class="task-head">
                {strings.message("moviola-task-head", { task: String(task), round: declared(aggregate.round), subagents: declared(aggregate.declared_subagents) })}
              </p>
              {#if aggregate.models.length === 0}
                <p class="faint" data-absent="models">{strings.message("moviola-no-session")}</p>
              {/if}
              {#each aggregate.models as use, i (i)}
                <p data-model>{modelLine(use)}</p>
              {/each}
              {#each aggregate.checks as check, i (i)}
                <p data-check>{checkLine(check)}{#if check.defects.length}{" · "}<span data-raw>{check.defects.join(", ")}</span>{/if}</p>
              {/each}
              <p data-defects>
                {strings.message("moviola-task-defects", { found: declared(aggregate.defects?.found), closed: declared(aggregate.defects?.closed) })}
              </p>
              {#if aggregate.defects?.closes.length}
                <p>{strings.message("moviola-task-closes", { refs: aggregate.defects.closes.join(", ") })}</p>
              {/if}
              {#if aggregate.tokens}
                <p class="faint" data-tokens>{tokenLine(aggregate.tokens)}</p>
              {:else}
                <p class="faint" data-absent="tokens">{strings.message("moviola-no-tokens")}</p>
              {/if}
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  <section>
    <h3>{strings.message("moviola-refs")}</h3>
    <dl class="frame-kv">
      <dt>{strings.message("moviola-issues")}</dt>
      <dd>
        {#each frame.links.issue as issue, i (i)}
          {#if issue.url}
            {@const url = issue.url}
            <button type="button" class="link" data-raw data-url={url} onclick={() => (opened = url)}>{issue.text}</button>
          {:else}
            <code data-raw>{issue.text}</code>
          {/if}
        {:else}
          {strings.message("moviola-none")}
        {/each}
      </dd>
      <dt>{strings.message("moviola-commits")}</dt>
      <dd>
        {#each frame.links.commit as commit, i (i)}
          {#if commit.url}
            {@const url = commit.url}
            <button type="button" class="link mono" data-raw data-url={url} onclick={() => (opened = url)}>{commit.text}</button>
          {:else}
            <code data-raw>{commit.hash || commit.text}</code>
          {/if}
        {:else}
          {strings.message("moviola-none")}
        {/each}
      </dd>
      <dt>{strings.message("moviola-reports")}</dt>
      <dd>
        {#if frame.links.report.length}
          <ul class="frame-reports">
            {#each frame.links.report as report, i (i)}
              <li>
                <code data-raw title={report.path ?? undefined}>{report.text}</code>
                {#if !report.exists}<span class="faint">{strings.message("moviola-not-here")}</span>{/if}
              </li>
            {/each}
          </ul>
        {:else}
          {strings.message("moviola-none")}
        {/if}
      </dd>
    </dl>
    {#if frame.projectUrl}
      {@const url = frame.projectUrl}
      <p><button type="button" class="link" data-url={url} onclick={() => (opened = url)}>{strings.message("moviola-tracker-project")}</button></p>
    {/if}
    {#if opened}
      <p class="frame-note" role="status">
        <span>{strings.message("moviola-link-note")}</span>
        <span class="mono frame-address" data-raw>{opened}</span>
      </p>
    {/if}
  </section>

  <nav class="frame-around" aria-label={strings.message("moviola-detail")}>
    {#if frame.parents.length}
      <div class="frame-line">
        <span class="frame-label">{strings.message("moviola-from")}</span>
        {#each frame.parents as parent (parent.index)}{@render jump(parent.index)}{/each}
      </div>
    {/if}
    {#if frame.children.length}
      <div class="frame-line">
        <span class="frame-label">{strings.message("moviola-then")}</span>
        {#each frame.children as child (child.index)}{@render jump(child.index)}{/each}
      </div>
    {/if}
    <div class="frame-steps">
      <button type="button" class="btn" disabled={!frame.previous} onclick={() => frame.previous && onselect(frame.previous.index)}>
        <span class="icon icon-prev" aria-hidden="true"></span>{strings.message("moviola-prev")}
      </button>
      <button type="button" class="btn" disabled={!frame.following} onclick={() => frame.following && onselect(frame.following.index)}>
        {strings.message("moviola-next")}<span class="icon icon-next" aria-hidden="true"></span>
      </button>
    </div>
  </nav>
</aside>
