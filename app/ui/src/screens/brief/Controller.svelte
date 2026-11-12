<script lang="ts">
  // What the controller left for the owner (the prototype's avvisi_controller): an unknown budget, an operation of
  // today it could not open, a stopped session it will not resume. The cause is each warning's own, as the
  // controller wrote it (#439/D16): never a sentence of the catalog. The values are the controller's, and none of
  // them is interpreted here.
  import type { ControllerView } from "../../lib/bindings";
  import { moment } from "../../lib/brief";
  import type { Catalog } from "../../lib/i18n";
  import { href } from "../../lib/route";

  interface Props {
    strings: Catalog;
    controller: ControllerView;
  }

  let { strings, controller }: Props = $props();

  interface Field {
    label: string;
    value: string;
    code?: boolean;
  }

  function fields(...list: [string, string | null, boolean?][]): Field[] {
    return list.flatMap(([label, value, code]) => (value ? [{ label: strings.message(label), value, code: code ?? false }] : []));
  }

  // A moment goes into a sentence only once it is read as one: what the controller wrote otherwise stays out of the
  // catalog's words, as text of its own.
  let round = $derived(controller.last_round ? moment(controller.last_round, strings) : null);
  let since = $derived(controller.budget?.since ? moment(controller.budget.since, strings) : null);
</script>

{#snippet details(list: Field[])}
  {#if list.length > 0}
    <dl class="alert-fields">
      {#each list as field (field.label)}
        <dt>{field.label}</dt>
        <dd data-raw class:mono={field.code}>{field.value}</dd>
      {/each}
    </dl>
  {/if}
{/snippet}

{#if round}
  <p class="note">
    {strings.message(controller.stale ? "brief-controller-stale" : "brief-controller-round", { when: round })}
  </p>
{/if}
<ul class="alerts">
  {#if controller.budget}
    <li class="alert">
      <p class="alert-lead">
        <span class="alert-icon" aria-hidden="true"></span>
        <span>{strings.message("brief-controller-budget", { rounds: controller.budget.rounds })}</span>
      </p>
      {#if since}
        <p class="note">{strings.message("brief-controller-budget-since", { when: since })}</p>
      {/if}
      {@render details(fields(["brief-why", controller.budget.reason]))}
    </li>
  {/if}
  {#each controller.to_open as open, index (index)}
    <li class="alert">
      <p class="alert-lead">
        <span class="alert-icon" aria-hidden="true"></span>
        <span>{strings.message("brief-controller-open")}</span>
        <span class="mono" data-raw>{open.name}</span>
      </p>
      {@render details(fields(["brief-why", open.reason], ["brief-where", open.folder, true], ["brief-prompt", open.prompt, true]))}
    </li>
  {/each}
  {#each controller.to_resume as stopped, index (index)}
    <li class="alert">
      <p class="alert-lead">
        <span class="alert-icon" aria-hidden="true"></span>
        <span>{strings.message("brief-controller-resume")}</span>
        {#if stopped.project}<span class="project" data-raw>{stopped.project}</span>{/if}
      </p>
      {@render details(
        fields(
          ["brief-why", stopped.reason],
          ["brief-how", stopped.how, true],
          ["brief-session-id", stopped.session, true],
          ["brief-resumes", stopped.resumes ? (moment(stopped.resumes, strings) ?? stopped.resumes) : null],
        ),
      )}
    </li>
  {/each}
</ul>
<p class="more-line"><a href={href("console")}>{strings.message("brief-stopped-link")}</a></p>
