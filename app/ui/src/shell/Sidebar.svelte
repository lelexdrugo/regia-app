<script lang="ts">
  // The sidebar: the wordmark, the sections, then what is not a section. Labels and not a bar of icons, because a
  // colleague who opens the app once a week must read where each one goes (design system README); the icons are
  // CSS masks of files, so the sidebar holds no SVG and no markup from anyone.
  import type { Catalog } from "../lib/i18n";
  import { href, type Section } from "../lib/route";

  interface Props {
    strings: Catalog;
    /** The product's name, from `get_app_info`: the catalogs leave it out (#226). */
    product: string;
    current: Section;
  }

  let { strings, product, current }: Props = $props();

  const MAIN: { section: Section; label: string }[] = [
    { section: "brief", label: "nav-brief" },
    { section: "daily", label: "nav-daily" },
    { section: "moviola", label: "nav-moviola" },
    { section: "console", label: "nav-console" },
  ];
</script>

<aside class="sidebar">
  <a class="wordmark" href={href("brief")} aria-label={strings.message("nav-home")}>
    <span class="mark" aria-hidden="true"><span class="mark-gate"></span><span class="mark-head"></span></span>
    <span class="wordmark-text" data-raw>{product}</span>
  </a>
  <nav class="nav" aria-label={strings.message("nav-main")}>
    {#each MAIN as item (item.section)}
      <a
        class="nav-item"
        class:on={current === item.section}
        href={href(item.section)}
        aria-current={current === item.section ? "page" : undefined}
      >
        <span class="icon icon-{item.section}" aria-hidden="true"></span>
        <span class="nav-label"><span class="nav-text">{strings.message(item.label)}</span></span>
      </a>
    {/each}
  </nav>
  <div class="nav-spacer"></div>
  <nav class="nav" aria-label={strings.message("nav-other")}>
    <a
      class="nav-item"
      class:on={current === "settings"}
      href={href("settings")}
      aria-current={current === "settings" ? "page" : undefined}
    >
      <span class="icon icon-settings" aria-hidden="true"></span>
      <span class="nav-label"><span class="nav-text">{strings.message("nav-settings")}</span></span>
    </a>
  </nav>
</aside>
