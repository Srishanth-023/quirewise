// The organizer's page list and its edits. Pure functions over arrays, so history (undo and redo)
// is just a stack of lists and every rule here is unit tested without a DOM.

export type Turn = 0 | 90 | 180 | 270;

/** A page of an input file. `source` is the file's id in the work list, `page` is zero-based. */
export interface FilePage {
  key: number;
  kind: 'page';
  source: number;
  page: number;
  rotate: Turn;
}

/** A new blank page. */
export interface BlankPage {
  key: number;
  kind: 'blank';
  width_pt: number;
  height_pt: number;
}

export type OrgPage = FilePage | BlankPage;

/** US Letter, the size blank pages get when there is no neighbor to copy. */
export const LETTER = { width_pt: 612, height_pt: 792 } as const;

let nextKey = 1;
/** A key no other page in this session has, so lists can be keyed and reordered safely. */
export function newKey(): number {
  return nextKey++;
}

/** The starting list: every page of every file, in file order. */
export function fromFiles(files: { id: number; pageCount: number }[]): OrgPage[] {
  return files.flatMap((file) =>
    Array.from({ length: file.pageCount }, (_, page): OrgPage => ({
      key: newKey(),
      kind: 'page',
      source: file.id,
      page,
      rotate: 0,
    })),
  );
}

const turn = (angle: number): Turn => ((((angle % 360) + 360) % 360) as Turn);

/** Rotates the pages at `at` by `delta` degrees (a multiple of 90). Blank pages swap their sides. */
export function rotate(pages: OrgPage[], at: ReadonlySet<number>, delta: number): OrgPage[] {
  return pages.map((p, i) => {
    if (!at.has(i)) return p;
    if (p.kind === 'page') return { ...p, rotate: turn(p.rotate + delta) };
    return Math.abs(delta) % 180 === 90 ? { ...p, width_pt: p.height_pt, height_pt: p.width_pt } : p;
  });
}

/** Removes the pages at `at`. */
export function remove(pages: OrgPage[], at: ReadonlySet<number>): OrgPage[] {
  return pages.filter((_, i) => !at.has(i));
}

/** Copies the pages at `at`, each copy placed right after its original. */
export function duplicate(pages: OrgPage[], at: ReadonlySet<number>): OrgPage[] {
  return pages.flatMap((p, i) => (at.has(i) ? [p, { ...p, key: newKey() }] : [p]));
}

/** Moves the pages at `at` as one block so that the first of them lands at `to` (an index in the
 * list without them). Their relative order is kept. */
export function moveTo(pages: OrgPage[], at: ReadonlySet<number>, to: number): OrgPage[] {
  const moving = pages.filter((_, i) => at.has(i));
  const rest = pages.filter((_, i) => !at.has(i));
  const index = Math.min(Math.max(to, 0), rest.length);
  return [...rest.slice(0, index), ...moving, ...rest.slice(index)];
}

/**
 * Moves the selection one step earlier (-1) or later (1) by swapping with neighbors. A selected
 * page at the edge stays, and so does a block behind it. Returns the list and the new indexes.
 */
export function nudge(
  pages: OrgPage[],
  at: ReadonlySet<number>,
  delta: -1 | 1,
): { pages: OrgPage[]; at: Set<number> } {
  const list = [...pages];
  const sorted = [...at].sort((x, y) => (delta < 0 ? x - y : y - x));
  const fixed = new Set<number>();
  const next = new Set<number>();
  for (const i of sorted) {
    const j = i + delta;
    const a = list[i];
    const b = list[j];
    if (!a || !b || fixed.has(j)) {
      fixed.add(i);
      next.add(i);
      continue;
    }
    list[i] = b;
    list[j] = a;
    next.add(j);
  }
  return { pages: list, at: next };
}

/** Inserts `incoming` pages after index `after` (-1 puts them first). */
export function insertAfter(pages: OrgPage[], after: number, incoming: OrgPage[]): OrgPage[] {
  const index = Math.min(Math.max(after + 1, 0), pages.length);
  return [...pages.slice(0, index), ...incoming, ...pages.slice(index)];
}

/** A blank page sized like `neighbor` (as it is displayed), or Letter without one. */
export function blankLike(neighbor: OrgPage | undefined, sizes: (page: FilePage) => { w: number; h: number } | undefined): BlankPage {
  if (neighbor?.kind === 'blank') return { key: newKey(), kind: 'blank', width_pt: neighbor.width_pt, height_pt: neighbor.height_pt };
  if (neighbor?.kind === 'page') {
    const size = sizes(neighbor);
    if (size) {
      const turned = neighbor.rotate === 90 || neighbor.rotate === 270;
      return { key: newKey(), kind: 'blank', width_pt: turned ? size.h : size.w, height_pt: turned ? size.w : size.h };
    }
  }
  return { key: newKey(), kind: 'blank', ...LETTER };
}

/** Pages `from` to `to` (one-based, inclusive) of one file, clamped to the file. */
export function pagesOf(source: number, pageCount: number, from: number, to: number): FilePage[] {
  const first = Math.min(Math.max(Math.floor(from), 1), pageCount);
  const last = Math.min(Math.max(Math.floor(to), first), pageCount);
  return Array.from({ length: last - first + 1 }, (_, i): FilePage => ({
    key: newKey(),
    kind: 'page',
    source,
    page: first - 1 + i,
    rotate: 0,
  }));
}

/** One entry of the `pages` option, as the core reads it. */
export type OptionPage =
  | { kind: 'page'; source: number; page: number; rotate: string }
  | { kind: 'blank'; width_pt: number; height_pt: number };

/** The `pages` option: files are referred to by their position in the list sent to the core. */
export function toOption(pages: OrgPage[], fileIds: number[]): OptionPage[] {
  return pages.flatMap((p): OptionPage[] => {
    if (p.kind === 'blank') return [{ kind: 'blank' as const, width_pt: p.width_pt, height_pt: p.height_pt }];
    const source = fileIds.indexOf(p.source);
    return source < 0 ? [] : [{ kind: 'page' as const, source, page: p.page, rotate: String(p.rotate) }];
  });
}

/** Undo and redo over whole page lists. */
export class History {
  #past: OrgPage[][] = [];
  #future: OrgPage[][] = [];

  get canUndo(): boolean {
    return this.#past.length > 0;
  }

  get canRedo(): boolean {
    return this.#future.length > 0;
  }

  /** Records `before` as the state to return to, and drops the redo branch. */
  record(before: OrgPage[]): void {
    this.#past.push(before);
    if (this.#past.length > 200) this.#past.shift();
    this.#future = [];
  }

  undo(current: OrgPage[]): OrgPage[] | undefined {
    const previous = this.#past.pop();
    if (previous) this.#future.push(current);
    return previous;
  }

  redo(current: OrgPage[]): OrgPage[] | undefined {
    const next = this.#future.pop();
    if (next) this.#past.push(current);
    return next;
  }

  clear(): void {
    this.#past = [];
    this.#future = [];
  }
}
