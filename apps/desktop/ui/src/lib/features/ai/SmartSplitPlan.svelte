<script lang="ts">
  import { t } from '../../i18n';
  import { ipc, toAppError, type SmartSplitOptions, type ToolDescriptor } from '../../ipc/client';
  import { files } from '../../stores/files.svelte';
  import { toolOptions } from '../../stores/options.svelte';
  import { toasts } from '../../stores/toasts.svelte';
  import { partsProblem, rowsFromParts, type PartRow } from './ai';

  interface Props {
    tool: ToolDescriptor;
  }

  let { tool }: Props = $props();

  const entry = $derived(files.selected);
  const pageCount = $derived(entry?.report?.page_count ?? 0);
  const values = $derived(toolOptions.values(tool));
  const instruction = $derived(typeof values.instruction === 'string' ? values.instruction : '');
  const rows = $derived<PartRow[]>(Array.isArray(values.parts) ? (values.parts as PartRow[]) : []);
  const problem = $derived(partsProblem(rows, pageCount));

  let working = $state(false);
  /** The instruction the parts were proposed for, to tell when they are out of date. */
  let proposedFor = $state('');

  const usable = $derived(!!entry && !!entry.report && !entry.report.locked && instruction.trim() !== '');
  const stale = $derived(rows.length > 0 && proposedFor !== instruction);

  async function preview(): Promise<void> {
    if (!entry || !usable) return;
    working = true;
    try {
      const options = {
        ...(toolOptions.request(tool, []) as unknown as SmartSplitOptions),
        parts: [],
      } satisfies SmartSplitOptions;
      const found = await ipc.smartSplitPreview(entry.path, files.passwordFor(entry.id), options);
      toolOptions.set(tool, 'parts', rowsFromParts(found));
      proposedFor = instruction;
    } catch (error) {
      toasts.error(toAppError(error));
    } finally {
      working = false;
    }
  }

  function clear(): void {
    toolOptions.set(tool, 'parts', []);
  }

  function edit(index: number, patch: Partial<PartRow>): void {
    toolOptions.set(
      tool,
      'parts',
      rows.map((row, i) => (i === index ? { ...row, ...patch } : row)),
    );
  }

  function remove(index: number): void {
    toolOptions.set(
      tool,
      'parts',
      rows.filter((_, i) => i !== index),
    );
  }

  const number = (text: string): number => (text.trim() === '' ? Number.NaN : Number.parseInt(text, 10));
</script>

<section class="plan" aria-labelledby="smart-split-title">
  <h3 id="smart-split-title">{t('smartSplit.title')}</h3>
  <p class="hint">{t('smartSplit.help')}</p>
  <button type="button" class="action" disabled={!usable || working} onclick={() => void preview()}>
    {working ? t('smartSplit.working') : t('smartSplit.preview')}
  </button>
  {#if !usable}
    <p class="hint">{t('smartSplit.needs')}</p>
  {/if}

  {#if rows.length > 0}
    {#if stale}<p class="warn" role="status">{t('smartSplit.stale')}</p>{/if}
    <ol class="parts">
      {#each rows as row, index (index)}
        <li>
          <div class="pages">
            <label>
              <span>{t('smartSplit.from')}</span>
              <input
                type="number"
                min="1"
                max={pageCount}
                value={Number.isFinite(row.start) ? row.start : ''}
                oninput={(e) => edit(index, { start: number(e.currentTarget.value) })}
              />
            </label>
            <label>
              <span>{t('smartSplit.to')}</span>
              <input
                type="number"
                min="1"
                max={pageCount}
                value={Number.isFinite(row.end) ? row.end : ''}
                oninput={(e) => edit(index, { end: number(e.currentTarget.value) })}
              />
            </label>
          </div>
          <label class="title">
            <span>{t('smartSplit.name')}</span>
            <input type="text" value={row.title} oninput={(e) => edit(index, { title: e.currentTarget.value })} />
          </label>
          <button type="button" aria-label={t('smartSplit.remove', { n: index + 1 })} onclick={() => remove(index)}>
            {t('option.remove')}
          </button>
        </li>
      {/each}
    </ol>
    {#if problem}
      <p class="error" role="alert">{t(problem as 'smartSplit.problem.range' | 'smartSplit.problem.order')}</p>
    {:else}
      <p class="hint" role="status">{t('smartSplit.count', { count: rows.length })}</p>
    {/if}
    <button type="button" class="action" onclick={clear}>{t('smartSplit.clear')}</button>
  {/if}
</section>

<style>
  .plan {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  h3 {
    margin: 0;
    font-size: var(--text-base);
  }
  .hint {
    margin: 0;
    color: var(--text-muted);
    font-size: var(--text-sm);
  }
  .warn,
  .error {
    margin: 0;
    font-weight: 500;
  }
  .error {
    color: var(--danger);
  }
  .action {
    align-self: flex-start;
    min-height: 32px;
    padding: 0 var(--space-4);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--text);
  }
  .action:disabled {
    color: var(--text-muted);
  }
  .parts {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  li {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-panel);
  }
  .pages {
    display: flex;
    gap: var(--space-4);
  }
  label {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-weight: 500;
    min-width: 0;
  }
  input {
    min-height: 32px;
    padding: 0 var(--space-3);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    min-width: 0;
  }
  input[type='number'] {
    width: 6em;
  }
  li button {
    align-self: flex-start;
    min-height: 32px;
    padding: 0 var(--space-4);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--text);
  }
</style>
