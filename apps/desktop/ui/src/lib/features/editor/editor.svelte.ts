import { untrack } from 'svelte';
import type { Annotation, EditItem, NewField, SignatureItem, SignatureSource, TextBlock } from '../../ipc/generated';
import type { ToolDescriptor } from '../../ipc/client';
import { ipc } from '../../ipc/client';
import type { FileEntry } from '../../stores/files.svelte';
import { toolOptions } from '../../stores/options.svelte';
import { palette } from './palette';
import {
  Timeline,
  boxAround,
  boxFromDrag,
  creatorsFor,
  freshName,
  pageOfEntry,
  signatureBox,
  simplify,
  toOptions,
  withBox,
  type Creator,
  type Entry,
  type Pt,
  type Region,
} from './model';

/** Default sizes of boxes made by a click without a drag, as fractions of the page. */
const CLICK_SIZE: Partial<Record<Creator, [number, number]>> = {
  text: [0.3, 0.05],
  image: [0.25, 0.15],
  rectangle: [0.25, 0.12],
  ellipse: [0.25, 0.12],
  line: [0.25, 0.04],
  arrow: [0.25, 0.04],
  link: [0.25, 0.04],
  whiteout: [0.25, 0.04],
  highlight: [0.4, 0.025],
  underline: [0.4, 0.025],
  strikeout: [0.4, 0.025],
  signature: [0.25, 0.08],
  initials: [0.1, 0.06],
  stamp: [0.2, 0.12],
  date: [0.2, 0.04],
  'text-field': [0.3, 0.035],
  checkbox: [0.025, 0.02],
  radio: [0.025, 0.02],
  dropdown: [0.3, 0.035],
  'signature-field': [0.3, 0.07],
};

interface Snapshot {
  entries: Entry[];
  values: Record<string, string>;
}

function copy(entries: Entry[]): Entry[] {
  return structuredClone($state.snapshot(entries)) as Entry[];
}

/**
 * What is being placed on the pages of the selected file. The editor view draws it and the tool
 * receives it: every change is sent to the options of the selected tool at once, like the
 * organizer's page list.
 */
class Editor {
  entries = $state<Entry[]>([]);
  values = $state<Record<string, string>>({});
  page = $state(1);
  creator = $state<Creator>('select');
  selectedId = $state<number | null>(null);
  /** The signature, initials or stamp that the next click places. */
  pending = $state<{ source: SignatureSource; aspect: number } | null>(null);
  /** Bumped on every change so buttons that depend on history refresh. */
  version = $state(0);
  #timeline = new Timeline<Snapshot>();
  #next = 1;
  #tool = $state.raw<ToolDescriptor | undefined>();
  #key = '';

  /** Follows the tool and the file: starts over when either changes. */
  sync(tool: ToolDescriptor, entry: FileEntry | undefined): void {
    this.#tool = tool;
    const key = `${tool.id}|${entry?.id ?? ''}|${entry?.report?.page_count ?? 0}`;
    if (key !== this.#key) {
      this.#key = key;
      this.entries = [];
      this.values = {};
      this.page = 1;
      this.creator = 'select';
      this.selectedId = null;
      this.pending = null;
      this.#timeline.clear();
      this.version++;
    }
    this.publish();
  }

  get creators(): Creator[] {
    return this.#tool ? (creatorsFor[this.#tool.id] ?? ['select']) : ['select'];
  }

  get canUndo(): boolean {
    void this.version;
    return this.#timeline.canUndo;
  }

  get canRedo(): boolean {
    void this.version;
    return this.#timeline.canRedo;
  }

  get selected(): Entry | undefined {
    return this.entries.find((e) => e.id === this.selectedId);
  }

  entriesOnPage(page: number): Entry[] {
    return this.entries.filter((e) => pageOfEntry(e) === page);
  }

