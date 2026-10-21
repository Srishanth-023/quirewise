<script lang="ts">
  import { t } from '../../i18n';
  import Icon from '../../components/Icon.svelte';
  import Thumbnail from '../../components/Thumbnail/Thumbnail.svelte';
  import type { ToolDescriptor } from '../../ipc/client';
  import { files } from '../../stores/files.svelte';
  import { organizer } from './organizer.svelte';

  interface Props {
    tool: ToolDescriptor;
  }

  let { tool }: Props = $props();

  // Follow the work list: the organizer starts over when a file is added or removed.
  $effect(() => {
    organizer.sync(tool, files.entries);
  });

  const otherFiles = $derived(
    files.entries.filter((e) => e.kind === 'pdf' && e.report && !e.report.locked),
  );
  let insertFile = $state<number | null>(null);
  let insertFrom = $state(1);
  let insertTo = $state(1);
  const insertSource = $derived(otherFiles.find((e) => e.id === (insertFile ?? otherFiles[0]?.id)));
  const insertMax = $derived(insertSource?.report?.page_count ?? 1);

  const selected = $derived(new Set(organizer.selection));
  const hasSelection = $derived(organizer.selection.length > 0);

  let dragging = $state<number | null>(null);
  let dragOver = $state<number | null>(null);
  let moved = $state(false);
  let startPoint = { x: 0, y: 0 };

  function onkey(event: KeyboardEvent): void {
    // Keys typed into a field belong to the field, not to the page list.
    if ((event.target as HTMLElement | null)?.closest('input, select, textarea')) return;
    const mod = event.ctrlKey || event.metaKey;
    if (mod && event.key.toLowerCase() === 'z') {
      event.preventDefault();
      if (event.shiftKey) organizer.redo();
      else organizer.undo();
    } else if (mod && event.key.toLowerCase() === 'y') {
      event.preventDefault();
      organizer.redo();
    } else if (mod && event.key.toLowerCase() === 'a') {
      event.preventDefault();
      organizer.selectAll();
    } else if (event.key === 'Delete') {
      event.preventDefault();
      organizer.removeSelected();
    } else if (event.altKey && event.key === 'ArrowLeft') {
      event.preventDefault();
      organizer.nudgeSelected(-1);
    } else if (event.altKey && event.key === 'ArrowRight') {
      event.preventDefault();
      organizer.nudgeSelected(1);
    }
  }

  function click(event: MouseEvent, index: number): void {
    organizer.pick(index, event.shiftKey ? 'range' : event.ctrlKey || event.metaKey ? 'toggle' : 'single');
  }

  // Dragging uses pointer events: the window-level file drop of the desktop shell takes over the
  // browser's own drag and drop, which would never reach these cards.
  function down(event: PointerEvent, index: number): void {
    if (event.button !== 0) return;
    dragging = index;
    moved = false;
    startPoint = { x: event.clientX, y: event.clientY };
  }

  function move(event: PointerEvent): void {
    if (dragging === null) return;
    if (!moved && Math.hypot(event.clientX - startPoint.x, event.clientY - startPoint.y) < 6) return;
    moved = true;
    const card = document.elementFromPoint(event.clientX, event.clientY)?.closest<HTMLElement>('[data-index]');
    dragOver = card ? Number(card.dataset.index) : null;
  }

  function up(): void {
    if (dragging !== null && moved && dragOver !== null) {
      if (!organizer.selection.includes(dragging)) organizer.pick(dragging, 'single');
      organizer.moveSelectedBefore(dragOver);
    }
    dragging = null;
    dragOver = null;
  }

  function addFromFile(): void {
    const id = insertSource?.id;
    if (id !== undefined) organizer.insertFromFile(id, insertFrom, insertTo);
  }
</script>

<svelte:window onkeydown={onkey} onpointermove={move} onpointerup={up} onpointercancel={up} />

