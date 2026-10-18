import type { ToolDescriptor } from '../../ipc/client';
import { files, type FileEntry } from '../../stores/files.svelte';
import { toolOptions } from '../../stores/options.svelte';
import {
  History,
  blankLike,
  duplicate,
  fromFiles,
  insertAfter,
  moveTo,
  nudge,
  pagesOf,
  remove,
  rotate,
  toOption,
  type FilePage,
  type OrgPage,
} from './model';

/** Files the organizer can show: PDFs that were read and are not locked. */
function usable(entries: FileEntry[]): FileEntry[] {
  return entries.filter((e) => e.kind === 'pdf' && e.report && !e.report.locked);
}

/** Changes whenever the set of usable files or their page counts change. */
function signature(entries: FileEntry[]): string {
  return usable(entries)
    .map((e) => `${e.id}:${e.report?.page_count ?? 0}`)
    .sort()
    .join(',');
}

/**
 * The organizer's working list. It starts with every page of every file and is edited as a whole;
 * the final list is what the Organize tool receives. Adding or removing a file starts over,
 * because the pages edited so far would no longer match the files.
 */
class Organizer {
  pages = $state<OrgPage[]>([]);
  selection = $state<number[]>([]);
  #history = new History();
  #signature = '';
  #tool: ToolDescriptor | undefined;
  /** Bumped on every change so the buttons that depend on history refresh. */
  version = $state(0);

  /** Follows the work list. Call it whenever the files or the selected tool change. */
  sync(tool: ToolDescriptor, entries: FileEntry[]): void {
    this.#tool = tool;
    const next = signature(entries);
    if (next !== this.#signature) {
      this.#signature = next;
      this.pages = fromFiles(usable(entries).map((e) => ({ id: e.id, pageCount: e.report?.page_count ?? 0 })));
      this.selection = [];
      this.#history.clear();
      this.version++;
    }
    this.publish();
  }

  get canUndo(): boolean {
    void this.version;
    return this.#history.canUndo;
  }

  get canRedo(): boolean {
    void this.version;
    return this.#history.canRedo;
  }

  get selected(): Set<number> {
    return new Set(this.selection);
  }

  /** Selects one page, or toggles it into the selection (Ctrl) or extends it from the last click (Shift). */
  pick(index: number, mode: 'single' | 'toggle' | 'range'): void {
    if (mode === 'toggle') {
      this.selection = this.selection.includes(index)
        ? this.selection.filter((i) => i !== index)
        : [...this.selection, index];
    } else if (mode === 'range' && this.selection.length > 0) {
      const anchor = this.selection[this.selection.length - 1] ?? index;
      const [from, to] = anchor < index ? [anchor, index] : [index, anchor];
      this.selection = Array.from({ length: to - from + 1 }, (_, i) => from + i);
    } else {
      this.selection = [index];
    }
  }

  selectAll(): void {
    this.selection = this.pages.map((_, i) => i);
  }

  clearSelection(): void {
    this.selection = [];
  }

  #edit(next: OrgPage[], selection?: number[]): void {
    this.#history.record(this.pages);
    this.pages = next;
    if (selection) this.selection = selection;
    this.version++;
    this.publish();
  }

  rotateSelected(delta: 90 | -90): void {
    if (this.selection.length > 0) this.#edit(rotate(this.pages, this.selected, delta));
  }

  removeSelected(): void {
    if (this.selection.length > 0) this.#edit(remove(this.pages, this.selected), []);
  }

  duplicateSelected(): void {
    if (this.selection.length === 0) return;
    this.#edit(duplicate(this.pages, this.selected), []);
  }

  nudgeSelected(delta: -1 | 1): void {
    if (this.selection.length === 0) return;
    const moved = nudge(this.pages, this.selected, delta);
    if (moved.pages.every((p, i) => p === this.pages[i])) return;
    this.#edit(moved.pages, [...moved.at].sort((a, b) => a - b));
  }

  /** Moves the selection (or the dragged page) so that it comes before page `target`. */
  moveSelectedBefore(target: number): void {
    if (this.selection.length === 0) return;
    const at = this.selected;
    const before = this.pages.filter((_, i) => i < target && !at.has(i)).length;
    this.#edit(
      moveTo(this.pages, at, before),
      this.selection.map((_, i) => before + i),
    );
  }

  /** A blank page after the last selected page, or at the end without a selection. */
  insertBlank(): void {
    const last = this.selection.length > 0 ? Math.max(...this.selection) : this.pages.length - 1;
    const blank = blankLike(this.pages[last], (page) => this.#displaySize(page));
    this.#edit(insertAfter(this.pages, last, [blank]), [last + 1]);
  }

  /** Pages `from` to `to` (one-based) of another file, after the selection or at the end. */
  insertFromFile(fileId: number, from: number, to: number): void {
    const entry = files.entries.find((e) => e.id === fileId);
    const count = entry?.report?.page_count ?? 0;
    if (count === 0) return;
    const incoming = pagesOf(fileId, count, from, to);
    const last = this.selection.length > 0 ? Math.max(...this.selection) : this.pages.length - 1;
    this.#edit(
      insertAfter(this.pages, last, incoming),
      incoming.map((_, i) => last + 1 + i),
    );
  }

  undo(): void {
    const previous = this.#history.undo(this.pages);
    if (!previous) return;
    this.pages = previous;
    this.selection = [];
    this.version++;
    this.publish();
  }

  redo(): void {
    const next = this.#history.redo(this.pages);
    if (!next) return;
    this.pages = next;
    this.selection = [];
    this.version++;
    this.publish();
  }

  /** Back to the untouched list. Undo can still return to the edited one. */
  reset(): void {
    const entries = usable(files.entries);
    this.#edit(fromFiles(entries.map((e) => ({ id: e.id, pageCount: e.report?.page_count ?? 0 }))), []);
  }

  /** Name of the file a page comes from, shown on every page so files are told apart by text. */
  sourceName(page: FilePage): string {
    return files.entries.find((e) => e.id === page.source)?.name ?? '';
  }

  #displaySize(page: FilePage): { w: number; h: number } | undefined {
    const info = files.entries.find((e) => e.id === page.source)?.report?.pages[page.page];
    if (!info) return undefined;
    const turned = info.rotation === 90 || info.rotation === 270;
    return turned ? { w: info.height_pt, h: info.width_pt } : { w: info.width_pt, h: info.height_pt };
  }

  /** Sends the current list to the options, with files named by their place in the work list. */
  publish(): void {
    if (!this.#tool) return;
    const ids = files.entries.map((e) => e.id);
    toolOptions.set(this.#tool, 'pages', toOption(this.pages, ids));
  }
}

export const organizer = new Organizer();
