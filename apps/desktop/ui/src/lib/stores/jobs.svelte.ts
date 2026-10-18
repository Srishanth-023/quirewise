import type { UnlistenFn } from '@tauri-apps/api/event';
import { ipc, onBatchEvent, toAppError, type BatchEvent } from '../ipc/client';
import type {
  JobStatus,
  OutputArtifact,
  RunBatchRequest,
  ToolDescriptor,
} from '../ipc/generated';
import { files } from './files.svelte';
import { toasts } from './toasts.svelte';

/**
 * Options a tool runs with. The core fills every missing field from the Appendix A defaults, so
 * sending only the tool tag runs with defaults. Schema-generated forms replace this in Phase 3.
 */
function defaultOptions(tool: ToolDescriptor): RunBatchRequest['options'] {
  return { tool: tool.id } as RunBatchRequest['options'];
}

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
    if (this.running || files.entries.length === 0) return;
    const entries = files.entries;
    this.#unitFiles =
      tool.input_mode === 'per-file' ? entries.map((e) => [e.id]) : [entries.map((e) => e.id)];
    files.resetStatuses();
    this.results = [];
    this.status = 'running';
    this.running = true;
    try {
      this.current = await ipc.runBatch({
        tool: tool.id,
        options: defaultOptions(tool),
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
        this.results = files.entries.flatMap((e) => e.outputs);
        break;
    }
  }
}

export const jobs = new Jobs();
