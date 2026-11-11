<script lang="ts">
  // The app's shell: the skip link, the sidebar and the section the address names (lib/route.ts). The screens come
  // with their slices; the ones that are not here yet say so (Later.svelte).
  import { onMount, tick, untrack } from "svelte";

  import type { BriefView, LanguageTag, Settings as Saved, SettingsChange } from "./lib/bindings";
  import type { Catalog } from "./lib/i18n";
  import { draw, type Look, type Saving } from "./lib/look";
  import { href, parse, type Route, same, type Section } from "./lib/route";
  import Brief from "./screens/Brief.svelte";
  import Later from "./screens/Later.svelte";
  import Settings from "./screens/Settings.svelte";
  import Help from "./shell/Help.svelte";
  import Sidebar from "./shell/Sidebar.svelte";

  interface Props {
    /** The catalog the app opened in. */
    strings: Catalog;
    /** Any language's catalog: the one chosen replaces `strings` without reloading. */
    catalogFor: (language: LanguageTag) => Catalog;
    product: string;
    load: () => Promise<BriefView>;
    /** The settings as `get_settings` answered them, before the shell was drawn. */
    settings: Saved;
    /** Saves the fields given, and answers what is saved (`set_settings`). */
    save: (change: SettingsChange) => Promise<Saved>;
    /** Called each time the screen the address names has drawn: the brief once its data or error is in the DOM. */
    onsettled?: () => void;
  }

  let { strings, catalogFor, product, load, settings, save, onsettled }: Props = $props();

  let route: Route = $state(parse(location.hash));
  let main: HTMLElement | undefined = $state();
  let look: Look = $state(untrack(() => ({ direction: settings.direction, density: settings.density })));
  let saving: Saving = $state({ kind: "idle" });
  // The catalog every screen reads: a new one redraws their text, and nothing else.
  let words: Catalog = $state.raw(untrack(() => strings));

  function speak(next: Catalog): void {
    words = next;
    document.documentElement.lang = next.language;
    document.title = next.message("app-title");
  }

  /**
   * Another language at once, in place: the same address, scroll and focus, since only text changes and focus
   * follows navigation alone. Then saved; a save that fails puts the language back, and says why.
   */
  async function changeLanguage(language: LanguageTag): Promise<void> {
    const before = words;
    speak(catalogFor(language));
    try {
      await save({ language });
      saving = { kind: "saved" };
    } catch (error) {
      speak(before);
      saving = { kind: "failed", reason: String(error) };
    }
  }

  /** Drawn at once, then saved; a save that fails puts the page back as it was, and says why. */
  async function changeLook(change: Partial<Look>): Promise<void> {
    const before = look;
    look = { ...look, ...change };
    draw(look);
    try {
      const saved = await save(change);
      look = { direction: saved.direction, density: saved.density };
      saving = { kind: "saved" };
    } catch (error) {
      look = before;
      saving = { kind: "failed", reason: String(error) };
    }
    draw(look);
  }

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

  // The keyboard: g and a letter goes to a section within 1.2 s, ? opens the help (its Esc is the dialog's own).
  // A letter acts only when no field holds the focus and no modifier is held; Esc leaves a field.
  const GO: Record<string, Section> = { b: "brief", d: "daily", m: "moviola", c: "console", s: "settings" };
  let going: ReturnType<typeof setTimeout> | null = null;
  let help = $state(false);
  let opener: HTMLElement | null = null;

  function openHelp(): void {
    if (!help) {
      opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      help = true;
    }
  }

  async function closeHelp(): Promise<void> {
    help = false;
    await tick();
    opener?.focus();
  }

  function keydown(event: KeyboardEvent): void {
    if (event.ctrlKey || event.metaKey || event.altKey || help) {
      return;
    }
    const target = event.target instanceof HTMLElement ? event.target : null;
    const typing = target?.matches("textarea, select, [contenteditable], input:not([type=radio], [type=checkbox])") ?? false;
    if (typing) {
      if (event.key === "Escape") {
        target?.blur();
      }
      return;
    }
    if (going !== null) {
      clearTimeout(going);
      going = null;
      const section = GO[event.key];
      if (section) {
        event.preventDefault();
        location.hash = href(section);
        return;
      }
    }
    if (event.key === "g") {
      going = setTimeout(() => (going = null), 1200);
    } else if (event.key === "?") {
      event.preventDefault();
      openHelp();
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

<svelte:window onkeydown={keydown} />

<div class="shell">
  <a class="skip" href="#main" onclick={skip}>{words.message("nav-skip")}</a>
  <Sidebar
    strings={words}
    {catalogFor}
    {product}
    {saving}
    current={route.section}
    onlanguage={changeLanguage}
    onhelp={openHelp}
  />
  <main id="main" tabindex="-1" bind:this={main}>
    {#if route.section === "brief"}
      <Brief strings={words} {load} {...onsettled ? { onsettled } : {}} />
    {:else if route.section === "settings"}
      <Settings strings={words} {catalogFor} {look} {saving} onlook={changeLook} onlanguage={changeLanguage} />
    {:else}
      <Later strings={words} title={LATER[route.section] ?? "nav-brief"} />
    {/if}
  </main>
  {#if help}
    <Help strings={words} onclose={closeHelp} />
  {/if}
</div>
