// The editor's pure logic: what can be placed on a page, how it moves and resizes, how strokes
// are thinned and fitted, and how the placed things become the options of a tool. Nothing here
// touches the screen, so it is tested on its own.

import type {
  Annotation,
  EditItem,
  FieldValue,
  NewField,
  SignatureItem,
  SignatureSource,
} from '../../ipc/generated';

export interface Region {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface Pt {
  x: number;
  y: number;
}

/** Smallest side of a box, as a fraction of the page. */
export const MIN_SIDE = 0.01;

const clamp01 = (n: number): number => Math.min(1, Math.max(0, n));

// ----- what can be placed ------------------------------------------------------------------

/** The tools of the editor that make things. `select` makes nothing and moves what is there. */
export type Creator =
  | 'select'
  | 'text'
  | 'image'
  | 'rectangle'
  | 'ellipse'
  | 'line'
  | 'arrow'
  | 'link'
  | 'whiteout'
  | 'edit-text'
  | 'highlight'
  | 'underline'
  | 'strikeout'
  | 'note'
  | 'freehand'
  | 'signature'
  | 'initials'
  | 'stamp'
  | 'date'
  | 'text-field'
  | 'checkbox'
  | 'radio'
  | 'dropdown'
  | 'signature-field';

/** The tools each editor offers, in the order of the toolbar. */
export const creatorsFor: Record<string, Creator[]> = {
  'edit-pdf': ['select', 'text', 'edit-text', 'image', 'rectangle', 'ellipse', 'line', 'arrow', 'link', 'whiteout'],
  annotate: ['select', 'highlight', 'underline', 'strikeout', 'note', 'freehand'],
  'sign-pdf': ['select', 'signature', 'initials', 'stamp', 'date'],
  'fill-sign': ['select', 'signature', 'initials', 'stamp', 'date'],
  'create-forms': ['select', 'text-field', 'checkbox', 'radio', 'dropdown', 'signature-field'],
};

/** The editors: the tools whose options the editor view builds. */
export const editorTools = Object.keys(creatorsFor);

/** How a creator is drawn: by dragging a box, by a click, or by a stroke. */
export type Gesture = 'box' | 'point' | 'stroke' | 'none';

export function gestureOf(creator: Creator): Gesture {
  switch (creator) {
    case 'select':
      return 'none';
    case 'note':
      return 'point';
    case 'freehand':
      return 'stroke';
    default:
      return 'box';
  }
}

/** The key of a tool's options each kind of entry goes to. */
export type Group = 'items' | 'annotations' | 'signatures' | 'fields';

export interface Entry {
  id: number;
  group: Group;
  data: EditItem | Annotation | SignatureItem | NewField;
}

export function groupOf(creator: Creator): Group | null {
  switch (creator) {
    case 'select':
      return null;
    case 'highlight':
    case 'underline':
    case 'strikeout':
    case 'note':
    case 'freehand':
      return 'annotations';
    case 'signature':
    case 'initials':
    case 'stamp':
    case 'date':
      return 'signatures';
    case 'text-field':
    case 'checkbox':
    case 'radio':
    case 'dropdown':
    case 'signature-field':
      return 'fields';
    default:
      return 'items';
  }
}

// ----- geometry ----------------------------------------------------------------------------

/** The box two corners span, in any drag direction; null when it is too small to be a box. */
export function boxFromDrag(a: Pt, b: Pt): Region | null {
  const x0 = clamp01(Math.min(a.x, b.x));
  const y0 = clamp01(Math.min(a.y, b.y));
  const x1 = clamp01(Math.max(a.x, b.x));
  const y1 = clamp01(Math.max(a.y, b.y));
  const width = x1 - x0;
  const height = y1 - y0;
  return width < MIN_SIDE || height < MIN_SIDE ? null : { x: x0, y: y0, width, height };
}

/** A box of the given size centered on a point, kept inside the page. */
export function boxAround(at: Pt, width: number, height: number): Region {
  const w = Math.min(width, 1);
  const h = Math.min(height, 1);
  return {
    x: Math.min(Math.max(at.x - w / 2, 0), 1 - w),
    y: Math.min(Math.max(at.y - h / 2, 0), 1 - h),
    width: w,
    height: h,
  };
}

/** Moves a box by a distance in page fractions and keeps it inside the page. */
export function moveBox(box: Region, dx: number, dy: number): Region {
  return {
    ...box,
    x: Math.min(Math.max(box.x + dx, 0), 1 - box.width),
    y: Math.min(Math.max(box.y + dy, 0), 1 - box.height),
  };
}

/** Moves the bottom right corner of a box, keeping the box on the page and above the minimum. */
export function resizeBox(box: Region, dx: number, dy: number): Region {
  return {
    ...box,
    width: Math.min(Math.max(box.width + dx, MIN_SIDE), 1 - box.x),
    height: Math.min(Math.max(box.height + dy, MIN_SIDE), 1 - box.y),
  };
}

/** Applies a typed change, in percent, to one side of a box. */
export function withPercent(box: Region, side: keyof Region, percent: number): Region {
  if (!Number.isFinite(percent)) return box;
  const next = { ...box, [side]: clamp01(percent / 100) };
  next.x = Math.min(next.x, 1 - MIN_SIDE);
  next.y = Math.min(next.y, 1 - MIN_SIDE);
  next.width = Math.min(Math.max(next.width, MIN_SIDE), 1 - next.x);
  next.height = Math.min(Math.max(next.height, MIN_SIDE), 1 - next.y);
  return next;
}

// ----- strokes -----------------------------------------------------------------------------

function distanceToLine(p: Pt, a: Pt, b: Pt): number {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const length = Math.hypot(dx, dy);
  if (length === 0) return Math.hypot(p.x - a.x, p.y - a.y);
  return Math.abs(dy * p.x - dx * p.y + b.x * a.y - b.y * a.x) / length;
}

/** Thins a stroke with the Douglas-Peucker rule: points that lie within `tolerance` of the line
 *  between their neighbours go. The ends always stay. */
export function simplify(points: Pt[], tolerance = 0.0015): Pt[] {
  if (points.length < 3) return points;
  const keep = new Array<boolean>(points.length).fill(false);
  keep[0] = true;
  keep[points.length - 1] = true;
  const stack: [number, number][] = [[0, points.length - 1]];
  while (stack.length > 0) {
    const [from, to] = stack.pop() as [number, number];
    let worst = 0;
    let at = -1;
    for (let i = from + 1; i < to; i++) {
      const d = distanceToLine(points[i] as Pt, points[from] as Pt, points[to] as Pt);
      if (d > worst) {
        worst = d;
        at = i;
      }
    }
    if (at >= 0 && worst > tolerance) {
      keep[at] = true;
      stack.push([from, at], [at, to]);
    }
  }
  return points.filter((_, i) => keep[i]);
}

/** The smallest box around strokes, or null when there are no points. */
export function boundsOf(strokes: Pt[][]): Region | null {
  const all = strokes.flat();
  if (all.length === 0) return null;
  const x0 = Math.min(...all.map((p) => p.x));
  const x1 = Math.max(...all.map((p) => p.x));
  const y0 = Math.min(...all.map((p) => p.y));
  const y1 = Math.max(...all.map((p) => p.y));
  return { x: x0, y: y0, width: x1 - x0, height: y1 - y0 };
}

/**
 * Strokes drawn on a pad of `padAspect` (width over height), as points in 0 to 1 of their own
 * bounding box, and the proportions of that box so a signature is placed without being squashed.
 */
export function normalizeStrokes(
  strokes: Pt[][],
  padAspect: number,
): { strokes: Pt[][]; aspect: number } | null {
  const box = boundsOf(strokes);
  if (!box) return null;
  // A stroke that is one dot or one line has no width or height; give it a little.
  const w = Math.max(box.width, 0.002);
  const h = Math.max(box.height, 0.002);
  return {
    strokes: strokes.map((s) => s.map((p) => ({ x: (p.x - box.x) / w, y: (p.y - box.y) / h }))),
    aspect: (w * padAspect) / h,
  };
}

/** The box a signature of the given proportions fills when it is dropped at a point. */
export function signatureBox(at: Pt, aspect: number, width = 0.25): Region {
  const height = width / Math.max(aspect, 0.1);
  return boxAround(at, width, Math.min(height, 0.4));
}

// ----- entries -----------------------------------------------------------------------------

/** The box of an entry, or null for entries that sit at a point or are made of strokes. */
export function boxOf(entry: Entry): Region | null {
  const d = entry.data as unknown as { region?: Region; at?: Pt };
  if (d.region) return d.region;
  if (entry.group === 'annotations') {
    const a = entry.data as Annotation;
    if (a.kind === 'highlight' || a.kind === 'underline' || a.kind === 'strikeout') {
      return a.regions[0] ?? null;
    }
    if (a.kind === 'note') return { x: a.at.x, y: a.at.y, width: 0.03, height: 0.03 };
    if (a.kind === 'freehand') return boundsOf(a.strokes);
  }
  return null;
}

/** The entry with its box changed, for the kinds that have one box. */
export function withBox(entry: Entry, box: Region): Entry {
  const data = entry.data as unknown as Record<string, unknown>;
  if ('region' in data) return { ...entry, data: { ...data, region: box } as unknown as Entry['data'] };
  if (entry.group === 'annotations') {
    const a = entry.data as Annotation;
    if (a.kind === 'highlight' || a.kind === 'underline' || a.kind === 'strikeout') {
      const [first, ...rest] = a.regions;
      const dx = box.x - (first?.x ?? 0);
      const dy = box.y - (first?.y ?? 0);
      return {
        ...entry,
        data: {
          ...a,
          regions: [box, ...rest.map((r) => moveBox(r, dx, dy))],
        },
      };
    }
    if (a.kind === 'note') return { ...entry, data: { ...a, at: { x: box.x, y: box.y } } };
    if (a.kind === 'freehand') {
      const old = boundsOf(a.strokes);
      if (!old) return entry;
      const sx = old.width > 0 ? box.width / old.width : 1;
      const sy = old.height > 0 ? box.height / old.height : 1;
      return {
        ...entry,
        data: {
          ...a,
          strokes: a.strokes.map((s) =>
            s.map((p) => ({ x: box.x + (p.x - old.x) * sx, y: box.y + (p.y - old.y) * sy })),
          ),
        },
      };
    }
  }
  return entry;
}

/** A short description of an entry for the list. */
export function labelOf(entry: Entry): string {
  const d = entry.data;
  if (entry.group === 'items') {
    const i = d as EditItem;
    switch (i.kind) {
      case 'text':
        return i.text.split('\n')[0]?.slice(0, 40) || 'Text';
      case 'image':
        return i.path.split(/[\\/]/).pop() ?? 'Picture';
      case 'shape':
        return i.shape;
      case 'link':
        return i.target.type === 'uri' ? i.target.uri : `Page ${i.target.page}`;
      case 'whiteout':
        return 'Patch';
    }
  }
  if (entry.group === 'annotations') {
    const a = d as Annotation;
    return a.kind === 'note' ? a.text.slice(0, 40) || 'Note' : a.kind;
  }
  if (entry.group === 'signatures') {
    const s = (d as SignatureItem).source;
    return s.type === 'typed' ? s.text : s.type;
  }
  const f = d as NewField;
  return `${f.name} (${f.kind.type})`;
}

/** The page an entry is on, from 1. */
export function pageOfEntry(entry: Entry): number {
  return (entry.data as { page: number }).page;
}

/** A name for a new field that is not used yet: `text_1`, `text_2`, ... */
export function freshName(base: string, taken: string[]): string {
  const clean = base.replace(/[^\p{L}\p{N}]+/gu, '_').replace(/^_+|_+$/g, '') || 'field';
  if (!taken.includes(clean)) return clean;
  for (let n = 2; ; n++) {
    const candidate = `${clean}_${n}`;
    if (!taken.includes(candidate)) return candidate;
  }
}

/** The options a tool receives: the entries of each group, and for filling the typed values. */
export function toOptions(
  toolId: string,
  entries: Entry[],
  values: Record<string, string>,
): Record<string, unknown> {
  const pick = <T>(group: Group): T[] => entries.filter((e) => e.group === group).map((e) => e.data as T);
  const typed: FieldValue[] = Object.entries(values)
    .filter(([, v]) => v !== '')
    .map(([name, value]) => ({ name, value }));
  switch (toolId) {
    case 'edit-pdf':
      return { items: pick<EditItem>('items') };
    case 'annotate':
      return { annotations: pick<Annotation>('annotations') };
    case 'sign-pdf':
      return { signatures: pick<SignatureItem>('signatures') };
    case 'fill-sign':
      return { values: typed, signatures: pick<SignatureItem>('signatures') };
    case 'create-forms':
      return { fields: pick<NewField>('fields') };
    default:
      return {};
  }
}

/** A kept-signature source, ready to place. */
export function placeable(source: SignatureSource): SignatureSource {
  return structuredClone(source);
}

// ----- history -----------------------------------------------------------------------------

/** Undo and redo over snapshots of any kind of state. */
export class Timeline<T> {
  #past: T[] = [];
  #future: T[] = [];

  get canUndo(): boolean {
    return this.#past.length > 0;
  }

  get canRedo(): boolean {
    return this.#future.length > 0;
  }

  /** Remembers `before` as the state to return to, and drops what could be redone. */
  record(before: T): void {
    this.#past.push(before);
    if (this.#past.length > 200) this.#past.shift();
    this.#future = [];
  }

  undo(current: T): T | undefined {
    const previous = this.#past.pop();
    if (previous !== undefined) this.#future.push(current);
    return previous;
  }

  redo(current: T): T | undefined {
    const next = this.#future.pop();
    if (next !== undefined) this.#past.push(current);
    return next;
  }

  clear(): void {
    this.#past = [];
    this.#future = [];
  }
}
