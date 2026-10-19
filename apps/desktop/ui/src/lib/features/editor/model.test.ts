import { describe, expect, it } from 'vitest';
import type { Annotation, EditItem, NewField, SignatureItem } from '../../ipc/generated';
import {
  Timeline,
  boundsOf,
  boxAround,
  boxFromDrag,
  boxOf,
  creatorsFor,
  freshName,
  gestureOf,
  groupOf,
  labelOf,
  moveBox,
  normalizeStrokes,
  resizeBox,
  signatureBox,
  simplify,
  toOptions,
  withBox,
  withPercent,
  type Entry,
} from './model';

const box = { x: 0.2, y: 0.3, width: 0.3, height: 0.1 };

function entry(group: Entry['group'], data: Entry['data'], id = 1): Entry {
  return { id, group, data };
}

describe('geometry', () => {
  it('makes a box from a drag in any direction and refuses slivers', () => {
    expect(boxFromDrag({ x: 0.5, y: 0.5 }, { x: 0.2, y: 0.1 })).toEqual({ x: 0.2, y: 0.1, width: 0.3, height: 0.4 });
    expect(boxFromDrag({ x: 0.5, y: 0.5 }, { x: 0.505, y: 0.9 })).toBeNull();
    expect(boxFromDrag({ x: -1, y: -1 }, { x: 2, y: 2 })).toEqual({ x: 0, y: 0, width: 1, height: 1 });
  });

  it('keeps a box on the page when it is centered, moved or resized', () => {
    expect(boxAround({ x: 0.02, y: 0.99 }, 0.2, 0.1)).toEqual({ x: 0, y: 0.9, width: 0.2, height: 0.1 });
    expect(moveBox(box, 1, -1)).toEqual({ ...box, x: 0.7, y: 0 });
    expect(resizeBox(box, 1, 1)).toEqual({ ...box, width: 0.8, height: 0.7 });
    expect(resizeBox(box, -1, -1)).toEqual({ ...box, width: 0.01, height: 0.01 });
  });

  it('applies a typed percent to one side and keeps the box legal', () => {
    expect(withPercent(box, 'x', 50)).toEqual({ ...box, x: 0.5, width: 0.3 });
    expect(withPercent(box, 'width', 90)).toEqual({ ...box, width: 0.8 });
    expect(withPercent(box, 'x', Number.NaN)).toBe(box);
    expect(withPercent(box, 'x', 100).width).toBeGreaterThan(0);
  });
});

describe('strokes', () => {
  it('drops points that lie on the line between their neighbours and keeps the corners', () => {
    const line = Array.from({ length: 50 }, (_, i) => ({ x: i / 49, y: 0.5 }));
    expect(simplify(line)).toEqual([line[0], line[49]]);
    const corner = [...line.slice(0, 25), { x: 0.5, y: 0.9 }, ...line.slice(25)];
    const thin = simplify(corner);
    expect(thin.length).toBeLessThanOrEqual(5);
    expect(thin).toContainEqual({ x: 0.5, y: 0.9 });
    expect(simplify([{ x: 0, y: 0 }])).toHaveLength(1);
  });

  it('fits strokes to their own box and reports the proportions', () => {
    const strokes = [
      [
        { x: 0.1, y: 0.2 },
        { x: 0.5, y: 0.2 },
      ],
      [{ x: 0.3, y: 0.6 }],
    ];
    const bounds = boundsOf(strokes);
    expect(bounds?.x).toBeCloseTo(0.1);
    expect(bounds?.width).toBeCloseTo(0.4);
    expect(bounds?.height).toBeCloseTo(0.4);
    const fitted = normalizeStrokes(strokes, 2);
    expect(fitted?.strokes[0]).toEqual([
      { x: 0, y: 0 },
      { x: 1, y: 0 },
    ]);
    expect(fitted?.strokes[1]?.[0]?.x).toBeCloseTo(0.5);
    expect(fitted?.strokes[1]?.[0]?.y).toBeCloseTo(1);
    // A pad twice as wide as tall, and a drawing as wide as tall on the page: two to one.
    expect(fitted?.aspect).toBeCloseTo(2, 5);
    expect(normalizeStrokes([], 2)).toBeNull();
    expect(normalizeStrokes([[{ x: 0.5, y: 0.5 }]], 2)?.aspect).toBeCloseTo(2, 5);
  });

  it('places a signature in proportion', () => {
    const b = signatureBox({ x: 0.5, y: 0.5 }, 4, 0.2);
    expect(b.width).toBeCloseTo(0.2);
    expect(b.height).toBeCloseTo(0.05);
    expect(signatureBox({ x: 0.5, y: 0.5 }, 0.5, 0.2).height).toBeLessThanOrEqual(0.4);
  });
});

