<script lang="ts">
  import { t } from '../../i18n';
  import type { Annotation, EditItem, NewField, SignatureItem } from '../../ipc/generated';
  import { editor } from './editor.svelte';
  import { palette } from './palette';
  import { boxOf, withBox, withPercent, type Entry, type Region } from './model';

  interface Props {
    entry: Entry;
    /** Installed font families the person can pick. */
    fonts: string[];
    pageCount: number;
  }

  let { entry, fonts, pageCount }: Props = $props();

  const box = $derived(boxOf(entry));
  const data = $derived(entry.data as unknown as Record<string, unknown>);

  function patch(change: Record<string, unknown>): void {
    editor.update(entry.id, (d) => ({ ...(d as object), ...change }) as Entry['data']);
  }

  function patchNested(key: string, change: Record<string, unknown>): void {
    const current = (data[key] as Record<string, unknown> | undefined) ?? {};
    patch({ [key]: { ...current, ...change } });
  }

  function side(key: keyof Region, text: string): void {
    if (!box) return;
    const next = withPercent(box, key, Number.parseFloat(text));
    editor.update(entry.id, () => withBox(entry, next).data);
  }

  const sides: { key: keyof Region; label: 'region.left' | 'region.top' | 'region.width' | 'region.height' }[] = [
    { key: 'x', label: 'region.left' },
    { key: 'y', label: 'region.top' },
    { key: 'width', label: 'region.width' },
    { key: 'height', label: 'region.height' },
  ];

  const percent = (n: number): number => Math.round(n * 1000) / 10;
  const num = (text: string, fallback: number): number => {
    const n = Number.parseFloat(text);
    return Number.isFinite(n) ? n : fallback;
  };

  const item = $derived(entry.group === 'items' ? (entry.data as EditItem) : null);
  const annotation = $derived(entry.group === 'annotations' ? (entry.data as Annotation) : null);
  const signature = $derived(entry.group === 'signatures' ? (entry.data as SignatureItem) : null);
  const field = $derived(entry.group === 'fields' ? (entry.data as NewField) : null);

  /** The font choice of a text style: a standard family or an installed one. */
  function fontValue(style: { font?: string; installed_font?: string | null } | undefined): string {
    return style?.installed_font ? `installed:${style.installed_font}` : (style?.font ?? 'helvetica');
  }

  function chooseFont(key: string, value: string): void {
    if (value.startsWith('installed:')) patchNested(key, { installed_font: value.slice(10) });
    else patchNested(key, { font: value, installed_font: null });
  }
</script>

