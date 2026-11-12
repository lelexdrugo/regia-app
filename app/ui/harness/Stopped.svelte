<script lang="ts">
  // The negative control of the brief proof's #439/D16 (prove/brief.js): the UX prototype's stopped line, which gave
  // every stopped session the same cause, whatever had stopped it. The page of the harness only.
  import type { BriefView } from "../src/lib/bindings";

  interface Props {
    brief: BriefView;
  }

  let { brief }: Props = $props();
</script>

<ul>
  {#each brief.controller?.to_resume ?? [] as stopped, index (index)}
    <li class="alert">
      <span data-raw>{stopped.project}</span> is stopped: the working disk is full. It restarts when there's space.
    </li>
  {/each}
</ul>
