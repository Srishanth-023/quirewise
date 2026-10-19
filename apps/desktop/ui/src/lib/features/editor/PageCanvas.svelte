<script lang="ts">
  import { t } from '../../i18n';
  import { ipc } from '../../ipc/client';
  import type { Annotation, FieldSuggestion, FormField, TextBlock } from '../../ipc/generated';
  import { thumbnail, type Thumb } from '../../components/Thumbnail/thumbnails';
  import { editor } from './editor.svelte';
  import ItemView from './ItemView.svelte';
  import { palette } from './palette';
  import { boxFromDrag, boxOf, gestureOf, moveBox, resizeBox, type Entry, type Pt } from './model';

  interface Props {
    path: string;
    password: string | null;
    /** Size of the displayed page in points. */
    pageSize: { width: number; height: number };
    /** Fields of the form on this page, for filling them. */
    formFields?: FormField[];
    /** Blocks of text on this page, for changing them. */
    blocks?: TextBlock[];
    /** Places the form finder suggests on this page. */
    suggestions?: FieldSuggestion[];
    onsuggestion?: (s: FieldSuggestion) => void;
  }

  let { path, password, pageSize, formFields = [], blocks = [], suggestions = [], onsuggestion }: Props = $props();

  let thumb = $state<Thumb | null>(null);
  let failed = $state(false);
  let surface: HTMLElement | undefined = $state();

  $effect(() => {
    const page = editor.page - 1;
    thumb = null;
    failed = false;
    let current = true;
    thumbnail(path, password, page, 1100)
      .then((value) => {
        if (current) thumb = value;
      })
      .catch(() => {
        if (current) failed = true;
      });
    return () => {
      current = false;
    };
  });

  const here = $derived(editor.entriesOnPage(editor.page));
  const freehand = $derived(
    here.filter((e) => e.group === 'annotations' && (e.data as Annotation).kind === 'freehand'),
  );
  const gesture = $derived(gestureOf(editor.creator));
  const ready = $derived(editor.ready(editor.creator));

  function fraction(event: PointerEvent): Pt {
    const box = surface?.getBoundingClientRect();
    if (!box || box.width <= 0 || box.height <= 0) return { x: 0, y: 0 };
    return {
      x: Math.min(1, Math.max(0, (event.clientX - box.left) / box.width)),
      y: Math.min(1, Math.max(0, (event.clientY - box.top) / box.height)),
    };
  }

  // ----- making things -----------------------------------------------------------------------

  let from = $state<Pt | null>(null);
  let to = $state<Pt | null>(null);
  let stroke = $state<Pt[]>([]);
  const dragged = $derived(from && to ? boxFromDrag(from, to) : null);

  function down(event: PointerEvent): void {
    if (event.button !== 0 || (event.target as HTMLElement).closest('button, input, select, .handle')) return;
    if (editor.creator === 'select') {
      editor.select(null);
      return;
    }
    if (!ready) return;
    (event.currentTarget as HTMLElement).setPointerCapture?.(event.pointerId);
    const at = fraction(event);
    if (gesture === 'stroke') stroke = [at];
    else {
      from = at;
      to = at;
    }
  }

  function move(event: PointerEvent): void {
    if (gesture === 'stroke' && stroke.length > 0) stroke = [...stroke, fraction(event)];
    else if (from) to = fraction(event);
  }

  async function up(event: PointerEvent): Promise<void> {
    const creator = editor.creator;
    if (gesture === 'stroke') {
      const points = stroke;
      stroke = [];
      if (points.length > 0) editor.placeStroke(points);
      return;
    }
    if (!from) return;
    const start = from;
    const end = fraction(event);
    from = null;
    to = null;
    if (gesture === 'point') {
      editor.placeNote(end);
      return;
    }
    const box = editor.boxFor(creator, start, end);
    if (creator === 'image') {
      const picked = await ipc.pickPaths('image', false);
      if (picked[0]) editor.place('image', box, { path: picked[0] });
      return;
    }
    editor.place(creator, box);
  }

  /** Places something at the middle of the page, for people who do not use a pointer. */
  export function addAtCenter(): void {
    const middle = { x: 0.5, y: 0.5 };
    if (gesture === 'point') editor.placeNote(middle);
    else if (gesture === 'box' && ready) editor.place(editor.creator, editor.boxFor(editor.creator, middle, middle));
  }

  // ----- moving and resizing -----------------------------------------------------------------

  let drag: { id: number; mode: 'move' | 'resize'; start: Pt; box: ReturnType<typeof boxOf> } | null = null;

  function grab(event: PointerEvent, entry: Entry, mode: 'move' | 'resize'): void {
    if (editor.creator !== 'select' || event.button !== 0) return;
    event.stopPropagation();
    editor.select(entry.id);
    const box = boxOf(entry);
    if (!box) return;
    (event.currentTarget as HTMLElement).setPointerCapture?.(event.pointerId);
    editor.commit();
    drag = { id: entry.id, mode, start: fraction(event), box };
  }

  function shift(event: PointerEvent): void {
    if (!drag?.box) return;
    const now = fraction(event);
    const dx = now.x - drag.start.x;
    const dy = now.y - drag.start.y;
    editor.live(drag.id, drag.mode === 'move' ? moveBox(drag.box, dx, dy) : resizeBox(drag.box, dx, dy));
  }

  function drop(): void {
    drag = null;
  }

  const style = (box: { x: number; y: number; width: number; height: number }): string =>
    `left:${box.x * 100}%;top:${box.y * 100}%;width:${box.width * 100}%;height:${box.height * 100}%`;

  function valueOf(field: FormField): string {
    return editor.values[field.name] ?? field.value;
  }