  #snapshot(): Snapshot {
    return {
      entries: copy(this.entries),
      values: { ...$state.snapshot(this.values) },
    };
  }

  #restore(s: Snapshot): void {
    this.entries = s.entries;
    this.values = s.values;
    if (!this.entries.some((e) => e.id === this.selectedId)) this.selectedId = null;
    this.version++;
    this.publish();
  }

  /** Records the state before a change, so undo can return to it. */
  #change(apply: () => void): void {
    this.#timeline.record(this.#snapshot());
    apply();
    this.version++;
    this.publish();
  }

  undo(): void {
    const previous = this.#timeline.undo(this.#snapshot());
    if (previous) this.#restore(previous);
  }

  redo(): void {
    const next = this.#timeline.redo(this.#snapshot());
    if (next) this.#restore(next);
  }

  choose(creator: Creator): void {
    this.creator = creator;
    if (creator !== 'select') this.selectedId = null;
  }

  select(id: number | null): void {
    this.selectedId = id;
  }

  /** Changes one entry; one undo step. */
  update(id: number, change: (data: Entry['data']) => Entry['data']): void {
    this.#change(() => {
      this.entries = this.entries.map((e) => (e.id === id ? { ...e, data: change(e.data) } : e));
    });
  }

  /** Changes one entry without a step of its own, for dragging: call `commit` first. */
  live(id: number, box: Region): void {
    this.entries = this.entries.map((e) => (e.id === id ? withBox(e, box) : e));
    this.publish();
  }

  /** Remembers the state before a drag starts. */
  commit(): void {
    this.#timeline.record(this.#snapshot());
    this.version++;
  }

  remove(id: number): void {
    this.#change(() => {
      this.entries = this.entries.filter((e) => e.id !== id);
      if (this.selectedId === id) this.selectedId = null;
    });
  }

  setValue(name: string, value: string): void {
    this.#change(() => {
      this.values = { ...this.values, [name]: value };
    });
  }

  /** Back to nothing placed. Undo can still return to what was there. */
  clear(): void {
    if (this.entries.length === 0 && Object.keys(this.values).length === 0) return;
    this.#change(() => {
      this.entries = [];
      this.values = {};
      this.selectedId = null;
    });
  }

  #add(group: Entry['group'], data: Entry['data'], more: { group: Entry['group']; data: Entry['data'] }[] = []): void {
    this.#change(() => {
      const made: Entry[] = [{ id: this.#next++, group, data }, ...more.map((m) => ({ id: this.#next++, group: m.group, data: m.data }))];
      this.entries = [...this.entries, ...made];
      // The thing that was asked for is selected, so its details can be edited at once.
      this.selectedId = made[made.length - 1]?.id ?? null;
    });
  }

  #names(): string[] {
    return this.entries
      .filter((e) => e.group === 'fields')
      .map((e) => (e.data as NewField).name);
  }

  /** The box for a gesture: the dragged box, or the default size centered where it was clicked. */
  boxFor(creator: Creator, from: Pt, to: Pt): Region {
    const dragged = boxFromDrag(from, to);
    if (dragged) return dragged;
    if ((creator === 'signature' || creator === 'initials' || creator === 'stamp') && this.pending) {
      return signatureBox(to, this.pending.aspect, creator === 'initials' ? 0.1 : 0.25);
    }
    const [w, h] = CLICK_SIZE[creator] ?? [0.25, 0.08];
    return boxAround(to, w, h);
  }

  /** Whether a creator can place something right now (signatures need one chosen first). */
  ready(creator: Creator): boolean {
    if (creator === 'signature' || creator === 'initials' || creator === 'stamp') return this.pending !== null;
    return creator !== 'select';
  }

  /** Places what the creator makes, in the box. Pictures and stamps are given their file. */
  place(creator: Creator, box: Region, extra?: { path?: string }): void {
    const page = this.page;
    switch (creator) {
      case 'text':
        this.#add('items', {
          kind: 'text',
          page,
          region: box,
          text: 'Text',
          style: { size_pt: 12, color: palette.black },
        } satisfies EditItem);
        return;
      case 'image':
        if (extra?.path) this.#add('items', { kind: 'image', page, region: box, path: extra.path } satisfies EditItem);
        return;
      case 'rectangle':
      case 'ellipse':
      case 'line':
      case 'arrow':
        this.#add('items', {
          kind: 'shape',
          page,
          shape: creator,
          region: box,
          stroke_color: palette.mark,
          stroke_width_pt: 2,
        } satisfies EditItem);
        return;
      case 'link':
        this.#add('items', {
          kind: 'link',
          page,
          region: box,
          target: { type: 'uri', uri: 'https://example.com' },
        } satisfies EditItem);
        return;
      case 'whiteout':
        this.#add('items', { kind: 'whiteout', page, region: box, color: palette.white } satisfies EditItem);
        return;
      case 'highlight':
      case 'underline':
      case 'strikeout':
        this.#add('annotations', {
          kind: creator,
          page,
          regions: [box],
          color: creator === 'highlight' ? palette.highlight : palette.mark,
        } satisfies Annotation);
        return;
      case 'signature':
      case 'initials':
      case 'stamp':
        if (this.pending) this.#add('signatures', { page, region: box, source: structuredClone($state.snapshot(this.pending.source)) } satisfies SignatureItem);
        return;
      case 'date':
        this.#add('signatures', {
          page,
          region: box,
          source: { type: 'date', format: 'iso', style: { size_pt: 12 } },
        } satisfies SignatureItem);
        return;
      case 'text-field':
        this.#add('fields', {
          page,
          region: box,
          name: freshName('text', this.#names()),
          kind: { type: 'text' },
        } satisfies NewField);
        return;
      case 'checkbox':
        this.#add('fields', {
          page,
          region: box,
          name: freshName('checkbox', this.#names()),
          kind: { type: 'checkbox' },
        } satisfies NewField);
        return;
      case 'radio': {
        // Radio buttons made one after another join one group.
        const last = [...this.entries].reverse().find((e) => e.group === 'fields');
        const lastField = last?.data as NewField | undefined;
        const joined = lastField?.kind.type === 'radio' ? lastField.name : freshName('choice', this.#names());
        const count = this.entries.filter(
          (e) => e.group === 'fields' && (e.data as NewField).name === joined,
        ).length;
        this.#add('fields', {
          page,
          region: box,
          name: joined,
          kind: { type: 'radio', option: `Option ${count + 1}` },
        } satisfies NewField);
        return;
      }
      case 'dropdown':
        this.#add('fields', {
          page,
          region: box,
          name: freshName('list', this.#names()),
          kind: { type: 'dropdown', options: ['Option 1', 'Option 2'] },
        } satisfies NewField);
        return;
      case 'signature-field':
        this.#add('fields', {
          page,
          region: box,
          name: freshName('signature', this.#names()),
          kind: { type: 'signature' },
        } satisfies NewField);
        return;
      default:
        return;
    }
  }

  /** A note at a point. */
  placeNote(at: Pt): void {
    this.#add('annotations', {
      kind: 'note',
      page: this.page,
      at,
      text: '',
      color: palette.note,
    } satisfies Annotation);
  }

  /** A freehand stroke, thinned. */
  placeStroke(points: Pt[]): void {
    const stroke = simplify(points);
    if (stroke.length === 0) return;
    this.#add('annotations', {
      kind: 'freehand',
      page: this.page,
      strokes: [stroke],
      color: palette.pen,
      width_pt: 2,
    } satisfies Annotation);
  }

  /** Changes a block of existing text: a patch over it and new text in its place. */
  replaceText(block: TextBlock): void {
    const pad = 0.004;
    const patch: Region = {
      x: Math.max(block.region.x - pad, 0),
      y: Math.max(block.region.y - pad, 0),
      width: Math.min(block.region.width + 2 * pad, 1 - Math.max(block.region.x - pad, 0)),
      height: Math.min(block.region.height + 2 * pad, 1 - Math.max(block.region.y - pad, 0)),
    };
    this.#add(
      'items',
      { kind: 'whiteout', page: this.page, region: patch, color: palette.white } satisfies EditItem,
      [
        {
          group: 'items',
          data: {
            kind: 'text',
            page: this.page,
            region: block.region,
            text: block.text,
            style: {
              size_pt: Math.round(block.size_pt * 10) / 10,
              color: palette.black,
              bold: block.bold,
              italic: block.italic,
            },
          } satisfies EditItem,
        },
      ],
    );
  }

  /** The suggestions of the form finder that the person accepted. */
  placeSuggested(fields: NewField[]): void {
    if (fields.length === 0) return;
    this.#change(() => {
      const taken = this.#names();
      const made: Entry[] = fields.map((f) => {
        const name = freshName(f.name, taken);
        taken.push(name);
        return { id: this.#next++, group: 'fields', data: { ...f, name } };
      });
      this.entries = [...this.entries, ...made];
      this.selectedId = made[made.length - 1]?.id ?? null;
    });
  }

  /** Sends what is placed to the tool's options. */
  publish(): void {
    const tool = this.#tool;
    if (!tool) return;
    const options = toOptions(tool.id, $state.snapshot(this.entries) as Entry[], $state.snapshot(this.values));
    untrack(() => {
      for (const [key, value] of Object.entries(options)) toolOptions.set(tool, key, value);
    });
  }

  /** Remembers a signature for next time, then makes it the one to place. */
  async keep(name: string, kind: 'signature' | 'initials' | 'stamp'): Promise<void> {
    if (!this.pending) return;
    await ipc.signatureSave({
      name,
      kind,
      aspect: this.pending.aspect,
      source: $state.snapshot(this.pending.source) as SignatureSource,
    });
  }
}

export const editor = new Editor();
