import type { UnlistenFn } from '@tauri-apps/api/event';
import { t } from '../../i18n';
import { ipc, onBatchEvent, toAppError, type AppError, type BatchEvent, type Recipe } from '../../ipc/client';
import type { FileStatus, JobStatus, OutputArtifact } from '../../ipc/generated';
import { ToolOptionsStore } from '../../stores/options.svelte';
import { toasts } from '../../stores/toasts.svelte';
import { tools } from '../../stores/tools.svelte';
import { addable, baseName, chainProblem, startsWithSource, type Roles } from './model';

/** One step while the recipe is being edited. Each step keeps its own option values. */
export interface DraftStep {
  key: number;
  toolId: string;
  store: ToolOptionsStore;
}

export interface Draft {
  /** Empty until the recipe has been saved once. */
  id: string;
  name: string;
  steps: DraftStep[];
}

/** One file of a recipe run. */
export interface RunRow {
  path: string;
  name: string;
  status: FileStatus;
  error?: AppError;
  outputs: OutputArtifact[];
  progress?: { done: number; total: number };
}

/** Saved recipes, the recipe being edited, and the run of one recipe over files. */
class Recipes {
  list = $state<Recipe[]>([]);
  roles = $state<Roles>({});
  loaded = $state(false);
  draft = $state<Draft | null>(null);

  /** The recipe chosen to run, by id. */
  chosen = $state<string | null>(null);
  paths = $state<string[]>([]);
  folder = $state<string | null>(null);
  recursive = $state(false);
  outputDir = $state<string | null>(null);

  rows = $state<RunRow[]>([]);
  running = $state(false);
  status = $state<JobStatus | null>(null);
  job: number | null = null;

  #nextKey = 1;
  #unlisten: UnlistenFn | undefined;

  async load(): Promise<void> {
    try {
      const [list, roles] = await Promise.all([ipc.recipesList(), ipc.recipeRoles()]);
      this.list = list;
      this.roles = Object.fromEntries(roles.map((r) => [r.tool, r.role]));
    } catch (error) {
      toasts.error(toAppError(error));
    } finally {
      this.loaded = true;
    }
  }

  get chosenRecipe(): Recipe | undefined {
    return this.list.find((r) => r.id === this.chosen);
  }

  /** The tools that fit after the steps of the draft. */
  get choices() {
    const ids = this.draft?.steps.map((s) => s.toolId) ?? [];
    return addable(tools.items, ids, this.roles);
  }

  /** Why the draft cannot be saved yet, or null when the chain is fine. */
  get problem() {
    if (!this.draft) return null;
    return chainProblem(
      this.draft.steps.map((s) => s.toolId),
      this.roles,
    );
  }

  // Editing

  startNew(): void {
    this.draft = { id: '', name: '', steps: [] };
  }

