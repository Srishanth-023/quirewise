<script lang="ts">
  import { t, tDynamic } from '../i18n';
  import type { ToolDescriptor } from '../ipc/client';
  import { files } from '../stores/files.svelte';
  import { toolName } from '../stores/tools.svelte';
  import { formFields, toolOptions } from '../stores/options.svelte';
  import FieldInput from './FieldInput.svelte';
  import type { Field, Values } from './schema';
  import { formFor } from './toolForms';

  interface Props {
    tool: ToolDescriptor;
  }

  let { tool }: Props = $props();

  const fields = $derived(formFields(tool));
  const hints = $derived(formFor(tool.id));
  const values = $derived(toolOptions.values(tool));
  const drawable = (field: Field): boolean => field.kind !== 'unsupported';

  /** Hidden fields wait for the value of the field they depend on. */
  const visible = (field: Field): boolean => {
    const rule = hints.visibleWhen?.[field.key];
    return !rule || values[rule.field] === rule.equals;
  };

  function groupValues(field: Field): Values {
    const current = values[field.key];
    return typeof current === 'object' && current !== null ? (current as Values) : {};
  }

  function listLabel(field: Field): string {
    return tDynamic(`option.${tool.id}.${field.key}`, tDynamic(`option.${field.key}`, field.key.replaceAll('_', ' ')));
  }
</script>

<form class="form" aria-label={t('options.formLabel', { tool: toolName(tool) })} onsubmit={(e) => e.preventDefault()}>
  {#each fields.filter((f) => drawable(f) && visible(f)) as field (field.key)}
    {#if field.kind === 'group'}
      <fieldset class="group">
        <legend>{listLabel(field)}</legend>
        {#each field.children ?? [] as child (child.key)}
          <FieldInput
            field={child}
            value={groupValues(field)[child.key]}
            toolId={tool.id}
            id={`${tool.id}-${field.key}-${child.key}`}
            onchange={(v) => toolOptions.set(tool, field.key, { ...groupValues(field), [child.key]: v })}
          />
        {/each}
      </fieldset>
    {:else if field.kind === 'list'}
      <!-- One group of settings per file, in the order of the file list. -->
      {#if files.entries.length > 0}
        <section class="per-file" aria-label={listLabel(field)}>
          <h3>{listLabel(field)}</h3>
          {#each files.entries as entry (entry.id)}
            <fieldset>
              <legend>{entry.name}</legend>
              {#each field.children ?? [] as child (child.key)}
                {@const own = toolOptions.perFile(tool, field, entry.id)}
                <FieldInput
                  field={child}
                  value={own[child.key]}
                  toolId={tool.id}
                  id={`${tool.id}-${field.key}-${entry.id}-${child.key}`}
                  onchange={(v) => (own[child.key] = v)}
                />
              {/each}
            </fieldset>
          {/each}
        </section>
      {/if}
    {:else}
      <FieldInput
        {field}
        value={values[field.key]}
        toolId={tool.id}
        id={`${tool.id}-${field.key}`}
        limits={hints.limits?.[field.key]}
        secret={hints.secret?.includes(field.key)}
        clearable={hints.clearable?.includes(field.key)}
        onchange={(v) => toolOptions.set(tool, field.key, v)}
      />
    {/if}
  {/each}
  {#if fields.some(drawable)}
    <button type="button" class="reset" onclick={() => toolOptions.reset(tool)}>{t('options.reset')}</button>
  {:else}
    <p class="none">{t('options.none')}</p>
  {/if}
</form>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
  }
  h3 {
    margin: 0 0 var(--space-3);
    font-size: var(--text-base);
  }
  .per-file {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  fieldset {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    margin: 0;
    padding: var(--space-3) var(--space-4) var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-panel);
    min-width: 0;
  }
  legend {
    padding: 0 var(--space-2);
    font-weight: 500;
    overflow-wrap: anywhere;
  }
  .reset {
    align-self: flex-start;
    min-height: 32px;
    padding: 0 var(--space-4);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--text);
  }
  .none {
    margin: 0;
    color: var(--text-muted);
  }
</style>
