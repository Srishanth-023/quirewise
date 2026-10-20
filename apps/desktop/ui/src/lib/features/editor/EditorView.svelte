<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { t, tDynamic } from '../../i18n';
  import Icon from '../../components/Icon.svelte';
  import { ipc, toAppError } from '../../ipc/client';
  import { toasts } from '../../stores/toasts.svelte';
  import { mergeTerms } from '../ai/ai';
  import type { RedactOptions, ToolDescriptor } from '../../ipc/client';
  import type { FieldSuggestion, FormField, NewField, RedactMatch, TextBlock } from '../../ipc/generated';
  import { files } from '../../stores/files.svelte';
import { toolOptions } from '../../stores/options.svelte';
  import PasswordPrompt from '../password/PasswordPrompt.svelte';
  import { editor } from './editor.svelte';
  import { boxOf, gestureOf, labelOf, withBox, type Entry } from './model';
  import PageCanvas from './PageCanvas.svelte';
  import { palette } from './palette';
  import Properties from './Properties.svelte';
  import SignatureMaker from './SignatureMaker.svelte';

  interface Props {
    tool: ToolDescriptor;
  }

  let { tool }: Props = $props();

  const entry = $derived(files.selected);
  const report = $derived(entry?.report ?? null);
  const locked = $derived(
    entry?.problem?.code === 'QW-PDF-ENCRYPTED' ||
      entry?.problem?.code === 'QW-PDF-WRONG-PASSWORD' ||
      report?.locked === true,
  );
  const usable = $derived(entry?.kind === 'pdf' && report !== null && !locked && !entry.problem);
  const password = $derived(entry ? files.passwordFor(entry.id) : null);
  const pageCount = $derived(report?.page_count ?? 0);

  // Follow the tool and the file: the editor starts over when either changes.
  $effect(() => {
    const current = entry;
    const descriptor = tool;
    untrack(() => editor.sync(descriptor, current));
  });

  /** The displayed size of the page: a turned page swaps its sides. */
  const pageSize = $derived.by(() => {
    const info = report?.pages[editor.page - 1];
    if (!info) return { width: 595, height: 842 };
    return info.rotation === 90 || info.rotation === 270
      ? { width: info.height_pt, height: info.width_pt }
      : { width: info.width_pt, height: info.height_pt };
  });

  let fonts = $state<string[]>([]);
  onMount(() => {
    ipc
      .editorFonts()
      .then((list) => (fonts = list))
      .catch(() => (fonts = []));
  });

  // ----- what the page can offer -------------------------------------------------------------

  let formFields = $state<FormField[]>([]);
  let formProblem = $state('');
  $effect(() => {
    formFields = [];
    formProblem = '';
    if (tool.id !== 'fill-sign' || !usable || !entry) return;
    let current = true;
    ipc
      .formFields(entry.path, password)
      .then((list) => {
        if (current) formFields = list;
      })
      .catch((error: { message?: string }) => {
        if (current) formProblem = error.message ?? '';
      });
    return () => {
      current = false;
    };
  });
  const fieldsHere = $derived(formFields.filter((f) => f.page === editor.page));

  let blocks = $state<TextBlock[]>([]);
  $effect(() => {
    blocks = [];
    if (editor.creator !== 'edit-text' || !usable || !entry) return;
    let current = true;
    ipc
      .textBlocks(entry.path, password, editor.page)
      .then((list) => {
        if (current) blocks = list;
      })
      .catch(() => {});
    return () => {
      current = false;
    };
  });

  let suggestions = $state<FieldSuggestion[]>([]);
  let suggested = $state(false);
  $effect(() => {
    // A new page or file clears what was found.
    void editor.page;
    void entry?.id;
    suggestions = [];
    suggested = false;
  });

  async function findFields(): Promise<void> {
    if (!entry) return;
    try {
      suggestions = await ipc.suggestFields(entry.path, password, editor.page);
    } catch {
      suggestions = [];
    }
    suggested = true;
  }

  const asField = (s: FieldSuggestion): NewField => ({
    page: s.page,
    region: s.region,
    name: s.name,
    kind: { type: s.kind },
  });

  function accept(s: FieldSuggestion): void {
    editor.placeSuggested([asField(s)]);
    suggestions = suggestions.filter((x) => x !== s);
  }

  function acceptAll(): void {
    editor.placeSuggested(suggestions.map(asField));
    suggestions = [];
  }

  // ----- what Redact would remove ------------------------------------------------------------

  let matches = $state<RedactMatch[]>([]);
  let matchProblem = $state('');
  let searching = $state(false);
  const matchesHere = $derived(matches.filter((m) => m.page === editor.page));
  $effect(() => {
    matches = [];
    matchProblem = '';
    searching = false;
    if (tool.id !== 'redact' || !usable || !entry) return;
    // Any edit of an option counts: the values are one object that is changed in place.
    void toolOptions.edits;
    const values = toolOptions.values(tool) as Record<string, unknown>;
    const lines = (key: string): string[] =>
      Array.isArray(values[key]) ? (values[key] as string[]).filter((l) => l.trim() !== '') : [];
    const terms = lines('terms');
    const patterns = lines('patterns');
    const presets = (Array.isArray(values.presets) ? values.presets : []) as NonNullable<
      RedactOptions['presets']
    >;
    const options: RedactOptions = {
      areas: [],
      terms,
      patterns,
      presets,
      case_sensitive: values.case_sensitive === true,
      pages: typeof values.pages === 'string' && values.pages.trim() !== '' ? values.pages : 'all',
      fill_color: palette.black,
      strip_metadata: true,
    };
    if (terms.length + patterns.length + presets.length === 0) return;
    const path = entry.path;
    const secret = password;
    let current = true;
    searching = true;
    const timer = setTimeout(() => {
      ipc
        .redactMatches(path, secret, options)
        .then((found) => {
          if (current) matches = found;
        })
        .catch((error: { message?: string }) => {
          if (current) matchProblem = error.message ?? '';
        })
        .finally(() => {
          if (current) searching = false;
        });
    }, 400);
    return () => {
      current = false;
      clearTimeout(timer);
    };
  });

  // ----- words the language model finds -------------------------------------------------------

  let asking = $state(false);
  const wantsModel = $derived.by(() => {
    void toolOptions.edits;
    return tool.id === 'redact' && (toolOptions.values(tool) as Record<string, unknown>).ai_suggestions === true;
  });

  /** Asks the model for personal details and adds them to the words, where the person can read and change them. */
  async function askModel(): Promise<void> {
    if (!entry || !usable) return;
    const values = toolOptions.values(tool) as Record<string, unknown>;
    const given = Array.isArray(values.terms) ? (values.terms as string[]) : [];
    const options = {
      areas: [],
      terms: given,
      patterns: [],
      presets: [],
      case_sensitive: false,
      pages: typeof values.pages === 'string' && values.pages.trim() !== '' ? values.pages : 'all',
      fill_color: palette.black,
      strip_metadata: true,
      ai_suggestions: true,
      ai: values.ai as RedactOptions['ai'],
    } satisfies RedactOptions;
    asking = true;
    try {
      const found = await ipc.redactSuggestions(entry.path, password, options);
      const merged = mergeTerms(given, found);
      toolOptions.set(tool, 'terms', merged);
      // What the model found is now in the words, so a run does not ask it again.
      toolOptions.set(tool, 'ai_suggestions', false);
      toasts.info(t('editor.ai.added', { count: merged.length - given.filter((g) => g.trim() !== '').length }));
    } catch (error) {
      toasts.error(toAppError(error));
    } finally {
      asking = false;
    }
  }

  // ----- toolbar and keys --------------------------------------------------------------------

  let canvas: { addAtCenter: () => void } | undefined = $state();

  const needsSignature = $derived(
    editor.creator === 'signature' || editor.creator === 'initials' || editor.creator === 'stamp',
  );
  const showMaker = $derived(tool.id === 'sign-pdf' || tool.id === 'fill-sign');
  const gesture = $derived(gestureOf(editor.creator));
  const canAddAtCenter = $derived(
    editor.creator !== 'select' &&
      editor.creator !== 'edit-text' &&
      editor.creator !== 'image' &&
      gesture !== 'stroke' &&
      editor.ready(editor.creator),
  );

  function nudge(item: Entry, dx: number, dy: number): void {
    const box = boxOf(item);
    if (!box) return;
    const x = Math.min(Math.max(box.x + dx, 0), 1 - box.width);
    const y = Math.min(Math.max(box.y + dy, 0), 1 - box.height);
    editor.update(item.id, () => withBox(item, { ...box, x, y }).data);
  }

  function onkey(event: KeyboardEvent): void {
    if (!usable) return;
    // Keys typed into a control belong to the control.
    if ((event.target as HTMLElement | null)?.closest('input, select, textarea, button')) return;
    const mod = event.ctrlKey || event.metaKey;
    const key = event.key.toLowerCase();
    if (mod && key === 'z') {
      event.preventDefault();
      if (event.shiftKey) editor.redo();
      else editor.undo();
    } else if (mod && key === 'y') {
      event.preventDefault();
      editor.redo();
    } else if (event.key === 'Escape') {
      editor.choose('select');
    } else if (editor.selected) {
      const step = event.shiftKey ? 0.05 : 0.005;
      if (event.key === 'Delete' || event.key === 'Backspace') {
        event.preventDefault();
        editor.remove(editor.selected.id);
      } else if (event.key === 'ArrowLeft') {
        event.preventDefault();
        nudge(editor.selected, -step, 0);
      } else if (event.key === 'ArrowRight') {
        event.preventDefault();
        nudge(editor.selected, step, 0);
      } else if (event.key === 'ArrowUp') {
        event.preventDefault();
        nudge(editor.selected, 0, -step);
      } else if (event.key === 'ArrowDown') {
        event.preventDefault();
        nudge(editor.selected, 0, step);
      }
    }
  }

  function goTo(page: number): void {
    editor.page = Math.min(Math.max(page, 1), Math.max(pageCount, 1));
  }
