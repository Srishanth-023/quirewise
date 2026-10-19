<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '../../i18n';
  import { ipc } from '../../ipc/client';
  import type { SavedSignature, SignatureSource } from '../../ipc/generated';
  import { editor } from './editor.svelte';
  import { palette } from './palette';
  import { normalizeStrokes, simplify, type Pt } from './model';

  interface Props {
    fonts: string[];
  }

  let { fonts }: Props = $props();

  type Tab = 'draw' | 'type' | 'picture' | 'saved';
  let tab = $state<Tab>('draw');

  /** The pad is three times as wide as tall. */
  const PAD_ASPECT = 3;
  let pad: HTMLElement | undefined = $state();
  let strokes = $state<Pt[][]>([]);
  let current = $state<Pt[] | null>(null);

  let text = $state('');
  let fontChoice = $state('times');
  let italic = $state(true);

  let keep = $state(false);
  let keepName = $state('');
  let kind = $state<'signature' | 'initials' | 'stamp'>('signature');
  let saved = $state<SavedSignature[]>([]);
  let problem = $state('');

  async function reload(): Promise<void> {
    try {
      saved = await ipc.signaturesList();
    } catch {
      saved = [];
    }
  }

  onMount(() => {
    void reload();
  });

  function at(event: PointerEvent): Pt {
    const box = pad?.getBoundingClientRect();
    if (!box || box.width <= 0 || box.height <= 0) return { x: 0, y: 0 };
    return {
      x: Math.min(1, Math.max(0, (event.clientX - box.left) / box.width)),
      y: Math.min(1, Math.max(0, (event.clientY - box.top) / box.height)),
    };
  }

  function down(event: PointerEvent): void {
    if (event.button !== 0) return;
    (event.currentTarget as HTMLElement).setPointerCapture?.(event.pointerId);
    current = [at(event)];
  }

  function move(event: PointerEvent): void {
    if (current) current = [...current, at(event)];
  }

  function up(): void {
    if (current && current.length > 0) strokes = [...strokes, simplify(current)];
    current = null;
  }

  const style = (): Record<string, unknown> =>
    fontChoice.startsWith('installed:')
      ? { installed_font: fontChoice.slice(10), size_pt: 24, italic }
      : { font: fontChoice, size_pt: 24, italic };

  async function remember(source: SignatureSource, aspect: number): Promise<void> {
    editor.pending = { source, aspect };
    editor.choose(kind === 'initials' ? 'initials' : kind === 'stamp' ? 'stamp' : 'signature');
    problem = '';
    if (keep && keepName.trim()) {
      try {
        await ipc.signatureSave({ name: keepName.trim(), kind, aspect, source });
        keep = false;
        keepName = '';
        await reload();
      } catch (error) {
        problem = (error as { message?: string }).message ?? '';
      }
    }
  }

  async function useDrawn(): Promise<void> {
    const fitted = normalizeStrokes(strokes, PAD_ASPECT);
    if (!fitted) return;
    await remember({ type: 'drawn', strokes: fitted.strokes, color: palette.ink, width_pt: 1.5 }, fitted.aspect);
  }

  async function useTyped(): Promise<void> {
    const value = text.trim();
    if (!value) return;
    const aspect = Math.max((value.length * 0.5) / 1.25, 0.8);
    await remember({ type: 'typed', text: value, style: style() as never }, aspect);
  }

  async function usePicture(): Promise<void> {
    const picked = await ipc.pickPaths('image', false);
    if (!picked[0]) return;
    keep = false;
    kind = 'stamp';
    await remember({ type: 'image', path: picked[0] }, 2.5);
  }

  function useSaved(s: SavedSignature): void {
    kind = s.kind;
    editor.pending = { source: s.source, aspect: s.aspect };
    editor.choose(s.kind === 'initials' ? 'initials' : s.kind === 'stamp' ? 'stamp' : 'signature');
  }

  async function forget(s: SavedSignature): Promise<void> {
    await ipc.signatureDelete(s.id);
    await reload();
  }

  const tabs: { id: Tab; label: 'editor.sign.draw' | 'editor.sign.type' | 'editor.sign.picture' | 'editor.sign.saved' }[] = [
    { id: 'draw', label: 'editor.sign.draw' },
    { id: 'type', label: 'editor.sign.type' },
    { id: 'picture', label: 'editor.sign.picture' },
    { id: 'saved', label: 'editor.sign.saved' },
  ];
</script>

