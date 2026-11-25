<script lang="ts">
  // A project's moviola: its lanes by direction on the Canvas (lib/moviola/stage.ts), the playhead with its
  // timecode, and the transport: previous and next moment, the playhead as a range, zoom and home. The timecode is
  // the catalog's date-day-time (#439/D30) and is also in the DOM, with its instant in `data-t`, for the proofs.
  // Titles and directions' ids are text from others: in the DOM they sit in data-raw, on the Canvas they are drawn.
  // Beside the stage, the frame of the moment chosen (Frame.svelte, #487): it follows the stage's selection, and
  // reads the moment's sidecar with `get_snapshot`, keeping only the answer for the moment still chosen.
  // The overview (#598): the filters under the search, which compose with it into one selection, one count and one
  // «Clear the filters»; the strip «time window» above the stage, which moves its camera; the project at the
  // playhead under it; and, while no moment is open, the project's directions in the frame's place.
  // The level of detail (#489): zoomed out, the stage draws each lane's moments in groups, and a line under it says
  // how many moments the groups in view hold, so no moment leaves the view without a word.
  // The list of moments (#490): the canvas is a listbox of every moment the filters keep, a window of it in the page,
  // the moment chosen named with aria-activedescendant (lib/moviola/listbox.ts), and at the zoom of groups each group
  // named once (#489/V9). While the stage has the focus the list says the moment chosen, so the status line and the
  // panel at the playhead stop speaking; a hidden line says when the level of detail changes.
  import { onMount, tick, untrack } from "svelte";

  import type { Catalog } from "../../lib/i18n";
  import type { CanvasText } from "../../lib/moviola/canvas2d";
  import { type Filters as FilterState, filtering, fromAddress, NO_FILTERS, visible } from "../../lib/moviola/filters";
  import { frame as frameOf, type Notes } from "../../lib/moviola/frame";
  import type { ListMode, ListText, Words } from "../../lib/moviola/listbox";
  import { type Model, prepare, stateAt } from "../../lib/moviola/model";
  import { isEmpty, type Names, parseQuery, search, stepWithin } from "../../lib/moviola/search";
  import { type GroupsShown, Stage, type ViewRange } from "../../lib/moviola/stage";
  import { KINDS } from "../../lib/moviola/scene";
  import { day, dayKey, HEALTH_ID, KIND_ID, LANE_ID, laneName, timecode } from "../../lib/moviola/text";
  import type { MoviolaSource, Snapshot } from "../../lib/moviola/types";
  import { href } from "../../lib/route";
  import AtDate, { type Decisions } from "./AtDate.svelte";
  import Directions from "./Directions.svelte";
  import Filters from "./Filters.svelte";
  import Frame from "./Frame.svelte";

  interface Props {
    strings: Catalog;
    source: MoviolaSource;
    project: string;
    /** The moment the address opens, if any. */
    moment?: string | null;
    /** The search the address asks for, if any. */
    query?: string | null;
    /** The filters' mode, the kinds hidden and the lane chosen, as the address asks for them (#598). */
    filterMode?: string | null;
    hide?: string | null;
    lane?: string | null;
    /** The project's name, when the portfolio was read: otherwise its id. */
    name: string;
    /** Off only for the harness's negative control of the themes. */
    observeTheme?: boolean;
    /** The list of moments as the product has it, or a negative control of the harness. */
    listMode?: ListMode;
    onsettled?: () => void;
  }

  let { strings, source, project, moment = null, query = null, filterMode = null, hide = null, lane = null, name, observeTheme = true, listMode = "window", onsettled }: Props =
    $props();

  type State = { kind: "loading" } | { kind: "ready"; model: Model } | { kind: "failed"; reason: string };
  let view: State = $state({ kind: "loading" });
  let host: HTMLElement | undefined = $state();
  let canvas: HTMLCanvasElement | undefined = $state();
  let strip: HTMLCanvasElement | undefined = $state();
  let stage: Stage | null = $state.raw(null);
  // What the DOM shows of the stage: the playhead, its place on the range, the selection, the part in view.
  let t = $state(0);
  let range = $state(0);
  let selected = $state(-1);
  let shown: ViewRange | null = $state(null);
  let grouped = $state<GroupsShown | null>(null);
  const groupsSaid = $derived(grouped ? strings.message("moviola-groups", { moments: grouped.moments, groups: grouped.groups }) : "");
  // The stage holds the focus: the list says the moment chosen, and the live lines keep quiet (#490). The hidden line
  // says when the stage starts or stops drawing groups, and nothing else.
  let onStage = $state(false);
  let levelSaid = $state("");
  let wasGrouped: boolean | null = null;
  const listing = $derived(listMode !== "none");
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

  // The filters (#598) and the lane chosen in the overview, from the address at first.
  let filters: FilterState = $state(untrack(() => fromAddress(filterMode, hide)));
  let chosen = $state(-1);
  const kept = $derived.by(() => (view.kind === "ready" && filtering(filters) ? view.model.nodes.map((node) => visible(node, filters)) : null));

  function laneIndex(model: Model, id: string | null): number {
    return id === null ? -1 : model.lanes.findIndex((l) => l.id === id);
  }

  $effect(() => {
    const made = stage;
    const mask = kept;
    const lane = chosen;
    made?.setEmphasis(mask, lane);
  });

  /** The moment an address names: chosen on the stage, its notes read. */
  async function open(made: Stage, model: Model, id: string | null): Promise<void> {
    const node = id ? model.byId.get(id) : undefined;
    if (!node) return;
    made.select(node.index);
    await tick();
    await reading;
    await tick();
  }

  // Another moment, search or filter in the address, once the stage is drawn: the screen has drawn when its notes
  // are in.
  $effect(() => {
    const id = moment;
    const wanted = query;
    const asking = [filterMode, hide, lane] as const;
    if (drawn && stage && view.kind === "ready") {
      const made = stage;
      const model = view.model;
      untrack(() => {
        text = wanted ?? "";
        filters = fromAddress(asking[0], asking[1]);
        chosen = laneIndex(model, asking[2]);
        void open(made, model, id).then(() => onsettled?.());
      });
    }
  });

  function close(): void {
    stage?.select(-1);
    canvas?.focus();
  }

  // The search (#397/D12): a word or a #number; Esc empties it, Enter opens the first moment found, «Clear the
  // filters» empties it and the filters. It lives with the screen: leaving the project forgets it.
  let text = $state(untrack(() => query ?? ""));
  let field: HTMLInputElement | undefined = $state();
  const parsed = $derived(parseQuery(text));
  const searching = $derived(!isEmpty(parsed));
  const found = $derived.by(() => (view.kind === "ready" ? search(view.model, parsed, names(strings, view.model)) : []));
  /** The moments the search finds that the filters keep: the one selection the screen counts and walks. */
  const hits = $derived(kept ? found.filter((node) => kept[node.index]) : found);
  const narrowing = $derived(searching || filtering(filters));
  const counted = $derived.by(() => (view.kind !== "ready" ? 0 : searching ? hits.length : kept ? kept.filter(Boolean).length : view.model.nodes.length));

  function names(catalog: Catalog, model: Model): Names {
    return {
      kind: (node) => catalog.message(KIND_ID[node.milestone.kind]),
      lane: (node) => laneName(catalog, model, model.lanes[node.lane] as Model["lanes"][number]),
    };
  }

  /** Why nothing is found: the filters hide what the search finds, the one task no moment names, or what the search
   * looks at. */
  function none(): string {
    if (found.length > 0) return strings.message("moviola-filter-hidden");
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
      const first = hits[0];
      if (first) choose(first.index);
    }
  }

  function clear(): void {
    text = "";
    field?.focus();
  }

  /** «Clear the filters» (`azzera i filtri` of the prototype, the search's of #487): the search and every filter go,
   * the mode stays. */
  function clearAll(): void {
    text = "";
    filters = { ...NO_FILTERS, mode: filters.mode };
    field?.focus();
  }

  // `/` finds a moment, `,` and `.` walk the moments, within the search and the filters when there are some. Only
  // when no field holds the focus, no modifier is held and no dialog is open, as the shell's own letters.
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
        const next = stepWithin(hits, stage.selected, direction);
        if (next) stage.select(next.index);
      } else {
        stage.step(direction);
      }
    }
  }

  /** The list's words (#490): a moment's kind, title, instant, lane and health; a group's place, moments, lane, span,
   * kinds and health. Titles and directions' ids are text from others. */
  function listWords(catalog: Catalog, model: Model): ListText {
    const laneWords = (index: number): Words[number] => {
      const lane = model.lanes[index];
      return !lane || lane.id === model.timeline.main_direction ? { text: catalog.message("moviola-lane-main") } : { text: lane.id, raw: true };
    };
    return {
      moment: (index) => {
        const node = model.nodes[index] as Model["nodes"][number];
        const m = node.milestone;
        const said: Words = [{ text: catalog.message(KIND_ID[m.kind]) }, { text: m.title, raw: true }, { text: timecode(catalog, node.t) }, laneWords(node.lane)];
        if (m.health !== "healthy") said.push({ text: catalog.message(HEALTH_ID[m.health]) });
        return said;
      },
      group: (group, position, total) => {
        const span = group.from === group.to ? timecode(catalog, group.from) : catalog.message("moviola-list-span", { from: timecode(catalog, group.from), to: timecode(catalog, group.to) });
        const said: Words = [
          { text: catalog.message("moviola-list-group", { position, total }) },
          { text: catalog.message("moviola-list-moments", { n: group.count }) },
          laneWords(group.lane),
          { text: span },
        ];
        KINDS.forEach((kind, k) => {
          const n = group.kinds[k] ?? 0;
          if (n > 0) said.push({ text: catalog.message("moviola-list-kind", { kind: catalog.message(KIND_ID[kind]), n }) });
        });
        if (group.atRisk > 0) said.push({ text: catalog.message("moviola-list-at-risk", { n: group.atRisk }) });
        if (group.waiting > 0) said.push({ text: catalog.message("moviola-list-waiting", { n: group.waiting }) });
        return said;
      },
    };
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
      chosen = laneIndex(model, lane);
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
      ...(strip ? { strip } : {}),
      observeTheme: untrack(() => observeTheme),
      list: untrack(() => listWords(strings, model)),
      listMode: untrack(() => listMode),
      onchange: (moved) => {
        t = moved.t;
        range = moved.rangeValue();
        // A moment chosen lets the lane go, as the prototype's address of a moment has no direction.
        if (moved.selected >= 0 && moved.selected !== selected) chosen = -1;
        selected = moved.selected;
      },
      onview: (part) => (shown = part),
      ongroups: (groups) => {
        grouped = groups;
        const now = groups !== null;
        if (wasGrouped !== null && now !== wasGrouped) levelSaid = strings.message(now ? "moviola-list-level-groups" : "moviola-list-level-detail");
        wasGrouped = now;
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
      stage.setListText(listWords(catalog, view.model));
    }
  });

  // The project at the playhead (#598): its last moment's open decisions, read once per moment and kept; an answer
  // for a moment no longer the last is dropped, and nothing waits for it.
  const at = $derived.by(() => (view.kind === "ready" ? stateAt(view.model, t) : null));
  const sidecars = new Map<string, Decisions>();
  let decisions: Decisions = $state({ kind: "none" });
  $effect(() => {
    const last = at?.last ?? null;
    if (!last) {
      decisions = { kind: "none" };
      return;
    }
    const id = last.milestone.id;
    untrack(() => {
      const known = sidecars.get(id);
      decisions = known ?? { kind: "loading" };
      if (known) return;
      sidecars.set(id, { kind: "loading" });
      source.snapshot(project, id).then(
        (answer) => {
          const got: Decisions = { kind: "ready", list: answer?.open_decisions ?? [] };
          sidecars.set(id, got);
          if (at?.last?.milestone.id === id) decisions = got;
        },
        () => {
          sidecars.set(id, { kind: "failed" });
          if (at?.last?.milestone.id === id) decisions = { kind: "failed" };
        },
      );
    });
  });

  function count(model: Model): { n: number; lanes: number } {
    return { n: model.nodes.length, lanes: model.lanes.length };
  }

  function scrub(event: Event): void {
    stage?.setRange(Number((event.currentTarget as HTMLInputElement).value));
  }

  /** The window the stage shows, as the prototype's strip writes it: the whole story, or from one instant to another. */
  function windowText(model: Model, part: ViewRange | null): string {
    if (!part || part.whole) return strings.message("moviola-window-whole", { from: timecode(strings, model.t0), to: timecode(strings, model.t1) });
    return strings.message("moviola-window-range", { from: timecode(strings, part.from), to: timecode(strings, part.to) });
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
          aria-describedby={narrowing ? "moviola-found" : undefined}
          placeholder={strings.message("moviola-search-placeholder")}
          onkeydown={searchKeys}
        />
        {#if text}
          <button type="button" class="btn icon-btn" aria-label={strings.message("moviola-search-clear")} title={strings.message("moviola-search-clear")} onclick={clear}>
            <span class="icon icon-close" aria-hidden="true"></span>
          </button>
        {/if}
      </span>
      {#if narrowing}
        <span class="faint" id="moviola-found" aria-live="polite" data-count={counted}>{strings.message("moviola-search-count", { n: counted, total: model.nodes.length })}</span>
        <button type="button" class="btn" id="moviola-clear" onclick={clearAll}>{strings.message("moviola-filter-clear")}</button>
      {/if}
    </div>
    <Filters {strings} {model} {filters} onchange={(next) => (filters = next)} />
    <div class="moviola-body">
    <div class="moviola-main">
    <!-- The moments found come right after the field: Tab goes from the search to them, then to the stage. -->
    {#if searching}
      <section class="results" aria-labelledby="moviola-results">
        <h2 id="moviola-results">{strings.message("moviola-results")}</h2>
        {#if hits.length}
          <ol class="results-list">
            {#each hits as hit (hit.index)}
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
          </div>
        {/if}
      </section>
    {/if}
    <section class="stage-panel" aria-label={strings.message("moviola-lanes-directions")}>
      <div class="strip-row">
        <span class="strip-label" id="moviola-window">{strings.message("moviola-window")}</span>
        <output
          class="strip-range mono"
          aria-labelledby="moviola-window"
          data-whole={!shown || shown.whole ? "true" : "false"}
          data-from={shown ? shown.from : model.t0}
          data-to={shown ? shown.to : model.t1}>{windowText(model, shown)}</output
        >
        <canvas bind:this={strip} class="strip" aria-hidden="true" title={strings.message("moviola-window-hint")}></canvas>
      </div>
      <div class="stage" bind:this={host}>
        <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
        <canvas
          bind:this={canvas}
          tabindex="0"
          role={listing ? "listbox" : undefined}
          aria-orientation={listing ? "horizontal" : undefined}
          aria-label={strings.message("moviola-stage")}
          aria-describedby={grouped ? "moviola-status moviola-groups" : "moviola-status"}
          onfocus={() => (onStage = true)}
          onblur={() => (onStage = false)}
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
      <p class="stage-status" id="moviola-status" aria-live={onStage && listing ? "off" : "polite"} data-selected={node ? node.milestone.id : ""}>
        {#if node}
          <span class="reel-kind">{strings.message(KIND_ID[node.milestone.kind])}</span>
          <span class="stage-title" data-raw title={node.milestone.title}>{node.milestone.title}</span>
          <span class="mono faint">{timecode(strings, node.t)}</span>
        {:else}
          <span class="faint">{strings.message("moviola-stage-hint")}</span>
        {/if}
      </p>
      <!-- Always there, empty at the zoom of detail, one line: the stage keeps its size when the zoom crosses into groups. -->
      <p class="stage-groups" id="moviola-groups" data-groups={grouped ? `${grouped.groups} ${grouped.moments}` : ""} title={groupsSaid || undefined}>{groupsSaid}</p>
      <p class="sr" id="moviola-level" aria-live="polite" data-said={levelSaid}>{levelSaid}</p>
    </section>
    {#if at}
      <AtDate {strings} {model} state={at} {decisions} live={!(onStage && listing)} onselect={(index) => stage?.select(index)} />
    {/if}
    </div>
    {#if current}
      <Frame {strings} {model} frame={current} {notes} {snapshot} onselect={(index) => stage?.select(index)} onclose={close} />
    {:else}
      <Directions {strings} {model} {chosen} onchoose={(index) => (chosen = index)} onselect={(index) => stage?.select(index)} />
    {/if}
    </div>
  {/if}
</div>
