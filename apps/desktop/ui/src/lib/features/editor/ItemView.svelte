<script lang="ts">
  import type { Annotation, EditItem, NewField, SignatureItem } from '../../ipc/generated';
  import { labelOf, type Entry } from './model';
  import { palette } from './palette';

  interface Props {
    entry: Entry;
    /** Width of the page in points, to size text and lines like the result will be. */
    pageWidthPt: number;
  }

  let { entry, pageWidthPt }: Props = $props();

  const item = $derived(entry.group === 'items' ? (entry.data as EditItem) : null);
  const annotation = $derived(entry.group === 'annotations' ? (entry.data as Annotation) : null);
  const signature = $derived(entry.group === 'signatures' ? (entry.data as SignatureItem) : null);
  const field = $derived(entry.group === 'fields' ? (entry.data as NewField) : null);
  const area = $derived(entry.group === 'areas');

  /** A length in points as a share of the page width, for the container-relative unit. */
  const cq = (pt: number): string => `${(pt / Math.max(pageWidthPt, 1)) * 100}cqw`;

  const today = new Date().toISOString().slice(0, 10);
</script>

{#if item?.kind === 'text'}
  <div
    class="text"
    style:font-size={cq(item.style?.size_pt ?? 12)}
    style:color={item.style?.color ?? palette.black}
    style:font-weight={item.style?.bold ? 700 : 400}
    style:font-style={item.style?.italic ? 'italic' : 'normal'}
    style:text-align={item.align ?? 'left'}
    style:font-family={item.style?.installed_font ??
      (item.style?.font === 'times' ? 'Times New Roman, serif' : item.style?.font === 'courier' ? 'Courier New, monospace' : 'Arial, sans-serif')}
  >{item.text}</div>
{:else if item?.kind === 'image'}
  <div class="placeholder image">{labelOf(entry)}</div>
{:else if item?.kind === 'shape'}
  <svg viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true">
    {#if item.shape === 'rectangle'}
      <rect x="0" y="0" width="100" height="100" fill={item.fill_color ?? 'none'} stroke={item.stroke_color} style:stroke-width={cq(item.stroke_width_pt ?? 2)} vector-effect="non-scaling-stroke" />
    {:else if item.shape === 'ellipse'}
      <ellipse cx="50" cy="50" rx="50" ry="50" fill={item.fill_color ?? 'none'} stroke={item.stroke_color} style:stroke-width={cq(item.stroke_width_pt ?? 2)} vector-effect="non-scaling-stroke" />
    {:else}
      <line x1="0" y1={item.flip ? 100 : 0} x2="100" y2={item.flip ? 0 : 100} stroke={item.stroke_color} style:stroke-width={cq(item.stroke_width_pt ?? 2)} vector-effect="non-scaling-stroke" />
      {#if item.shape === 'arrow'}
        <circle cx="100" cy={item.flip ? 0 : 100} r="3" fill={item.stroke_color} />
      {/if}
    {/if}
  </svg>
{:else if item?.kind === 'link'}
  <div class="link" title={labelOf(entry)}></div>
{:else if item?.kind === 'whiteout'}
  <div class="patch" style:background={item.color ?? palette.white}></div>
{:else if annotation?.kind === 'highlight'}
  <div class="highlight" style:background={annotation.color ?? palette.highlight}></div>
{:else if annotation?.kind === 'underline'}
  <div class="underline" style:border-color={annotation.color ?? palette.mark}></div>
{:else if annotation?.kind === 'strikeout'}
  <svg viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true">
    <line x1="0" y1="50" x2="100" y2="50" stroke={annotation.color ?? palette.mark} stroke-width="2" vector-effect="non-scaling-stroke" />
  </svg>
{:else if annotation?.kind === 'note'}
  <div class="note" style:background={annotation.color ?? palette.note} title={annotation.text}></div>
{:else if annotation?.kind === 'freehand'}
  <!-- Drawn by the canvas over the whole page, so the strokes keep their place. -->
{:else if signature}
  {#if signature.source.type === 'typed'}
    <div
      class="typed"
      style:font-size={cq(signature.source.style?.size_pt ?? 24)}
      style:font-style={signature.source.style?.italic ? 'italic' : 'normal'}
      style:color={signature.source.style?.color ?? palette.ink}
    >{signature.source.text}</div>
  {:else if signature.source.type === 'drawn'}
    <svg viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true">
      {#each signature.source.strokes as stroke, i (i)}
        <polyline
          points={stroke.map((p) => `${p.x * 100},${p.y * 100}`).join(' ')}
          fill="none"
          stroke={signature.source.color ?? palette.ink}
          style:stroke-width={cq(signature.source.width_pt ?? 1.5)}
          stroke-linecap="round"
          stroke-linejoin="round"
          vector-effect="non-scaling-stroke"
        />
      {/each}
    </svg>
  {:else if signature.source.type === 'date'}
    <div class="typed" style:font-size={cq(signature.source.style?.size_pt ?? 12)}>{today}</div>
  {:else}
    <div class="placeholder image">{labelOf(entry)}</div>
  {/if}
{:else if area}
  <div class="area"></div>
{:else if field}
  <div class="field" data-kind={field.kind.type}>
    <span>{field.name}</span>
  </div>
{/if}

<style>
  .text,
  .typed {
    width: 100%;
    height: 100%;
    overflow: hidden;
    white-space: pre-wrap;
    line-height: 1.2;
    pointer-events: none;
  }
  .typed {
    display: grid;
    place-items: center;
    white-space: nowrap;
  }
  svg {
    display: block;
    width: 100%;
    height: 100%;
    overflow: visible;
    pointer-events: none;
  }
  .placeholder {
    width: 100%;
    height: 100%;
    display: grid;
    place-items: center;
    border: 1px dashed var(--control-border);
    background: color-mix(in srgb, var(--surface) 70%, transparent);
    color: var(--text-muted);
    font-size: var(--text-sm);
    overflow: hidden;
  }
  .link {
    width: 100%;
    height: 100%;
    border: 1px dashed var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }
  .patch,
  .highlight {
    width: 100%;
    height: 100%;
  }
  .highlight {
    opacity: 0.45;
    mix-blend-mode: multiply;
  }
  .underline {
    width: 100%;
    height: 100%;
    border-bottom: 2px solid;
  }
  .note {
    width: 100%;
    height: 100%;
    min-width: 14px;
    min-height: 14px;
    border: 1px solid var(--ink);
    border-radius: 2px;
  }
  .area {
    width: 100%;
    height: 100%;
    border: 1px dashed var(--on-accent);
    background: color-mix(in srgb, var(--ink) 80%, transparent);
  }
  .field {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    padding: 0 4px;
    border: 1px solid var(--field-edge);
    background: var(--field-fill);
    color: var(--ink-muted);
    font-size: 11px;
    overflow: hidden;
    pointer-events: none;
  }
  .field[data-kind='checkbox'],
  .field[data-kind='radio'] {
    justify-content: center;
    padding: 0;
  }
  .field[data-kind='radio'] {
    border-radius: 4px;
  }
  .field[data-kind='checkbox'] span,
  .field[data-kind='radio'] span {
    display: none;
  }
  .field[data-kind='signature'] {
    border-style: dashed;
  }
</style>
