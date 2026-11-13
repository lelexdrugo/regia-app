<script lang="ts">
  // A project's moviola: its lanes by direction on the Canvas (lib/moviola/stage.ts), the playhead with its
  // timecode, and the transport: previous and next moment, the playhead as a range, zoom and home. The timecode is
  // the catalog's date-day-time (#439/D30) and is also in the DOM, with its instant in `data-t`, for the proofs.
  // Titles and directions' ids are text from others: in the DOM they sit in data-raw, on the Canvas they are drawn.
  // Beside the stage, the frame of the moment chosen (Frame.svelte, #487): it follows the stage's selection, and
  // reads the moment's sidecar with `get_snapshot`, keeping only the answer for the moment still chosen.
  import { onMount, tick, untrack } from "svelte";

  import type { Catalog } from "../../lib/i18n";
  import type { CanvasText } from "../../lib/moviola/canvas2d";
  import { frame as frameOf, type Notes } from "../../lib/moviola/frame";
  import { type Model, prepare } from "../../lib/moviola/model";
  import { isEmpty, type Names, parseQuery, search, stepWithin } from "../../lib/moviola/search";
  import { Stage } from "../../lib/moviola/stage";
  import { day, dayKey, KIND_ID, LANE_ID, laneName, timecode } from "../../lib/moviola/text";
  import type { MoviolaSource, Snapshot } from "../../lib/moviola/types";
  import { href } from "../../lib/route";
  import Frame from "./Frame.svelte";

  interface Props {
    strings: Catalog;
    source: MoviolaSource;
    project: string;
    /** The moment the address opens, if any. */
    moment?: string | null;
    /** The search the address asks for, if any. */
    query?: string | null;
    /** The project's name, when the portfolio was read: otherwise its id. */
    name: string;
    /** Off only for the harness's negative control of the themes. */
    observeTheme?: boolean;
    onsettled?: () => void;
  }

  let { strings, source, project, moment = null, query = null, name, observeTheme = true, onsettled }: Props = $props();

  type State = { kind: "loading" } | { kind: "ready"; model: Model } | { kind: "failed"; reason: string };
  let view: State = $state({ kind: "loading" });
  let host: HTMLElement | undefined = $state();
  let canvas: HTMLCanvasElement | undefined = $state();
  let stage: Stage | null = $state.raw(null);
  // What the DOM shows of the stage: the playhead, its place on the range, and the selection.
  let t = $state(0);
  let range = $state(0);
  let selected = $state(-1);
  // The sidecar of the moment chosen: only the answer for the one still chosen counts.
  let notes: Notes = $state({ kind: "loading" });
  let snapshot: Snapshot | null = $state.raw(null);
  let asked = "";
  let reading: Promise<void> = Promise.resolve();
  let drawn = $state(false);

  async function readNotes(id: string): Promise<void> {
    asked = id;
    notes = { kind: "loading" };
    snapshot = null;
    try {
      const answer = await source.snapshot(project, id);
      if (asked !== id) return;
      snapshot = answer;
      notes = answer ? { kind: "ready" } : { kind: "none" };
    } catch (error) {
      if (asked === id) notes = { kind: "failed", reason: String(error) };
    }
  }

  $effect(() => {
    const node = view.kind === "ready" ? view.model.nodes[selected] : undefined;
    if (node) {
      const id = node.milestone.id;
      untrack(() => {
        if (id !== asked) reading = readNotes(id);
      });
    } else {
      asked = "";
    }
  });

  const current = $derived.by(() => (view.kind === "ready" && selected >= 0 ? frameOf(view.model, selected, snapshot) : null));

  /** The moment an address names: chosen on the stage, its notes read. */
  async function open(made: Stage, model: Model, id: string | null): Promise<void> {
    const node = id ? model.byId.get(id) : undefined;
    if (!node) return;
    made.select(node.index);
    await tick();
    await reading;
    await tick();
  }

  // Another moment or search in the address, once the stage is drawn: the screen has drawn when its notes are in.
  $effect(() => {
    const id = moment;
    const wanted = query;
    if (drawn && stage && view.kind === "ready") {
      const made = stage;
      const model = view.model;
      untrack(() => {
        text = wanted ?? "";
        void open(made, model, id).then(() => onsettled?.());
      });
    }
  });

  function close(): void {
    stage?.select(-1);
    canvas?.focus();
  }

  // The search (#397/D12): a word or a #number; Esc empties it, Enter opens the first moment found, «Clear the
  // filters» empties it from the list. It lives with the screen: leaving the project forgets it.
  let text = $state(untrack(() => query ?? ""));
  let field: HTMLInputElement | undefined = $state();
  const parsed = $derived(parseQuery(text));
  const searching = $derived(!isEmpty(parsed));
  const found = $derived.by(() => (view.kind === "ready" ? search(view.model, parsed, names(strings, view.model)) : []));

  function names(catalog: Catalog, model: Model): Names {
    return {
      kind: (node) => catalog.message(KIND_ID[node.milestone.kind]),
      lane: (node) => laneName(catalog, model, model.lanes[node.lane] as Model["lanes"][number]),
    };
  }

  /** Why nothing is found: the one task no moment names, or what the search looks at. */
  function none(): string {
    const only = parsed.words.length === 0 && parsed.tasks.length === 1 ? parsed.tasks[0] : undefined;
    return only !== undefined ? strings.message("moviola-search-none-task", { task: String(only) }) : strings.message("moviola-search-none", { query: text.trim() });
  }

  function choose(index: number): void {
    stage?.select(index);
    canvas?.focus();
  }

  function searchKeys(event: KeyboardEvent): void {
    if (event.key === "Escape" && text) {
      event.preventDefault();
      event.stopPropagation();
      text = "";
    } else if (event.key === "Enter") {
      event.preventDefault();
      const first = found[0];
      if (first) choose(first.index);
    }
  }

  function clear(): void {
    text = "";
    field?.focus();
  }

  // `/` finds a moment, `,` and `.` walk the moments, within the search when there is one. Only when no field holds
  // the focus, no modifier is held and no dialog is open, as the shell's own letters.
  function keys(event: KeyboardEvent): void {
    const target = event.target instanceof HTMLElement ? event.target : null;
    if (event.ctrlKey || event.metaKey || event.altKey || document.querySelector("dialog[open]")) return;
    if (target?.matches("textarea, select, [contenteditable], input:not([type=radio], [type=checkbox], [type=range])")) return;
    if (event.key === "/") {
      event.preventDefault();
      field?.focus();
      field?.select();
    } else if ((event.key === "," || event.key === ".") && stage) {
      event.preventDefault();
      const direction = event.key === "." ? 1 : -1;
      if (searching) {
        const next = stepWithin(found, stage.selected, direction);
        if (next) stage.select(next.index);
      } else {
        stage.step(direction);
      }
    }
  }

  function words(catalog: Catalog, model: Model): CanvasText {
    return {
      day: (at) => day(catalog, at),
      timecode: (at) => timecode(catalog, at),
      laneName: (lane) => laneName(catalog, model, lane),
      laneStatus: (lane) => catalog.message(LANE_ID[lane.status]),
    };
  }

  async function read(): Promise<void> {
    view = { kind: "loading" };
    try {
      const model = prepare(await source.timeline(project));
      t = model.t1;
      view = { kind: "ready", model };
    } catch (error) {
      view = { kind: "failed", reason: String(error) };
    }
    await tick();
    if (view.kind !== "ready" || view.model.nodes.length === 0) {
      onsettled?.();
    }
  }

  // The stage, once its canvas is in the DOM; another language redraws its words, nothing else.
  $effect(() => {
    if (view.kind !== "ready" || !host || !canvas || view.model.nodes.length === 0) {
      return;
    }
    const model = view.model;
    const made = new Stage(host, canvas, model, {
      text: untrack(() => words(strings, model)),
      dayKey,
      observeTheme: untrack(() => observeTheme),
      onchange: (moved) => {
        t = moved.t;
        range = moved.rangeValue();
        selected = moved.selected;
      },
    });
    stage = made;
    void made.ready.then(async () => {
      range = made.rangeValue();
      await open(made, model, untrack(() => moment));
      drawn = true;
      onsettled?.();
    });
    return () => {
      made.destroy();
      stage = null;
      drawn = false;
    };
  });

  $effect(() => {
    const catalog = strings;
    if (stage && view.kind === "ready") {
      stage.setText(words(catalog, view.model));
    }
  });

  function count(model: Model): { n: number; lanes: number } {
    return { n: model.nodes.length, lanes: model.lanes.length };
  }

  function scrub(event: Event): void {
    stage?.setRange(Number((event.currentTarget as HTMLInputElement).value));
  }

  onMount(read);
