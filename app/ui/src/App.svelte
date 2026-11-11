<script lang="ts">
  // The app's shell: the skip link, the sidebar and the section the address names (lib/route.ts). The screens come
  // with their slices; the ones that are not here yet say so (Later.svelte).
  import { onMount, tick } from "svelte";

  import type { BriefView } from "./lib/bindings";
  import type { Catalog } from "./lib/i18n";
  import { parse, type Route, same } from "./lib/route";
  import Brief from "./screens/Brief.svelte";
  import Later from "./screens/Later.svelte";
  import Sidebar from "./shell/Sidebar.svelte";

  interface Props {
    strings: Catalog;
    product: string;
    load: () => Promise<BriefView>;
    /** Called each time the screen the address names has drawn: the brief once its data or error is in the DOM. */
    onsettled?: () => void;
  }

  let { strings, product, load, onsettled }: Props = $props();

  let route: Route = $state(parse(location.hash));
  let main: HTMLElement | undefined = $state();

  const LATER: Record<string, string> = { daily: "nav-daily", moviola: "nav-moviola", console: "nav-console" };

  function heading(): HTMLElement | null {
    return main?.querySelector<HTMLElement>("h1") ?? null;
  }

  async function follow(): Promise<void> {
    const next = parse(location.hash);
    if (same(next, route)) {
      await tick();
      onsettled?.();
      return;
    }
    route = next;
    await tick();
    // Focus follows navigation, and only navigation: the keyboard and a screen reader land on the new screen.
    heading()?.focus({ preventScroll: true });
    if (route.section !== "brief") {
      onsettled?.();
    }
  }

  function skip(event: MouseEvent): void {
    // The address stays the section's: #main would read as an unknown section.
    event.preventDefault();
    heading()?.focus();
  }

  onMount(() => {
    addEventListener("hashchange", follow);
    if (route.section !== "brief") {
      void tick().then(() => onsettled?.());
    }
    return () => removeEventListener("hashchange", follow);
  });
</script>

<div class="shell">
  <a class="skip" href="#main" onclick={skip}>{strings.message("nav-skip")}</a>
  <Sidebar {strings} {product} current={route.section} />
  <main id="main" tabindex="-1" bind:this={main}>
    {#if route.section === "brief"}
      <Brief {strings} {load} {...onsettled ? { onsettled } : {}} />
    {:else if route.section === "settings"}
      <Later {strings} title="nav-settings" />
    {:else}
      <Later {strings} title={LATER[route.section] ?? "nav-brief"} />
    {/if}
  </main>
</div>
