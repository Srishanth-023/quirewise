<script lang="ts">
  import { t } from '../../i18n';
  import { thumbnail, type Thumb } from '../../components/Thumbnail/thumbnails';
  import { files } from '../../stores/files.svelte';
  import { toolOptions } from '../../stores/options.svelte';
  import type { ToolDescriptor } from '../../ipc/client';
  import { pointToFraction, regionFromDrag, toPercent, withPercent, type Region } from './region';

  interface Props {
    tool: ToolDescriptor;
  }

  let { tool }: Props = $props();

  const entry = $derived(files.selected);
  const pageCount = $derived(entry?.report?.page_count ?? 0);
  const region = $derived((toolOptions.values(tool).region as Region | null) ?? null);

  let pageNumber = $state(1);
  let thumb = $state<Thumb | null>(null);
  let failed = $state(false);
  let surface: HTMLElement | undefined = $state();
  let dragFrom: { x: number; y: number } | null = $state(null);
  let dragTo: { x: number; y: number } | null = $state(null);

  const shown = $derived(
    dragFrom && dragTo ? regionFromDrag(dragFrom, dragTo) : region,
  );

  $effect(() => {
    const file = entry;
    const page = Math.min(Math.max(pageNumber, 1), Math.max(pageCount, 1)) - 1;
    thumb = null;
    failed = false;
    if (!file || !file.report || file.report.locked) return;
    let current = true;
    thumbnail(file.path, files.passwordFor(file.id), page, 320)
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

  function fraction(event: PointerEvent): { x: number; y: number } {
    const box = surface?.getBoundingClientRect();
    return pointToFraction(
      { x: event.clientX, y: event.clientY },
      box ?? { left: 0, top: 0, width: 0, height: 0 },
    );
  }

  function down(event: PointerEvent): void {
    if (event.button !== 0) return;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    dragFrom = fraction(event);
    dragTo = dragFrom;
  }

  function move(event: PointerEvent): void {
    if (dragFrom) dragTo = fraction(event);
  }

  function up(event: PointerEvent): void {
    if (!dragFrom) return;
    const drawn = regionFromDrag(dragFrom, fraction(event));
    if (drawn) toolOptions.set(tool, 'region', drawn);
    dragFrom = null;
    dragTo = null;
  }

  function type(side: keyof Region, text: string): void {
    const next = withPercent(region, side, Number.parseFloat(text));
    if (next) toolOptions.set(tool, 'region', next);
  }

  const sides: { key: keyof Region; label: string }[] = [
    { key: 'x', label: 'region.left' },
    { key: 'y', label: 'region.top' },
    { key: 'width', label: 'region.width' },
    { key: 'height', label: 'region.height' },
  ];
</script>

<section class="picker" aria-labelledby={`${tool.id}-region-title`}>
  <h3 id={`${tool.id}-region-title`}>{t('region.title')}</h3>
  <p class="help">{t('region.help')}</p>

  {#if !entry || pageCount === 0}
    <p class="muted">{t('region.needFile')}</p>
  {:else}
    <div class="row">
      <label for={`${tool.id}-region-page`}>{t('region.page')}</label>
      <input
        id={`${tool.id}-region-page`}
        type="number"
        min="1"
        max={pageCount}
        step="1"
        value={pageNumber}
        oninput={(e) => {
          const n = Number.parseInt(e.currentTarget.value, 10);
          if (Number.isFinite(n)) pageNumber = Math.min(Math.max(n, 1), pageCount);
        }}
      />
      <span class="muted mono">/ {pageCount}</span>
    </div>

    <!-- Drawing is a pointer convenience. The four number fields below do the same by keyboard. -->
    <div
      class="surface"
      bind:this={surface}
      role="presentation"
      onpointerdown={down}
      onpointermove={move}
      onpointerup={up}
      onpointercancel={() => {
        dragFrom = null;
        dragTo = null;
      }}
    >
      {#if thumb}
        <img src={thumb.url} width={thumb.width} height={thumb.height} alt={t('viewer.page', { n: pageNumber })} draggable="false" />
      {:else}
        <div class="blank" role="img" aria-label={t('viewer.page', { n: pageNumber })}>
          {#if failed}{t('viewer.failed')}{/if}
        </div>
      {/if}
      {#if shown}
        <div
          class="box"
          style:left={`${shown.x * 100}%`}
          style:top={`${shown.y * 100}%`}
          style:width={`${shown.width * 100}%`}
          style:height={`${shown.height * 100}%`}
        ></div>
      {/if}
    </div>

    <fieldset class="numbers">
      <legend>{t('region.numbers')}</legend>
      {#each sides as side (side.key)}
        <label>
          <span>{t(side.label as 'region.left')}</span>
          <input
            type="number"
            min="0"
            max="100"
            step="0.5"
            value={region ? toPercent(region[side.key]) : ''}
            onchange={(e) => type(side.key, e.currentTarget.value)}
          />
        </label>
      {/each}
    </fieldset>

    <p class:warn={!region} class="status" role="status">
      {region ? t('region.set') : t('region.required')}
    </p>
    {#if region}
      <button type="button" class="clear" onclick={() => toolOptions.set(tool, 'region', null)}>{t('region.clear')}</button>
    {/if}
  {/if}
</section>

<style>
  .picker {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  h3 {
    margin: 0;
    font-size: var(--text-base);
  }
  .help,
  .muted {
    margin: 0;
    color: var(--text-muted);
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }
  .surface {
    position: relative;
    align-self: flex-start;
    max-width: 100%;
    cursor: crosshair;
    touch-action: none;
    user-select: none;
  }
  img,
  .blank {
    display: block;
    max-width: 100%;
    height: auto;
    border: 1px solid var(--control-border);
    background: var(--surface-2);
  }
  .blank {
    width: 240px;
    aspect-ratio: 612 / 792;
    display: grid;
    place-items: center;
    color: var(--text-muted);
  }
  .box {
    position: absolute;
    border: 2px solid var(--accent);
    background: var(--selection);
    opacity: 0.6;
    pointer-events: none;
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
  .numbers label {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  input[type='number'] {
    min-height: 32px;
    padding: 0 var(--space-3);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    min-width: 0;
  }
  .status {
    margin: 0;
    color: var(--success);
  }
  .status.warn {
    color: var(--warning);
  }
  .clear {
    align-self: flex-start;
    min-height: 32px;
    padding: 0 var(--space-4);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--text);
  }
</style>
