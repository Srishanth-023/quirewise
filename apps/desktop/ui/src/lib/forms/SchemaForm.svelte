<script lang="ts">
  import { t, tDynamic } from '../i18n';
  import type { ToolDescriptor } from '../ipc/client';
  import { files } from '../stores/files.svelte';
  import { toolName } from '../stores/tools.svelte';
  import { formFields, toolOptions } from '../stores/options.svelte';
  import FieldInput from './FieldInput.svelte';
  import CertificatePicker from './CertificatePicker.svelte';
  import LanguagePicker from './LanguagePicker.svelte';
  import AiPicker from './AiPicker.svelte';
  import LanguageChoice from './LanguageChoice.svelte';
  import type { Field, Values } from './schema';
  import { formFor, ruleMet } from './toolForms';
  import { itemDefaults } from './schema';

  interface Props {
    tool: ToolDescriptor;
  }

  let { tool }: Props = $props();

  const fields = $derived(formFields(tool));
  const hints = $derived(formFor(tool.id));
  const values = $derived(toolOptions.values(tool));
  const drawable = (field: Field): boolean => field.kind !== 'unsupported';

  /** Hidden fields wait for the value of the field they depend on. */
  const visible = (field: Field): boolean => ruleMet(hints.visibleWhen?.[field.key], values);

  const isRows = (field: Field): boolean => field.kind === 'list' && !!hints.rows?.includes(field.key);

  function rowsOf(field: Field): Values[] {
    const current = values[field.key];
    return Array.isArray(current) ? (current as Values[]) : [];
  }

  /** A new row starts from the schema defaults; numbers without one start at their minimum or 1. */
  function addRow(field: Field): void {
    const row = itemDefaults(field);
    for (const child of field.children ?? []) {
      if ((child.kind === 'integer' || child.kind === 'number') && typeof row[child.key] !== 'number') {
        // A page or a count starts at 1; an amount such as an angle starts at nothing.
        row[child.key] = child.min ?? (child.kind === 'integer' ? 1 : 0);
      }
    }
    toolOptions.set(tool, field.key, [...rowsOf(field), row]);
  }

  function removeRow(field: Field, index: number): void {
    toolOptions.set(tool, field.key, rowsOf(field).filter((_, i) => i !== index));
  }

  function moveRow(field: Field, index: number, by: number): void {
    const rows = [...rowsOf(field)];
    const target = index + by;
    if (target < 0 || target >= rows.length) return;
    [rows[index], rows[target]] = [rows[target] as Values, rows[index] as Values];
    toolOptions.set(tool, field.key, rows);
  }

  function setCell(field: Field, index: number, key: string, value: unknown): void {
    toolOptions.set(
      tool,
      field.key,
      rowsOf(field).map((row, i) => (i === index ? { ...row, [key]: value } : row)),
    );
  }

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
    {#if field.kind === 'group' && hints.aiPicker?.includes(field.key)}
      <AiPicker
        id={`${tool.id}-${field.key}`}
        label={listLabel(field)}
        value={groupValues(field)}
        onchange={(v) => toolOptions.set(tool, field.key, v)}
      />
    {:else if field.kind === 'group'}
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
    {:else if isRows(field)}
      <section class="rows" aria-label={listLabel(field)}>
        <h3>{listLabel(field)}</h3>
        {#each rowsOf(field) as row, index (index)}
          <fieldset>
            <legend>{t('rows.item', { n: index + 1 })}</legend>
            {#each field.children ?? [] as child (child.key)}
              <FieldInput
                field={child}
                value={row[child.key]}
                toolId={tool.id}
                id={`${tool.id}-${field.key}-${index}-${child.key}`}

                onchange={(v) => setCell(field, index, child.key, v)}
              />
            {/each}
            <div class="row-actions">
              <button type="button" disabled={index === 0} onclick={() => moveRow(field, index, -1)}>{t('rows.up')}</button>
              <button type="button" disabled={index === rowsOf(field).length - 1} onclick={() => moveRow(field, index, 1)}>{t('rows.down')}</button>
              <button type="button" onclick={() => removeRow(field, index)}>{t('rows.remove')}</button>
            </div>
          </fieldset>
        {/each}
        <button type="button" class="add" onclick={() => addRow(field)}>{t('rows.add')}</button>
      </section>
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
    {:else if hints.certificatePicker?.includes(field.key)}
      <CertificatePicker
        id={`${tool.id}-${field.key}`}
        label={listLabel(field)}
        value={typeof values[field.key] === 'string' ? (values[field.key] as string) : ''}
        onchange={(v) => toolOptions.set(tool, field.key, v)}
      />
    {:else if hints.languageChoice?.includes(field.key)}
      <LanguageChoice
        id={`${tool.id}-${field.key}`}
        label={listLabel(field)}
        hint={tDynamic(`option.${tool.id}.${field.key}.hint`, '')}
        value={typeof values[field.key] === 'string' ? (values[field.key] as string) : 'en'}
        drawableOnly={tool.id === 'translate-pdf' || values.format === 'pdf'}
        onchange={(v) => toolOptions.set(tool, field.key, v)}
      />
    {:else if hints.languagePicker?.includes(field.key)}
      <LanguagePicker
        id={`${tool.id}-${field.key}`}
        label={listLabel(field)}
        value={Array.isArray(values[field.key]) ? (values[field.key] as string[]) : []}
        onchange={(v) => toolOptions.set(tool, field.key, v)}
      />
    {:else}
      <FieldInput
        {field}
        value={values[field.key]}
        toolId={tool.id}
        id={`${tool.id}-${field.key}`}
        limits={hints.limits?.[field.key]}
        secret={hints.secret?.includes(field.key)}
        clearable={hints.clearable?.includes(field.key)}
        color={hints.color?.includes(field.key)}
        picker={hints.picker?.[field.key]}
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
  .rows {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .row-actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }
  .row-actions button,
  .add {
    align-self: flex-start;
    min-height: 32px;
    padding: 0 var(--space-4);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--text);
  }
  .row-actions button:disabled {
    color: var(--text-muted);
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