</script>

<div
  class="surface"
  class:drawing={editor.creator !== 'select'}
  bind:this={surface}
  role="presentation"
  style:--aspect={`${pageSize.width} / ${pageSize.height}`}
  onpointerdown={down}
  onpointermove={(e) => {
    move(e);
    shift(e);
  }}
  onpointerup={(e) => {
    void up(e);
    drop();
  }}
  onpointercancel={() => {
    from = null;
    to = null;
    stroke = [];
    drop();
  }}
>
  {#if thumb}
    <img class="page-bg" src={thumb.url} alt={t('viewer.page', { n: editor.page })} draggable="false" />
  {:else}
    <div class="blank page-bg" role="img" aria-label={t('viewer.page', { n: editor.page })}>
      {#if failed}{t('viewer.failed')}{/if}
    </div>
  {/if}

  {#if freehand.length > 0 || stroke.length > 0}
    <svg class="ink" viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true">
      {#each freehand as entry (entry.id)}
        {@const a = entry.data as Extract<Annotation, { kind: 'freehand' }>}
        {#each a.strokes as s, i (i)}
          <polyline
            points={s.map((p) => `${p.x * 100},${p.y * 100}`).join(' ')}
            fill="none"
            stroke={a.color ?? palette.pen}
            style:stroke-width={`${((a.width_pt ?? 2) / pageSize.width) * 100}cqw`}
            stroke-linecap="round"
            stroke-linejoin="round"
            vector-effect="non-scaling-stroke"
          />
        {/each}
      {/each}
      {#if stroke.length > 0}
        <polyline
          points={stroke.map((p) => `${p.x * 100},${p.y * 100}`).join(' ')}
          fill="none"
          stroke={palette.pen}
          style:stroke-width={`${(2 / pageSize.width) * 100}cqw`}
          stroke-linecap="round"
          vector-effect="non-scaling-stroke"
        />
      {/if}
    </svg>
  {/if}

  {#each here as entry (entry.id)}
    {@const box = boxOf(entry)}
    {#if box}
      <!-- Moved with the pointer; the same move is done by the number fields and arrow keys. -->
      <div
        class="entry"
        class:selected={entry.id === editor.selectedId}
        class:movable={editor.creator === 'select'}
        role="presentation"
        style={style(box)}
        data-entry-id={entry.id}
        onpointerdown={(e) => grab(e, entry, 'move')}
      >
        <ItemView {entry} pageWidthPt={pageSize.width} />
        {#if entry.id === editor.selectedId && editor.creator === 'select'}
          <span class="handle" role="presentation" onpointerdown={(e) => grab(e, entry, 'resize')}></span>
        {/if}
      </div>
    {/if}
  {/each}

  {#each formFields as field (field.name + field.options.join('|') + field.region.x + field.region.y)}
    <div class="fill" style={style(field.region)}>
      {#if field.kind === 'text'}
        <input
          type="text"
          aria-label={field.name}
          value={valueOf(field)}
          disabled={field.read_only}
          style:font-size={`${Math.max(field.region.height * pageSize.height * 0.6, 6)}px`}
          onchange={(e) => editor.setValue(field.name, e.currentTarget.value)}
        />
      {:else if field.kind === 'dropdown' || field.kind === 'list-box'}
        <select
          aria-label={field.name}
          value={valueOf(field)}
          disabled={field.read_only}
          onchange={(e) => editor.setValue(field.name, e.currentTarget.value)}
        >
          <option value=""></option>
          {#each field.options as option (option)}
            <option value={option}>{option}</option>
          {/each}
        </select>
      {:else if field.kind === 'checkbox'}
        <input
          type="checkbox"
          aria-label={field.name}
          checked={valueOf(field) === 'true'}
          disabled={field.read_only}
          onchange={(e) => editor.setValue(field.name, String(e.currentTarget.checked))}
        />
      {:else if field.kind === 'radio'}
        <input
          type="radio"
          name={field.name}
          aria-label={`${field.name}: ${field.options[0] ?? ''}`}
          checked={valueOf(field) === field.options[0]}
          disabled={field.read_only}
          onchange={() => editor.setValue(field.name, field.options[0] ?? '')}
        />
      {/if}
    </div>
  {/each}

  {#if editor.creator === 'edit-text'}
    {#each blocks as block, i (i)}
      <button
        type="button"
        class="block"
        style={style(block.region)}
        title={block.text}
        aria-label={t('editor.editBlock', { text: block.text.slice(0, 60) })}
        onclick={() => {
          editor.replaceText(block);
          editor.choose('select');
        }}
      ></button>
    {/each}
  {/if}

  {#each suggestions as s, i (i)}
    <button
      type="button"
      class="suggest"
      style={style(s.region)}
      aria-label={t('editor.addSuggested', { name: s.name })}
      onclick={() => onsuggestion?.(s)}
    ></button>
  {/each}

  {#if dragged}
    <div class="rubber" style={style(dragged)}></div>
  {/if}
</div>

<style>
  .surface {
    position: relative;
    container-type: inline-size;
    width: 100%;
    max-width: 880px;
    aspect-ratio: var(--aspect);
    touch-action: none;
    user-select: none;
    border: 1px solid var(--control-border);
    background: var(--surface-2);
  }
  .surface.drawing {
    cursor: crosshair;
  }
  .page-bg {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: contain;
  }
  .blank {
    display: grid;
    place-items: center;
    color: var(--text-muted);
  }
  .ink {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
  }
  .entry {
    position: absolute;
  }
  .entry.movable {
    cursor: move;
  }
  .entry.selected {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .handle {
    position: absolute;
    right: -6px;
    bottom: -6px;
    width: 12px;
    height: 12px;
    border: 2px solid var(--accent);
    background: var(--bg);
    cursor: nwse-resize;
  }
  .rubber {
    position: absolute;
    border: 2px solid var(--accent);
    background: var(--selection);
    opacity: 0.6;
    pointer-events: none;
  }
  .fill {
    position: absolute;
    display: flex;
  }
  .fill input[type='text'],
  .fill select {
    width: 100%;
    height: 100%;
    min-width: 0;
    padding: 0 3px;
    border: 1px solid var(--field-edge);
    background: var(--field-fill);
    color: var(--ink);
    box-sizing: border-box;
  }
  .fill input[type='checkbox'],
  .fill input[type='radio'] {
    width: 100%;
    height: 100%;
    margin: 0;
  }
  .block,
  .suggest {
    position: absolute;
    padding: 0;
    border: 1px dashed var(--accent);
    background: color-mix(in srgb, var(--accent) 10%, transparent);
    cursor: pointer;
  }
  .block:hover,
  .block:focus-visible {
    background: color-mix(in srgb, var(--accent) 25%, transparent);
  }
  .suggest {
    border-color: var(--success);
    background: color-mix(in srgb, var(--success) 15%, transparent);
  }
</style>
