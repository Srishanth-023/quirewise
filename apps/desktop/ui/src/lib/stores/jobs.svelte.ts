import type { UnlistenFn } from '@tauri-apps/api/event';
import { ipc, onBatchEvent, toAppError, type BatchEvent } from '../ipc/client';
import type { JobStatus, OutputArtifact, ToolDescriptor } from '../ipc/generated';
import { files } from './files.svelte';
import { organizer } from '../features/organize/organizer.svelte';
import { toolOptions } from './options.svelte';
import { runsWithoutFiles } from '../forms/toolForms';
import { toasts } from './toasts.svelte';

/**
 * Batch job state. A job's units map to files: one unit per file for per-file tools, or a single
 * unit covering every file for merge-style tools.
 */
class Jobs {
  current = $state<number | null>(null);
  running = $state(false);
  status = $state<JobStatus | null>(null);
  /** Result files of the last finished job, for the completion panel. */
  results = $state<OutputArtifact[]>([]);
  #unitFiles: number[][] = [];
  /** Results of runs that had no file to attach them to, such as a printed web address. */
  #extra: OutputArtifact[] = [];
  #unlisten: UnlistenFn | undefined;

  /** Starts listening for batch events. Safe to call more than once. */
  async listen(): Promise<void> {
    this.#unlisten ??= await onBatchEvent((event) => this.#apply(event));
  }

  stop(): void {
    this.#unlisten?.();
    this.#unlisten = undefined;
  }

  /** Runs `tool` on every file in the work list. */
  async run(tool: ToolDescriptor): Promise<void> {
    const noFiles = files.entries.length === 0 && runsWithoutFiles(tool.id, toolOptions.values(tool));
    if (this.running || (files.entries.length === 0 && !noFiles)) return;
    const entries = files.entries;
    this.#extra = [];
    this.#unitFiles = noFiles
      ? [[]]
      : tool.input_mode === 'per-file'
        ? entries.map((e) => [e.id])
        : [entries.map((e) => e.id)];
    // The organizer names files by their place in the list, which may have changed since the last edit.
    if (tool.id === 'organize') organizer.publish();
    files.resetStatuses();
    this.results = [];
    this.status = 'running';
    this.running = true;
    try {
      this.current = await ipc.runBatch({
        tool: tool.id,
        options: toolOptions.request(
          tool,
          entries.map((e) => e.id),
        ),
        inputs: entries.map((e) => ({ path: e.path, password: files.passwordFor(e.id) })),
        output_dir: null,
        naming_pattern: null,
      });
    } catch (error) {
      this.running = false;
      this.status = null;
      toasts.error(toAppError(error));
    }
  }

  async cancel(): Promise<void> {
    if (this.current !== null && this.running) await ipc.cancelJob(this.current);
  }

  async retryFailed(): Promise<void> {
    if (this.current === null || this.running) return;
    try {
      await ipc.retryFailed(this.current);
      this.running = true;
      this.status = 'running';
    } catch (error) {
      toasts.error(toAppError(error));
    }
  }

  get hasFailures(): boolean {
    return files.entries.some((e) =>
      ['wrong-password', 'damaged', 'out-of-range', 'failed'].includes(e.status),
    );
  }

  #apply(event: BatchEvent): void {
    if (event.job !== this.current) return;
    const ids = this.#unitFiles[event.type === 'job-finished' ? 0 : event.unit] ?? [];
    switch (event.type) {
      case 'progress':
        for (const id of ids) {
          files.patch(id, { status: 'running', progress: { done: event.done, total: event.total } });
        }
        break;
      case 'unit-finished':
        if (ids.length === 0) {
          this.#extra = [...this.#extra, ...event.outputs];
          if (event.error) toasts.error(event.error);
        }
        for (const id of ids) {
          files.patch(id, {
            status: event.status,
            error: event.error ?? undefined,
            outputs: event.outputs,
            progress: undefined,
          });
        }
        break;
      case 'job-finished':
        this.running = false;
        this.status = event.status;
        this.results = [...files.entries.flatMap((e) => e.outputs), ...this.#extra];
        break;
    }
  }
}

export const jobs = new Jobs();
