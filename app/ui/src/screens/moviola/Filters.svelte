<script lang="ts">
  // The moviola's filters (#598): the prototype viewer's `disegnaFiltri`. Basic hides kinds; advanced also keeps only
  // who worked, one lane, one health, or the moments with checks or defects declared, and going back to basic clears
  // them, so none stays on out of sight. Every control is a native one: Tab reaches it, Space and the arrow keys
  // change it. Lane ids and models are text from others, inside the catalog's words.
  import type { Catalog } from "../../lib/i18n";
  import { ALL_KINDS, basic, type Filters, modeOf, NONE, whoOptions, whoParts } from "../../lib/moviola/filters";
  import type { Model } from "../../lib/moviola/model";
  import { HEALTH_ID, KIND_ID, LANE_ID, laneName } from "../../lib/moviola/text";
  import type { Health, MilestoneKind } from "../../lib/moviola/types";

  interface Props {
    strings: Catalog;
    model: Model;
    filters: Filters;
    onchange: (filters: Filters) => void;
  }

  let { strings, model, filters, onchange }: Props = $props();

  const mode = $derived(modeOf(filters));
  const who = $derived(whoOptions(model));
  const HEALTHS: readonly Health[] = ["healthy", "at_risk", "waiting_on_owner"];
  const MODES = ["basic", "advanced"] as const;

  function show(kind: MilestoneKind, on: boolean): void {
    const hidden = filters.hidden.filter((k) => k !== kind);
    onchange({ ...filters, hidden: on ? hidden : [...hidden, kind] });
  }

  function choose(choice: (typeof MODES)[number]): void {
    onchange(choice === "basic" ? basic(filters) : { ...filters, mode: "advanced" });
  }

  function whoName(key: string): string {
    const none = strings.message("moviola-not-declared");
    if (key === NONE) return none;
    const parts = whoParts(key);
    return strings.message(parts.cited ? "moviola-filter-who-cited" : "moviola-filter-who-use", { agent: parts.agent ?? none, model: parts.model ?? none });
  }

  function value(event: Event): string {
    return (event.currentTarget as HTMLSelectElement).value;
  }
</script>

<div class="moviola-filters" role="group" aria-label={strings.message("moviola-filters")} data-mode={mode} data-hidden={filters.hidden.join(",")}>
  <div class="filter-group" role="radiogroup" aria-labelledby="moviola-filter-mode">
    <span class="filter-name" id="moviola-filter-mode">{strings.message("moviola-filter-mode")}</span>
    {#each MODES as choice (choice)}
      <label>
        <input type="radio" name="moviola-filter-mode" value={choice} checked={mode === choice} onchange={() => choose(choice)} />
        <span>{strings.message(choice === "basic" ? "moviola-filter-basic" : "moviola-filter-advanced")}</span>
      </label>
    {/each}
  </div>
  <div class="filter-group" role="group" aria-labelledby="moviola-filter-type">
    <span class="filter-name" id="moviola-filter-type">{strings.message("moviola-filter-type")}</span>
    {#each ALL_KINDS as kind (kind)}
      <label data-kind={kind}>
        <input type="checkbox" checked={!filters.hidden.includes(kind)} onchange={(event) => show(kind, event.currentTarget.checked)} />
        <span>{strings.message(KIND_ID[kind])}</span>
      </label>
    {/each}
  </div>
  {#if mode === "advanced"}
    <div class="filter-group filter-more">
      <label class="filter-select">
        <span class="filter-name">{strings.message("moviola-filter-who")}</span>
        <select value={filters.who} onchange={(event) => onchange({ ...filters, mode: "advanced", who: value(event) })}>
          <option value="">{strings.message("moviola-filter-who-any")}</option>
          {#each who as key (key)}
            <option value={key}>{whoName(key)}</option>
          {/each}
        </select>
      </label>
      <label class="filter-select">
        <span class="filter-name">{strings.message("moviola-filter-lane")}</span>
        <select value={filters.lane} onchange={(event) => onchange({ ...filters, mode: "advanced", lane: value(event) })}>
          <option value="">{strings.message("moviola-filter-lane-any")}</option>
          {#each model.lanes as lane (lane.id)}
            <option value={lane.id}>{strings.message("moviola-filter-lane-option", { lane: laneName(strings, model, lane), status: strings.message(LANE_ID[lane.status]) })}</option>
          {/each}
        </select>
      </label>
      <label class="filter-select">
        <span class="filter-name">{strings.message("moviola-health")}</span>
        <select value={filters.health} onchange={(event) => onchange({ ...filters, mode: "advanced", health: value(event) as Health | "" })}>
          <option value="">{strings.message("moviola-filter-health-any")}</option>
          {#each HEALTHS as health (health)}
            <option value={health}>{strings.message(HEALTH_ID[health])}</option>
          {/each}
        </select>
      </label>
      <label>
        <input type="checkbox" checked={filters.declared} onchange={(event) => onchange({ ...filters, mode: "advanced", declared: event.currentTarget.checked })} />
        <span>{strings.message("moviola-filter-declared")}</span>
      </label>
    </div>
  {/if}
</div>
