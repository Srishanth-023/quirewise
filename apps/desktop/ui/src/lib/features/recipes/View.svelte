<script lang="ts">
  import { onMount } from 'svelte';
  import { t, tDynamic } from '../../i18n';
  import Button from '../../components/Button/Button.svelte';
  import Icon from '../../components/Icon.svelte';
  import ProgressBar from '../../components/ProgressBar/ProgressBar.svelte';
  import SchemaForm from '../../forms/SchemaForm.svelte';
  import { tools, toolName } from '../../stores/tools.svelte';
  import { ipc, toAppError } from '../../ipc/client';
  import { toasts } from '../../stores/toasts.svelte';
  import { recipes } from './recipes.svelte';
  import { baseName } from './model';

  let toAdd = $state('');
  let renamingId = $state<string | null>(null);
  let renameText = $state('');

  const draft = $derived(recipes.draft);
  const choices = $derived(recipes.choices);
  const problem = $derived(recipes.problem);
  const problemText = $derived(
    problem
      ? t(`recipes.problem.${problem.reason}` as 'recipes.problem.excluded', { step: problem.step })
      : '',
  );
  const canSave = $derived(!!draft && draft.name.trim() !== '' && draft.steps.length > 0 && !problem);
  const chosen = $derived(recipes.chosenRecipe);
  const summary = (steps: { options: { tool: string } }[]): string =>
    steps.map((s) => tDynamic(`tool.${s.options.tool}`, s.options.tool)).join(' > ');
  const resultFiles = $derived(recipes.rows.flatMap((r) => r.outputs));
  const announcement = $derived(
    recipes.running || recipes.rows.length === 0 || !recipes.status || recipes.status === 'running'
      ? ''
      : t('queue.finished', {
          done: recipes.rows.filter((r) => r.status === 'done').length,
          failed: recipes.rows.filter((r) => r.status !== 'done').length,
        }),
  );

  function addStep(): void {
    if (toAdd === '') return;
    recipes.addStep(toAdd);
    toAdd = '';
  }

  async function openOutput(action: (path: string) => Promise<void>, path: string): Promise<void> {
    try {
      await action(path);
    } catch (error) {
      toasts.error(toAppError(error));
    }
  }

  async function commitRename(): Promise<void> {
    if (renamingId === null) return;
    if (await recipes.rename(renamingId, renameText)) renamingId = null;
  }

  onMount(() => {
    void recipes.load();
    void recipes.listen();
    return () => recipes.stop();
  });
</script>