</script>

<svelte:window onkeydown={onkey} />

<section class="editor" aria-labelledby="editor-title">
  <h2 id="editor-title">{entry ? entry.name : t('editor.title')}</h2>

  {#if !entry}
    <p class="empty">{t('viewer.empty')}</p>
  {:else if entry.kind !== 'pdf'}
    <p class="empty">{entry.kind === 'image' ? t('viewer.imageFile') : t('viewer.otherFile')}</p>
  {:else if locked}
    <PasswordPrompt {entry} />
  {:else if entry.problem}
    <p class="error">{entry.problem.message} <span class="mono">({entry.problem.code})</span></p>
  {:else if report}
    <p class="help">{t(`editor.help.${tool.id}` as 'editor.help.edit-pdf')}</p>

    <div class="bar" role="toolbar" aria-label={t('editor.toolbar')}>
      <div class="group" role="group" aria-label={t('editor.creators')}>
        {#each editor.creators as creator (creator)}
          <button type="button" aria-pressed={editor.creator === creator} onclick={() => editor.choose(creator)}>
            {tDynamic(`editor.creator.${creator}`, creator)}
          </button>
        {/each}
      </div>
      <div class="group">
        <button type="button" disabled={!editor.canUndo} onclick={() => editor.undo()}>
          <Icon name="arrow-u-up-left" /> {t('organize.undo')}
        </button>
        <button type="button" disabled={!editor.canRedo} onclick={() => editor.redo()}>
          <Icon name="arrow-u-up-right" /> {t('organize.redo')}
        </button>
        <button type="button" disabled={editor.entries.length === 0} onclick={() => editor.clear()}>
          {t('editor.clear')}
        </button>
      </div>
      <div class="group pages">
        <button type="button" disabled={editor.page <= 1} aria-label={t('editor.previousPage')} onclick={() => goTo(editor.page - 1)}>
          <Icon name="arrow-left" />
        </button>
        <label>
          <span class="visually-hidden">{t('editor.page')}</span>
          <input type="number" min="1" max={pageCount} value={editor.page} onchange={(e) => goTo(Number(e.currentTarget.value))} />
        </label>
        <span class="mono" aria-hidden="true">/ {pageCount}</span>
        <button type="button" disabled={editor.page >= pageCount} aria-label={t('editor.nextPage')} onclick={() => goTo(editor.page + 1)}>
          <Icon name="arrow-right" />
        </button>
      </div>
      {#if canAddAtCenter}
        <button type="button" onclick={() => canvas?.addAtCenter()}>{t('editor.addAtCenter')}</button>
      {/if}
      {#if tool.id === 'create-forms'}
        <button type="button" onclick={findFields}>{t('editor.findFields')}</button>
      {/if}
    </div>

    {#if needsSignature && !editor.pending}
      <p class="hint" role="status">{t('editor.chooseSignature')}</p>
    {/if}
    {#if formProblem}<p class="error" role="alert">{formProblem}</p>{/if}
    {#if tool.id === 'fill-sign' && formFields.length === 0 && !formProblem}
      <p class="hint">{t('editor.noFormFields')}</p>
    {/if}
    {#if wantsModel}
      <div class="found" role="status">
        <span>{t('editor.ai.help')}</span>
        <button type="button" disabled={asking || !usable} onclick={() => void askModel()}>
          {asking ? t('editor.ai.asking') : t('editor.ai.ask')}
        </button>
      </div>
    {/if}
    {#if tool.id === 'redact'}
      <p class="hint" role="status">
        {#if matchProblem}
          <span class="error">{matchProblem}</span>
        {:else if searching}
          {t('editor.matches.searching')}
        {:else if matches.length > 0}
          {t('editor.matches.found', { count: matches.length })}
        {:else}
          {t('editor.matches.none')}
        {/if}
      </p>
    {/if}
    {#if suggested}
      <div class="found" role="status">
        <span>{suggestions.length === 0 ? t('editor.noneFound') : t('editor.found', { count: suggestions.length })}</span>
        {#if suggestions.length > 0}
          <button type="button" onclick={acceptAll}>{t('editor.addAll')}</button>
        {/if}
      </div>
    {/if}

    <div class="layout">
      <div class="stage">
        <PageCanvas
          bind:this={canvas}
          path={entry.path}
          {password}
          {pageSize}
          formFields={fieldsHere}
          {blocks}
          suggestions={suggestions.filter((s) => s.page === editor.page)}
          onsuggestion={accept}
          matches={matchesHere}
        />
      </div>

      <aside class="side" aria-label={t('editor.sidebar')}>
        {#if showMaker}
          <SignatureMaker {fonts} />
        {/if}

        {#if editor.selected}
          {#key editor.selected.id}
            <Properties entry={editor.selected} {fonts} {pageCount} />
          {/key}
        {/if}

        <section aria-labelledby="placed-title">
          <h3 id="placed-title">{t('editor.placed', { count: editor.entries.length })}</h3>
          {#if editor.entries.length === 0 && Object.keys(editor.values).length === 0}
            <p class="hint">{t('editor.nothingPlaced')}</p>
          {:else}
            <ul class="placed">
              {#each editor.entries as item (item.id)}
                <li>
                  <button
                    type="button"
                    aria-pressed={item.id === editor.selectedId}
                    onclick={() => {
                      editor.choose('select');
                      editor.select(item.id);
                      const page = (item.data as { page: number }).page;
                      if (page) goTo(page);
                    }}
                  >
                    <span class="mono">{(item.data as { page: number }).page}</span>
                    {labelOf(item)}
                  </button>
                </li>
              {/each}
            </ul>
            {#if Object.keys(editor.values).length > 0}
              <p class="hint">{t('editor.filled', { count: Object.keys(editor.values).length })}</p>
            {/if}
          {/if}
        </section>
      </aside>
    </div>
  {/if}
</section>

<style>
  .editor {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    min-width: 0;
  }
  h2,
  h3 {
    margin: 0;
  }
  h2 {
    overflow-wrap: anywhere;
  }
  h3 {
    font-size: var(--text-base);
  }
  .help,
  .hint,
  .empty {
    margin: 0;
    color: var(--text-muted);
  }
  .error {
    margin: 0;
    color: var(--danger);
  }
  .bar,
  .group {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
  }
  .bar button {
    min-height: 32px;
    padding: 0 var(--space-4);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--text);
  }
  .bar button[aria-pressed='true'] {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--on-accent);
  }
  .bar button:disabled {
    opacity: 0.5;
  }
  .pages input {
    width: 4.5rem;
    min-height: 32px;
    padding: 0 var(--space-3);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
  }
  .found {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
  }
  .layout {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(240px, 320px);
    gap: var(--space-5);
    align-items: start;
  }
  .side {
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
    min-width: 0;
  }
  .placed {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .placed button {
    width: 100%;
    display: flex;
    gap: var(--space-3);
    min-height: 32px;
    align-items: center;
    padding: 0 var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--text);
    text-align: left;
    overflow: hidden;
  }
  .placed button[aria-pressed='true'] {
    border-color: var(--accent);
  }
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }
  @media (max-width: 900px) {
    .layout {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
