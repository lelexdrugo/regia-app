<script lang="ts">
  // The settings this version has, in their general area: how the app looks and how dense it is. Each choice is
  // a group of radio buttons, so the arrow keys move within it, and each saves itself.
  import type { Density, Direction } from "../lib/bindings";
  import type { Catalog } from "../lib/i18n";
  import { DENSITIES, DIRECTIONS, type Look, type Saving } from "../lib/look";

  interface Props {
    strings: Catalog;
    look: Look;
    /** What the last save said. */
    saving: Saving;
    onlook: (change: Partial<Look>) => void;
  }

  let { strings, look, saving, onlook }: Props = $props();

  const DIRECTION_NAMES: Record<Direction, string> = {
    montaggio: "dir-montaggio",
    foglio: "dir-foglio",
    nero: "dir-nero",
  };
  const DENSITY_NAMES: Record<Density, string> = {
    compact: "settings-density-compact",
    comfortable: "settings-density-comfortable",
  };
</script>

<div class="screen">
  <header class="titlerow">
    <h1 tabindex="-1">{strings.message("settings-title", { area: strings.message("settings-area-general") })}</h1>
  </header>
  <div class="settings">
    <section class="setting" aria-labelledby="setting-look">
      <div class="setting-label">
        <h2 id="setting-look">{strings.message("settings-look")}</h2>
        <p class="note" id="setting-look-help">{strings.message("settings-look-help-three")}</p>
      </div>
      <div class="choices" role="radiogroup" aria-labelledby="setting-look" aria-describedby="setting-look-help">
        {#each DIRECTIONS as direction (direction)}
          <label class="choice">
            <input
              type="radio"
              name="direction"
              value={direction}
              checked={look.direction === direction}
              onchange={() => onlook({ direction })}
            />
            <span>{strings.message(DIRECTION_NAMES[direction])}</span>
          </label>
        {/each}
      </div>
    </section>
    <section class="setting" aria-labelledby="setting-density">
      <div class="setting-label">
        <h2 id="setting-density">{strings.message("settings-density")}</h2>
        <p class="note" id="setting-density-help">{strings.message("settings-density-help")}</p>
      </div>
      <div class="choices" role="radiogroup" aria-labelledby="setting-density" aria-describedby="setting-density-help">
        {#each DENSITIES as density (density)}
          <label class="choice">
            <input
              type="radio"
              name="density"
              value={density}
              checked={look.density === density}
              onchange={() => onlook({ density })}
            />
            <span>{strings.message(DENSITY_NAMES[density])}</span>
          </label>
        {/each}
      </div>
    </section>
  </div>
  <p class="saving note" role="status">
    {#if saving.kind === "saved"}
      {strings.message("settings-saved")}
    {:else if saving.kind === "failed"}
      <span class="warning">{strings.message("settings-save-failed", { reason: saving.reason })}</span>
    {/if}
  </p>
</div>
