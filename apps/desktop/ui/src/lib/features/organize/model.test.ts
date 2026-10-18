import { describe, expect, it } from 'vitest';
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

const label = (pages: OrgPage[]): string[] =>
  pages.map((p) => (p.kind === 'page' ? `${p.source}:${p.page + 1}` : 'blank'));
const set = (...n: number[]): Set<number> => new Set(n);

describe('organizer model', () => {
  const start = (): OrgPage[] => fromFiles([{ id: 7, pageCount: 3 }, { id: 9, pageCount: 2 }]);

  it('starts with every page of every file in file order, each with its own key', () => {
    const pages = start();
    expect(label(pages)).toEqual(['7:1', '7:2', '7:3', '9:1', '9:2']);
    expect(new Set(pages.map((p) => p.key)).size).toBe(5);
  });

  it('rotates pages clockwise and counter-clockwise, wrapping around', () => {
    let pages = rotate(start(), set(0), 90);
    expect((pages[0] as FilePage).rotate).toBe(90);
    pages = rotate(pages, set(0), -180);
    expect((pages[0] as FilePage).rotate).toBe(270);
    expect((pages[1] as FilePage).rotate).toBe(0);
  });

  it('swaps the sides of a blank page when it is turned a quarter', () => {
    const blank = blankLike(undefined, () => undefined);
    const turned = rotate([blank], set(0), 90)[0];
    expect(turned).toMatchObject({ kind: 'blank', width_pt: 792, height_pt: 612 });
  });

  it('removes and duplicates selected pages', () => {
    expect(label(remove(start(), set(1, 3)))).toEqual(['7:1', '7:3', '9:2']);
    expect(label(duplicate(start(), set(0, 4)))).toEqual(['7:1', '7:1', '7:2', '7:3', '9:1', '9:2', '9:2']);
    const copies = duplicate(start(), set(0));
    expect(copies[0]?.key).not.toBe(copies[1]?.key);
  });

  it('moves a block to a position and keeps its inner order', () => {
    expect(label(moveTo(start(), set(0, 1), 3))).toEqual(['7:3', '9:1', '9:2', '7:1', '7:2']);
    expect(label(moveTo(start(), set(4), 0))).toEqual(['9:2', '7:1', '7:2', '7:3', '9:1']);
    expect(label(moveTo(start(), set(0), 99))).toEqual(['7:2', '7:3', '9:1', '9:2', '7:1']);
  });

  it('nudges the selection one step and stops at the edges', () => {
    const earlier = nudge(start(), set(2, 3), -1);
    expect(label(earlier.pages)).toEqual(['7:1', '7:3', '9:1', '7:2', '9:2']);
    expect([...earlier.at].sort()).toEqual([1, 2]);
    const stuck = nudge(start(), set(0, 1), -1);
    expect(label(stuck.pages)).toEqual(label(start()));
    expect([...stuck.at].sort()).toEqual([0, 1]);
    const later = nudge(start(), set(3, 4), 1);
    expect(label(later.pages)).toEqual(label(start()));
    expect(label(nudge(start(), set(0), 1).pages)).toEqual(['7:2', '7:1', '7:3', '9:1', '9:2']);
  });

  it('inserts blanks sized like the neighbor as displayed', () => {
    const pages = rotate(start(), set(0), 90);
    const size = () => ({ w: 600, h: 800 });
    const blank = blankLike(pages[0], size);
    expect(blank).toMatchObject({ width_pt: 800, height_pt: 600 });
    expect(label(insertAfter(pages, 0, [blank]))).toEqual(['7:1', 'blank', '7:2', '7:3', '9:1', '9:2']);
    expect(label(insertAfter(pages, -1, [blank])).at(0)).toBe('blank');
  });

  it('takes a clamped page range from another file', () => {
    expect(pagesOf(9, 5, 2, 4).map((p) => p.page)).toEqual([1, 2, 3]);
    expect(pagesOf(9, 5, 0, 99).map((p) => p.page)).toEqual([0, 1, 2, 3, 4]);
    expect(pagesOf(9, 5, 4, 2).map((p) => p.page)).toEqual([3]);
  });

  it('turns the list into the option the core expects, with files by position', () => {
    const pages = rotate(insertAfter(start(), 0, [blankLike(undefined, () => undefined)]), set(2), 270);
    const option = toOption(pages, [9, 7]);
    expect(option[0]).toEqual({ kind: 'page', source: 1, page: 0, rotate: '0' });
    expect(option[1]).toEqual({ kind: 'blank', width_pt: 612, height_pt: 792 });
    expect(option[2]).toEqual({ kind: 'page', source: 1, page: 1, rotate: '270' });
    expect(option[4]).toEqual({ kind: 'page', source: 0, page: 0, rotate: '0' });
    // A page whose file was removed from the list is dropped rather than sent.
    expect(toOption(start(), [9])).toHaveLength(2);
  });
});

describe('History', () => {
  it('undoes and redoes whole lists and forgets the redo branch after a new edit', () => {
    const history = new History();
    const a = fromFiles([{ id: 1, pageCount: 2 }]);
    const b = remove(a, set(0));
    history.record(a);
    expect(history.canUndo).toBe(true);
    expect(history.undo(b)).toBe(a);
    expect(history.canRedo).toBe(true);
    expect(history.redo(a)).toBe(b);
    history.undo(b);
    history.record(a);
    expect(history.canRedo).toBe(false);
    expect(new History().undo(a)).toBeUndefined();
  });
});