<section class="maker" aria-labelledby="maker-title">
  <h3 id="maker-title">{t('editor.sign.title')}</h3>

  <div class="tabs" role="group" aria-label={t('editor.sign.how')}>
    {#each tabs as tabItem (tabItem.id)}
      <button type="button" aria-pressed={tab === tabItem.id} onclick={() => (tab = tabItem.id)}>{t(tabItem.label)}</button>
    {/each}
  </div>

  {#if tab !== 'saved' && tab !== 'picture'}
    <label>
      <span>{t('editor.sign.kind')}</span>
      <select bind:value={kind}>
        <option value="signature">{t('editor.creator.signature')}</option>
        <option value="initials">{t('editor.creator.initials')}</option>
      </select>
    </label>
  {/if}

  {#if tab === 'draw'}
    <!-- Drawing is a pointer convenience; typing a name works from the keyboard. -->
    <div
      class="pad"
      bind:this={pad}
      role="presentation"
      onpointerdown={down}
      onpointermove={move}
      onpointerup={up}
      onpointercancel={up}
    >
      <svg viewBox="0 0 300 100" preserveAspectRatio="none" aria-hidden="true">
        {#each [...strokes, ...(current ? [current] : [])] as s, i (i)}
          <polyline points={s.map((p) => `${p.x * 300},${p.y * 100}`).join(' ')} fill="none" stroke={palette.ink} stroke-width="2" stroke-linecap="round" stroke-linejoin="round" vector-effect="non-scaling-stroke" />
        {/each}
      </svg>
      {#if strokes.length === 0 && !current}<span class="empty">{t('editor.sign.drawHere')}</span>{/if}
    </div>
    <div class="row">
      <button type="button" onclick={() => (strokes = [])} disabled={strokes.length === 0}>{t('editor.sign.clear')}</button>
      <button type="button" class="primary" onclick={useDrawn} disabled={strokes.length === 0}>{t('editor.sign.use')}</button>
    </div>
  {:else if tab === 'type'}
    <label>
      <span>{t('editor.sign.name')}</span>
      <input type="text" bind:value={text} />
    </label>
    <label>
      <span>{t('editor.font')}</span>
      <select bind:value={fontChoice}>
        <option value="times">Times</option>
        <option value="helvetica">Helvetica</option>
        <option value="courier">Courier</option>
        {#each fonts as family (family)}
          <option value={`installed:${family}`}>{family}</option>
        {/each}
      </select>
    </label>
    <label class="check">
      <input type="checkbox" bind:checked={italic} />
      <span>{t('editor.italic')}</span>
    </label>
    <p class="preview" style:font-style={italic ? 'italic' : 'normal'} style:font-family={fontChoice.startsWith('installed:') ? fontChoice.slice(10) : fontChoice === 'times' ? 'Times New Roman, serif' : fontChoice === 'courier' ? 'Courier New, monospace' : 'Arial, sans-serif'}>{text || ' '}</p>
    <div class="row">
      <button type="button" class="primary" onclick={useTyped} disabled={text.trim() === ''}>{t('editor.sign.use')}</button>
    </div>
  {:else if tab === 'picture'}
    <p class="hint">{t('editor.sign.pictureHint')}</p>
    <div class="row">
      <button type="button" class="primary" onclick={usePicture}>{t('editor.sign.choosePicture')}</button>
    </div>
  {:else}
    {#if saved.length === 0}
      <p class="hint">{t('editor.sign.noneSaved')}</p>
    {:else}
      <ul class="saved">
        {#each saved as s (s.id)}
          <li>
            <span>{s.name}</span>
            <button type="button" onclick={() => useSaved(s)}>{t('editor.sign.useSaved', { name: s.name })}</button>
            <button type="button" class="danger" onclick={() => forget(s)}>{t('editor.sign.forget', { name: s.name })}</button>
          </li>
        {/each}
      </ul>
    {/if}
  {/if}

  {#if tab === 'draw' || tab === 'type'}
    <label class="check">
      <input type="checkbox" bind:checked={keep} />
      <span>{t('editor.sign.keep')}</span>
    </label>
    {#if keep}
      <label>
        <span>{t('editor.sign.keepName')}</span>
        <input type="text" bind:value={keepName} />
      </label>
      <p class="hint">{t('editor.sign.keepHint')}</p>
    {/if}
  {/if}

  {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
  <p class="status" role="status">{editor.pending ? t('editor.sign.ready') : t('editor.sign.none')}</p>
</section>

<style>
  .maker {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
  }
  h3 {
    margin: 0;
    font-size: var(--text-base);
  }
  .tabs,
  .row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }
  button {
    min-height: 32px;
    padding: 0 var(--space-4);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--text);
  }
  button[aria-pressed='true'],
  .primary {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--on-accent);
  }
  button:disabled {
    opacity: 0.5;
  }
  .danger {
    border-color: var(--danger);
    color: var(--danger);
  }
  label {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  label.check {
    flex-direction: row;
    align-items: center;
  }
  input[type='text'],
  select {
    min-height: 32px;
    padding: 0 var(--space-3);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    font: inherit;
  }
  .pad {
    position: relative;
    width: 100%;
    max-width: 420px;
    aspect-ratio: 3 / 1;
    border: 1px solid var(--control-border);
    background: var(--paper);
    cursor: crosshair;
    touch-action: none;
  }
  .pad svg {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
  .empty {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    color: var(--ink-muted);
    pointer-events: none;
  }
  .preview {
    margin: 0;
    padding: var(--space-3);
    border: 1px solid var(--border);
    background: var(--paper);
    color: var(--ink);
    font-size: 28px;
    min-height: 48px;
  }
  .saved {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .saved li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
  }
  .hint,
  .status {
    margin: 0;
    color: var(--text-muted);
  }
  .problem {
    margin: 0;
    color: var(--danger);
  }
</style>