</script>

<svelte:window onkeydown={keys} />

<div class="screen moviola" data-moviola={view.kind} aria-busy={view.kind === "loading"}>
  <header class="titlerow">
    <div class="titlerow-main">
      <p class="crumb"><a href={href("moviola")}>{strings.message("moviola-back")}</a></p>
      <h1 tabindex="-1" data-raw title={name}>{name}</h1>
      {#if view.kind === "ready" && view.model.nodes.length > 0}
        <p class="titlerow-sub">{strings.message("moviola-timeline-sub", count(view.model))}</p>
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
  {:else if view.model.nodes.length === 0}
    <p class="empty">{strings.message("moviola-sub-empty")}</p>
  {:else}
    {@const model = view.model}
    {@const node = model.nodes[selected]}
    <div class="moviola-tools" role="search">
      <span class="search-wrap">
        <span class="icon icon-search" aria-hidden="true"></span>
        <input
          bind:this={field}
          bind:value={text}
          id="search"
          class="search-field"
          type="search"
          maxlength="200"
          autocomplete="off"
          spellcheck="false"
          aria-label={strings.message("moviola-search")}
          aria-describedby={searching ? "moviola-found" : undefined}
          placeholder={strings.message("moviola-search-placeholder")}
          onkeydown={searchKeys}
        />
        {#if text}
          <button type="button" class="btn icon-btn" aria-label={strings.message("moviola-search-clear")} title={strings.message("moviola-search-clear")} onclick={clear}>
            <span class="icon icon-close" aria-hidden="true"></span>
          </button>
        {/if}
      </span>
      {#if searching}
        <span class="faint" id="moviola-found" aria-live="polite">{strings.message("moviola-search-count", { n: found.length, total: model.nodes.length })}</span>
      {/if}
    </div>
    <div class="moviola-body">
    <div class="moviola-main">
    <!-- The moments found come right after the field: Tab goes from the search to them, then to the stage. -->
    {#if searching}
      <section class="results" aria-labelledby="moviola-results">
        <h2 id="moviola-results">{strings.message("moviola-results")}</h2>
        {#if found.length}
          <ol class="results-list">
            {#each found as hit (hit.index)}
              <li>
                <button type="button" class="result" aria-current={hit.index === selected ? "true" : undefined} data-id={hit.milestone.id} onclick={() => choose(hit.index)}>
                  <span class="mono faint">{timecode(strings, hit.t)}</span>
                  <span class="reel-kind">{strings.message(KIND_ID[hit.milestone.kind])}</span>
                  <span class="result-title" data-raw>{hit.milestone.title}</span>
                </button>
              </li>
            {/each}
          </ol>
        {:else}
          <div class="noresult" role="status">
            <p>{none()}</p>
            <button type="button" class="btn" onclick={clear}>{strings.message("moviola-filter-clear")}</button>
          </div>
        {/if}
      </section>
    {/if}
    <section class="stage-panel" aria-label={strings.message("moviola-lanes-directions")}>
      <div class="stage" bind:this={host}>
        <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
        <canvas
          bind:this={canvas}
          tabindex="0"
          aria-label={strings.message("moviola-stage")}
          aria-describedby="moviola-status"
        ></canvas>
      </div>
      <div class="transport">
        <button type="button" class="btn icon-btn" aria-label={strings.message("moviola-prev")} title={strings.message("moviola-prev")} onclick={() => stage?.step(-1)}>
          <span class="icon icon-prev" aria-hidden="true"></span>
        </button>
        <button type="button" class="btn icon-btn" aria-label={strings.message("moviola-next")} title={strings.message("moviola-next")} onclick={() => stage?.step(1)}>
          <span class="icon icon-next" aria-hidden="true"></span>
        </button>
        <output class="timecode mono" data-t={t}>{timecode(strings, t)}</output>
        <input
          class="scrub"
          type="range"
          min="0"
          max="1000"
          step="1"
          value={range}
          aria-label={strings.message("moviola-playhead")}
          aria-valuetext={timecode(strings, t)}
          oninput={scrub}
        />
        <button type="button" class="btn icon-btn" aria-label={strings.message("moviola-zoom-out")} title={strings.message("moviola-zoom-out")} onclick={() => stage?.zoom(0.8)}>
          <span class="icon icon-minus" aria-hidden="true"></span>
        </button>
        <button type="button" class="btn icon-btn" aria-label={strings.message("moviola-zoom-in")} title={strings.message("moviola-zoom-in")} onclick={() => stage?.zoom(1.25)}>
          <span class="icon icon-plus" aria-hidden="true"></span>
        </button>
        <button type="button" class="btn icon-btn" aria-label={strings.message("moviola-fit")} title={strings.message("moviola-fit")} onclick={() => stage?.home()}>
          <span class="icon icon-fit" aria-hidden="true"></span>
        </button>
      </div>
      <p class="stage-status" id="moviola-status" aria-live="polite" data-selected={node ? node.milestone.id : ""}>
        {#if node}
          <span class="reel-kind">{strings.message(KIND_ID[node.milestone.kind])}</span>
          <span class="stage-title" data-raw title={node.milestone.title}>{node.milestone.title}</span>
          <span class="mono faint">{timecode(strings, node.t)}</span>
        {:else}
          <span class="faint">{strings.message("moviola-stage-hint")}</span>
        {/if}
      </p>
    </section>
    </div>
    {#if current}
      <Frame {strings} {model} frame={current} {notes} onselect={(index) => stage?.select(index)} onclose={close} />
    {:else}
      <aside class="frame frame-empty" aria-labelledby="moviola-frame-none">
        <h2 id="moviola-frame-none">{strings.message("moviola-nothing-title")}</h2>
        <p class="faint">{strings.message("moviola-nothing-body")}</p>
      </aside>
    {/if}
    </div>
  {/if}
</div>
