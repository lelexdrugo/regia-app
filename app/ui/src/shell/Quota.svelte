<script module lang="ts">
  export interface Quota {
    service: string;
    /** Percent of the window used, 0 to 100. */
    used: number;
  }
</script>

<script lang="ts">
  // The principal service's quota in the 5-hour window, at the foot of the sidebar: what is used and what is left,
  // both in words (#439/D29 found "claude 6 %" ambiguous, and what is left had to be worked out by hand), and a
  // meter that turns at 75 %, where nothing starts automatically any more.
  import type { Catalog } from "../lib/i18n";

  interface Props {
    strings: Catalog;
    quota: Quota;
  }

  let { strings, quota }: Props = $props();

  let used = $derived(Math.min(100, Math.max(0, Math.round(quota.used))));
</script>

<div class="quota">
  <meter
    min="0"
    max="100"
    low="75"
    high="75"
    optimum="0"
    value={used}
    aria-label={strings.message("nav-quota-title")}
  ></meter>
  <p class="quota-text">
    <span data-raw>{quota.service}</span>
    {strings.message("nav-quota-used-left", { used, left: 100 - used })}
  </p>
</div>