  edit(recipe: Recipe): void {
    const steps: DraftStep[] = [];
    for (const step of recipe.steps) {
      const tool = tools.items.find((t) => t.id === step.options.tool);
      if (!tool) continue;
      const store = new ToolOptionsStore();
      store.load(tool, step.options as Record<string, unknown>);
      steps.push({ key: this.#nextKey++, toolId: tool.id, store });
    }
    this.draft = { id: recipe.id ?? '', name: recipe.name, steps };
  }

  stopEditing(): void {
    this.draft = null;
  }

  addStep(toolId: string): void {
    if (!this.draft || !tools.items.some((t) => t.id === toolId)) return;
    this.draft.steps.push({ key: this.#nextKey++, toolId, store: new ToolOptionsStore() });
  }

  removeStep(index: number): void {
    this.draft?.steps.splice(index, 1);
  }

  moveStep(index: number, by: number): void {
    const steps = this.draft?.steps;
    const target = index + by;
    if (!steps || target < 0 || target >= steps.length) return;
    const [moved] = steps.splice(index, 1);
    if (moved) steps.splice(target, 0, moved);
  }

  /** The recipe the draft describes, with every step's options in the form the core reads. */
  #toRecipe(draft: Draft): Recipe {
    return {
      id: draft.id,
      name: draft.name,
      steps: draft.steps.flatMap((step) => {
        const tool = tools.items.find((t) => t.id === step.toolId);
        return tool ? [{ options: step.store.request(tool, []) as Recipe['steps'][number]['options'] }] : [];
      }),
    };
  }

  async save(): Promise<boolean> {
    if (!this.draft) return false;
    try {
      const saved = await ipc.recipeSave(this.#toRecipe(this.draft));
      await this.load();
      this.chosen = saved.id ?? null;
      this.draft = null;
      return true;
    } catch (error) {
      toasts.error(toAppError(error));
      return false;
    }
  }

  async remove(id: string): Promise<void> {
    try {
      await ipc.recipeDelete(id);
      if (this.chosen === id) this.chosen = null;
      await this.load();
    } catch (error) {
      toasts.error(toAppError(error));
    }
  }

  async rename(id: string, name: string): Promise<boolean> {
    try {
      await ipc.recipeRename(id, name);
      await this.load();
      return true;
    } catch (error) {
      toasts.error(toAppError(error));
      return false;
    }
  }

  async exportRecipe(id: string): Promise<boolean> {
    try {
      return await ipc.recipeExport(id);
    } catch (error) {
      toasts.error(toAppError(error));
      return false;
    }
  }

  async importRecipe(): Promise<Recipe | null> {
    try {
      const imported = await ipc.recipeImport();
      if (imported) {
        await this.load();
        this.chosen = imported.id ?? null;
      }
      return imported;
    } catch (error) {
      toasts.error(toAppError(error));
      return null;
    }
  }

  // Running

  async addFiles(): Promise<void> {
    try {
      const picked = await ipc.pickFiles();
      this.paths = [...new Set([...this.paths, ...picked])];
    } catch (error) {
      toasts.error(toAppError(error));
    }
  }

  async chooseFolder(): Promise<void> {
    try {
      this.folder = (await ipc.pickFolder()) ?? this.folder;
    } catch (error) {
      toasts.error(toAppError(error));
    }
  }

  async chooseOutput(): Promise<void> {
    try {
      this.outputDir = (await ipc.pickFolder()) ?? this.outputDir;
    } catch (error) {
      toasts.error(toAppError(error));
    }
  }

  removePath(path: string): void {
    this.paths = this.paths.filter((p) => p !== path);
  }

  /** The files a run would start from: the chosen files, then the folder's, without repeats. */
  async resolveInputs(recipe: Recipe): Promise<string[]> {
    const withOffice = startsWithSource(
      recipe.steps.map((s) => s.options.tool),
      this.roles,
    );
    const fromFolder = this.folder ? await ipc.folderFiles(this.folder, this.recursive, withOffice) : [];
    return [...new Set([...this.paths, ...fromFolder])];
  }

  async listen(): Promise<void> {
    this.#unlisten ??= await onBatchEvent((event) => this.#apply(event));
  }

  stop(): void {
    this.#unlisten?.();
    this.#unlisten = undefined;
  }

  async run(recipe: Recipe): Promise<void> {
    if (this.running) return;
    try {
      const inputs = await this.resolveInputs(recipe);
      if (inputs.length === 0) {
        toasts.error({ code: 'QW-OPTION-INVALID', message: t('recipes.nothingToRun') });
        return;
      }
      this.rows = inputs.map((path) => ({ path, name: baseName(path), status: 'pending', outputs: [] }));
      this.status = 'running';
      this.running = true;
      this.job = await ipc.runRecipe({
        recipe,
        inputs: inputs.map((path) => ({ path, password: null })),
        folder: null,
        output_dir: this.outputDir,
        naming_pattern: null,
      });
    } catch (error) {
      this.running = false;
      this.status = null;
      toasts.error(toAppError(error));
    }
  }

  async cancel(): Promise<void> {
    if (this.job !== null && this.running) await ipc.cancelJob(this.job);
  }

  async retryFailed(): Promise<void> {
    if (this.job === null || this.running) return;
    try {
      await ipc.retryFailed(this.job);
      this.running = true;
      this.status = 'running';
    } catch (error) {
      toasts.error(toAppError(error));
    }
  }

  get hasFailures(): boolean {
    return this.rows.some((r) => ['wrong-password', 'damaged', 'out-of-range', 'failed'].includes(r.status));
  }

  #apply(event: BatchEvent): void {
    if (this.job === null || event.job !== this.job) return;
    if (event.type === 'job-finished') {
      this.running = false;
      this.status = event.status;
      return;
    }
    const row = this.rows[event.unit];
    if (!row) return;
    if (event.type === 'progress') {
      row.status = 'running';
      row.progress = { done: event.done, total: event.total };
    } else {
      row.status = event.status;
      row.error = event.error ?? undefined;
      row.outputs = event.outputs;
      row.progress = undefined;
    }
  }
}

export const recipes = new Recipes();