<section class="recipes" aria-labelledby="recipes-title">
  <h1 id="recipes-title">{t('recipes.title')}</h1>
  <p class="lead">{t('recipes.lead')}</p>
  <p class="sr" role="status">{announcement}</p>

  {#if draft}
    <form class="builder" aria-label={t('recipes.builder')} onsubmit={(e) => e.preventDefault()}>
      <div class="field">
        <label for="recipe-name">{t('recipes.name')}</label>
        <input id="recipe-name" type="text" maxlength="80" bind:value={draft.name} />
      </div>

      {#if draft.steps.length === 0}
        <p class="muted">{t('recipes.noSteps')}</p>
      {/if}
      <ol class="steps">
        {#each draft.steps as step, index (step.key)}
          {@const tool = tools.items.find((x) => x.id === step.toolId)}
          {#if tool}
            <li>
              <details>
                <summary>{t('recipes.step', { n: index + 1, tool: toolName(tool) })}</summary>
                <SchemaForm {tool} store={step.store} />
              </details>
              <span class="move">
                <button
                  type="button"
                  aria-label={t('recipes.moveUp', { n: index + 1 })}
                  disabled={index === 0}
                  onclick={() => recipes.moveStep(index, -1)}><Icon name="arrow-up" /></button>
                <button
                  type="button"
                  aria-label={t('recipes.moveDown', { n: index + 1 })}
                  disabled={index === draft.steps.length - 1}
                  onclick={() => recipes.moveStep(index, 1)}><Icon name="arrow-down" /></button>
                <button
                  type="button"
                  aria-label={t('recipes.removeStep', { n: index + 1 })}
                  onclick={() => recipes.removeStep(index)}><Icon name="x" /></button>
              </span>
            </li>
          {/if}
        {/each}
      </ol>

      {#if problem}
        <p class="error" role="alert">{problemText}</p>
      {/if}

      <div class="add">
        <label for="recipe-add">{t('recipes.addStep')}</label>
        <select id="recipe-add" bind:value={toAdd} disabled={choices.length === 0}>
          <option value="">{choices.length === 0 ? t('recipes.nothingFits') : t('recipes.pickTool')}</option>
          {#each choices as tool (tool.id)}
            <option value={tool.id}>{toolName(tool)}</option>
          {/each}
        </select>
        <Button onclick={addStep} disabled={toAdd === ''}>{t('recipes.add')}</Button>
      </div>

      <div class="actions">
        <Button variant="primary" disabled={!canSave} onclick={() => void recipes.save()}>{t('recipes.save')}</Button>
        <Button onclick={() => recipes.stopEditing()}>{t('recipes.cancelEdit')}</Button>
      </div>
    </form>
  {:else}
    <div class="actions">
      <Button variant="primary" onclick={() => recipes.startNew()}><Icon name="plus" /> {t('recipes.new')}</Button>
      <Button onclick={() => void recipes.importRecipe()}>{t('recipes.import')}</Button>
    </div>

    {#if recipes.loaded && recipes.list.length === 0}
      <p class="muted">{t('recipes.empty')}</p>
    {/if}

    <ul class="list" aria-label={t('recipes.saved')}>
      {#each recipes.list as recipe (recipe.id)}
        <li class:chosen={recipes.chosen === recipe.id}>
          <div class="what">
            {#if renamingId === recipe.id}
              <label class="sr" for="recipe-rename">{t('recipes.name')}</label>
              <input
                id="recipe-rename"
                type="text"
                maxlength="80"
                bind:value={renameText}
                onkeydown={(e) => {
                  if (e.key === 'Enter') void commitRename();
                  else if (e.key === 'Escape') renamingId = null;
                }}
              />
            {:else}
              <strong>{recipe.name}</strong>
            {/if}
            <span class="muted">{summary(recipe.steps)}</span>
          </div>
          <div class="row-actions">
            {#if renamingId === recipe.id}
              <Button onclick={() => void commitRename()}>{t('recipes.rename')}</Button>
            {:else}
              <Button variant="primary" onclick={() => (recipes.chosen = recipe.id ?? null)}>{t('recipes.use')}</Button>
              <Button onclick={() => recipes.edit(recipe)}>{t('recipes.edit')}</Button>
              <Button onclick={() => {
                renamingId = recipe.id ?? '';
                renameText = recipe.name;
              }}>{t('recipes.rename')}</Button>
              <Button onclick={() => void recipes.exportRecipe(recipe.id ?? '')}>{t('recipes.export')}</Button>
              <Button onclick={() => void recipes.remove(recipe.id ?? '')}>{t('recipes.delete')}</Button>
            {/if}
          </div>
        </li>
      {/each}
    </ul>

    {#if chosen}
      <section class="run" aria-labelledby="run-title">
        <h2 id="run-title">{t('recipes.runTitle', { name: chosen.name })}</h2>

        <div class="actions">
          <Button onclick={() => void recipes.addFiles()} disabled={recipes.running}>{t('recipes.addFiles')}</Button>
          <Button onclick={() => void recipes.chooseFolder()} disabled={recipes.running}>{t('recipes.chooseFolder')}</Button>
          <Button onclick={() => void recipes.chooseOutput()} disabled={recipes.running}>{t('recipes.chooseOutput')}</Button>
        </div>

        {#if recipes.paths.length > 0}
          <ul class="paths" aria-label={t('recipes.files')}>
            {#each recipes.paths as path (path)}
              <li>
                <span class="mono">{baseName(path)}</span>
                <button
                  type="button"
                  aria-label={t('files.remove', { name: baseName(path) })}
                  disabled={recipes.running}
                  onclick={() => recipes.removePath(path)}><Icon name="x" /></button>
              </li>
            {/each}
          </ul>
        {/if}
        {#if recipes.folder}
          <p class="muted">{t('recipes.folderIs', { path: recipes.folder })}</p>
          <label class="check">
            <input type="checkbox" bind:checked={recipes.recursive} disabled={recipes.running} />
            {t('recipes.recursive')}
          </label>
        {/if}
        <p class="muted">
          {recipes.outputDir ? t('recipes.outputIs', { path: recipes.outputDir }) : t('recipes.outputNextToSource')}
        </p>

        <div class="actions">
          {#if recipes.running}
            <Button onclick={() => void recipes.cancel()}><Icon name="stop" /> {t('queue.cancel')}</Button>
          {:else}
            <Button
              variant="primary"
              disabled={recipes.paths.length === 0 && !recipes.folder}
              onclick={() => void recipes.run(chosen)}
            ><Icon name="play" /> {t('recipes.run', { name: chosen.name })}</Button>
            {#if recipes.hasFailures}
              <Button onclick={() => void recipes.retryFailed()}><Icon name="arrow-clockwise" /> {t('queue.retry')}</Button>
            {/if}
          {/if}
        </div>

        {#if recipes.rows.length > 0}
          <ul class="results" aria-label={t('recipes.results')}>
            {#each recipes.rows as row (row.path)}
              <li>
                <span class="mono">{row.name}</span>
                <span class="status" class:bad={['wrong-password', 'damaged', 'out-of-range', 'failed'].includes(row.status)}>
                  {tDynamic(`status.${row.status}`)}
                </span>
                {#if row.progress}
                  <div class="progress">
                    <ProgressBar current={row.progress.done} total={row.progress.total} label={t('queue.runningLabel', { name: row.name })} />
                  </div>
                {/if}
                {#if row.error}
                  <p class="error">{row.error.message} <span class="mono">({row.error.code})</span></p>
                {/if}
              </li>
            {/each}
          </ul>
        {/if}

        {#if !recipes.running && resultFiles.length > 0}
          <ul class="saved" aria-label={t('result.title')}>
            {#each resultFiles as result (result.path)}
              <li>
                <span>{t('result.saved', { name: baseName(result.path) })}</span>
                <Button onclick={() => void openOutput(ipc.openOutput, result.path)}>{t('result.open')}</Button>
                <Button onclick={() => void openOutput(ipc.revealOutput, result.path)}>{t('result.reveal')}</Button>
              </li>
            {/each}
          </ul>
        {/if}
      </section>
    {/if}
  {/if}
</section>

<style>
  .recipes {
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
    max-width: 90ch;
  }
  h1,
  h2 {
    margin: 0;
  }
  .lead,
  .muted {
    margin: 0;
    color: var(--text-muted);
  }
  .actions,
  .row-actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }
  .builder,
  .run {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: var(--space-5);
    border: 1px solid var(--border);
    border-radius: var(--radius-panel);
    background: var(--surface);
  }
  .field,
  .add {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  label {
    font-weight: 500;
  }
  .check {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    font-weight: 400;
  }
  input[type='text'],
  select {
    min-height: 32px;
    padding: 0 var(--space-3);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    min-width: 0;
  }
  .add {
    align-items: flex-start;
  }
  .steps,
  .list,
  .paths,
  .results,
  .saved {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .steps {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .steps > li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-panel);
    background: var(--bg);
  }
  summary {
    cursor: pointer;
    font-weight: 600;
    min-height: var(--target-min);
    display: flex;
    align-items: center;
  }
  details[open] > summary {
    margin-bottom: var(--space-4);
  }
  .move {
    display: flex;
    gap: var(--space-2);
    align-items: flex-start;
  }
  .move button,
  .paths button {
    min-width: var(--target-min);
    min-height: 32px;
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--text);
  }
  .move button:disabled {
    color: var(--text-muted);
  }
  .list {
    border: 1px solid var(--border);
    border-radius: var(--radius-panel);
    background: var(--surface);
  }
  .list > li {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--border);
  }
  .list > li:last-child {
    border-bottom: 0;
  }
  .list > li.chosen {
    background: var(--selection);
  }
  .what {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .paths > li,
  .saved > li {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: 32px;
  }
  .results > li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: var(--space-2) var(--space-3);
    padding: var(--space-2) 0;
    border-bottom: 1px solid var(--border);
  }
  .progress,
  .results .error {
    grid-column: 1 / -1;
  }
  .error {
    margin: 0;
    color: var(--danger);
  }
  .status.bad {
    color: var(--danger);
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }
</style>