{#snippet fontSelect(key: string, style: { font?: string; installed_font?: string | null } | undefined)}
  <label>
    <span>{t('editor.font')}</span>
    <select value={fontValue(style)} onchange={(e) => chooseFont(key, e.currentTarget.value)}>
      <option value="helvetica">Helvetica</option>
      <option value="times">Times</option>
      <option value="courier">Courier</option>
      {#if fonts.length > 0}
        <optgroup label={t('editor.installedFonts')}>
          {#each fonts as family (family)}
            <option value={`installed:${family}`}>{family}</option>
          {/each}
        </optgroup>
      {/if}
    </select>
  </label>
{/snippet}

<section class="props" aria-labelledby="props-title">
  <h3 id="props-title">{t('editor.properties')}</h3>

  {#if item?.kind === 'text'}
    <label>
      <span>{t('editor.text')}</span>
      <textarea rows="3" value={item.text} onchange={(e) => patch({ text: e.currentTarget.value })}></textarea>
    </label>
    {@render fontSelect('style', item.style)}
    <label>
      <span>{t('editor.size')}</span>
      <input type="number" min="4" max="400" step="1" value={item.style?.size_pt ?? 12} onchange={(e) => patchNested('style', { size_pt: num(e.currentTarget.value, 12) })} />
    </label>
    <label>
      <span>{t('editor.color')}</span>
      <input type="color" value={item.style?.color ?? palette.black} onchange={(e) => patchNested('style', { color: e.currentTarget.value })} />
    </label>
    <label class="check">
      <input type="checkbox" checked={item.style?.bold ?? false} onchange={(e) => patchNested('style', { bold: e.currentTarget.checked })} />
      <span>{t('editor.bold')}</span>
    </label>
    <label class="check">
      <input type="checkbox" checked={item.style?.italic ?? false} onchange={(e) => patchNested('style', { italic: e.currentTarget.checked })} />
      <span>{t('editor.italic')}</span>
    </label>
    <label>
      <span>{t('editor.align')}</span>
      <select value={item.align ?? 'left'} onchange={(e) => patch({ align: e.currentTarget.value })}>
        <option value="left">{t('editor.alignLeft')}</option>
        <option value="center">{t('editor.alignCenter')}</option>
        <option value="right">{t('editor.alignRight')}</option>
      </select>
    </label>
  {:else if item?.kind === 'shape'}
    <label>
      <span>{t('editor.color')}</span>
      <input type="color" value={item.stroke_color ?? palette.mark} onchange={(e) => patch({ stroke_color: e.currentTarget.value })} />
    </label>
    <label>
      <span>{t('editor.lineWidth')}</span>
      <input type="number" min="0.5" max="40" step="0.5" value={item.stroke_width_pt ?? 2} onchange={(e) => patch({ stroke_width_pt: num(e.currentTarget.value, 2) })} />
    </label>
    {#if item.shape === 'rectangle' || item.shape === 'ellipse'}
      <label class="check">
        <input type="checkbox" checked={!!item.fill_color} onchange={(e) => patch({ fill_color: e.currentTarget.checked ? palette.highlight : null })} />
        <span>{t('editor.fill')}</span>
      </label>
      {#if item.fill_color}
        <label>
          <span>{t('editor.fillColor')}</span>
          <input type="color" value={item.fill_color} onchange={(e) => patch({ fill_color: e.currentTarget.value })} />
        </label>
      {/if}
    {:else}
      <label class="check">
        <input type="checkbox" checked={item.flip ?? false} onchange={(e) => patch({ flip: e.currentTarget.checked })} />
        <span>{t('editor.flip')}</span>
      </label>
    {/if}
  {:else if item?.kind === 'link'}
    <label>
      <span>{t('editor.linkTo')}</span>
      <select
        value={item.target.type}
        onchange={(e) => patch({ target: e.currentTarget.value === 'uri' ? { type: 'uri', uri: 'https://example.com' } : { type: 'page', page: 1 } })}
      >
        <option value="uri">{t('editor.linkWeb')}</option>
        <option value="page">{t('editor.linkPage')}</option>
      </select>
    </label>
    {#if item.target.type === 'uri'}
      <label>
        <span>{t('editor.address')}</span>
        <input type="url" value={item.target.uri} onchange={(e) => patch({ target: { type: 'uri', uri: e.currentTarget.value } })} />
      </label>
    {:else}
      <label>
        <span>{t('editor.targetPage')}</span>
        <input type="number" min="1" max={pageCount} step="1" value={item.target.page} onchange={(e) => patch({ target: { type: 'page', page: Math.min(Math.max(Math.round(num(e.currentTarget.value, 1)), 1), pageCount) } })} />
      </label>
    {/if}
  {:else if item?.kind === 'whiteout'}
    <label>
      <span>{t('editor.color')}</span>
      <input type="color" value={item.color ?? palette.white} onchange={(e) => patch({ color: e.currentTarget.value })} />
    </label>
    <p class="hint">{t('editor.whiteoutHint')}</p>
  {:else if annotation}
    <label>
      <span>{t('editor.color')}</span>
      <input type="color" value={annotation.color ?? palette.highlight} onchange={(e) => patch({ color: e.currentTarget.value })} />
    </label>
    {#if annotation.kind === 'note'}
      <label>
        <span>{t('editor.noteText')}</span>
        <textarea rows="3" value={annotation.text} onchange={(e) => patch({ text: e.currentTarget.value })}></textarea>
      </label>
    {:else if annotation.kind === 'freehand'}
      <label>
        <span>{t('editor.lineWidth')}</span>
        <input type="number" min="0.5" max="40" step="0.5" value={annotation.width_pt ?? 2} onchange={(e) => patch({ width_pt: num(e.currentTarget.value, 2) })} />
      </label>
    {/if}
  {:else if signature}
    {#if signature.source.type === 'typed'}
      {@const src = signature.source}
      <label>
        <span>{t('editor.text')}</span>
        <input type="text" value={src.text} onchange={(e) => patchNested('source', { text: e.currentTarget.value })} />
      </label>
      <label>
        <span>{t('editor.size')}</span>
        <input type="number" min="4" max="400" step="1" value={src.style?.size_pt ?? 24} onchange={(e) => patchNested('source', { style: { ...src.style, size_pt: num(e.currentTarget.value, 24) } })} />
      </label>
    {:else if signature.source.type === 'date'}
      <label>
        <span>{t('editor.dateFormat')}</span>
        <select value={signature.source.format ?? 'iso'} onchange={(e) => patchNested('source', { format: e.currentTarget.value })}>
          <option value="iso">2026-10-19</option>
          <option value="long">19 October 2026</option>
          <option value="us">10/19/2026</option>
          <option value="eu">19/10/2026</option>
        </select>
      </label>
    {/if}
  {:else if field}
    <label>
      <span>{t('editor.fieldName')}</span>
      <input type="text" value={field.name} onchange={(e) => patch({ name: e.currentTarget.value })} />
    </label>
    <label>
      <span>{t('editor.tooltip')}</span>
      <input type="text" value={field.tooltip ?? ''} onchange={(e) => patch({ tooltip: e.currentTarget.value })} />
    </label>
    <label class="check">
      <input type="checkbox" checked={field.required ?? false} onchange={(e) => patch({ required: e.currentTarget.checked })} />
      <span>{t('editor.required')}</span>
    </label>
    {#if field.kind.type === 'text'}
      <label class="check">
        <input type="checkbox" checked={field.kind.multiline ?? false} onchange={(e) => patch({ kind: { ...field.kind, multiline: e.currentTarget.checked } })} />
        <span>{t('editor.multiline')}</span>
      </label>
    {:else if field.kind.type === 'radio'}
      <label>
        <span>{t('editor.choiceName')}</span>
        <input type="text" value={field.kind.option} onchange={(e) => patch({ kind: { ...field.kind, option: e.currentTarget.value } })} />
      </label>
    {:else if field.kind.type === 'dropdown'}
      <label>
        <span>{t('editor.choices')}</span>
        <textarea
          rows="4"
          value={field.kind.options.join('\n')}
          onchange={(e) => patch({ kind: { ...field.kind, options: e.currentTarget.value.split('\n').map((o) => o.trim()).filter(Boolean), value: '' } })}
        ></textarea>
      </label>
    {/if}
  {/if}

  {#if box}
    <fieldset class="numbers">
      <legend>{t('editor.position')}</legend>
      {#each sides as s (s.key)}
        <label>
          <span>{t(s.label)}</span>
          <input type="number" min="0" max="100" step="0.5" value={percent(box[s.key])} onchange={(e) => side(s.key, e.currentTarget.value)} />
        </label>
      {/each}
    </fieldset>
  {/if}

  <button type="button" class="danger" onclick={() => editor.remove(entry.id)}>{t('editor.delete')}</button>
</section>

<style>
  .props {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
  }
  h3 {
    margin: 0;
    font-size: var(--text-base);
  }
  label {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
  }
  label.check {
    flex-direction: row;
    align-items: center;
  }
  input[type='text'],
  input[type='url'],
  input[type='number'],
  select,
  textarea {
    min-height: 32px;
    padding: 0 var(--space-3);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    min-width: 0;
    font: inherit;
  }
  textarea {
    padding: var(--space-2) var(--space-3);
    resize: vertical;
  }
  input[type='color'] {
    width: 48px;
    height: 32px;
    padding: 2px;
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: var(--bg);
  }
  .numbers {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-3);
    margin: 0;
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-panel);
  }
  .numbers legend {
    padding: 0 var(--space-2);
    font-weight: 500;
  }
  .hint {
    margin: 0;
    color: var(--text-muted);
  }
  .danger {
    align-self: flex-start;
    min-height: 32px;
    padding: 0 var(--space-4);
    border: 1px solid var(--danger);
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--danger);
  }
</style>