<section class="organizer" aria-labelledby="organizer-title">
  <h2 id="organizer-title">{t('organize.title')}</h2>
  <p class="help">{t('organize.help')}</p>

  <div class="bar" role="toolbar" aria-label={t('organize.toolbar')}>
    <button type="button" disabled={!organizer.canUndo} onclick={() => organizer.undo()}>
      <Icon name="arrow-u-up-left" /> {t('organize.undo')}
    </button>
    <button type="button" disabled={!organizer.canRedo} onclick={() => organizer.redo()}>
      <Icon name="arrow-u-up-right" /> {t('organize.redo')}
    </button>
    <button type="button" disabled={!hasSelection} onclick={() => organizer.rotateSelected(-90)}>
      <Icon name="arrow-counter-clockwise" /> {t('organize.rotateLeft')}
    </button>
    <button type="button" disabled={!hasSelection} onclick={() => organizer.rotateSelected(90)}>
      <Icon name="arrow-clockwise" /> {t('organize.rotateRight')}
    </button>
    <button type="button" disabled={!hasSelection} onclick={() => organizer.nudgeSelected(-1)}>
      <Icon name="arrow-left" /> {t('organize.earlier')}
    </button>
    <button type="button" disabled={!hasSelection} onclick={() => organizer.nudgeSelected(1)}>
      <Icon name="arrow-right" /> {t('organize.later')}
    </button>
    <button type="button" disabled={!hasSelection} onclick={() => organizer.duplicateSelected()}>
      <Icon name="copy" /> {t('organize.duplicate')}
    </button>
    <button type="button" disabled={!hasSelection} onclick={() => organizer.removeSelected()}>
      <Icon name="trash" /> {t('organize.delete')}
    </button>
    <button type="button" onclick={() => organizer.insertBlank()}>
      <Icon name="plus" /> {t('organize.blank')}
    </button>
    <button type="button" onclick={() => organizer.reset()}>{t('organize.reset')}</button>
  </div>

  {#if otherFiles.length > 0}
    <form class="insert" onsubmit={(e) => { e.preventDefault(); addFromFile(); }}>
      <label>
        <span>{t('organize.fromFile')}</span>
        <select
          value={insertSource?.id}
          onchange={(e) => {
            insertFile = Number(e.currentTarget.value);
            insertFrom = 1;
            insertTo = otherFiles.find((f) => f.id === insertFile)?.report?.page_count ?? 1;
          }}
        >
          {#each otherFiles as file (file.id)}
            <option value={file.id}>{file.name}</option>
          {/each}
        </select>
      </label>
      <label>
        <span>{t('organize.fromPage')}</span>
        <input type="number" min="1" max={insertMax} bind:value={insertFrom} />
      </label>
      <label>
        <span>{t('organize.toPage')}</span>
        <input type="number" min="1" max={insertMax} bind:value={insertTo} />
      </label>
      <button type="submit"><Icon name="file-plus" /> {t('organize.insertPages')}</button>
    </form>
  {/if}

  <p class="count mono" role="status">
    {t('organize.count', { pages: organizer.pages.length, selected: organizer.selection.length })}
  </p>

  {#if organizer.pages.length === 0}
    <p class="empty">{t('organize.empty')}</p>
  {:else}
    <ul class="grid" aria-label={t('organize.pages')}>
      {#each organizer.pages as page, index (page.key)}
        {@const turn = page.kind === 'page' ? page.rotate : 0}
        <li
          data-index={index}
          class:selected={selected.has(index)}
          class:target={dragOver === index && dragging !== null && moved}
        >
          <button
            type="button"
            class="card"
            aria-pressed={selected.has(index)}
            aria-label={page.kind === 'page'
              ? t('organize.pageLabel', { n: index + 1, name: organizer.sourceName(page), page: page.page + 1, turn })
              : t('organize.blankLabel', { n: index + 1 })}
            onclick={(e) => click(e, index)}
            onpointerdown={(e) => down(e, index)}
          >
            <span class="picture">
              {#if page.kind === 'page'}
                {@const entry = files.entries.find((f) => f.id === page.source)}
                {#if entry}
                  <Thumbnail
                    path={entry.path}
                    password={files.passwordFor(entry.id)}
                    page={page.page}
                    edge={140}
                    {turn}
                    caption={false}
                  />
                {/if}
              {:else}
                <span class="blank">{t('organize.blankPage')}</span>
              {/if}
            </span>
            <span class="label mono">{index + 1}</span>
            {#if page.kind === 'page'}
              <span class="source">{organizer.sourceName(page)} · {page.page + 1}{turn ? ` · ${turn}°` : ''}</span>
            {/if}
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .organizer {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  h2 {
    margin: 0;
  }
  .help,
  .empty {
    margin: 0;
    color: var(--text-muted);
  }
  .bar,
  .insert {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--space-3);
  }
  .insert label {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  button,
  select,
  input {
    min-height: 32px;
    padding: 0 var(--space-4);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: var(--surface);
    color: var(--text);
  }
  .bar button,
  .insert button {
    display: inline-flex;
    align-items: center;
    gap: var(--space-3);
  }
  button:disabled {
    color: var(--text-muted);
    border-color: var(--border);
  }
  input[type='number'] {
    width: 80px;
  }
  .count {
    margin: 0;
    color: var(--text-muted);
  }
  .grid {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: var(--space-4);
    user-select: none;
  }
  li {
    border: 2px solid transparent;
    border-radius: var(--radius-panel);
  }
  li.selected {
    border-color: var(--accent);
    background: var(--selection);
  }
  li.target {
    border-inline-start-color: var(--focus);
  }
  .card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    padding: var(--space-3);
    border: 0;
    background: transparent;
    cursor: grab;
    touch-action: none;
  }
  .picture {
    display: grid;
    place-items: center;
    width: 100%;
    height: 150px;
    overflow: hidden;
  }
  .picture :global(img) {
    max-width: 120px;
    max-height: 120px;
    width: auto;
    height: auto;
  }
  .blank {
    display: grid;
    place-items: center;
    width: 96px;
    height: 124px;
    border: 1px dashed var(--control-border);
    background: var(--bg);
    color: var(--text-muted);
    font-size: var(--text-xs);
  }
  .label {
    font-weight: 500;
  }
  .source {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-muted);
    font-size: var(--text-xs);
  }
</style>
