// Development-only stand-in for the Tauri IPC, so the UI can be worked on in a plain browser.
// It is imported behind `import.meta.env.DEV` in main.ts and never reaches a production build.
// Page images are a real PDFium render (made with `qw thumbnail`), not drawn art.
import samplePage from './sample-page.png?inline';
import type { BatchEvent } from '../lib/ipc/generated';
import {
  byTextSchema,
  everyNSchema,
  mergeSchema,
  organizeSchema,
  rotateSchema,
} from '../tests/fixtures';

type Handler = (message: { event: string; id: number; payload: unknown }) => void;

const listeners = new Map<string, Map<number, Handler>>();
const callbacks = new Map<number, Handler>();
let nextCallback = 1;
let nextJob = 1;

function emit(event: string, payload: unknown): void {
  for (const [id, handler] of listeners.get(event) ?? []) handler({ event, id, payload });
}

const sampleBase64 = samplePage.slice(samplePage.indexOf(',') + 1);

const report = (name: string, locked: boolean) => ({
  file_size: 184_320,
  locked,
  pdf_version: locked ? null : '1.7',
  page_count: locked ? 0 : 12,
  pages: locked
    ? []
    : Array.from({ length: 12 }, (_, i) => ({ number: i + 1, width_pt: 612, height_pt: 792, rotation: 0 })),
  fonts: locked ? [] : [{ name: 'Helvetica', embedded: false }, { name: 'ABCDEE+Calibri', embedded: true }],
  images: { count: locked ? 0 : 4, min_dpi: 96, max_dpi: 300, scanned_pages: locked ? 0 : 12 },
  encryption: locked ? { method: 'AES-256', key_bits: 256, revision: 6 } : null,
  permissions: locked
    ? { print: true, copy: false, modify: false, annotate: true, fill_forms: true, assemble: false }
    : null,
  signature_count: 0,
  has_forms: false,
  has_javascript: false,
  pdfa_claim: name.includes('archive') ? 'PDF/A-2b' : null,
  has_text_layer: true,
  embedded_files: 0,
  size_breakdown: { images: 90_000, fonts: 30_000, content: 40_000, other: 24_320 },
  suggestions: locked ? ['unlock'] : [],
});

/** Tool list for the browser preview, with the same schema shapes the core produces. */
const previewTools = [
  { id: 'merge', label: 'merged', input_mode: 'all-together', options_schema: mergeSchema },
  { id: 'split-every-n', label: 'part', input_mode: 'per-file', options_schema: everyNSchema },
  { id: 'rotate', label: 'rotated', input_mode: 'per-file', options_schema: rotateSchema },
  { id: 'organize', label: 'organized', input_mode: 'all-together', options_schema: organizeSchema },
  { id: 'split-by-text', label: 'part', input_mode: 'per-file', options_schema: byTextSchema },
].map((tool) => ({ group: 'organize', ...tool }));

/** Handles every command the UI sends. Unknown commands fail loudly so gaps are noticed. */
async function invoke(cmd: string, args: Record<string, unknown> = {}): Promise<unknown> {
  switch (cmd) {
    case 'app_version':
      return '0.1.0';
    case 'list_tools':
      return new URLSearchParams(location.search).has('tools') ? previewTools : [];
    case 'pick_paths':
      return args.kind === 'image' ? ['C:/Docs/logo.png'] : ['C:/Docs/notes.txt'];
    case 'pick_files':
      return ['C:\\Docs\\report.pdf', 'C:\\Docs\\locked.pdf'];
    case 'classify_inputs':
      return {
        files: (args.paths as string[]).map((path) => ({
          path,
          name: path.split('\\').pop(),
          kind: 'pdf',
          error: null,
          modified: 1_700_000_000 + path.length,
        })),
        suggestions: (args.paths as string[]).length > 1 ? ['merge'] : [],
      };
    case 'inspect_document': {
      const path = String(args.path);
      const locked = path.includes('locked');
      if (locked && args.password === 'open') return report(path, false);
      if (locked && args.password) {
        throw { code: 'QW-PDF-WRONG-PASSWORD', message: 'The password is not correct. Check it and try again.' };
      }
      return report(path, locked);
    }
    case 'render_thumbnail':
      if (String(args.path).includes('locked') && args.password !== 'open') {
        throw { code: 'QW-PDF-ENCRYPTED', message: 'This file is password protected.' };
      }
      return { width: 232, height: 300, page_count: 12, png: sampleBase64 };
    case 'recent_list':
      return [
        { path: 'C:\\Docs\\report.pdf', name: 'report.pdf', opened_at: 1 },
        { path: 'C:\\Docs\\archive.pdf', name: 'archive.pdf', opened_at: 2 },
      ];
    case 'recent_add':
    case 'recent_clear':
    case 'save_settings':
    case 'retry_failed':
      return null;
    case 'get_settings':
      return { version: 1, theme: 'system', remember_recent_files: true };
    case 'run_batch': {
      const job = nextJob++;
      const inputs = (args.request as { inputs: { path: string }[] }).inputs;
      inputs.forEach((input, unit) => {
        let done = 0;
        const timer = setInterval(() => {
          done += 3;
          emit('batch', { type: 'progress', job, unit, done, total: 12 } satisfies BatchEvent);
          if (done >= 12) {
            clearInterval(timer);
            emit('batch', {
              type: 'unit-finished',
              job,
              unit,
              status: 'done',
              error: null,
              outputs: [{ path: input.path.replace('.pdf', ' (rotated).pdf'), pages: 12, bytes: 1 }],
            } satisfies BatchEvent);
            if (unit === inputs.length - 1) {
              setTimeout(() => emit('batch', { type: 'job-finished', job, status: 'completed' } satisfies BatchEvent), 50);
            }
          }
        }, 250);
      });
      return job;
    }
    case 'cancel_job':
      return true;
    case 'open_output':
    case 'reveal_output':
      return null;
    case 'plugin:event|listen': {
      const { event, handler } = args as { event: string; handler: number };
      const bucket = listeners.get(event) ?? new Map<number, Handler>();
      const fn = callbacks.get(handler);
      if (fn) bucket.set(handler, fn);
      listeners.set(event, bucket);
      return handler;
    }
    case 'plugin:event|unlisten':
      return null;
    default:
      throw new Error(`mockTauri: unhandled command ${cmd}`);
  }
}

/** Installs the fake Tauri internals on `window`. */
export function installMockTauri(): void {
  const w = window as unknown as Record<string, unknown>;
  w.__TAURI_INTERNALS__ = {
    invoke,
    transformCallback(callback: Handler): number {
      const id = nextCallback++;
      callbacks.set(id, callback);
      return id;
    },
    unregisterCallback(id: number): void {
      callbacks.delete(id);
    },
    convertFileSrc: (path: string) => path,
    metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main', windowLabel: 'main' } },
  };
  w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => undefined };
}