describe('entries', () => {
  const text: EditItem = { kind: 'text', page: 2, region: box, text: 'Hello\nworld' };
  const highlight: Annotation = { kind: 'highlight', page: 1, regions: [box, { ...box, y: 0.5 }] };
  const ink: Annotation = {
    kind: 'freehand',
    page: 1,
    strokes: [
      [
        { x: 0.1, y: 0.1 },
        { x: 0.3, y: 0.2 },
      ],
    ],
  };

  it('finds and changes the box of every kind of entry', () => {
    expect(boxOf(entry('items', text))).toEqual(box);
    expect(withBox(entry('items', text), { ...box, x: 0.5 }).data).toMatchObject({ region: { x: 0.5 } });
    expect(boxOf(entry('annotations', highlight))).toEqual(box);
    const moved = withBox(entry('annotations', highlight), { ...box, x: 0.4, y: 0.4 });
    const regions = (moved.data as Extract<Annotation, { kind: 'highlight' }>).regions;
    expect(regions[0]).toMatchObject({ x: 0.4, y: 0.4 });
    // The second line keeps its distance from the first.
    expect(regions[1]?.x).toBeCloseTo(0.4);
    expect(regions[1]?.y).toBeCloseTo(0.6);
    const note: Annotation = { kind: 'note', page: 1, at: { x: 0.1, y: 0.2 }, text: 'Check' };
    expect(withBox(entry('annotations', note), { x: 0.5, y: 0.6, width: 0.03, height: 0.03 }).data).toMatchObject({
      at: { x: 0.5, y: 0.6 },
    });
    const resized = withBox(entry('annotations', ink), { x: 0.2, y: 0.2, width: 0.4, height: 0.2 });
    const b = boundsOf((resized.data as Extract<Annotation, { kind: 'freehand' }>).strokes);
    expect(b?.x).toBeCloseTo(0.2);
    expect(b?.width).toBeCloseTo(0.4);
  });

  it('describes entries for the list', () => {
    expect(labelOf(entry('items', text))).toBe('Hello');
    expect(labelOf(entry('items', { kind: 'link', page: 1, region: box, target: { type: 'page', page: 4 } }))).toBe('Page 4');
    expect(labelOf(entry('items', { kind: 'image', page: 1, region: box, path: 'C:\\pics\\logo.png' }))).toBe('logo.png');
    expect(labelOf(entry('annotations', { kind: 'note', page: 1, at: { x: 0, y: 0 }, text: 'Look here' }))).toBe('Look here');
    const typed: SignatureItem = { page: 1, region: box, source: { type: 'typed', text: 'A. Lovelace' } };
    expect(labelOf(entry('signatures', typed))).toBe('A. Lovelace');
    const field: NewField = { page: 1, region: box, name: 'email', kind: { type: 'text' } };
    expect(labelOf(entry('fields', field))).toBe('email (text)');
  });

  it('gives fields names that are not taken', () => {
    expect(freshName('Full name', [])).toBe('Full_name');
    expect(freshName('text', ['text', 'text_2'])).toBe('text_3');
    expect(freshName('???', [])).toBe('field');
  });

  it('knows how each creator is drawn and where it goes', () => {
    expect(gestureOf('select')).toBe('none');
    expect(gestureOf('note')).toBe('point');
    expect(gestureOf('freehand')).toBe('stroke');
    expect(gestureOf('whiteout')).toBe('box');
    expect(groupOf('text')).toBe('items');
    expect(groupOf('highlight')).toBe('annotations');
    expect(groupOf('date')).toBe('signatures');
    expect(groupOf('radio')).toBe('fields');
    expect(groupOf('select')).toBeNull();
    for (const list of Object.values(creatorsFor)) expect(list[0]).toBe('select');
  });
});

describe('options', () => {
  const text: EditItem = { kind: 'text', page: 1, region: box, text: 'Hi' };
  const sig: SignatureItem = { page: 1, region: box, source: { type: 'date' } };
  const entries = [entry('items', text, 1), entry('signatures', sig, 2)];

  it('sends each tool the group it owns', () => {
    expect(toOptions('edit-pdf', entries, {})).toEqual({ items: [text] });
    expect(toOptions('sign-pdf', entries, {})).toEqual({ signatures: [sig] });
    expect(toOptions('annotate', entries, {})).toEqual({ annotations: [] });
    expect(toOptions('create-forms', entries, {})).toEqual({ fields: [] });
  });

  it('sends typed form values with the signatures and leaves empty ones out', () => {
    expect(toOptions('fill-sign', entries, { name: 'Ada', city: '' })).toEqual({
      values: [{ name: 'name', value: 'Ada' }],
      signatures: [sig],
    });
  });
});

describe('timeline', () => {
  it('goes back and forth and forgets the future after a new change', () => {
    const t = new Timeline<string>();
    expect(t.canUndo || t.canRedo).toBe(false);
    t.record('a');
    t.record('b');
    expect(t.undo('c')).toBe('b');
    expect(t.canRedo).toBe(true);
    expect(t.redo('b')).toBe('c');
    expect(t.undo('c')).toBe('b');
    t.record('x');
    expect(t.canRedo).toBe(false);
    expect(t.undo('y')).toBe('x');
    t.clear();
    expect(t.undo('z')).toBeUndefined();
  });
});
