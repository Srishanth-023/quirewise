import { ipc, toAppError, type AppError, type DocumentReport } from '../ipc/client';
import type { FileStatus, InputKindDto, OutputArtifact, Suggestion } from '../ipc/generated';
import { t } from '../i18n';
import { toasts } from './toasts.svelte';

/** Status shown for one file in the batch queue. */
export type EntryStatus = 'pending' | 'running' | FileStatus;

/** One file in the work list. */
export interface FileEntry {
  id: number;
  path: string;
  name: string;
  kind: InputKindDto;
  report?: DocumentReport;
  /** Why the file cannot be inspected, such as a wrong password. */
  problem?: AppError;
  status: EntryStatus;
  progress?: { done: number; total: number };
  outputs: OutputArtifact[];
  error?: AppError;
}

/**
 * The files the user is working with. Passwords live only in this object's memory and are never
 * persisted or logged.
 */
class FileSet {
  entries = $state<FileEntry[]>([]);
  suggestions = $state<Suggestion[]>([]);
  selectedId = $state<number | null>(null);
  #passwords = new Map<number, string>();
  #next = 1;

  get selected(): FileEntry | undefined {
    return this.entries.find((e) => e.id === this.selectedId) ?? this.entries[0];
  }

  /** Password entered for a file this session, if any. */
  passwordFor(id: number): string | null {
    return this.#passwords.get(id) ?? null;
  }

  /** Classifies and adds paths, then inspects the PDFs. Unusable files are reported, not added. */
  async add(paths: string[]): Promise<void> {
    if (paths.length === 0) return;
    let analysis;
    try {
      // Classify the whole set, so suggestions describe everything in the list, not just this drop.
      analysis = await ipc.classifyInputs([...this.entries.map((e) => e.path), ...paths]);
    } catch (error) {
      toasts.error(toAppError(error));
      return;
    }
    const fresh: FileEntry[] = [];
    for (const file of analysis.files) {
      if (!file.kind || file.error) {
        const reason = file.error?.message ?? '';
        toasts.info(t('files.skipped', { name: file.name, reason }));
        continue;
      }
      if (this.entries.some((e) => e.path === file.path)) continue;
      fresh.push({
        id: this.#next++,
        path: file.path,
        name: file.name,
        kind: file.kind,
        status: 'pending',
        outputs: [],
      });
    }
    this.entries = [...this.entries, ...fresh];
    this.suggestions = analysis.suggestions;
    if (this.selectedId === null && fresh[0]) this.selectedId = fresh[0].id;
    await Promise.all(fresh.map((entry) => this.#afterAdd(entry)));
  }

  async #afterAdd(entry: FileEntry): Promise<void> {
    // Recent files are a convenience; a failure must never block the work.
    void ipc.recentAdd(entry.path).catch(() => undefined);
    if (entry.kind === 'pdf') await this.inspect(entry.id);
  }

  /** Reads the PDF's properties, using the remembered password when there is one. */
  async inspect(id: number): Promise<void> {
    const entry = this.#find(id);
    if (!entry || entry.kind !== 'pdf') return;
    try {
      const report = await ipc.inspectDocument(entry.path, this.passwordFor(id));
      this.#patch(id, { report, problem: undefined });
      if (report.suggestions.length > 0) {
        this.suggestions = [...new Set([...this.suggestions, ...report.suggestions])];
      }
    } catch (error) {
      this.#patch(id, { problem: toAppError(error) });
    }
  }

  /** Remembers a password for this session and reads the file again. */
  async unlock(id: number, password: string): Promise<void> {
    this.#passwords.set(id, password);
    await this.inspect(id);
    if (this.#find(id)?.problem?.code === 'QW-PDF-WRONG-PASSWORD') this.#passwords.delete(id);
  }

  remove(id: number): void {
    this.entries = this.entries.filter((e) => e.id !== id);
    this.#passwords.delete(id);
    if (this.selectedId === id) this.selectedId = this.entries[0]?.id ?? null;
  }

  clear(): void {
    this.entries = [];
    this.suggestions = [];
    this.selectedId = null;
    this.#passwords.clear();
  }

  /** Moves a file up (-1) or down (1) in the order, which decides merge order. */
  move(id: number, delta: -1 | 1): void {
    const from = this.entries.findIndex((e) => e.id === id);
    const to = from + delta;
    if (from < 0 || to < 0 || to >= this.entries.length) return;
    const next = [...this.entries];
    const [item] = next.splice(from, 1);
    if (item) next.splice(to, 0, item);
    this.entries = next;
  }

  select(id: number): void {
    this.selectedId = id;
  }

  /** Applies a change to one entry. */
  patch(id: number, change: Partial<FileEntry>): void {
    this.#patch(id, change);
  }

  /** Resets every file to waiting before a run. */
  resetStatuses(): void {
    this.entries = this.entries.map((e) => ({
      ...e,
      status: 'pending',
      progress: undefined,
      outputs: [],
      error: undefined,
    }));
  }

  #find(id: number): FileEntry | undefined {
    return this.entries.find((e) => e.id === id);
  }

  #patch(id: number, change: Partial<FileEntry>): void {
    this.entries = this.entries.map((e) => (e.id === id ? { ...e, ...change } : e));
  }
}

export const files = new FileSet();
